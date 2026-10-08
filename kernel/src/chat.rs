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
//!
//! Esforço: cada resposta começa em `[chat] esforco_padrao` (tabela de
//! esforço do cérebro). Com a ferramenta `aprofundar`, o modelo pede um
//! nível mais profundo para o RESTO DESTA resposta, até `esforco_maximo`; a
//! próxima mensagem do dono volta ao padrão.

use std::sync::Arc;

use serde_json::json;

use crate::config::Config;
use crate::dados;
use crate::db::Banco;
use crate::esforco::{self, NivelEsforco};
use crate::ferramentas::{
    CaixaDeFerramentas, ContextoChamada, ResultadoFerramenta, interpretar_argumentos,
};
use crate::historico;
use crate::identidade::{BlocosPrompt, Identidade};
use crate::interocepcao::{self, Interocepcao};
use crate::mcp::PonteMcp;
use crate::memoria::Memoria;
use crate::memoria::central::MemoriaCentral;
use crate::nim::{self, Ferramenta, Mensagem, Uso};
use crate::orquestrador::{AoReceber, Origem, Orquestrador, reemprestar};
use crate::subagentes::ControleSubagentes;

/// Texto devolvido quando o modelo insiste em ferramentas além do limite.
pub const AVISO_LIMITE_RODADAS: &str = "(parei: limite de rodadas de ferramentas atingido)";

/// Ferramenta só do chat: mais esforço no resto desta resposta.
pub const APROFUNDAR: &str = "aprofundar";

/// O que um turno de conversa devolve.
#[derive(Debug, Clone)]
pub struct RespostaTurno {
    pub texto: String,
    /// Tokens somados de todas as chamadas do turno.
    pub uso: Uso,
    /// Quantas ferramentas foram executadas no turno.
    pub ferramentas_usadas: usize,
}

