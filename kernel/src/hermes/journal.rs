//! `metacognition/journal.jsonl` → tabela `diario`.
//!
//! Uma entrada JSON por linha. Campos mapeados: momento, ação,
//! expectativa, sinais, risco, confiança e resultado (quando houver).
//! O resto vai para `extras` (preservado) e para o relatório.

use serde_json::{Map, Value};

use super::campos::{Leitor, confianca_de, hash_estavel, momento_de};
use super::segredos::parece_conter_segredo;
use super::{Contexto, Secao};
use crate::diario::{self, EntradaImportada};
use crate::importacoes;
use crate::tempo::agora_ms;

const ARQUIVO: &str = "metacognition/journal.jsonl";

const ID: &[&str] = &["id", "entry_id", "uuid"];
const MOMENTO: &[&str] = &[
    "ts",
    "timestamp",
    "time",
    "created_at",
    "criado_em",
    "data",
    "date",
    "quando",
    "momento",
];
const ACAO: &[&str] = &[
    "action", "acao", "ação", "decision", "decisao", "decisão", "task", "tarefa", "event",
    "evento", "title", "titulo",
];
const EXPECTATIVA: &[&str] = &[
    "expectation",
    "expectativa",
    "expected",
    "prediction",
    "previsao",
    "previsão",
    "hypothesis",
    "hipotese",
    "hipótese",
];
const SINAIS: &[&str] = &[
    "signals",
    "sinais",
    "signal",
    "sinal",
    "evidence",
    "evidencias",
    "evidências",
];
const RISCO: &[&str] = &["risk", "risco", "risks", "riscos"];
const CONFIANCA: &[&str] = &[
    "confidence",
    "confianca",
    "confiança",
    "certainty",
    "certeza",
];
const RESULTADO: &[&str] = &["outcome", "resultado", "result", "observed", "observado"];
const RESULTADO_MOMENTO: &[&str] = &["outcome_at", "resolved_at", "resultado_em"];
const GOAL: &[&str] = &["goal_id", "goal", "objetivo", "goal_ref"];

pub(super) fn importar(ctx: &mut Contexto<'_>) -> anyhow::Result<Secao> {
    let mut secao = Secao::nova("Diário: metacognition/journal.jsonl → tabela diario");
    if !ctx.tem(ARQUIVO) {
        secao.linhas.push("(arquivo não encontrado)".into());
        return Ok(secao);
    }
    let texto = match ctx.ler(ARQUIVO) {
        Ok(t) => t,
        Err(e) => {
            secao.erros.push(format!("{e:#}"));
            return Ok(secao);
        }
    };
    let mut ligados_a_goal = 0;
    for (indice, linha) in texto.lines().enumerate() {
        let numero = indice + 1;
        let linha = linha.trim();
        if linha.is_empty() {
            continue;
        }
        let objeto: Map<String, Value> = match serde_json::from_str::<Value>(linha) {
            Ok(Value::Object(o)) => o,
            Ok(_) => {
                secao
                    .erros
                    .push(format!("linha {numero}: não é um objeto JSON"));
                continue;
            }
            Err(e) => {
                secao
                    .erros
                    .push(format!("linha {numero}: JSON inválido ({e})"));
                continue;
            }
        };
        if let Some(tipo) = parece_conter_segredo(linha) {
            secao.erros.push(format!(
                "linha {numero}: parece conter um segredo ({tipo}); NÃO importada"
            ));
            continue;
        }
        if importar_linha(ctx, &mut secao, numero, linha, &objeto)? {
            ligados_a_goal += 1;
        }
    }
    if ligados_a_goal > 0 {
        secao.linhas.push(format!(
            "{ligados_a_goal} entrada(s) ligada(s) a goals importados"
        ));
    }
    Ok(secao)
}

/// Importa uma linha. Devolve `true` se ela ficou ligada a um goal.
fn importar_linha(
    ctx: &mut Contexto<'_>,
    secao: &mut Secao,
    numero: usize,
    linha: &str,
    objeto: &Map<String, Value>,
) -> anyhow::Result<bool> {
    let mut l = Leitor::novo(objeto);
    let id = l.texto("id", ID);
    let momento_bruto = l.valor("momento", MOMENTO);
    let acao = l.texto("acao", ACAO);
    let expectativa = l.texto("expectativa", EXPECTATIVA).unwrap_or_default();
    let sinais = l.texto("sinais", SINAIS);
    let risco = l.texto("risco", RISCO);
    let confianca_bruta = l.valor("confianca", CONFIANCA);
    let resultado = l.texto("resultado", RESULTADO);
    let resultado_ms = l
        .valor("resultado_em", RESULTADO_MOMENTO)
        .and_then(momento_de);
    let goal_hermes = l.texto("goal", GOAL);
    secao.anotar(&l);
    let mut extras = l.extras();

    let chave = match &id {
        Some(id) => format!("hermes:journal:{id}"),
        None => format!("hermes:journal:fnv:{}", hash_estavel(linha)),
    };
    // Avisos só aparecem para entradas novas.
    let nova = !importacoes::ja_importado(ctx.banco, &chave)?;
    let momento_ms = match momento_bruto.and_then(momento_de) {
        Some(ms) => ms,
        None => {
            if nova {
                secao.linhas.push(format!(
                    "! linha {numero}: sem data reconhecível; usei o momento da importação"
                ));
            }
            if let Some(v) = momento_bruto {
                extras.insert("momento_original".into(), v.clone());
            }
            agora_ms()
        }
    };
    let confianca = confianca_bruta.and_then(|v| {
        let convertida = confianca_de(v);
        if convertida.is_none() {
            if nova {
                secao.linhas.push(format!(
                    "! linha {numero}: confiança {v} não é número; preservada em extras"
                ));
            }
            extras.insert("confianca_original".into(), v.clone());
        }
        convertida
    });
    // Liga ao goal importado, se houver (por ID do Hermes).
    let mut goal_id = None;
    let mut ligado = false;
    if let Some(g) = &goal_hermes {
        match importacoes::destino(ctx.banco, &format!("hermes:goal:{g}"))? {
            Some(destino) => {
                goal_id = destino.parse::<i64>().ok();
                ligado = true;
            }
            None if ctx.goals_planejados.contains(g) => ligado = true,
            None => {
                extras.insert("goal_hermes".into(), Value::String(g.clone()));
            }
        }
    }

    let rotulo = id.clone().unwrap_or_else(|| format!("linha {numero}"));
    if !nova {
        secao.ja_importados += 1;
        return Ok(ligado);
    }
    if ctx.aplicar {
        let entrada = EntradaImportada {
            momento_ms,
            origem: "hermes".into(),
            goal_id,
            acao: acao.unwrap_or_else(|| "(sem ação registrada no Hermes)".into()),
            expectativa,
            sinais,
            risco,
            confianca,
            resultado,
            resultado_ms,
            extras: (!extras.is_empty()).then(|| Value::Object(extras).to_string()),
        };
        diario::importar(ctx.banco, &entrada, &chave, linha)?;
    }
    secao.novos += 1;
    if secao.novos <= 20 {
        secao.linhas.push(format!("+ {rotulo}"));
    } else if secao.novos == 21 {
        secao.linhas.push("+ ... (demais entradas omitidas)".into());
    }
    Ok(ligado)
}
