//! O daemon: processo de longa duração que mantém o Abiyss vivo 24/7.
//!
//! - Só um daemon por vez (trava em `data/daemon.lock`).
//! - A cada `cron_verificacao_segundos`: dispara crons vencidos e grava
//!   um "sinal de vida" (código puro, sem modelo).
//! - A cada `heartbeat_segundos`: um ciclo de heartbeat.
//! - SIGTERM (systemd) ou Ctrl+C: para com educação, mesmo no meio de um ciclo.

use std::fs::{File, OpenOptions};
use std::time::Duration;

use anyhow::{Context, bail};
use rusqlite::{OptionalExtension, params};
use tokio::time::MissedTickBehavior;

use crate::config::Config;
use crate::cron;
use crate::db::Banco;
use crate::heartbeat::Heartbeat;
use crate::orquestrador::Orquestrador;
use crate::tempo::agora_ms;

/// Chaves da tabela `estado_daemon`.
pub const CHAVE_PID: &str = "pid";
pub const CHAVE_INICIADO: &str = "iniciado_ms";
pub const CHAVE_SINAL_DE_VIDA: &str = "sinal_de_vida_ms";
pub const CHAVE_PARADO: &str = "parado_ms";

/// Trava de instância única. Enquanto este valor existir, nenhum outro
/// daemon consegue iniciar. O sistema operacional solta a trava sozinho
/// se o processo morrer.
pub struct TravaDaemon {
    _arquivo: File,
}

impl TravaDaemon {
    pub fn adquirir(config: &Config) -> anyhow::Result<TravaDaemon> {
        let caminho = config.caminho_trava_daemon();
        if let Some(pasta) = caminho.parent() {
            std::fs::create_dir_all(pasta)?;
        }
        let arquivo = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&caminho)
            .with_context(|| format!("não consegui abrir {}", caminho.display()))?;
        match arquivo.try_lock() {
            Ok(()) => Ok(TravaDaemon { _arquivo: arquivo }),
            Err(std::fs::TryLockError::WouldBlock) => {
                bail!(
                    "já existe um daemon do Abiyss rodando (trava em {})",
                    caminho.display()
                )
            }
            Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
        }
    }
}

