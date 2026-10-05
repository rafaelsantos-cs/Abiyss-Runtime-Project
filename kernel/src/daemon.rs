//! O daemon: processo de longa duração que mantém o Abiyss vivo 24/7.
//!
//! - Só um daemon por vez (trava em `data/daemon.lock`).
//! - A cada `cron_verificacao_segundos`: dispara crons vencidos e grava
//!   um "sinal de vida" (código puro, sem modelo).
//! - A cada `heartbeat_segundos`: um ciclo de heartbeat.
//! - A cada `retencao.checkpoint_minutos`: checkpoint do WAL.
//! - A cada `retencao.manutencao_minutos`: retenção (detalhe antigo vira
//!   agregado diário) e vacuum incremental.
//! - Em paralelo: o executor de sub-agentes.
//! - SIGTERM (systemd) ou Ctrl+C: para com educação, mesmo no meio de um ciclo.
//! - Sob o systemd (`Type=notify`): avisa READY=1 ao subir, STOPPING=1 ao
//!   parar e, com `WatchdogSec=`, manda WATCHDOG=1 enquanto o loop principal
//!   estiver andando (ver `vigiar`).

use std::fs::{File, OpenOptions};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use anyhow::{Context, bail};
use rusqlite::{OptionalExtension, params};
use tokio::time::MissedTickBehavior;

use crate::config::Config;
use crate::cron;
use crate::db::Banco;
use crate::ferramentas::CaixaDeFerramentas;
use crate::heartbeat::Heartbeat;
use crate::manutencao;
use crate::orquestrador::Orquestrador;
use crate::subagentes::{self, ExecutorSubagentes};
use crate::tempo::agora_ms;

/// Chaves da tabela `estado_daemon`.
pub const CHAVE_PID: &str = "pid";
pub const CHAVE_INICIADO: &str = "iniciado_ms";
pub const CHAVE_SINAL_DE_VIDA: &str = "sinal_de_vida_ms";
pub const CHAVE_PARADO: &str = "parado_ms";
pub const CHAVE_MANUTENCAO: &str = "manutencao_ms";
pub const CHAVE_MANUTENCAO_RESUMO: &str = "manutencao_resumo";

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

/// Avisa o systemd (sem efeito fora dele).
fn avisar_systemd(mensagem: &str) {
    #[cfg(unix)]
    crate::systemd::notificar(mensagem);
    #[cfg(not(unix))]
    let _ = mensagem;
}

/// O loop principal está andando? `ultima_volta_ms` é atualizado a cada
/// volta completa do loop (no mínimo a cada `cron_verificacao_segundos`
/// quando nada demora).
pub fn loop_saudavel(ultima_volta_ms: i64, agora_ms: i64, max_travado_segundos: u64) -> bool {
    agora_ms - ultima_volta_ms <= max_travado_segundos as i64 * 1000
}

