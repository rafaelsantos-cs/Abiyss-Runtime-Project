//! Sessão de conversa com o usuário (`abiyss chat`).
//!
//! A cada mensagem do usuário:
//! 1. grava a mensagem no histórico;
//! 2. monta o contexto: system prompt (regras + núcleo + índice das skills
//!    + interocepção) + histórico recente;
//! 3. chama o cérebro pelo pool, com origem `Conversa` (fatia reservada);
//! 4. se o modelo pedir ferramentas, executa, grava os resultados
//!    (rotulados como dado) e volta ao passo 2 — até `max_rodadas_ferramentas`;
//! 5. grava e devolve a resposta final.

use std::sync::Arc;

use serde_json::json;

use crate::config::Config;
use crate::db::Banco;
use crate::ferramentas::CaixaDeFerramentas;
use crate::historico;
use crate::identidade::{BlocosPrompt, Identidade};
use crate::interocepcao::{self, Interocepcao};
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

    /// Monta a lista de mensagens enviada ao modelo.
    fn montar_contexto(&self) -> anyhow::Result<Vec<Mensagem>> {
        // O núcleo é relido a cada turno: editar o arquivo vale na hora.
        let identidade = Identidade::carregar(&self.config.caminho_identidade());
        // Data/hora e estado do "corpo", medidos por código a cada turno.
        let corpo = match Interocepcao::medir(&self.config, &self.banco) {
            Ok(i) => i.como_texto(),
            Err(_) => format!("Data e hora: {}", interocepcao::agora_formatado()),
        };
        let blocos = BlocosPrompt {
            // Só nome + descrição; o texto completo vem por `ler_skill`.
            skills: self.ferramentas.indice_skills(),
            contexto: Some(corpo),
        };
        let mut mensagens = vec![Mensagem::sistema(identidade.prompt_sistema_com(&blocos))];
        mensagens.extend(historico::carregar(
            &self.banco,
            self.conversa,
            self.config.chat.historico_max_mensagens,
        )?);
        Ok(mensagens)
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

        for rodada in 1..=max_rodadas {
            let mensagens = self.montar_contexto()?;
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
            historico::adicionar(&self.banco, self.conversa, &resposta.mensagem)?;

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
                let resultado = self.ferramentas.executar(chamada).await;
                ferramentas_usadas += 1;
                let mensagem = Mensagem::resultado_ferramenta(
                    &chamada.id,
                    &chamada.function.name,
                    resultado.texto,
                );
                historico::adicionar(&self.banco, self.conversa, &mensagem)?;
            }
        }

        Ok(RespostaTurno {
            texto: AVISO_LIMITE_RODADAS.to_string(),
            uso,
            ferramentas_usadas,
        })
    }
}
