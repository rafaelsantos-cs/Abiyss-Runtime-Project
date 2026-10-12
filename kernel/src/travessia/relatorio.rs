//! O relatório de várias execuções: taxa de sucesso e medianas por braço e
//! por fase (nunca uma execução isolada), com as fases de névoa separadas.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::metricas::MetricaFase;

/// Lê as métricas de todas as execuções (`<execucoes>/*/metricas.jsonl`).
pub fn ler_execucoes(pasta: &Path) -> anyhow::Result<Vec<MetricaFase>> {
    let mut todas = Vec::new();
    let Ok(entradas) = std::fs::read_dir(pasta) else {
        return Ok(todas);
    };
    let mut pastas: Vec<_> = entradas.flatten().map(|e| e.path()).collect();
    pastas.sort();
    for p in pastas {
        let arquivo = p.join("metricas.jsonl");
        let Ok(texto) = std::fs::read_to_string(&arquivo) else {
            continue;
        };
        for linha in texto.lines().filter(|l| !l.trim().is_empty()) {
            match serde_json::from_str::<MetricaFase>(linha) {
                Ok(m) => todas.push(m),
                Err(e) => tracing::warn!("linha ignorada em {}: {e}", arquivo.display()),
            }
        }
    }
    Ok(todas)
}

pub fn mediana(valores: &mut [f64]) -> Option<f64> {
    if valores.is_empty() {
        return None;
    }
    valores.sort_by(|a, b| a.total_cmp(b));
    let n = valores.len();
    Some(if n % 2 == 1 {
        valores[n / 2]
    } else {
        (valores[n / 2 - 1] + valores[n / 2]) / 2.0
    })
}

fn med<F: Fn(&MetricaFase) -> Option<f64>>(lista: &[&MetricaFase], f: F) -> String {
    let mut v: Vec<f64> = lista.iter().filter_map(|m| f(m)).collect();
    match mediana(&mut v) {
        Some(x) if x.fract() == 0.0 => format!("{x:.0}"),
        Some(x) => format!("{x:.1}"),
        None => "—".into(),
    }
}

fn porcento(parte: usize, todo: usize) -> String {
    if todo == 0 {
        "—".into()
    } else {
        format!("{:.0}%", parte as f64 * 100.0 / todo as f64)
    }
}

/// Uma linha da tabela (de uma fase ou de um grupo de fases).
fn linha(rotulo: &str, lista: &[&MetricaFase]) -> String {
    let jogadas: Vec<&MetricaFase> = lista
        .iter()
        .copied()
        .filter(|m| m.resultado != "nao_jogada")
        .collect();
    let concluidas: Vec<&MetricaFase> = lista.iter().copied().filter(|m| m.concluida).collect();
    let problemas: u32 = lista.iter().map(|m| m.subagentes_com_problema).sum();
    let r429: u32 = lista.iter().map(|m| m.respostas_429).sum();
    let acima: u32 = lista
        .iter()
        .flat_map(|m| m.uso.values())
        .map(|u| u.minutos_acima_de_90)
        .sum();
    let codigo = lista.iter().filter(|m| m.codigo_proprio).count();
    let travas = lista.iter().filter(|m| m.trava_do_harness).count();
    format!(
        "| {rotulo} | {} | {} | {} | {} | {} | {} | {} | {} | {problemas} | {} / {} | {r429} | {} / {} | {acima} | {} | {codigo} | {travas} |",
        lista.len(),
        porcento(concluidas.len(), lista.len()),
        med(&concluidas, |m| m.eficiencia.map(|e| e * 100.0)),
        med(&jogadas, |m| Some(m.jogadas as f64)),
        med(&jogadas, |m| Some(m.invalidas as f64)),
        med(&jogadas, |m| Some(m.reinicios as f64)),
        med(&jogadas, |m| Some(m.simulacoes as f64)),
        med(&jogadas, |m| Some(m.delegacoes as f64)),
        med(&jogadas, |m| m.espera_cerebro_p50_ms.map(|v| v as f64)),
        med(&jogadas, |m| m.espera_cerebro_p95_ms.map(|v| v as f64)),
        med(&jogadas, |m| m
            .uso
            .get("cerebro")
            .map(|u| u.pico_por_minuto as f64)),
        med(&jogadas, |m| m
            .uso
            .get("subagentes")
            .map(|u| u.pico_por_minuto as f64)),
        med(&jogadas, |m| Some(m.tempo_s)),
    )
}

const CABECALHO: &str = "| fase | n | sucesso | eficiência % | jogadas | inválidas | reinícios | simulações | delegações | sub-agentes c/ problema | espera do cérebro p50 / p95 (ms) | 429 | pico RPM cérebro / sub | min > 90% | tempo (s) | código próprio | travas |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|";

/// Quantas fases seguidas, desde a primeira, a execução concluiu.
fn alcance(execucao: &[&MetricaFase]) -> u32 {
    let mut ordenadas = execucao.to_vec();
    ordenadas.sort_by_key(|m| m.fase);
    ordenadas.iter().take_while(|m| m.concluida).count() as u32
}