/// Tarefa do watchdog: a cada `intervalo`, manda WATCHDOG=1 se o loop
/// principal deu uma volta há pouco tempo. Se o loop travar, os avisos
/// param e o systemd reinicia o serviço. Se o próprio tokio travar, esta
/// tarefa nem roda — e o efeito é o mesmo.
async fn vigiar(ultima_volta_ms: Arc<AtomicI64>, max_travado_segundos: u64, intervalo: Duration) {
    let mut tique = tokio::time::interval(intervalo);
    let mut avisou_travado = false;
    loop {
        tique.tick().await;
        let ultima = ultima_volta_ms.load(Ordering::Relaxed);
        if loop_saudavel(ultima, agora_ms(), max_travado_segundos) {
            avisar_systemd("WATCHDOG=1");
            avisou_travado = false;
        } else if !avisou_travado {
            tracing::error!(
                "loop principal parado há mais de {max_travado_segundos} s; \
                 deixando de avisar o watchdog do systemd"
            );
            avisar_systemd("STATUS=loop principal travado");
            avisou_travado = true;
        }
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
    pub executor: Arc<ExecutorSubagentes>,
}

impl Daemon {
    /// `ferramentas` é a caixa base dos sub-agentes (cada nível recebe
    /// uma versão restrita dela).
    pub fn novo(
        config: Config,
        banco: Banco,
        orquestrador: Orquestrador,
        ferramentas: Arc<CaixaDeFerramentas>,
    ) -> Daemon {
        let executor = ExecutorSubagentes::novo(
            config.clone(),
            banco.clone(),
            orquestrador.clone(),
            ferramentas,
        );
        let heartbeat = Heartbeat::novo(config.clone(), banco.clone(), orquestrador)
            .com_subagentes(executor.controle());
        Daemon {
            config,
            banco,
            heartbeat,
            executor,
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

    /// Retenção + vacuum, fora das threads do tokio (pode demorar no
    /// primeiro dia de um banco grande).
    async fn manutencao(&self) {
        let banco = self.banco.clone();
        let config = self.config.retencao.clone();
        let rodada = tokio::task::spawn_blocking(move || {
            manutencao::rodada_completa(&banco, &config, agora_ms())
        })
        .await;
        match rodada {
            Ok(Ok(resumo)) => {
                tracing::info!("manutenção do banco: {resumo}");
                let _ = gravar_estado(&self.banco, CHAVE_MANUTENCAO, &agora_ms().to_string());
                let _ = gravar_estado(&self.banco, CHAVE_MANUTENCAO_RESUMO, &resumo);
            }
            Ok(Err(e)) => tracing::error!("manutenção do banco falhou: {e:#}"),
            Err(e) => tracing::error!("tarefa de manutenção morreu: {e}"),
        }
    }

    /// Checkpoint do WAL (rápido; ocupado = tenta no próximo intervalo).
    fn checkpoint(&self) {
        match manutencao::checkpoint_wal(&self.banco) {
            Ok(r) if r.ocupado => {
                tracing::debug!("checkpoint do WAL parcial (banco ocupado): {r:?}")
            }
            Ok(r) => tracing::debug!("checkpoint do WAL: {r:?}"),
            Err(e) => tracing::warn!("checkpoint do WAL falhou: {e:#}"),
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
            subagentes::recuperar_interrompidos(&self.banco)?;
            self.tique_de_cron();
            self.um_ciclo().await;
            // Sub-agentes delegados neste ciclo rodam até o fim antes de sair.
            self.executor.executar_pendentes_e_esperar().await?;
            gravar_estado(&self.banco, CHAVE_PARADO, &agora_ms().to_string())?;
            return Ok(());
        }

        // O executor de sub-agentes roda numa tarefa separada.
        let executor = tokio::spawn(Arc::clone(&self.executor).rodar());

        let mut parada = SinalDeParada::novo()?;
        let mut tique_cron = tokio::time::interval(Duration::from_secs(
            self.config.daemon.cron_verificacao_segundos,
        ));
        let mut tique_heartbeat =
            tokio::time::interval(Duration::from_secs(self.config.daemon.heartbeat_segundos));
        let retencao = &self.config.retencao;
        let mut tique_manutencao =
            tokio::time::interval(Duration::from_secs(retencao.manutencao_minutos * 60));
        let mut tique_checkpoint =
            tokio::time::interval(Duration::from_secs(retencao.checkpoint_minutos * 60));
        // Se um ciclo demorar mais que o intervalo, não "compensa" depois.
        for tique in [
            &mut tique_cron,
            &mut tique_heartbeat,
            &mut tique_manutencao,
            &mut tique_checkpoint,
        ] {
            tique.set_missed_tick_behavior(MissedTickBehavior::Delay);
        }

        // Watchdog do systemd (só se a unit tiver WatchdogSec=).
        let ultima_volta_ms = Arc::new(AtomicI64::new(agora_ms()));
        #[cfg(unix)]
        let vigia = crate::systemd::intervalo_watchdog().map(|intervalo| {
            tracing::info!("watchdog do systemd ligado (aviso a cada {intervalo:?})");
            tokio::spawn(vigiar(
                Arc::clone(&ultima_volta_ms),
                self.config.daemon.max_travado_segundos,
                intervalo,
            ))
        });
        #[cfg(not(unix))]
        let vigia: Option<tokio::task::JoinHandle<()>> = None;

        tracing::info!(
            "daemon do Abiyss no ar (heartbeat a cada {} s)",
            self.config.daemon.heartbeat_segundos
        );
        avisar_systemd(&format!(
            "READY=1\nSTATUS=no ar (heartbeat a cada {} s)",
            self.config.daemon.heartbeat_segundos
        ));
        loop {
            tokio::select! {
                _ = tique_cron.tick() => self.tique_de_cron(),
                _ = tique_checkpoint.tick() => self.checkpoint(),
                _ = tique_manutencao.tick() => self.manutencao().await,
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
            ultima_volta_ms.store(agora_ms(), Ordering::Relaxed);
        }
        avisar_systemd("STOPPING=1\nSTATUS=parando");
        if let Some(vigia) = vigia {
            vigia.abort();
        }
        // Para o executor (e os sub-agentes dele) e registra quem ficou no meio.
        executor.abort();
        let _ = executor.await;
        let interrompidos = subagentes::recuperar_interrompidos(&self.banco)?;
        if interrompidos > 0 {
            tracing::warn!("{interrompidos} sub-agente(s) interrompido(s) pela parada");
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
    fn loop_saudavel_ate_o_limite() {
        assert!(loop_saudavel(1_000, 1_000, 10));
        assert!(loop_saudavel(1_000, 11_000, 10));
        assert!(!loop_saudavel(1_000, 11_001, 10));
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
