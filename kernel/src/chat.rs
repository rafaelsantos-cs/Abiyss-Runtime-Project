//! Sessão de conversa com o usuário (`abiyss chat`).
//!
//! A cada mensagem do usuário:
//! 1. grava a mensagem no histórico;
//! 2. monta o contexto: system prompt (regras + núcleo + memória central +
//!    índice das skills + interocepção) + histórico recente;
//! 3. chama o cérebro pelo pool, com origem `Conversa` (fatia reservada);
//! 4. se o modelo pedir ferramentas, executa, grava os resultados
//!    (rotulados como dado) e volta ao passo 2 — até `max_rodadas_ferramentas`;
//! 5. grava e devolve a resposta final.
//!
//! Origem do conteúdo (para a regra dura da memória interna):
//! - resultados de ferramentas externas são gravados com `origem_externa`;
//! - respostas do modelo geradas DEPOIS de um resultado externo no mesmo
//!   turno também (são a paráfrase mais direta daquele conteúdo);
//! - antes de cada chamada ao modelo, o kernel olha a janela de histórico
//!   que vai no contexto: se houver algo marcado, as ferramentas pedidas
//!   nessa rodada recebem essa origem (é o que `memoria_propor` usa).
//!
//! A marca some quando as mensagens marcadas saem da janela; ela não se
//! propaga de turno em turno (senão uma conversa longa nunca mais poderia
//! guardar nada na memória interna).

use std::sync::Arc;

use serde_json::json;

use crate::config::Config;
use crate::db::Banco;
use crate::ferramentas::{CaixaDeFerramentas, ContextoChamada};
use crate::historico;
use crate::identidade::{BlocosPrompt, Identidade};
use crate::interocepcao::{self, Interocepcao};
use crate::memoria::central::MemoriaCentral;
use crate::nim::{self, Mensagem, Uso};
use crate::orquestrador::{AoReceber, Origem, Orquestrador, reemprestar};

/// Texto devolvido quando o modelo insiste em ferramentas além do limite.
pub const AVISO_LIMITE_RODADAS: &str = "(parei: limite de rodadas de ferramentas atingido)";

/// O que um turno de conversa devolve.
#[derive(Debug, Clone)]
pub struct RespostaTurno {
    pub texto: String,
    /// Tokens somados de todas as chamadas do turno.
    pub uso: Uso,
    /// Quantas ferramentas foram executadas no turno.
    pub ferramentas_usadas: usize,
}

pub struct SessaoChat {
    config: Config,
    orquestrador: Orquestrador,
    banco: Banco,
    ferramentas: Arc<CaixaDeFerramentas>,
    /// ID da conversa no banco.
    pub conversa: i64,
}

impl SessaoChat {
    /// Começa uma conversa nova.
    pub fn nova(
        config: Config,
        orquestrador: Orquestrador,
        banco: Banco,
        ferramentas: Arc<CaixaDeFerramentas>,
    ) -> anyhow::Result<Self> {
        let conversa = historico::criar_conversa(&banco)?;
        Ok(SessaoChat {
            config,
            orquestrador,
            banco,
            ferramentas,
            conversa,
        })
    }

    /// Retoma uma conversa existente.
    pub fn retomar(
        config: Config,
        orquestrador: Orquestrador,
        banco: Banco,
        ferramentas: Arc<CaixaDeFerramentas>,
        conversa: i64,
    ) -> anyhow::Result<Self> {
        if !historico::conversa_existe(&banco, conversa)? {
            anyhow::bail!("conversa {conversa} não existe");
        }
        Ok(SessaoChat {
            config,
            orquestrador,
            banco,
            ferramentas,
            conversa,
        })
    }

