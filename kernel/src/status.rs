//! `abiyss status`: uma foto do estado do Abiyss, montada só com código
//! (nenhuma chamada ao modelo, nenhuma chave necessária).

use std::fmt::Write as _;

use crate::config::Config;
use crate::cron;
use crate::daemon;
use crate::db::Banco;
use crate::eventos;
use crate::goals;
use crate::heartbeat;
use crate::identidade::Identidade;
use crate::interocepcao::Interocepcao;
use crate::latencia;
use crate::memoria::central::MemoriaCentral;
use crate::memoria::propostas;
use crate::sono;
use crate::tempo::{agora_ms, formatar_ms};
use crate::vigilancia;

/// Resultado de `abiyss status --verificar`, para monitor externo
/// (timer do systemd, healthcheck): código de saída + uma linha.
#[derive(Debug, Clone, PartialEq)]
pub struct Verificacao {
    /// 0 = tudo ok; 1 = degradado; 2 = daemon parado.
    pub codigo: i32,
    pub linha: String,
}

/// O que a verificação olha. Separado para os testes não dependerem de
/// relógio nem de processo rodando.
#[derive(Debug, Clone, Default)]
pub struct EntradaVerificacao {
    pub rodando: bool,
    pub sinal_de_vida_ms: Option<i64>,
    pub agora_ms: i64,
    pub cron_verificacao_segundos: u64,
    /// Outros problemas já detectados (disjuntor aberto, último sono falhou...).
    pub problemas: Vec<String>,
}

/// Decide o código de saída. Sinal de vida mais velho que 3 verificações
/// de cron = o loop principal não está andando.
pub fn avaliar(entrada: &EntradaVerificacao) -> Verificacao {
    if !entrada.rodando {
        return Verificacao {
            codigo: 2,
            linha: "PARADO: o daemon do Abiyss não está rodando".into(),
        };
    }
    let mut problemas = Vec::new();
    let limite_ms = entrada.cron_verificacao_segundos as i64 * 3 * 1000;
    match entrada.sinal_de_vida_ms {
        None => problemas.push("nenhum sinal de vida registrado".to_string()),
        Some(ms) if entrada.agora_ms - ms > limite_ms => problemas.push(format!(
            "sinal de vida atrasado ({} s; limite {} s)",
            (entrada.agora_ms - ms) / 1000,
            limite_ms / 1000
        )),
        Some(_) => {}
    }
    problemas.extend(entrada.problemas.iter().cloned());
    if problemas.is_empty() {
        Verificacao {
            codigo: 0,
            linha: "OK: daemon rodando, sinal de vida em dia".into(),
        }
    } else {
        Verificacao {
            codigo: 1,
            linha: format!("DEGRADADO: {}", problemas.join("; ")),
        }
    }
}

/// `abiyss status --verificar`: só código, nenhuma chamada ao modelo.
pub fn verificar(config: &Config, banco: &Banco) -> anyhow::Result<Verificacao> {
    let sinal =
        daemon::ler_estado(banco, daemon::CHAVE_SINAL_DE_VIDA)?.and_then(|v| v.parse::<i64>().ok());
    Ok(avaliar(&EntradaVerificacao {
        rodando: daemon::esta_rodando(config),
        sinal_de_vida_ms: sinal,
        agora_ms: agora_ms(),
        cron_verificacao_segundos: config.daemon.cron_verificacao_segundos,
        problemas: problemas_conhecidos(config, banco, agora_ms())?,
    }))
}

/// Sem sono há mais que isto (com o sono ligado e já tendo dormido uma
/// vez) = degradado.
const SONO_ATRASADO_MS: i64 = 50 * 3_600_000;

/// Problemas registrados por outras partes do kernel.
fn problemas_conhecidos(config: &Config, banco: &Banco, agora: i64) -> anyhow::Result<Vec<String>> {
    let mut problemas = Vec::new();
    let disjuntor = vigilancia::ler_disjuntor(banco)?;
    if disjuntor.aberto_ate_ms.is_some()
        && let Some(d) = vigilancia::descrever_disjuntor(&disjuntor, agora)
    {
        problemas.push(format!("disjuntor do heartbeat {d}"));
    }
    if let Some(s) = sono::ultimo(banco)? {
        if s.estado == "falhou" {
            problemas.push(format!(
                "o último sono (revisão de {}) falhou: {}",
                s.dia,
                s.erro.as_deref().unwrap_or("sem detalhe")
            ));
        } else if config.sono.ativo
            && s.estado != "rodando"
            && agora - s.fim_ms.unwrap_or(s.inicio_ms) > SONO_ATRASADO_MS
        {
            problemas.push(format!(
                "sem dormir há {} h",
                (agora - s.fim_ms.unwrap_or(s.inicio_ms)) / 3_600_000
            ));
        }
    }
    Ok(problemas)
}

