//! Métricas por fase e a linha do tempo de uma execução.
//!
//! Fontes: as métricas do jogo (`jogo.jsonl`, escritas pelo servidor), o
//! gabarito (o ótimo; só o harness lê), o banco da instância
//! (`chamadas_modelo`, `subagentes`) e os eventos que o harness anotou
//! (ferramentas do cérebro e dos sub-agentes, travas).
//!
//! Cada fase cobre o intervalo desde o fim da fase anterior (ou o começo da
//! execução) até o fim dela: assim toda chamada ao modelo cai em alguma fase,
//! inclusive a que pediu `proxima_fase`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Gabarito;
use super::instancia::Instancia;
use crate::db::Banco;

/// Uso de um pool de chamadas numa fase.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsoPool {
    /// Tentativas registradas (inclui 429 e erros).
    pub chamadas: u32,
    /// Chamadas por minuto, na média da fase.
    pub media_por_minuto: f64,
    /// Maior número de chamadas em qualquer janela de 60 s.
    pub pico_por_minuto: u32,
    /// Minutos (de 60 s, contados do começo da fase) com pelo menos 90% do limite.
    pub minutos_acima_de_90: u32,
    /// O limite configurado do pool.
    pub limite_por_minuto: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricaFase {
    pub execucao: String,
    pub braco: String,
    pub modo: String,
    pub seed: String,
    pub numero_execucao: u32,
    pub fase: u32,
    pub nevoa: bool,
    /// `concluida`, `falha`, `desistencia` ou `nao_jogada`.
    pub resultado: String,
    pub concluida: bool,
    /// Na falha: `jogadas`, `reinicios`, `tempo`; `desistencia`; `trava_do_harness`.
    pub motivo: String,
    pub otimo: u32,
    pub jogadas: u32,
    /// ótimo ÷ jogadas, só nas fases concluídas.
    pub eficiencia: Option<f64>,
    pub invalidas: u32,
    pub reinicios: u32,
    pub simulacoes: u32,
    pub tempo_s: f64,
    pub chamadas_cerebro: u32,
    pub delegacoes: u32,
    pub delegacoes_por_nivel: BTreeMap<String, u32>,
    pub delegacoes_por_esforco: BTreeMap<String, u32>,
    pub subagentes_com_problema: u32,
    pub uso: BTreeMap<String, UsoPool>,
    pub espera_cerebro_p50_ms: Option<i64>,
    pub espera_cerebro_p95_ms: Option<i64>,
    pub respostas_429: u32,
    /// Alguém rodou código no terminal nesta fase (ex.: um solucionador)?
    pub codigo_proprio: bool,
    /// Quem: "cerebro" ou "subagente:<id>", sem repetição.
    pub codigo_proprio_quem: Vec<String>,
    pub trava_do_harness: bool,
}

/// Percentil pelo posto mais próximo (lista já ordenada).
pub fn percentil(ordenados: &[i64], p: f64) -> Option<i64> {
    if ordenados.is_empty() {
        return None;
    }
    let posto = ((p / 100.0) * ordenados.len() as f64).ceil() as usize;
    Some(ordenados[posto.clamp(1, ordenados.len()) - 1])
}

/// Uso de um pool a partir dos instantes (ms) das tentativas, numa janela.
pub fn uso_do_pool(mut instantes: Vec<i64>, inicio: i64, fim: i64, limite: u32) -> UsoPool {
    instantes.sort_unstable();
    let chamadas = instantes.len() as u32;
    let minutos = ((fim - inicio).max(1) as f64 / 60_000.0).max(1.0 / 60.0);
    let mut pico = 0u32;
    let mut j = 0;
    for i in 0..instantes.len() {
        while instantes[i] - instantes[j] >= 60_000 {
            j += 1;
        }
        pico = pico.max((i - j + 1) as u32);
    }
    let corte = ((limite as f64) * 0.9).ceil() as u32;
    let mut por_minuto: BTreeMap<i64, u32> = BTreeMap::new();
    for t in &instantes {
        *por_minuto.entry((t - inicio).max(0) / 60_000).or_default() += 1;
    }
    let minutos_acima_de_90 = por_minuto
        .values()
        .filter(|n| corte > 0 && **n >= corte)
        .count() as u32;
    UsoPool {
        chamadas,
        media_por_minuto: (chamadas as f64 / minutos * 100.0).round() / 100.0,
        pico_por_minuto: pico,
        minutos_acima_de_90,
        limite_por_minuto: limite,
    }
}