/// As ferramentas da conversa com o dono (`abiyss chat` e o gateway):
/// nativas + MCP + memória + delegação + `responder_pedido`. `delegar` só
/// grava o pedido no banco; quem executa é o daemon.
pub fn caixa_de_conversa(
    config: &Config,
    banco: Banco,
    mcp: Arc<PonteMcp>,
    memoria: Arc<Memoria>,
) -> anyhow::Result<CaixaDeFerramentas> {
    let controle = ControleSubagentes::novo(config.clone(), banco.clone(), None);
    Ok(CaixaDeFerramentas::da_config(config)?
        .com_mcp(mcp)
        .com_memoria(memoria)
        .com_subagentes(controle)
        .com_pedidos(banco))
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
        let mut corpo = match Interocepcao::medir(&self.config, &self.banco) {
            Ok(i) => i.como_texto(),
            Err(_) => format!("Data e hora: {}", interocepcao::agora_formatado()),
        };
        // Pedidos pendentes ao dono (bloco do kernel, rotulado como dado).
        if self.ferramentas.responde_pedidos()
            && let Some(bloco) = crate::pedidos::bloco_para_chat(&self.banco)?
        {
            corpo.push_str("\n\n");
            corpo.push_str(&crate::dados::rotular("kernel:pedidos", &bloco));
        }
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
        // Esforço desta resposta: o padrão, até o modelo pedir mais.
        let (padrao, maximo) = (
            self.config.chat.esforco_padrao,
            self.config.chat.esforco_maximo,
        );
        let mut nivel = padrao;

        for rodada in 1..=max_rodadas {
            let (mensagens, contexto) = self.montar_contexto()?;
            let modelo = esforco::modelo_no_nivel(&self.config.modelos, "cerebro", nivel);
            let mut definicoes = self.ferramentas.definicoes();
            definicoes.extend(definicao_aprofundar(padrao, maximo));
            let mut pedido = nim::montar_pedido(&modelo, mensagens, definicoes);
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
                let resultado = if chamada.function.name == APROFUNDAR {
                    aprofundar(&mut nivel, padrao, maximo, &chamada.function.arguments)
                } else {
                    self.ferramentas.executar_com(chamada, &contexto).await
                };
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

/// Definição de `aprofundar`: só aparece se houver nível acima do padrão.
fn definicao_aprofundar(padrao: NivelEsforco, maximo: NivelEsforco) -> Option<Ferramenta> {
    let acima: Vec<&str> = NivelEsforco::TODOS
        .iter()
        .filter(|n| **n > padrao && **n <= maximo)
        .map(|n| n.como_texto())
        .collect();
    if acima.is_empty() {
        return None;
    }
    Some(Ferramenta::nova(
        APROFUNDAR,
        format!(
            "Pede mais esforço de raciocínio para o RESTO DESTA resposta (só desta: a próxima \
             mensagem do dono volta ao padrão, {}). Use quando a pergunta pedir raciocínio difícil, \
             um plano longo ou uma revisão cuidadosa; não use para conversa simples. Depois de \
             chamar, responda normalmente. Máximo: {}.",
            padrao.como_texto(),
            maximo.como_texto()
        ),
        json!({
            "type": "object",
            "properties": {
                "nivel": {"type": "string", "enum": acima},
                "motivo": {"type": "string", "description": "Por que esta resposta precisa de mais esforço"}
            },
            "required": ["nivel"]
        }),
    ))
}

/// Executa `aprofundar`: sobe `nivel` (nunca desce) até o `maximo`. O
/// resultado é do kernel (não traz conteúdo externo).
fn aprofundar(
    nivel: &mut NivelEsforco,
    padrao: NivelEsforco,
    maximo: NivelEsforco,
    argumentos: &str,
) -> ResultadoFerramenta {
    let (texto, erro) = match subir_esforco(nivel, padrao, maximo, argumentos) {
        Ok(texto) => (texto, false),
        Err(e) => (format!("ERRO: {e:#}"), true),
    };
    ResultadoFerramenta {
        texto: dados::rotular(APROFUNDAR, &texto),
        erro,
        origem_externa: None,
    }
}

fn subir_esforco(
    nivel: &mut NivelEsforco,
    padrao: NivelEsforco,
    maximo: NivelEsforco,
    argumentos: &str,
) -> anyhow::Result<String> {
    if maximo <= padrao {
        anyhow::bail!("ferramenta '{APROFUNDAR}' não está disponível aqui");
    }
    let args = interpretar_argumentos(argumentos)?;
    let texto = args["nivel"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("falta 'nivel'"))?;
    let pedido = NivelEsforco::de_texto(texto).ok_or_else(|| {
        anyhow::anyhow!(
            "nível inválido '{texto}' (use {})",
            esforco::nomes_dos_niveis()
        )
    })?;
    let alvo = pedido.min(maximo);
    if alvo <= *nivel {
        return Ok(format!(
            "o esforço desta resposta já está em {}; nada mudou",
            nivel.como_texto()
        ));
    }
    *nivel = alvo;
    let corte = if pedido > maximo {
        format!(
            " (pedido {}: o máximo é {})",
            pedido.como_texto(),
            maximo.como_texto()
        )
    } else {
        String::new()
    };
    Ok(format!(
        "esforço desta resposta: {}{corte}; a próxima mensagem do dono volta a {}",
        alvo.como_texto(),
        padrao.como_texto()
    ))
}

#[cfg(test)]
mod testes {
    use super::*;
    use NivelEsforco::*;

    #[test]
    fn aprofundar_sobe_ate_o_maximo_e_nunca_desce() {
        let mut nivel = Medium;
        let r = aprofundar(&mut nivel, Medium, High, r#"{"nivel": "ultra"}"#);
        assert!(!r.erro);
        assert_eq!(nivel, High);
        assert!(
            r.texto
                .contains("esforço desta resposta: high (pedido ultra: o máximo é high)")
        );
        assert_eq!(r.origem_externa, None);

        let r = aprofundar(&mut nivel, Medium, High, r#"{"nivel": "low"}"#);
        assert_eq!(nivel, High, "nunca desce");
        assert!(r.texto.contains("já está em high"));

        let r = aprofundar(&mut nivel, Medium, High, r#"{"nivel": "max"}"#);
        assert!(r.erro);
        assert!(aprofundar(&mut nivel, Medium, High, "{}").erro);

        // Máximo igual ao padrão: a ferramenta nem é oferecida.
        assert!(definicao_aprofundar(Medium, Medium).is_none());
        let mut nivel = Medium;
        assert!(aprofundar(&mut nivel, Medium, Medium, r#"{"nivel": "high"}"#).erro);
        assert_eq!(nivel, Medium);
    }

    #[test]
    fn definicao_lista_so_os_niveis_acima_do_padrao() {
        let def = definicao_aprofundar(Medium, Xhigh).unwrap();
        assert_eq!(def.nome(), APROFUNDAR);
        assert_eq!(
            def.function.parameters["properties"]["nivel"]["enum"],
            json!(["high", "xhigh"])
        );
        assert!(def.function.description.contains("Máximo: xhigh"));
    }
}