/// Monta o relatório em texto.
pub fn relatorio(config: &Config, banco: &Banco) -> anyhow::Result<String> {
    let mut t = String::new();
    let agora = agora_ms();

    // Daemon
    let rodando = daemon::esta_rodando(config);
    let ler = |chave: &str| daemon::ler_estado(banco, chave).ok().flatten();
    writeln!(t, "Daemon: {}", if rodando { "RODANDO" } else { "parado" })?;
    if let Some(pid) = ler(daemon::CHAVE_PID) {
        writeln!(t, "  pid (última execução): {pid}")?;
    }
    if let Some(ms) = ler(daemon::CHAVE_INICIADO).and_then(|v| v.parse::<i64>().ok()) {
        writeln!(t, "  iniciado em: {}", formatar_ms(ms))?;
    }
    if let Some(ms) = ler(daemon::CHAVE_SINAL_DE_VIDA).and_then(|v| v.parse::<i64>().ok()) {
        writeln!(
            t,
            "  último sinal de vida: {} ({} s atrás)",
            formatar_ms(ms),
            (agora - ms) / 1000
        )?;
    }

    if let Some(ms) = ler(daemon::CHAVE_MANUTENCAO).and_then(|v| v.parse::<i64>().ok()) {
        writeln!(
            t,
            "  última manutenção do banco: {} — {}",
            formatar_ms(ms),
            ler(daemon::CHAVE_MANUTENCAO_RESUMO).unwrap_or_default()
        )?;
    }

    // Identidade
    let identidade = Identidade::ler(&config.caminho_identidade());
    if !identidade.encontrado {
        writeln!(
            t,
            "Identidade: AUSENTE ({})",
            config.caminho_identidade().display()
        )?;
    } else if identidade.placeholders > 0 {
        writeln!(
            t,
            "Identidade: {} trecho(s) {{{{PREENCHER}}}} pendente(s) em {}",
            identidade.placeholders,
            config.caminho_identidade().display()
        )?;
    } else {
        writeln!(t, "Identidade: ok")?;
    }

    // Memória central e propostas
    let central = MemoriaCentral::da_config(config);
    let uso = central.uso();
    writeln!(
        t,
        "Memória central: {uso}/{} caracteres{}",
        central.limite(),
        if uso > central.limite() {
            " — ACIMA DO ORÇAMENTO (vai inteira para o prompt; corrija o arquivo)"
        } else {
            ""
        }
    )?;
    writeln!(
        t,
        "Propostas de memória pendentes: {} (aplique com `abiyss sleep`)",
        propostas::pendentes(banco)?.len()
    )?;

    // Sono
    match sono::ultimo(banco)? {
        None => writeln!(
            t,
            "Último sono: nenhum ainda{}",
            if config.sono.ativo {
                format!(" (janela às {})", config.sono.inicio)
            } else {
                " (sono automático desligado)".to_string()
            }
        )?,
        Some(s) => {
            writeln!(
                t,
                "Último sono: revisão de {} — {} ({}, {}), {} chamada(s); {}",
                s.dia,
                s.estado,
                s.gatilho,
                formatar_ms(s.inicio_ms),
                s.chamadas,
                s.resumo.as_deref().unwrap_or("sem resumo")
            )?;
            if let Some(e) = s.erro {
                writeln!(t, "  problemas: {e}")?;
            }
        }
    }
    // Vigilância: disjuntor e estagnação.
    let disjuntor = vigilancia::ler_disjuntor(banco)?;
    writeln!(
        t,
        "Disjuntor do heartbeat: {}",
        vigilancia::descrever_disjuntor(&disjuntor, agora).unwrap_or_else(|| "fechado".into())
    )?;
    if let Some(e) = vigilancia::ler_estagnacao(banco)? {
        let ativa = match e.goal_id {
            Some(id) => goals::obter(banco, id)
                .map(|g| g.atualizado_ms == e.goal_atualizado_ms)
                .unwrap_or(false),
            None => false,
        };
        if ativa {
            writeln!(
                t,
                "Estagnação: goal #{} parado; revisão periódica a cada {}× o intervalo",
                e.goal_id.unwrap_or_default(),
                e.multiplicador
            )?;
        }
    }
    for p in problemas_conhecidos(config, banco, agora)? {
        writeln!(t, "ATENÇÃO: {p}")?;
    }

    // Goals
    let contagem = goals::contar_por_estado(banco)?;
    let partes: Vec<String> = contagem
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(e, n)| format!("{e}: {n}"))
        .collect();
    writeln!(
        t,
        "Goals: {}",
        if partes.is_empty() {
            "nenhum".to_string()
        } else {
            partes.join(", ")
        }
    )?;
    if let Some(g) = goals::em_foco(&goals::listar(banco, false)?) {
        writeln!(t, "  em foco: #{} [{}] {}", g.id, g.estado, g.titulo)?;
    }

    // Fila e crons
    writeln!(
        t,
        "Eventos pendentes na fila: {}",
        eventos::contar_pendentes(banco)?
    )?;
    let crons = cron::listar(banco)?;
    writeln!(t, "Crons: {}", crons.len())?;
    for c in crons.iter().filter(|c| c.ativo).take(5) {
        writeln!(
            t,
            "  {} [{}] próximo: {}",
            c.nome,
            c.expressao,
            formatar_ms(c.proximo_ms)
        )?;
    }

    // Sub-agentes
    let vivos = crate::subagentes::ativos(banco)?;
    writeln!(t, "Sub-agentes ativos: {}", vivos.len())?;
    for s in vivos.iter().take(5) {
        writeln!(
            t,
            "  #{} [{}] {}: {}",
            s.id,
            s.nivel.como_texto(),
            s.estado.como_texto(),
            s.tarefa
        )?;
    }

    // Último ciclo
    match heartbeat::ultimo_ciclo(banco, false)? {
        None => writeln!(t, "Último ciclo: nenhum ainda")?,
        Some(c) => {
            writeln!(
                t,
                "Último ciclo: {} — {} ({})",
                formatar_ms(c.inicio_ms),
                if c.chamou_modelo {
                    "chamou o modelo"
                } else {
                    "sem chamada"
                },
                c.motivo
            )?;
            if let Some(e) = c.erro {
                writeln!(t, "  erro: {e}")?;
            }
        }
    }

    // Latência das chamadas ao modelo (últimas 24 h, só tentativas com sucesso).
    let latencias = latencia::resumo_desde(banco, agora - latencia::JANELA_24H_MS)?;
    if latencias.is_empty() {
        writeln!(t, "Latência (24 h): nenhuma chamada ao modelo")?;
    } else {
        writeln!(t, "Latência (24 h, p50 / p95):")?;
        for l in &latencias {
            writeln!(
                t,
                "  {} [{}]: 1º token {} / {}; total {} / {} ({} ok, {} falha(s))",
                l.modelo,
                l.pool,
                latencia::formatar(l.primeiro_token.p50),
                latencia::formatar(l.primeiro_token.p95),
                latencia::formatar(l.total.p50),
                latencia::formatar(l.total.p95),
                l.sucessos,
                l.falhas
            )?;
        }
    }

    // Interocepção (a mesma que vai para o contexto do modelo).
    writeln!(t, "\nInterocepção:")?;
    let corpo = Interocepcao::medir(config, banco)?;
    for linha in corpo.como_texto().lines() {
        writeln!(t, "  {linha}")?;
    }
    Ok(t)
}

