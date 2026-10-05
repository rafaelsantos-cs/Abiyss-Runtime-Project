//! Ritmo do dia: vigília, descanso e sono.
//!
//! Tudo calculado por CÓDIGO, no fuso local da máquina (o systemd do
//! Abiyss define `TZ=America/Sao_Paulo`):
//! - **vigília**: dentro das horas ativas, o heartbeat roda como sempre;
//! - **descanso**: fora delas, sem revisão periódica de goal; eventos e
//!   mudanças de goal ainda acordam o modelo, mas o heartbeat roda a cada
//!   `heartbeat_descanso_segundos`;
//! - **sono**: a consolidação noturna está rodando (o heartbeat fica
//!   pausado). Quem sabe disso é o daemon, não o relógio.
//!
//! As funções recebem o minuto do dia (0..1440) para os testes não
//! dependerem do fuso da máquina.

use std::time::Duration;

use anyhow::{Context, bail};
use chrono::{Local, TimeZone, Timelike};
use serde::Deserialize;

use crate::tempo::agora_ms;

/// `[ritmo]` no abiyss.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigRitmo {
    /// "HH:MM-HH:MM" no fuso local. Pode cruzar a meia-noite
    /// ("22:00-06:00"). Início igual ao fim = o dia inteiro é vigília.
    pub horas_ativas: String,
    /// Intervalo do heartbeat fora das horas ativas.
    pub heartbeat_descanso_segundos: u64,
}

impl Default for ConfigRitmo {
    fn default() -> Self {
        ConfigRitmo {
            horas_ativas: "07:00-23:00".to_string(),
            heartbeat_descanso_segundos: 1800,
        }
    }
}

impl ConfigRitmo {
    pub fn validar(&self) -> anyhow::Result<()> {
        Janela::de_texto(&self.horas_ativas).context("ritmo.horas_ativas")?;
        if self.heartbeat_descanso_segundos == 0 {
            bail!("ritmo.heartbeat_descanso_segundos precisa ser > 0");
        }
        Ok(())
    }

    /// Janela já validada (a config foi validada ao carregar).
    pub fn janela(&self) -> Janela {
        Janela::de_texto(&self.horas_ativas).unwrap_or(Janela::DIA_INTEIRO)
    }
}

/// Fase do dia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fase {
    Vigilia,
    Descanso,
    Sono,
}

impl Fase {
    pub fn como_texto(&self) -> &'static str {
        match self {
            Fase::Vigilia => "vigília",
            Fase::Descanso => "descanso",
            Fase::Sono => "sono",
        }
    }
}

/// Intervalo do dia em minutos desde a meia-noite, [inicio, fim).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Janela {
    pub inicio: u32,
    pub fim: u32,
}

impl Janela {
    pub const DIA_INTEIRO: Janela = Janela { inicio: 0, fim: 0 };

    /// "HH:MM-HH:MM".
    pub fn de_texto(texto: &str) -> anyhow::Result<Janela> {
        let (a, b) = texto
            .split_once('-')
            .with_context(|| format!("use HH:MM-HH:MM, recebi '{texto}'"))?;
        Ok(Janela {
            inicio: minuto_de_texto(a)?,
            fim: minuto_de_texto(b)?,
        })
    }

    /// O minuto do dia está dentro da janela? Cruza a meia-noite quando
    /// `fim < inicio`; `inicio == fim` = o dia inteiro.
    pub fn contem(&self, minuto: u32) -> bool {
        let m = minuto % MINUTOS_NO_DIA;
        match self.inicio.cmp(&self.fim) {
            std::cmp::Ordering::Equal => true,
            std::cmp::Ordering::Less => m >= self.inicio && m < self.fim,
            std::cmp::Ordering::Greater => m >= self.inicio || m < self.fim,
        }
    }
}

pub const MINUTOS_NO_DIA: u32 = 24 * 60;

