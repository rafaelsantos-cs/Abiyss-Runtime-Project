//! `metacognition/goals/*.json` → tabela `goals`.
//!
//! Cada arquivo é um goal (objeto JSON) ou uma lista de goals. O estado de
//! origem é mapeado para a máquina de estados do kernel; estado
//! desconhecido vira `proposto`, com o estado original registrado no motivo
//! do evento de criação.

use serde_json::{Map, Value};

use super::campos::{Leitor, momento_de, valor_como_texto};
use super::{Contexto, Secao};
use crate::goals::{self, EstadoGoal, GoalImportado};
use crate::importacoes;
use crate::memoria::cofre::sem_acentos;
use crate::tempo::agora_ms;

const PASTA: &str = "metacognition/goals/";

const ID: &[&str] = &["id", "goal_id", "slug", "uuid"];
const TITULO: &[&str] = &[
    "title", "titulo", "título", "name", "nome", "goal", "objetivo",
];
const NUCLEO: &[&str] = &[
    "core",
    "nucleo",
    "núcleo",
    "essence",
    "essencia",
    "essência",
    "success_criteria",
    "criterio",
    "critério",
    "criterio_de_pronto",
    "done_when",
    "definition_of_done",
];
const DESCRICAO: &[&str] = &[
    "description",
    "descricao",
    "descrição",
    "details",
    "detalhes",
    "why",
    "porque",
    "context",
    "contexto",
];
const PRIORIDADE: &[&str] = &[
    "priority",
    "prioridade",
    "importance",
    "importancia",
    "importância",
];
const ESTADO: &[&str] = &["status", "state", "estado", "lifecycle", "fase", "stage"];
const CRIADO: &[&str] = &[
    "created_at",
    "criado_em",
    "created",
    "criado",
    "ts",
    "timestamp",
    "data",
];

/// Estado do Hermes → estado do kernel. `None` = desconhecido.
pub fn estado_do_hermes(texto: &str) -> Option<EstadoGoal> {
    let normal: String = sem_acentos(&texto.trim().to_lowercase())
        .chars()
        .map(|c| if c == ' ' || c == '-' { '_' } else { c })
        .collect();
    let estado = match normal.as_str() {
        "proposed" | "proposto" | "draft" | "rascunho" | "idea" | "ideia" | "backlog" | "new"
        | "novo" | "suggested" | "sugerido" | "pending" | "pendente" => EstadoGoal::Proposto,
        "committed" | "comprometido" | "accepted" | "aceito" | "planned" | "planejado" | "todo"
        | "to_do" | "ready" => EstadoGoal::Comprometido,
        "active" | "ativo" | "in_progress" | "inprogress" | "em_andamento" | "andamento"
        | "executing" | "executando" | "doing" | "fazendo" | "running" | "started" | "iniciado"
        | "working" | "ongoing" => EstadoGoal::Executando,
        "validating" | "validando" | "validation" | "review" | "reviewing" | "em_revisao"
        | "revisao" | "testing" | "verifying" | "verificando" => EstadoGoal::Validando,
        "done" | "completed" | "complete" | "concluido" | "finished" | "finalizado"
        | "achieved" | "alcancado" | "success" | "sucesso" => EstadoGoal::Concluido,
        "blocked" | "bloqueado" | "waiting" | "aguardando" | "on_hold" | "paused" | "pausado"
        | "stalled" | "parado" => EstadoGoal::Bloqueado,
        "abandoned" | "abandonado" | "cancelled" | "canceled" | "cancelado" | "dropped"
        | "descartado" | "failed" | "falhou" | "rejected" | "rejeitado" => EstadoGoal::Abandonado,
        _ => return None,
    };
    Some(estado)
}

/// Prioridade numérica a partir de número ou texto ("alta", "high"...).
fn prioridade_de(valor: &Value) -> Option<i64> {
    match valor {
        Value::Number(n) => n.as_f64().map(|x| x.round() as i64),
        Value::String(s) => match sem_acentos(&s.trim().to_lowercase()).as_str() {
            "alta" | "high" | "urgente" | "urgent" | "critica" | "critical" => Some(2),
            "media" | "medium" | "normal" => Some(1),
            "baixa" | "low" => Some(0),
            outro => outro.parse::<i64>().ok(),
        },
        _ => None,
    }
}

pub(super) fn importar(ctx: &mut Contexto<'_>) -> anyhow::Result<Secao> {
    let mut secao = Secao::nova("Goals: metacognition/goals/*.json → tabela goals");
    let arquivos: Vec<String> = ctx
        .inventario
        .arquivos
        .iter()
        .filter(|a| {
            a.strip_prefix(PASTA)
                .is_some_and(|nome| !nome.contains('/') && nome.ends_with(".json"))
        })
        .cloned()
        .collect();
    if arquivos.is_empty() {
        secao.linhas.push("(nenhum arquivo encontrado)".into());
        return Ok(secao);
    }

    let mut desconhecidos = 0;
    for arquivo in arquivos {
        let texto = match ctx.ler(&arquivo) {
            Ok(t) => t,
            Err(e) => {
                secao.erros.push(format!("{arquivo}: {e:#}"));
                continue;
            }
        };
        let valor: Value = match serde_json::from_str(&texto) {
            Ok(v) => v,
            Err(e) => {
                secao.erros.push(format!(
                    "{arquivo}: JSON inválido ({e}); nada importado deste arquivo"
                ));
                continue;
            }
        };
        let objetos: Vec<(usize, Map<String, Value>)> = match valor {
            Value::Object(o) => vec![(0, o)],
            Value::Array(lista) => lista
                .into_iter()
                .enumerate()
                .filter_map(|(i, v)| match v {
                    Value::Object(o) => Some((i, o)),
                    _ => None,
                })
                .collect(),
            _ => {
                secao.erros.push(format!(
                    "{arquivo}: não é um objeto nem uma lista de objetos"
                ));
                continue;
            }
        };
        let base = arquivo
            .trim_start_matches(PASTA)
            .trim_end_matches(".json")
            .to_string();
        let varios = objetos.len() > 1;
        for (indice, objeto) in objetos {
            let reserva = if varios {
                format!("{base}#{indice}")
            } else {
                base.clone()
            };
            if importar_um(ctx, &mut secao, &arquivo, &objeto, &reserva)? {
                desconhecidos += 1;
            }
        }
    }
    if desconhecidos > 0 {
        secao.linhas.push(format!(
            "ATENÇÃO: {desconhecidos} goal(s) com estado desconhecido viraram 'proposto' (podem entrar em foco no heartbeat); revise com `abiyss goal list`"
        ));
    }
    Ok(secao)
}