/// O relatório em Markdown.
pub fn gerar(metricas: &[MetricaFase]) -> String {
    let mut s = String::from("# Travessia — relatório\n\n");
    if metricas.is_empty() {
        s.push_str("Nenhuma execução encontrada.\n");
        return s;
    }
    s.push_str(
        "Medianas sobre as execuções (eficiência = ótimo ÷ jogadas, só nas fases concluídas; as \
         outras colunas, nas fases jogadas). n = execuções que chegaram à fase.\n",
    );
    let bracos: BTreeSet<&str> = metricas.iter().map(|m| m.braco.as_str()).collect();
    for braco in bracos {
        let deste: Vec<&MetricaFase> = metricas.iter().filter(|m| m.braco == braco).collect();
        let execucoes: BTreeMap<&str, Vec<&MetricaFase>> =
            deste.iter().fold(BTreeMap::new(), |mut mapa, m| {
                mapa.entry(m.execucao.as_str())
                    .or_insert_with(Vec::new)
                    .push(*m);
                mapa
            });
        let seeds: BTreeSet<&str> = deste.iter().map(|m| m.seed.as_str()).collect();
        let modos: BTreeSet<&str> = deste.iter().map(|m| m.modo.as_str()).collect();
        s.push_str(&format!(
            "\n## Braço {braco}\n\n{} execução(ões), {} seed(s), modo {}.\n\n{CABECALHO}\n",
            execucoes.len(),
            seeds.len(),
            modos.into_iter().collect::<Vec<_>>().join(", ")
        ));
        let fases: BTreeSet<u32> = deste.iter().map(|m| m.fase).collect();
        for fase in &fases {
            let lista: Vec<&MetricaFase> =
                deste.iter().copied().filter(|m| m.fase == *fase).collect();
            let rotulo = if lista.first().is_some_and(|m| m.nevoa) {
                format!("{fase} (névoa)")
            } else {
                fase.to_string()
            };
            s.push_str(&linha(&rotulo, &lista));
            s.push('\n');
        }
        let sem: Vec<&MetricaFase> = deste.iter().copied().filter(|m| !m.nevoa).collect();
        let com: Vec<&MetricaFase> = deste.iter().copied().filter(|m| m.nevoa).collect();
        if !sem.is_empty() {
            s.push_str(&linha("**sem névoa**", &sem));
            s.push('\n');
        }
        if !com.is_empty() {
            s.push_str(&linha("**com névoa**", &com));
            s.push('\n');
        }
        let mut concluidas: Vec<f64> = execucoes
            .values()
            .map(|e| e.iter().filter(|m| m.concluida).count() as f64)
            .collect();
        let mut alcances: Vec<f64> = execucoes.values().map(|e| alcance(e) as f64).collect();
        s.push_str(&format!(
            "\nFases concluídas por execução (mediana): {}. Alcance (fases seguidas desde a 1ª, a nota \
             do modo campanha): {}.\n",
            mediana(&mut concluidas).map(|x| format!("{x:.1}")).unwrap_or_else(|| "—".into()),
            mediana(&mut alcances).map(|x| format!("{x:.1}")).unwrap_or_else(|| "—".into()),
        ));
    }
    s
}

#[cfg(test)]
mod testes {
    use super::*;

    fn m(
        execucao: &str,
        braco: &str,
        fase: u32,
        concluida: bool,
        jogadas: u32,
        otimo: u32,
    ) -> MetricaFase {
        MetricaFase {
            execucao: execucao.into(),
            braco: braco.into(),
            modo: "avaliacao".into(),
            seed: "s".into(),
            numero_execucao: 1,
            fase,
            nevoa: fase >= 8,
            resultado: if concluida { "concluida" } else { "falha" }.into(),
            concluida,
            motivo: String::new(),
            otimo,
            jogadas,
            eficiencia: concluida.then(|| otimo as f64 / jogadas as f64),
            invalidas: 0,
            reinicios: 0,
            simulacoes: 2,
            tempo_s: 10.0,
            chamadas_cerebro: 5,
            delegacoes: 1,
            delegacoes_por_nivel: BTreeMap::new(),
            delegacoes_por_esforco: BTreeMap::new(),
            subagentes_com_problema: 0,
            uso: BTreeMap::new(),
            espera_cerebro_p50_ms: Some(100),
            espera_cerebro_p95_ms: Some(900),
            respostas_429: 1,
            codigo_proprio: false,
            codigo_proprio_quem: vec![],
            trava_do_harness: false,
        }
    }

    #[test]
    fn mediana_par_e_impar() {
        assert_eq!(mediana(&mut [3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(mediana(&mut [4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(mediana(&mut []), None);
    }

    #[test]
    fn relatorio_por_braco_com_nevoa_separada() {
        let dados = vec![
            m("e1", "A", 1, true, 10, 8),
            m("e1", "A", 8, false, 50, 40),
            m("e2", "A", 1, true, 16, 8),
            m("e2", "A", 8, true, 40, 40),
            m("e3", "B", 1, false, 30, 8),
        ];
        let r = gerar(&dados);
        assert!(r.contains("## Braço A") && r.contains("## Braço B"));
        // Fase 1 do braço A: 2 execuções, 100% de sucesso, eficiência 80% e 50% → mediana 65%.
        assert!(r.contains("| 1 | 2 | 100% | 65 |"), "{r}");
        assert!(r.contains("| 8 (névoa) | 2 | 50% | 100 |"), "{r}");
        assert!(r.contains("**com névoa**"));
        assert!(
            r.contains("Alcance (fases seguidas desde a 1ª, a nota do modo campanha): 1.5."),
            "{r}"
        );
    }
}
