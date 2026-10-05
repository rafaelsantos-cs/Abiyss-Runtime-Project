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
use crate::tempo::{agora_ms, formatar_ms};

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
