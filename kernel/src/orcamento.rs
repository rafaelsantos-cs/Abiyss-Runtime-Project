//! Orçamento diário do trabalho autônomo.
//!
//! Limites por minuto (os baldes) não seguram um loop que roda o dia
//! inteiro. Aqui conta-se, POR CÓDIGO, o que o Abiyss gastou sozinho desde
//! a meia-noite (fuso local): chamadas do heartbeat e do sono no pool do
//! cérebro + todas as chamadas dos sub-agentes. Cada tentativa conta (é
//! ela que gasta o limite do NIM).
//!
//! - abaixo de `fracao_alerta`: normal;
//! - a partir dela: alerta (o heartbeat só reage a eventos);
//! - no limite: esgotado (o heartbeat não chama o modelo e não delega).
//!
//! A conversa com o dono NUNCA entra nesta conta nem é cortada por ela.
//! O sono tem uma fatia reservada (`reserva_sono_chamadas`) que o
//! heartbeat e os sub-agentes não podem gastar.

use anyhow::bail;
use rusqlite::params;
use serde::Deserialize;

use crate::db::Banco;

/// `[orcamento]` no abiyss.toml. 0 = sem limite.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigOrcamento {
    pub chamadas_autonomas_por_dia: u64,
    pub tokens_autonomos_por_dia: u64,
    pub reserva_sono_chamadas: u64,
    pub fracao_alerta: f64,
}

impl Default for ConfigOrcamento {
    fn default() -> Self {
        ConfigOrcamento {
            chamadas_autonomas_por_dia: 1500,
            tokens_autonomos_por_dia: 0,
            reserva_sono_chamadas: 12,
            fracao_alerta: 0.8,
        }
    }
}

impl ConfigOrcamento {
    pub fn validar(&self) -> anyhow::Result<()> {
        if !(self.fracao_alerta > 0.0 && self.fracao_alerta <= 1.0) {
            bail!("orcamento.fracao_alerta precisa estar entre 0 (exclusive) e 1");
        }
        if self.chamadas_autonomas_por_dia > 0
            && self.reserva_sono_chamadas >= self.chamadas_autonomas_por_dia
        {
            bail!(
                "orcamento.reserva_sono_chamadas ({}) precisa ser menor que chamadas_autonomas_por_dia ({})",
                self.reserva_sono_chamadas,
                self.chamadas_autonomas_por_dia
            );
        }
        Ok(())
    }
}

/// O que já foi gasto hoje.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UsoOrcamento {
    pub chamadas: u64,
    pub tokens: u64,
}

/// Situação do orçamento para quem pergunta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NivelOrcamento {
    Normal,
    Alerta,
    Esgotado,
}

/// Quem está gastando: o sono pode usar a sua reserva; o resto não.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gasto {
    HeartbeatOuSubagentes,
    Sono,
}

/// Soma o trabalho autônomo desde `desde_ms`.
pub fn uso_desde(banco: &Banco, desde_ms: i64) -> anyhow::Result<UsoOrcamento> {
    let (chamadas, tokens): (i64, i64) = banco.conexao().query_row(
        "SELECT COUNT(*), COALESCE(SUM(tokens_entrada + tokens_saida), 0)
         FROM chamadas_modelo
         WHERE momento_ms >= ?1
           AND ((pool = 'cerebro' AND origem IN ('autonomo', 'sono')) OR pool = 'subagentes')",
        params![desde_ms],
        |l| Ok((l.get(0)?, l.get(1)?)),
    )?;
    Ok(UsoOrcamento {
        chamadas: chamadas.max(0) as u64,
        tokens: tokens.max(0) as u64,
    })
}

/// Limite de chamadas para quem gasta (0 = sem limite).
fn limite_chamadas(config: &ConfigOrcamento, gasto: Gasto) -> u64 {
    match (config.chamadas_autonomas_por_dia, gasto) {
        (0, _) => 0,
        (total, Gasto::Sono) => total,
        (total, Gasto::HeartbeatOuSubagentes) => total.saturating_sub(config.reserva_sono_chamadas),
    }
}

/// Decide o nível (função pura).
pub fn avaliar(config: &ConfigOrcamento, uso: UsoOrcamento, gasto: Gasto) -> NivelOrcamento {
    let mut nivel = NivelOrcamento::Normal;
    for (usado, limite) in [
        (uso.chamadas, limite_chamadas(config, gasto)),
        (uso.tokens, config.tokens_autonomos_por_dia),
    ] {
        if limite == 0 {
            continue;
        }
        if usado >= limite {
            return NivelOrcamento::Esgotado;
        }
        if usado as f64 >= limite as f64 * config.fracao_alerta {
            nivel = NivelOrcamento::Alerta;
        }
    }
    nivel
}