    /// Monta a lista de mensagens enviada ao modelo e diz se essa janela
    /// tem conteúdo externo.
    fn montar_contexto(&self) -> anyhow::Result<(Vec<Mensagem>, ContextoChamada)> {
        // O núcleo é relido a cada turno: editar o arquivo vale na hora.
        let identidade = Identidade::carregar(&self.config.caminho_identidade());
        // Data/hora e estado do "corpo", medidos por código a cada turno.
        let corpo = match Interocepcao::medir(&self.config, &self.banco) {
            Ok(i) => i.como_texto(),
            Err(_) => format!("Data e hora: {}", interocepcao::agora_formatado()),
        };
        let blocos = BlocosPrompt {
            // Relida a cada turno, como o núcleo.
            memoria_central: MemoriaCentral::da_config(&self.config).bloco_para_prompt(),
            // Só nome + descrição; o texto completo vem por `ler_skill`.
            skills: self.ferramentas.indice_skills(),
            contexto: Some(corpo),
        };
        let mut mensagens = vec![Mensagem::sistema(identidade.prompt_sistema_com(&blocos))];
        let janela = self.config.chat.historico_max_mensagens;
        mensagens.extend(historico::carregar(&self.banco, self.conversa, janela)?);
        let origens = historico::origens_externas(&self.banco, self.conversa, janela)?;
        Ok((mensagens, ContextoChamada::com_origens(&origens)))
    }

    /// Envia uma mensagem do usuário e devolve a resposta do Abiyss.
    /// Com `ao_receber = Some(...)`, o texto chega em streaming.
    pub async fn enviar(
        &mut self,
        texto: &str,
        mut ao_receber: AoReceber<'_>,
    ) -> anyhow::Result<RespostaTurno> {
        historico::adicionar(&self.banco, self.conversa, &Mensagem::usuario(texto))?;

        let max_rodadas = self.config.chat.max_rodadas_ferramentas.max(1);
        let mut uso = Uso::default();
        let mut ferramentas_usadas = 0;
        // Origens externas trazidas por ferramentas NESTE turno.
        let mut origens_do_turno: Vec<String> = Vec::new();

        for rodada in 1..=max_rodadas {
            let (mensagens, contexto) = self.montar_contexto()?;
            let mut pedido = nim::montar_pedido(
                &self.config.modelos.cerebro,
                mensagens,
                self.ferramentas.definicoes(),
            );
            // Última rodada: pede uma resposta em texto, sem novas ferramentas.
            if rodada == max_rodadas && !pedido.tools.is_empty() {
                pedido.tool_choice = Some(json!("none"));
            }

            let resposta = self
                .orquestrador
                .cerebro
                .chamar(Origem::Conversa, &pedido, reemprestar(&mut ao_receber))
                .await?;
            uso.somar(&resposta.uso);
            // Resposta escrita depois de conteúdo externo deste turno: derivada dele.
            let derivada = if origens_do_turno.is_empty() {
                None
            } else {
                Some(origens_do_turno.join(", "))
            };
            historico::adicionar_com_origem(
                &self.banco,
                self.conversa,
                &resposta.mensagem,
                derivada.as_deref(),
            )?;

            let chamadas = resposta.mensagem.chamadas().to_vec();
            if chamadas.is_empty() {
                return Ok(RespostaTurno {
                    texto: resposta.mensagem.texto().to_string(),
                    uso,
                    ferramentas_usadas,
                });
            }

            // Executa TODAS as chamadas pedidas (a API exige um resultado
            // para cada uma) e grava os resultados.
            for chamada in &chamadas {
                let resultado = self.ferramentas.executar_com(chamada, &contexto).await;
                ferramentas_usadas += 1;
                if let Some(origem) = &resultado.origem_externa
                    && !origens_do_turno.contains(origem)
                {
                    origens_do_turno.push(origem.clone());
                }
                let mensagem = Mensagem::resultado_ferramenta(
                    &chamada.id,
                    &chamada.function.name,
                    resultado.texto,
                );
                historico::adicionar_com_origem(
                    &self.banco,
                    self.conversa,
                    &mensagem,
                    resultado.origem_externa.as_deref(),
                )?;
            }
        }

        Ok(RespostaTurno {
            texto: AVISO_LIMITE_RODADAS.to_string(),
            uso,
            ferramentas_usadas,
        })
    }
}