#[cfg(test)]
mod testes {
    use super::*;

    fn entrada(rodando: bool, sinal: Option<i64>) -> EntradaVerificacao {
        EntradaVerificacao {
            rodando,
            sinal_de_vida_ms: sinal,
            agora_ms: 100_000,
            cron_verificacao_segundos: 10,
            problemas: vec![],
        }
    }

    #[test]
    fn codigos_de_saida_da_verificacao() {
        assert_eq!(avaliar(&entrada(false, Some(100_000))).codigo, 2);
        assert_eq!(avaliar(&entrada(true, Some(95_000))).codigo, 0);
        // 3 × 10 s = 30 s de tolerância.
        assert_eq!(avaliar(&entrada(true, Some(70_000))).codigo, 0);
        let atrasado = avaliar(&entrada(true, Some(69_000)));
        assert_eq!(atrasado.codigo, 1);
        assert!(atrasado.linha.contains("sinal de vida atrasado"));
        assert_eq!(avaliar(&entrada(true, None)).codigo, 1);
        let mut com_problema = entrada(true, Some(99_000));
        com_problema.problemas.push("disjuntor aberto".into());
        let v = avaliar(&com_problema);
        assert_eq!(v.codigo, 1);
        assert!(v.linha.contains("disjuntor aberto"));
    }
}