/// "HH:MM" → minutos desde a meia-noite.
pub fn minuto_de_texto(texto: &str) -> anyhow::Result<u32> {
    let (h, m) = texto
        .trim()
        .split_once(':')
        .with_context(|| format!("horário '{texto}' fora do formato HH:MM"))?;
    let h: u32 = h
        .trim()
        .parse()
        .with_context(|| format!("hora inválida em '{texto}'"))?;
    let m: u32 = m
        .trim()
        .parse()
        .with_context(|| format!("minuto inválido em '{texto}'"))?;
    if h > 23 || m > 59 {
        bail!("horário '{texto}' fora do relógio");
    }
    Ok(h * 60 + m)
}

/// Fase pelo relógio (o sono, quem sabe é o daemon).
pub fn fase_pelo_relogio(config: &ConfigRitmo, minuto: u32) -> Fase {
    if config.janela().contem(minuto) {
        Fase::Vigilia
    } else {
        Fase::Descanso
    }
}

/// Intervalo até o próximo heartbeat nesta fase.
pub fn intervalo_heartbeat(fase: Fase, heartbeat_segundos: u64, config: &ConfigRitmo) -> Duration {
    match fase {
        Fase::Vigilia => Duration::from_secs(heartbeat_segundos),
        Fase::Descanso | Fase::Sono => Duration::from_secs(config.heartbeat_descanso_segundos),
    }
}

/// Minuto do dia agora, no fuso local.
pub fn minuto_local_agora() -> u32 {
    let agora = Local::now();
    agora.hour() * 60 + agora.minute()
}

/// Fase pelo relógio, agora.
pub fn fase_agora(config: &ConfigRitmo) -> Fase {
    fase_pelo_relogio(config, minuto_local_agora())
}

/// Meia-noite de hoje (fuso local), em ms.
pub fn inicio_do_dia_local_ms() -> i64 {
    let hoje = Local::now().date_naive();
    let meia_noite = hoje.and_hms_opt(0, 0, 0).expect("meia-noite sempre existe");
    Local
        .from_local_datetime(&meia_noite)
        .earliest()
        .map(|d| d.timestamp_millis())
        .unwrap_or_else(agora_ms)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn janelas_com_e_sem_meia_noite() {
        let dia = Janela::de_texto("07:00-23:00").unwrap();
        assert!(!dia.contem(6 * 60 + 59));
        assert!(dia.contem(7 * 60));
        assert!(dia.contem(22 * 60 + 59));
        assert!(!dia.contem(23 * 60));

        let noite = Janela::de_texto("22:00-06:00").unwrap();
        assert!(noite.contem(23 * 60));
        assert!(noite.contem(0));
        assert!(noite.contem(5 * 60 + 59));
        assert!(!noite.contem(6 * 60));
        assert!(!noite.contem(12 * 60));

        let sempre = Janela::de_texto("00:00-00:00").unwrap();
        assert!(sempre.contem(0) && sempre.contem(13 * 60));

        assert!(Janela::de_texto("7h-23h").is_err());
        assert!(Janela::de_texto("24:00-01:00").is_err());
        assert!(Janela::de_texto("07:60-08:00").is_err());
    }

    #[test]
    fn fase_e_intervalo() {
        let config = ConfigRitmo::default();
        assert_eq!(fase_pelo_relogio(&config, 9 * 60), Fase::Vigilia);
        assert_eq!(fase_pelo_relogio(&config, 3 * 60), Fase::Descanso);
        assert_eq!(
            intervalo_heartbeat(Fase::Vigilia, 300, &config),
            Duration::from_secs(300)
        );
        assert_eq!(
            intervalo_heartbeat(Fase::Descanso, 300, &config),
            Duration::from_secs(1800)
        );
        assert!(minuto_local_agora() < MINUTOS_NO_DIA);
        assert!(inicio_do_dia_local_ms() <= agora_ms());
    }
}