/// Importa um goal. Devolve `true` se o estado era desconhecido.
fn importar_um(
    ctx: &mut Contexto<'_>,
    secao: &mut Secao,
    arquivo: &str,
    objeto: &Map<String, Value>,
    id_reserva: &str,
) -> anyhow::Result<bool> {
    let mut l = Leitor::novo(objeto);
    let id = l.texto("id", ID).unwrap_or_else(|| id_reserva.to_string());
    let titulo = l.texto("titulo", TITULO);
    let nucleo = l.texto("nucleo", NUCLEO);
    let descricao = l.texto("descricao", DESCRICAO).unwrap_or_default();
    let prioridade_bruta = l.valor("prioridade", PRIORIDADE);
    let estado_bruto = l.texto("estado", ESTADO);
    let criado = l.valor("criado", CRIADO).and_then(momento_de);
    secao.anotar(&l);
    let mut extras = l.extras();

    // Avisos só aparecem para goals novos (não a cada reimportação).
    let mut avisos = Vec::new();
    let prioridade = match prioridade_bruta {
        Some(v) => prioridade_de(v).unwrap_or_else(|| {
            avisos.push(format!(
                "! {id}: prioridade {v} não reconhecida; usei 0 (original em extras)"
            ));
            extras.insert("prioridade_original".into(), v.clone());
            0
        }),
        None => 0,
    };
    let titulo = titulo.unwrap_or_else(|| {
        avisos.push(format!("! {id}: sem título no Hermes; usei o ID"));
        format!("(goal do Hermes sem título: {id})")
    });
    let nucleo = match nucleo {
        Some(n) => n,
        None => {
            avisos.push(format!(
                "! {id}: sem critério de pronto; núcleo = descrição ou título"
            ));
            if descricao.is_empty() {
                titulo.clone()
            } else {
                descricao.chars().take(300).collect()
            }
        }
    };
    let (estado, motivo, desconhecido) = match estado_bruto.as_deref() {
        Some(original) => match estado_do_hermes(original) {
            Some(e) => (
                e,
                format!("importado do Hermes ({arquivo}); estado original '{original}' → {e}"),
                false,
            ),
            None => (
                EstadoGoal::Proposto,
                format!(
                    "importado do Hermes ({arquivo}); estado original '{original}' desconhecido → proposto"
                ),
                true,
            ),
        },
        None => (
            EstadoGoal::Proposto,
            format!("importado do Hermes ({arquivo}); sem estado na origem → proposto"),
            false,
        ),
    };

    let chave = format!("hermes:goal:{id}");
    ctx.goals_planejados.insert(id.clone());
    if importacoes::ja_importado(ctx.banco, &chave)? {
        secao.ja_importados += 1;
        secao
            .linhas
            .push(format!("= {id} \"{titulo}\" (já importado)"));
        return Ok(false);
    }
    secao.linhas.extend(avisos);
    let original = valor_como_texto(&Value::Object(objeto.clone()));
    if ctx.aplicar {
        let goal = GoalImportado {
            titulo: titulo.clone(),
            nucleo,
            descricao,
            prioridade,
            estado,
            motivo,
            criado_ms: criado.unwrap_or_else(agora_ms),
            extras: (!extras.is_empty()).then(|| Value::Object(extras).to_string()),
        };
        goals::importar(ctx.banco, &goal, &chave, &original)?;
    }
    secao.novos += 1;
    let de = estado_bruto.as_deref().unwrap_or("(sem estado)");
    let aviso = if desconhecido {
        " ⚠ desconhecido"
    } else {
        ""
    };
    secao
        .linhas
        .push(format!("+ {id} \"{titulo}\": '{de}' → {estado}{aviso}"));
    Ok(desconhecido)
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn estados_conhecidos_e_desconhecidos() {
        assert_eq!(estado_do_hermes("Active"), Some(EstadoGoal::Executando));
        assert_eq!(
            estado_do_hermes("in-progress"),
            Some(EstadoGoal::Executando)
        );
        assert_eq!(
            estado_do_hermes("Em andamento"),
            Some(EstadoGoal::Executando)
        );
        assert_eq!(estado_do_hermes("done"), Some(EstadoGoal::Concluido));
        assert_eq!(estado_do_hermes("concluído"), Some(EstadoGoal::Concluido));
        assert_eq!(estado_do_hermes("on hold"), Some(EstadoGoal::Bloqueado));
        assert_eq!(estado_do_hermes("cancelled"), Some(EstadoGoal::Abandonado));
        assert_eq!(estado_do_hermes("hibernando"), None);
        assert_eq!(estado_do_hermes("archived"), None);
        assert_eq!(prioridade_de(&json!("alta")), Some(2));
        assert_eq!(prioridade_de(&json!(2.6)), Some(3));
        assert_eq!(prioridade_de(&json!("??")), None);
    }
}
