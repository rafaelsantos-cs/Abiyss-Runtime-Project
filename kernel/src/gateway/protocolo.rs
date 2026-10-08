//! Protocolo entre o kernel e o adaptador: JSON, uma mensagem por linha,
//! num socket Unix local. O campo `tipo` diz o que é.
//!
//! Adaptador → kernel:
//! - `ola {versao}`: primeira linha de cada conexão;
//! - `mensagem {mensagem}`: uma mensagem do Discord (só fatos);
//! - `enviado {ref, ids}`: a saída `ref` foi entregue (IDs no Discord);
//! - `falhou {ref, erro}`: não deu para entregar (o kernel tenta de novo).
//!
//! Kernel → adaptador:
//! - `ola {versao, dono_id, canais, pessoas, ultimo_dm, ultimos_canais,
//!   workspace}`: quem é o dono, quais canais ouvir, para quem pode haver
//!   DM, de onde buscar o que chegou com o adaptador fora do ar e a única
//!   pasta de onde sai arquivo;
//! - `recebido {id, estado}`: a mensagem foi gravada (ou ignorada);
//! - `enviar {ref, canal_id, dm_para, responder_a, texto, anexo?}`:
//!   entregar uma mensagem. Destino: `canal_id` (um canal permitido),
//!   `dm_para` (DM a esta pessoa) ou os dois nulos (DM do dono). `anexo` =
//!   arquivo do workspace. Confirmar com `enviado`;
//! - `resposta_inicio {ref, canal_id, responder_a}`, `resposta_parcial
//!   {ref, texto}` e `resposta_fim {ref, texto}`: a resposta da conversa
//!   chegando aos poucos. `texto` é sempre o texto INTEIRO até ali (vazio =
//!   pensando). O adaptador edita uma mensagem só, no ritmo dele;
//!   confirmar o `resposta_fim {ref, canal_id, dm_para, responder_a, texto}` com
//!   `enviado`. Um `resposta_fim` de uma `ref` que o adaptador não conhece
//!   (reconectou no meio) vale como `enviar`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::confianca::MensagemDiscord;

/// Versão do protocolo. Muda quando uma mensagem muda de forma.
pub const VERSAO: u32 = 2;
/// Maior linha aceita do adaptador (uma mensagem do Discord tem no máximo
/// 4000 caracteres; folga para o JSON).
pub const MAX_LINHA: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum DoAdaptador {
    Ola {
        versao: u32,
    },
    Mensagem {
        mensagem: MensagemDiscord,
    },
    Enviado {
        #[serde(rename = "ref")]
        referencia: i64,
        #[serde(default)]
        ids: Vec<String>,
    },
    Falhou {
        #[serde(rename = "ref")]
        referencia: i64,
        #[serde(default)]
        erro: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum ParaAdaptador {
    Ola {
        versao: u32,
        dono_id: String,
        /// Canais permitidos (o adaptador só ouve estes, além das DMs).
        canais: Vec<String>,
        /// Pessoas conhecidas (o adaptador pode mandar DM a elas).
        pessoas: Vec<String>,
        ultimo_dm: Option<String>,
        /// Canal → último ID que o kernel já tem.
        ultimos_canais: BTreeMap<String, String>,
        /// Pasta real do workspace: o adaptador só manda arquivo de dentro dela.
        workspace: String,
    },
    Recebido {
        id: String,
        estado: String,
    },
    Enviar {
        #[serde(rename = "ref")]
        referencia: i64,
        canal_id: Option<String>,
        dm_para: Option<String>,
        responder_a: Option<String>,
        texto: String,
        /// Caminho real de um arquivo do workspace para anexar.
        #[serde(skip_serializing_if = "Option::is_none")]
        anexo: Option<String>,
    },
    RespostaInicio {
        #[serde(rename = "ref")]
        referencia: i64,
        canal_id: Option<String>,
        dm_para: Option<String>,
        responder_a: Option<String>,
    },
    RespostaParcial {
        #[serde(rename = "ref")]
        referencia: i64,
        texto: String,
    },
    /// Repete o destino: um adaptador que reconectou no meio (não conhece
    /// a `ref`) entrega como um `enviar`.
    RespostaFim {
        #[serde(rename = "ref")]
        referencia: i64,
        canal_id: Option<String>,
        dm_para: Option<String>,
        responder_a: Option<String>,
        texto: String,
    },
}

/// Destino como fica gravado na saída (`canal_id` da tabela): nulo = DM do
/// dono; `dm:<id>` = DM a esta pessoa; outro valor = um canal permitido.
pub const PREFIXO_DM: &str = "dm:";

/// O destino gravado → (`canal_id`, `dm_para`) do protocolo.
pub fn destino(gravado: Option<&str>) -> (Option<String>, Option<String>) {
    match gravado {
        None => (None, None),
        Some(d) => match d.strip_prefix(PREFIXO_DM) {
            Some(pessoa) => (None, Some(pessoa.to_string())),
            None => (Some(d.to_string()), None),
        },
    }
}

impl ParaAdaptador {
    /// A linha JSON (sem o `\n`).
    pub fn linha(&self) -> String {
        serde_json::to_string(self).expect("mensagem do protocolo sempre vira JSON")
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn destinos_gravados() {
        assert_eq!(destino(None), (None, None));
        assert_eq!(destino(Some("dm:42")), (None, Some("42".into())));
        assert_eq!(destino(Some("77")), (Some("77".into()), None));
    }

    #[test]
    fn formato_das_linhas() {
        let m: DoAdaptador =
            serde_json::from_value(json!({"tipo": "enviado", "ref": 7, "ids": ["1", "2"]}))
                .unwrap();
        assert_eq!(
            m,
            DoAdaptador::Enviado {
                referencia: 7,
                ids: vec!["1".into(), "2".into()]
            }
        );
        let m: DoAdaptador = serde_json::from_value(json!({
            "tipo": "mensagem",
            "mensagem": {"id": "5", "canal_id": "6", "dm": true, "autor_id": "7", "texto": "oi"}
        }))
        .unwrap();
        assert!(matches!(m, DoAdaptador::Mensagem { mensagem } if mensagem.texto == "oi"));
        assert!(serde_json::from_str::<DoAdaptador>(r#"{"tipo": "outra"}"#).is_err());

        let linha = ParaAdaptador::Enviar {
            referencia: 3,
            canal_id: None,
            dm_para: None,
            responder_a: Some("5".into()),
            texto: "olá".into(),
            anexo: None,
        }
        .linha();
        let v: serde_json::Value = serde_json::from_str(&linha).unwrap();
        assert_eq!(
            v,
            json!({"tipo": "enviar", "ref": 3, "canal_id": null, "dm_para": null, "responder_a": "5", "texto": "olá"})
        );
    }
}