struct Chamada {
    momento: i64,
    pool: String,
    origem: String,
    modelo: String,
    status: String,
    espera: Option<i64>,
    duracao: i64,
    nivel_esforco: Option<String>,
    entrada: i64,
    saida: i64,
}

struct Subagente {
    id: i64,
    nivel: String,
    esforco: Option<String>,
    estado: String,
    criado: i64,
    terminado: Option<i64>,
    relatorio: Option<Value>,
    tarefa: String,
    tokens: i64,
    rodadas: i64,
}

fn ler_chamadas(banco: &Banco) -> anyhow::Result<Vec<Chamada>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT momento_ms, pool, origem, modelo, status, espera_ms, duracao_ms, nivel_esforco,
                tokens_entrada, tokens_saida
         FROM chamadas_modelo ORDER BY momento_ms",
    )?;
    let lista = consulta
        .query_map([], |l| {
            Ok(Chamada {
                momento: l.get(0)?,
                pool: l.get(1)?,
                origem: l.get(2)?,
                modelo: l.get(3)?,
                status: l.get(4)?,
                espera: l.get(5)?,
                duracao: l.get(6)?,
                nivel_esforco: l.get(7)?,
                entrada: l.get(8)?,
                saida: l.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lista)
}

fn ler_subagentes(banco: &Banco) -> anyhow::Result<Vec<Subagente>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT id, nivel, esforco, estado, criado_ms, terminado_ms, relatorio, tarefa, tokens, rodadas
         FROM subagentes ORDER BY id",
    )?;
    let lista = consulta
        .query_map([], |l| {
            let relatorio: Option<String> = l.get(6)?;
            Ok(Subagente {
                id: l.get(0)?,
                nivel: l.get(1)?,
                esforco: l.get(2)?,
                estado: l.get(3)?,
                criado: l.get(4)?,
                terminado: l.get(5)?,
                relatorio: relatorio.and_then(|t| serde_json::from_str(&t).ok()),
                tarefa: l.get(7)?,
                tokens: l.get(8)?,
                rodadas: l.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lista)
}

/// Um sub-agente "com problema": falhou, expirou, foi cancelado, ou o
/// relatório diz que falhou ou não veio no formato.
fn com_problema(s: &Subagente) -> bool {
    if matches!(s.estado.as_str(), "falhou" | "expirado" | "cancelado") {
        return true;
    }
    match &s.relatorio {
        Some(r) => {
            r["status"] == "falhou"
                || r["duvidas"].as_array().is_some_and(|d| {
                    d.iter()
                        .any(|x| x.as_str().is_some_and(|t| t.contains("formato JSON")))
                })
        }
        None => s.estado == "concluido",
    }
}

fn ler_jsonl(caminho: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(caminho)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Calcula e grava as métricas por fase, a linha do tempo e o resumo.
pub fn fechar(
    instancia: &Instancia,
    banco: &Banco,
    gabaritos: &[Gabarito],
    eventos: &[Value],
    motivo_fim: &str,
) -> anyhow::Result<Vec<MetricaFase>> {
    let arquivos = &instancia.arquivos;
    let config = &instancia.config;
    let jogo: BTreeMap<u32, Value> = ler_jsonl(&arquivos.jogo)
        .into_iter()
        .filter(|l| l["tipo"] == "fase")
        .filter_map(|l| Some((l["numero"].as_u64()? as u32, l)))
        .collect();
    let execucao: Value =
        serde_json::from_str(&std::fs::read_to_string(&arquivos.execucao)?).unwrap_or(json!({}));
    let catalogo = std::path::PathBuf::from(execucao["catalogo"].as_str().unwrap_or(""));
    let chamadas = ler_chamadas(banco)?;
    let subagentes = ler_subagentes(banco)?;
    let travadas: Vec<u32> = eventos
        .iter()
        .filter(|e| e["tipo"] == "fase" && e["evento"] == "trava_do_harness")
        .filter_map(|e| e["numero"].as_u64().map(|n| n as u32))
        .collect();
    let limites: BTreeMap<String, u32> = BTreeMap::from([
        (
            "cerebro".to_string(),
            config.pools.cerebro.requisicoes_por_minuto,
        ),
        (
            "subagentes".to_string(),
            config.pools.subagentes.requisicoes_por_minuto,
        ),
    ]);

    let mut metricas = Vec::new();
    let mut anterior_fim: i64 = chamadas.first().map(|c| c.momento - 1).unwrap_or(0);
    for g in gabaritos {
        let nevoa = std::fs::read_to_string(catalogo.join(super::arquivo_fase(g.numero)))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|d| d["nevoa"].as_bool())
            .unwrap_or(false);
        let linha = jogo.get(&g.numero);
        let (inicio, fim) = match linha {
            Some(l) => (anterior_fim, l["fim_ms"].as_i64().unwrap_or(anterior_fim)),
            None => (anterior_fim, anterior_fim),
        };
        let dentro = |t: i64| linha.is_some() && t > inicio && t <= fim;
        let mut m = MetricaFase {
            execucao: instancia.id.clone(),
            braco: instancia.opcoes.rotulo_braco(),
            modo: instancia.opcoes.modo.como_texto().into(),
            seed: instancia.opcoes.seed.clone(),
            numero_execucao: instancia.opcoes.numero,
            fase: g.numero,
            nevoa,
            resultado: "nao_jogada".into(),
            concluida: false,
            motivo: String::new(),
            otimo: g.otimo,
            jogadas: 0,
            eficiencia: None,
            invalidas: 0,
            reinicios: 0,
            simulacoes: 0,
            tempo_s: 0.0,
            chamadas_cerebro: 0,
            delegacoes: 0,
            delegacoes_por_nivel: BTreeMap::new(),
            delegacoes_por_esforco: BTreeMap::new(),
            subagentes_com_problema: 0,
            uso: BTreeMap::new(),
            espera_cerebro_p50_ms: None,
            espera_cerebro_p95_ms: None,
            respostas_429: 0,
            codigo_proprio: false,
            codigo_proprio_quem: Vec::new(),
            trava_do_harness: travadas.contains(&g.numero),
        };
        if let Some(l) = linha {
            let num = |k: &str| l[k].as_u64().unwrap_or(0) as u32;
            m.resultado = l["resultado"].as_str().unwrap_or("").to_string();
            m.concluida = m.resultado == "concluida";
            m.motivo = if m.trava_do_harness {
                "trava_do_harness".into()
            } else {
                l["motivo"].as_str().unwrap_or("").to_string()
            };
            m.jogadas = num("jogadas");
            m.invalidas = num("invalidas");
            m.reinicios = num("reinicios");
            m.simulacoes = num("simulacoes");
            m.tempo_s = l["tempo_s"].as_f64().unwrap_or(0.0);
            if m.concluida && m.jogadas > 0 {
                m.eficiencia =
                    Some(((g.otimo as f64 / m.jogadas as f64) * 1000.0).round() / 1000.0);
            }
            for (pool, limite) in &limites {
                let instantes: Vec<i64> = chamadas
                    .iter()
                    .filter(|c| &c.pool == pool && dentro(c.momento))
                    .map(|c| c.momento)
                    .collect();
                m.uso
                    .insert(pool.clone(), uso_do_pool(instantes, inicio, fim, *limite));
            }
            m.chamadas_cerebro = m.uso.get("cerebro").map(|u| u.chamadas).unwrap_or(0);
            let mut esperas: Vec<i64> = chamadas
                .iter()
                .filter(|c| c.pool == "cerebro" && dentro(c.momento))
                .filter_map(|c| c.espera)
                .collect();
            esperas.sort_unstable();
            m.espera_cerebro_p50_ms = percentil(&esperas, 50.0);
            m.espera_cerebro_p95_ms = percentil(&esperas, 95.0);
            m.respostas_429 = chamadas
                .iter()
                .filter(|c| c.status == "limite" && dentro(c.momento))
                .count() as u32;
            for s in subagentes.iter().filter(|s| dentro(s.criado)) {
                m.delegacoes += 1;
                *m.delegacoes_por_nivel.entry(s.nivel.clone()).or_default() += 1;
                *m.delegacoes_por_esforco
                    .entry(s.esforco.clone().unwrap_or_else(|| "padrao".into()))
                    .or_default() += 1;
                if com_problema(s) {
                    m.subagentes_com_problema += 1;
                }
            }
            for e in eventos.iter().filter(|e| {
                e["tipo"] == "ferramenta"
                    && e["nome"]
                        .as_str()
                        .is_some_and(|n| n.starts_with("terminal__"))
                    && dentro(e["t"].as_i64().unwrap_or(0))
            }) {
                m.codigo_proprio = true;
                let quem = e["quem"].as_str().unwrap_or("?").to_string();
                if !m.codigo_proprio_quem.contains(&quem) {
                    m.codigo_proprio_quem.push(quem);
                }
            }
            anterior_fim = fim;
        }
        metricas.push(m);
    }

    // Arquivos: métricas, linha do tempo e resumo.
    let mut texto = String::new();
    for m in &metricas {
        texto.push_str(&serde_json::to_string(m)?);
        texto.push('\n');
    }
    std::fs::write(&arquivos.metricas, texto)?;

    let mut linha_do_tempo: Vec<Value> = eventos.to_vec();
    for c in &chamadas {
        linha_do_tempo.push(json!({
            "tipo": "chamada_modelo", "t": c.momento, "pool": c.pool, "origem": c.origem,
            "modelo": c.modelo, "nivel_esforco": c.nivel_esforco, "status": c.status,
            "duracao_ms": c.duracao, "espera_ms": c.espera, "tokens_entrada": c.entrada,
            "tokens_saida": c.saida,
        }));
    }
    for s in &subagentes {
        let tarefa: String = s.tarefa.chars().take(200).collect();
        linha_do_tempo.push(json!({
            "tipo": "delegacao", "t": s.criado, "id": s.id, "nivel": s.nivel,
            "esforco": s.esforco, "tarefa": tarefa,
        }));
        if let Some(t) = s.terminado {
            linha_do_tempo.push(json!({
                "tipo": "subagente_fim", "t": t, "id": s.id, "estado": s.estado,
                "status": s.relatorio.as_ref().map(|r| r["status"].clone()),
                "tokens": s.tokens, "rodadas": s.rodadas,
            }));
        }
    }
    for (numero, l) in &jogo {
        linha_do_tempo.push(
            json!({"tipo": "fase", "t": l["fim_ms"], "numero": numero, "evento": "fim",
                                   "resultado": l["resultado"]}),
        );
    }
    linha_do_tempo.sort_by_key(|e| e["t"].as_i64().unwrap_or(0));
    let mut texto = String::new();
    for e in &linha_do_tempo {
        texto.push_str(&serde_json::to_string(e)?);
        texto.push('\n');
    }
    std::fs::write(&arquivos.linha_do_tempo, texto)?;
    std::fs::write(&arquivos.resumo, resumo(instancia, &metricas, motivo_fim))?;
    Ok(metricas)
}

/// O resumo em texto de uma execução.
pub fn resumo(instancia: &Instancia, metricas: &[MetricaFase], motivo_fim: &str) -> String {
    let mut s = format!(
        "Execução {} · braço {} · modo {} · seed {} · nº {}\nfim: {motivo_fim}\n\n",
        instancia.id,
        instancia.opcoes.rotulo_braco(),
        instancia.opcoes.modo.como_texto(),
        instancia.opcoes.seed,
        instancia.opcoes.numero
    );
    for m in metricas {
        let eficiencia = m
            .eficiencia
            .map(|e| format!("{:.0}%", e * 100.0))
            .unwrap_or_else(|| "—".into());
        s.push_str(&format!(
            "fase {:2}{}: {:11} {:16} jogadas {:3} (ótimo {:3}, eficiência {:>4}) inválidas {:2} \
             reinícios {} simulações {:3} delegações {:2} chamadas {:3} {:.0} s\n",
            m.fase,
            if m.nevoa { "*" } else { " " },
            m.resultado,
            if m.motivo.is_empty() {
                ""
            } else {
                m.motivo.as_str()
            },
            m.jogadas,
            m.otimo,
            eficiencia,
            m.invalidas,
            m.reinicios,
            m.simulacoes,
            m.delegacoes,
            m.chamadas_cerebro,
            m.tempo_s
        ));
    }
    let concluidas = metricas.iter().filter(|m| m.concluida).count();
    s.push_str(&format!(
        "\n{concluidas} de {} fase(s) concluída(s). * = fase com névoa.\n",
        metricas.len()
    ));
    s
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn percentis_pelo_posto_mais_proximo() {
        let v = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        assert_eq!(percentil(&v, 50.0), Some(50));
        assert_eq!(percentil(&v, 95.0), Some(100));
        assert_eq!(percentil(&[7], 95.0), Some(7));
        assert_eq!(percentil(&[], 50.0), None);
    }

    #[test]
    fn uso_do_pool_media_pico_e_minutos_cheios() {
        // 40 chamadas no primeiro minuto (limite 40), 5 no segundo.
        let mut t: Vec<i64> = (0..40).map(|i| 1_000 + i * 1_000).collect();
        t.extend((0..5).map(|i| 61_000 + i * 10_000));
        let u = uso_do_pool(t, 0, 120_000, 40);
        assert_eq!(u.chamadas, 45);
        assert_eq!(u.media_por_minuto, 22.5);
        assert_eq!(u.pico_por_minuto, 40);
        assert_eq!(u.minutos_acima_de_90, 1);
        let vazio = uso_do_pool(vec![], 0, 1, 40);
        assert_eq!((vazio.chamadas, vazio.pico_por_minuto), (0, 0));
    }
}