/// O daemon está rodando? (Tenta a trava: se conseguir, ninguém está.)
pub fn esta_rodando(config: &Config) -> bool {
    let caminho = config.caminho_trava_daemon();
    let Ok(arquivo) = OpenOptions::new().write(true).open(&caminho) else {
        return false;
    };
    matches!(arquivo.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

pub fn gravar_estado(banco: &Banco, chave: &str, valor: &str) -> anyhow::Result<()> {
    banco.conexao().execute(
        "INSERT INTO estado_daemon (chave, valor) VALUES (?1, ?2)
         ON CONFLICT(chave) DO UPDATE SET valor = excluded.valor",
        params![chave, valor],
    )?;
    Ok(())
}

pub fn ler_estado(banco: &Banco, chave: &str) -> anyhow::Result<Option<String>> {
    let valor = banco
        .conexao()
        .query_row(
            "SELECT valor FROM estado_daemon WHERE chave = ?1",
            params![chave],
            |l| l.get(0),
        )
        .optional()?;
    Ok(valor)
}

/// Espera SIGTERM ou Ctrl+C.
struct SinalDeParada {
    #[cfg(unix)]
    termino: tokio::signal::unix::Signal,
}

impl SinalDeParada {
    fn novo() -> anyhow::Result<SinalDeParada> {
        Ok(SinalDeParada {
            #[cfg(unix)]
            termino: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?,
        })
    }

    async fn esperar(&mut self) {
        #[cfg(unix)]
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = self.termino.recv() => {}
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Opções de linha de comando do daemon.
#[derive(Debug, Clone, Default)]
pub struct OpcoesDaemon {
    /// Roda um único ciclo e sai (útil para testar na VM).
    pub uma_vez: bool,
}

/// Peças que o daemon usa. Separado para os testes poderem montar com o mock.
pub struct Daemon {
    pub config: Config,
    pub banco: Banco,
    pub heartbeat: Heartbeat,
}

impl Daemon {
    pub fn novo(config: Config, banco: Banco, orquestrador: Orquestrador) -> Daemon {
        let heartbeat = Heartbeat::novo(config.clone(), banco.clone(), orquestrador);
        Daemon {
            config,
            banco,
            heartbeat,
        }
    }

    /// Tarefa barata e determinística: crons + sinal de vida.
    fn tique_de_cron(&self) {
        let agora = agora_ms();
        match cron::disparar_vencidos(&self.banco, agora) {
            Ok(nomes) if !nomes.is_empty() => {
                tracing::info!("crons disparados: {}", nomes.join(", "))
            }
            Ok(_) => {}
            Err(e) => tracing::error!("falha ao verificar crons: {e:#}"),
        }
        if let Err(e) = gravar_estado(&self.banco, CHAVE_SINAL_DE_VIDA, &agora.to_string()) {
            tracing::warn!("não consegui gravar o sinal de vida: {e:#}");
        }
    }

    async fn um_ciclo(&self) {
        match self.heartbeat.ciclo().await {
            Ok(r) if r.chamou_modelo => {
                tracing::info!(
                    "heartbeat: chamou o modelo ({}); ações: {}",
                    r.motivo,
                    if r.resultados.is_empty() {
                        "nenhuma".to_string()
                    } else {
                        r.resultados.join(" | ")
                    }
                );
                if let Some(erro) = r.erro {
                    tracing::warn!("heartbeat: {erro}");
                }
            }
            Ok(r) => tracing::debug!("heartbeat sem chamada ao modelo: {}", r.motivo),
            Err(e) => tracing::error!("heartbeat falhou: {e:#}"),
        }
    }

    /// Loop principal. Volta quando receber o sinal de parada.
    pub async fn rodar(&self, opcoes: &OpcoesDaemon) -> anyhow::Result<()> {
        gravar_estado(&self.banco, CHAVE_PID, &std::process::id().to_string())?;
        gravar_estado(&self.banco, CHAVE_INICIADO, &agora_ms().to_string())?;

        if opcoes.uma_vez {
            self.tique_de_cron();
            self.um_ciclo().await;
            gravar_estado(&self.banco, CHAVE_PARADO, &agora_ms().to_string())?;
            return Ok(());
        }

        let mut parada = SinalDeParada::novo()?;
        let mut tique_cron = tokio::time::interval(Duration::from_secs(
            self.config.daemon.cron_verificacao_segundos,
        ));
        let mut tique_heartbeat =
            tokio::time::interval(Duration::from_secs(self.config.daemon.heartbeat_segundos));
        // Se um ciclo demorar mais que o intervalo, não "compensa" depois.
        tique_cron.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tique_heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);

        tracing::info!(
            "daemon do Abiyss no ar (heartbeat a cada {} s)",
            self.config.daemon.heartbeat_segundos
        );
        loop {
            tokio::select! {
                _ = tique_cron.tick() => self.tique_de_cron(),
                _ = tique_heartbeat.tick() => {
                    // O ciclo pode demorar (raciocínio longo); a parada não espera ele terminar.
                    tokio::select! {
                        _ = self.um_ciclo() => {}
                        _ = parada.esperar() => {
                            tracing::info!("parada pedida no meio de um ciclo");
                            break;
                        }
                    }
                }
                _ = parada.esperar() => break,
            }
        }
        gravar_estado(&self.banco, CHAVE_PARADO, &agora_ms().to_string())?;
        tracing::info!("daemon do Abiyss parado");
        Ok(())
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::config::config_de_teste;

    #[test]
    fn so_um_daemon_por_vez() {
        let pasta = tempfile::tempdir().unwrap();
        let config = config_de_teste("http://127.0.0.1:9/v1", pasta.path());
        assert!(!esta_rodando(&config));
        let trava = TravaDaemon::adquirir(&config).unwrap();
        assert!(esta_rodando(&config));
        assert!(TravaDaemon::adquirir(&config).is_err());
        drop(trava);
        assert!(!esta_rodando(&config));
        assert!(TravaDaemon::adquirir(&config).is_ok());
    }

    #[test]
    fn estado_do_daemon_no_banco() {
        let banco = Banco::em_memoria().unwrap();
        assert_eq!(ler_estado(&banco, CHAVE_PID).unwrap(), None);
        gravar_estado(&banco, CHAVE_PID, "1").unwrap();
        gravar_estado(&banco, CHAVE_PID, "2").unwrap();
        assert_eq!(ler_estado(&banco, CHAVE_PID).unwrap().as_deref(), Some("2"));
    }
}
