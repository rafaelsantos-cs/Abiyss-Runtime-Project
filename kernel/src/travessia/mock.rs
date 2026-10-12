//! Roteiro do mock do NIM para o benchmark (`abiyss mock-nim --roteiro
//! travessia` e os testes): um "cérebro" que joga mecanicamente e
//! "sub-agentes" que olham o mapa e relatam.
//!
//! Não resolve fase nenhuma de verdade (só tenta "V L", "V L"): serve para
//! exercitar o caminho inteiro do harness (ferramentas do jogo, delegação,
//! `aguardar`, travas, métricas, replay) sem gastar cota.
//!
//! O cérebro é reconhecido por ter a ferramenta `jogo__executar` (os
//! sub-agentes não têm). Cada passo depende só da última ferramenta
//! chamada e do resultado dela.

use std::time::Duration;

use serde_json::{Value, json};

use crate::nim::mock::{ChamadaMock, RespostaMock};

/// O plano que o cérebro do mock tenta em toda fase.
pub const PLANO_DO_MOCK: [&str; 2] = ["V L", "V L"];

fn tem_ferramenta(corpo: &Value, nome: &str) -> bool {
    corpo["tools"]
        .as_array()
        .is_some_and(|l| l.iter().any(|t| t["function"]["name"] == nome))
}

/// A última ferramenta pedida pelo modelo e o texto do resultado dela.
fn ultima_ferramenta(corpo: &Value) -> Option<(String, String)> {
    let mensagens = corpo["messages"].as_array()?;
    let (indice, chamada) = mensagens.iter().enumerate().rev().find_map(|(i, m)| {
        let lista = m["tool_calls"].as_array()?;
        let primeira = lista.first()?;
        Some((i, primeira.clone()))
    })?;
    let id = chamada["id"].as_str().unwrap_or("");
    let nome = chamada["function"]["name"].as_str()?.to_string();
    let resultado = mensagens[indice + 1..]
        .iter()
        .find(|m| m["role"] == "tool" && m["tool_call_id"].as_str() == Some(id))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("")
        .to_string();
    Some((nome, resultado))
}

fn pedir(nome: &str, argumentos: Value) -> RespostaMock {
    RespostaMock::Ferramentas(vec![ChamadaMock {
        nome: nome.into(),
        argumentos,
    }])
}

fn cerebro(corpo: &Value) -> RespostaMock {
    let delega = tem_ferramenta(corpo, "delegar");
    let Some((nome, resultado)) = ultima_ferramenta(corpo) else {
        return pedir("jogo__regras", json!({}));
    };
    match nome.as_str() {
        "jogo__regras" => pedir("jogo__ver", json!({})),
        "jogo__ver" if delega => pedir(
            "delegar",
            json!({"nivel": "low", "tarefa": "Olhe o mapa com jogo__ver e diga onde está a Chave Dourada.",
                   "prazo": 60}),
        ),
        "delegar" => pedir("aguardar", json!({"todos": true, "max_segundos": 30})),
        "jogo__ver" | "aguardar" => pedir("jogo__executar", json!({"plano": PLANO_DO_MOCK})),
        "jogo__executar" if resultado.contains("CONCLUÍDA") || resultado.contains("FALHOU") => {
            pedir("jogo__proxima_fase", json!({}))
        }
        "jogo__executar" => pedir("jogo__desistir_fase", json!({})),
        "jogo__desistir_fase" => pedir("jogo__proxima_fase", json!({})),
        "jogo__proxima_fase"
            if resultado.contains("Não há mais fases")
                || resultado.contains("execução terminou")
                || resultado.contains("Execução encerrada") =>
        {
            RespostaMock::texto("Fim do jogo.")
        }
        _ => pedir("jogo__ver", json!({})),
    }
}

fn subagente(corpo: &Value) -> RespostaMock {
    if ultima_ferramenta(corpo).is_none() && tem_ferramenta(corpo, "jogo__ver") {
        return pedir("jogo__ver", json!({}));
    }
    RespostaMock::texto(
        json!({"status": "concluido", "resumo": "olhei o mapa (mock)", "artefatos": [],
               "confianca": 0.5, "duvidas": []})
        .to_string(),
    )
}

/// O roteiro: cada resposta depois de `atraso` (latência de mentira).
pub fn roteiro_travessia(
    atraso: Duration,
) -> impl Fn(&Value) -> RespostaMock + Send + Sync + 'static {
    move |corpo: &Value| {
        let resposta = if tem_ferramenta(corpo, "jogo__executar") {
            cerebro(corpo)
        } else {
            subagente(corpo)
        };
        if atraso.is_zero() {
            resposta
        } else {
            resposta.atrasada(atraso)
        }
    }
}