/// Linha para a interocepção e o status.
pub fn descrever(config: &ConfigOrcamento, uso: UsoOrcamento) -> String {
    let limite = limite_chamadas(config, Gasto::HeartbeatOuSubagentes);
    let chamadas = if limite == 0 {
        format!("{} chamada(s) (sem limite)", uso.chamadas)
    } else {
        format!(
            "{}/{} chamada(s) ({:.0}%)",
            uso.chamadas,
            limite,
            uso.chamadas as f64 * 100.0 / limite as f64
        )
    };
    let tokens = if config.tokens_autonomos_por_dia == 0 {
        format!("{} token(s)", uso.tokens)
    } else {
        format!(
            "{}/{} token(s)",
            uso.tokens, config.tokens_autonomos_por_dia
        )
    };
    let nivel = match avaliar(config, uso, Gasto::HeartbeatOuSubagentes) {
        NivelOrcamento::Normal => "",
        NivelOrcamento::Alerta => " — ALERTA: só eventos acordam o modelo",
        NivelOrcamento::Esgotado => " — ESGOTADO: trabalho autônomo parado até amanhã",
    };
    format!("{chamadas}, {tokens}{nivel}")
}

#[cfg(test)]
mod testes {
    use super::*;

    fn config(chamadas: u64, tokens: u64) -> ConfigOrcamento {
        ConfigOrcamento {
            chamadas_autonomas_por_dia: chamadas,
            tokens_autonomos_por_dia: tokens,
            reserva_sono_chamadas: 10,
            fracao_alerta: 0.8,
        }
    }

    fn uso(chamadas: u64, tokens: u64) -> UsoOrcamento {
        UsoOrcamento { chamadas, tokens }
    }

    #[test]
    fn niveis_e_reserva_do_sono() {
        use Gasto::*;
        use NivelOrcamento::*;
        let c = config(110, 0);
        // Heartbeat: limite 110 - 10 = 100; alerta a partir de 80.
        assert_eq!(avaliar(&c, uso(79, 0), HeartbeatOuSubagentes), Normal);
        assert_eq!(avaliar(&c, uso(80, 0), HeartbeatOuSubagentes), Alerta);
        assert_eq!(avaliar(&c, uso(100, 0), HeartbeatOuSubagentes), Esgotado);
        // O sono ainda tem a reserva.
        assert_eq!(avaliar(&c, uso(100, 0), Sono), Alerta);
        assert_eq!(avaliar(&c, uso(110, 0), Sono), Esgotado);
        // Tokens também contam; 0 = sem limite.
        let t = config(0, 1000);
        assert_eq!(avaliar(&t, uso(5000, 999), HeartbeatOuSubagentes), Alerta);
        assert_eq!(
            avaliar(&t, uso(5000, 1000), HeartbeatOuSubagentes),
            Esgotado
        );
        assert_eq!(
            avaliar(&config(0, 0), uso(1_000_000, 1_000_000), Sono),
            Normal
        );
        assert!(descrever(&c, uso(100, 5)).contains("ESGOTADO"));
    }

    #[test]
    fn validacao() {
        assert!(config(110, 0).validar().is_ok());
        assert!(config(10, 0).validar().is_err(), "reserva >= total");
        assert!(config(0, 0).validar().is_ok());
        let mut c = config(110, 0);
        c.fracao_alerta = 0.0;
        assert!(c.validar().is_err());
    }

    #[test]
    fn conta_so_trabalho_autonomo() {
        let banco = Banco::em_memoria().unwrap();
        for (momento, pool, origem, tokens) in [
            (100, "cerebro", "conversa", 1000),
            (100, "cerebro", "autonomo", 10),
            (100, "cerebro", "sono", 20),
            (100, "subagentes", "low", 30),
            (5, "cerebro", "autonomo", 40),
        ] {
            banco
                .conexao()
                .execute(
                    "INSERT INTO chamadas_modelo (momento_ms, pool, origem, modelo, tentativa, status,
                       tokens_entrada, tokens_saida, duracao_ms) VALUES (?1, ?2, ?3, 'm', 1, 'ok', ?4, 0, 1)",
                    params![momento, pool, origem, tokens],
                )
                .unwrap();
        }
        assert_eq!(uso_desde(&banco, 50).unwrap(), uso(3, 60));
    }
}
