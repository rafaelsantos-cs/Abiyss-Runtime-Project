//! O daemon: processo de longa duração que mantém o Abiyss vivo 24/7.
//!
//! - Só um daemon por vez (trava em `data/daemon.lock`).
//! - A cada `cron_verificacao_segundos`: dispara crons vencidos e grava
//!   um "sinal de vida" (código puro, sem modelo).
//! - A cada `heartbeat_segundos`: um ciclo de heartbeat.
//! - O loop principal NUNCA espera trabalho longo: o ciclo do heartbeat e
//!   a manutenção ficam guardados como "trabalhos em andamento" e são
//!   acompanhados pelo mesmo `select!` que dispara os crons. Um ciclo tem
//!   tempo máximo e um pânico dentro dele não derruba o daemon.
//! - Uma vez por dia, na janela de `[sono]` (ou por `abiyss sleep
//!   --completo`): o sono. Enquanto ele roda, o heartbeat fica pausado; o
//!   primeiro ciclo depois dele vê o evento `sono` (ver `sono`).
//! - A cada `retencao.checkpoint_minutos`: checkpoint do WAL.
//! - A cada `retencao.manutencao_minutos`: retenção (detalhe antigo vira
//!   agregado diário) e vacuum incremental.
//! - Em paralelo: o executor de sub-agentes.
//! - SIGTERM (systemd) ou Ctrl+C: para com educação, mesmo no meio de um ciclo.
//! - Sob o systemd (`Type=notify`): avisa READY=1 ao subir, STOPPING=1 ao
//!   parar e, com `WatchdogSec=`, manda WATCHDOG=1 enquanto o loop principal
//!   estiver andando (ver `vigiar`).

use std::fs::{File, OpenOptions};
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use anyhow::{Context, bail};
use futures::FutureExt;
use rusqlite::{OptionalExtension, params};
use tokio::time::{Instant, MissedTickBehavior};

use crate::config::Config;
use crate::cron;
use crate::db::Banco;
use crate::eventos;
use crate::ferramentas::CaixaDeFerramentas;
use crate::heartbeat::{self, Heartbeat};
use crate::manutencao;
use crate::orquestrador::Orquestrador;
use crate::ritmo;
use crate::sono::{self, Gatilho, Sono};
use crate::subagentes::{self, ExecutorSubagentes};
use crate::tempo::{agora_ms, formatar_duracao, formatar_ms};

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

/// Como terminou um trabalho supervisionado.
#[derive(Debug, PartialEq)]
pub enum Desfecho<T> {
    Ok(T),
    /// Passou do tempo máximo e foi interrompido.
    TempoEsgotado,
    /// Entrou em pânico; a mensagem do pânico (quando é texto).
    Panico(String),
}

/// Roda `futuro` com tempo máximo, capturando pânico. Nada que aconteça
/// dentro dele derruba quem chamou.
pub async fn supervisionar<F, T>(limite: Duration, futuro: F) -> Desfecho<T>
where
    F: Future<Output = T>,
{
    match AssertUnwindSafe(tokio::time::timeout(limite, futuro))
        .catch_unwind()
        .await
    {
        Ok(Ok(valor)) => Desfecho::Ok(valor),
        Ok(Err(_)) => Desfecho::TempoEsgotado,
        Err(panico) => {
            let mensagem = panico
                .downcast_ref::<&str>()
                .map(|t| t.to_string())
                .or_else(|| panico.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "pânico sem mensagem".to_string());
            Desfecho::Panico(mensagem)
        }
    }
}

/// Um trabalho longo em andamento, acompanhado pelo loop principal.
type Trabalho<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Espera o trabalho terminar; sem trabalho, espera para sempre (assim o
/// braço do `select!` simplesmente nunca dispara).
async fn esperar_trabalho(trabalho: &mut Option<Trabalho<'_>>) {
    match trabalho {
        Some(futuro) => futuro.as_mut().await,
        None => std::future::pending().await,
    }
}

/// Quanto esperar até o próximo ciclo. Um ciclo que pediu continuação
/// (deixou algo para o próximo ler) antecipa o seguinte, até
/// `max_seguidas` vezes em sequência; depois volta ao intervalo normal.
pub fn proxima_espera(
    pediu_continuacao: bool,
    seguidas: &mut u32,
    max_seguidas: u32,
    continuacao: Duration,
    normal: Duration,
) -> Duration {
    if pediu_continuacao && *seguidas < max_seguidas {
        *seguidas += 1;
        continuacao
    } else {
        *seguidas = 0;
        normal
    }
}

/// Quanto tempo o Abiyss ficou fora do ar antes desta execução.
#[derive(Debug, Clone, PartialEq)]
pub struct Ausencia {
    /// Fim da execução anterior (parada limpa ou último sinal de vida).
    pub desde_ms: i64,
    pub ate_ms: i64,
    /// A execução anterior parou com educação (SIGTERM, Ctrl+C)?
    pub parada_limpa: bool,
}

/// Compara o fim da execução anterior com agora. O fim é o `parado_ms`
/// (se a execução anterior parou com educação depois de iniciar) ou o
/// último sinal de vida (queda). Ausência menor que o limite, ou primeira
/// execução, não conta.
pub fn avaliar_ausencia(
    iniciado_antes: Option<i64>,
    parado: Option<i64>,
    sinal_de_vida: Option<i64>,
    agora: i64,
    limite_minutos: u64,
) -> Option<Ausencia> {
    let iniciado = iniciado_antes?;
    let parada_limpa = parado.is_some_and(|p| p >= iniciado);
    let desde = if parada_limpa {
        parado?
    } else {
        sinal_de_vida.unwrap_or(iniciado).max(iniciado)
    };
    (agora - desde > limite_minutos as i64 * 60_000).then_some(Ausencia {
        desde_ms: desde,
        ate_ms: agora,
        parada_limpa,
    })
}

/// Texto do evento `kernel/reinicio`.
pub fn texto_ausencia(
    a: &Ausencia,
    crons_atrasados: usize,
    subagentes_interrompidos: usize,
) -> String {
    let motivo = if a.parada_limpa {
        "parada limpa".to_string()
    } else {
        format!(
            "queda (último sinal de vida em {})",
            formatar_ms(a.desde_ms)
        )
    };
    format!(
        "Fiquei fora do ar de {} até {} ({}); motivo: {motivo}; {crons_atrasados} cron(s) \
         atrasado(s) disparam agora uma vez só (agrupados); {subagentes_interrompidos} \
         sub-agente(s) interrompido(s).",
        formatar_ms(a.desde_ms),
        formatar_ms(a.ate_ms),
        formatar_duracao(a.ate_ms - a.desde_ms)
    )
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
    pub sono: Sono,
    /// O último ciclo pediu continuação (ex.: consultou uma skill).
    continuacao_pedida: AtomicBool,
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
        let sono = Sono::novo(config.clone(), banco.clone(), orquestrador.clone());
        let heartbeat = Heartbeat::novo(config.clone(), banco.clone(), orquestrador)
            .com_subagentes(executor.controle());
        Daemon {
            config,
            banco,
            heartbeat,
            executor,
            sono,
            continuacao_pedida: AtomicBool::new(false),
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
                if r.pedir_continuacao {
                    self.continuacao_pedida.store(true, Ordering::Relaxed);
                }
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

    /// Um ciclo com tempo máximo e pânico isolado. Ciclo interrompido fica
    /// registrado na tabela `ciclos` com o motivo.
    async fn um_ciclo_supervisionado(&self) {
        let inicio = agora_ms();
        let limite = Duration::from_secs(self.config.daemon.max_duracao_ciclo_segundos);
        let erro = match supervisionar(limite, self.um_ciclo()).await {
            Desfecho::Ok(()) => return,
            Desfecho::TempoEsgotado => format!(
                "tempo esgotado: o ciclo passou de {} s e foi interrompido",
                limite.as_secs()
            ),
            Desfecho::Panico(mensagem) => format!("pânico no ciclo: {mensagem}"),
        };
        tracing::error!("heartbeat: {erro}");
        if let Err(e) = heartbeat::registrar_ciclo_interrompido(&self.banco, inicio, &erro) {
            tracing::warn!("não consegui registrar o ciclo interrompido: {e:#}");
        }
        // Ciclo interrompido conta como falha para o disjuntor.
        if let Err(e) = crate::vigilancia::registrar_no_disjuntor(
            &self.banco,
            &self.config.vigilancia,
            false,
            Some(&erro),
            agora_ms(),
        ) {
            tracing::warn!("não consegui atualizar o disjuntor: {e:#}");
        }
    }

    /// É hora de dormir? (Janela, recuperação ou pedido manual.)
    fn decidir_sono(&self) -> Option<(chrono::NaiveDate, Gatilho)> {
        let pedido = match sono::tomar_pedido(&self.banco) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("não consegui ler o pedido de sono: {e:#}");
                false
            }
        };
        let agora = chrono::Local::now().naive_local();
        sono::decidir(
            &self.config.sono,
            agora,
            // Erro ao consultar = "já dormiu": melhor perder uma noite que
            // dormir em loop.
            |dia| sono::ja_dormiu(&self.banco, dia).unwrap_or(true),
            pedido,
        )
    }

    /// O sono com tempo máximo e pânico isolado. Nunca derruba o daemon.
    async fn sono_supervisionado(&self, dia: chrono::NaiveDate, gatilho: Gatilho) {
        let limite = Duration::from_secs(self.config.sono.max_duracao_minutos * 60);
        let erro = match supervisionar(limite, self.sono.dormir(dia, gatilho)).await {
            Desfecho::Ok(Ok(r)) => {
                tracing::info!("sono {}: {}", r.estado, r.dia);
                return;
            }
            Desfecho::Ok(Err(e)) => format!("o sono falhou: {e:#}"),
            Desfecho::TempoEsgotado => format!(
                "tempo esgotado: o sono passou de {} min e foi interrompido",
                limite.as_secs() / 60
            ),
            Desfecho::Panico(mensagem) => format!("pânico no sono: {mensagem}"),
        };
        tracing::error!("{erro}");
        if let Err(e) = sono::registrar_falha(&self.banco, &erro) {
            tracing::warn!("não consegui registrar a falha do sono: {e:#}");
        }
    }

    /// Loop principal. Volta quando receber SIGTERM ou Ctrl+C.
    pub async fn rodar(&self, opcoes: &OpcoesDaemon) -> anyhow::Result<()> {
        let mut parada = SinalDeParada::novo()?;
        self.rodar_ate(opcoes, async move { parada.esperar().await })
            .await
    }

    /// Loop principal. Volta quando `parar` terminar (o sinal de parada;
    /// nos testes, um canal).
    pub async fn rodar_ate<P>(&self, opcoes: &OpcoesDaemon, parar: P) -> anyhow::Result<()>
    where
        P: Future<Output = ()>,
    {
        // Antes de sobrescrever: como terminou a execução anterior?
        let numero = |chave: &str| -> anyhow::Result<Option<i64>> {
            Ok(ler_estado(&self.banco, chave)?.and_then(|v| v.parse().ok()))
        };
        let agora = agora_ms();
        let ausencia = avaliar_ausencia(
            numero(CHAVE_INICIADO)?,
            numero(CHAVE_PARADO)?,
            numero(CHAVE_SINAL_DE_VIDA)?,
            agora,
            self.config.daemon.aviso_ausencia_minutos,
        );
        gravar_estado(&self.banco, CHAVE_PID, &std::process::id().to_string())?;
        gravar_estado(&self.banco, CHAVE_INICIADO, &agora.to_string())?;
        let interrompidos = sono::marcar_interrompidos(&self.banco)?;
        if interrompidos > 0 {
            tracing::warn!(
                "um sono foi interrompido pela última parada; será refeito se der tempo"
            );
        }
        let subagentes_interrompidos = subagentes::recuperar_interrompidos(&self.banco)?;
        if let Some(a) = ausencia {
            let atrasados = cron::listar(&self.banco)?
                .iter()
                .filter(|c| c.ativo && c.proximo_ms <= agora)
                .count();
            let texto = texto_ausencia(&a, atrasados, subagentes_interrompidos);
            tracing::info!("{texto}");
            eventos::publicar(
                &self.banco,
                eventos::TIPO_KERNEL,
                eventos::ORIGEM_REINICIO,
                &texto,
            )?;
        }

        if opcoes.uma_vez {
            self.tique_de_cron();
            self.um_ciclo_supervisionado().await;
            // Sub-agentes delegados neste ciclo rodam até o fim antes de sair.
            self.executor.executar_pendentes_e_esperar().await?;
            gravar_estado(&self.banco, CHAVE_PARADO, &agora_ms().to_string())?;
            return Ok(());
        }

        // O executor de sub-agentes roda numa tarefa separada.
        let executor = tokio::spawn(Arc::clone(&self.executor).rodar());

        tokio::pin!(parar);
        let mut tique_cron = tokio::time::interval(Duration::from_secs(
            self.config.daemon.cron_verificacao_segundos,
        ));
        let retencao = &self.config.retencao;
        let mut tique_manutencao =
            tokio::time::interval(Duration::from_secs(retencao.manutencao_minutos * 60));
        let mut tique_checkpoint =
            tokio::time::interval(Duration::from_secs(retencao.checkpoint_minutos * 60));
        // Se uma volta demorar mais que o intervalo, não "compensa" depois.
        for tique in [
            &mut tique_cron,
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

        // Trabalhos longos em andamento. Ficam guardados aqui e são
        // acompanhados pelo mesmo `select!` dos tiques baratos: crons, sinal
        // de vida e watchdog continuam andando enquanto um ciclo pensa.
        let mut ciclo: Option<Trabalho<'_>> = None;
        let mut manutencao: Option<Trabalho<'_>> = None;
        // O sono é exclusivo com o heartbeat: espera o ciclo em andamento
        // terminar e, enquanto dorme, nenhum ciclo começa.
        let mut sono: Option<Trabalho<'_>> = None;
        let mut sono_pendente: Option<(chrono::NaiveDate, Gatilho)> = None;
        // O primeiro ciclo roda logo ao subir (como antes).
        let mut proximo_ciclo = Instant::now();
        let mut continuacoes_seguidas = 0;
        loop {
            tokio::select! {
                _ = tique_cron.tick() => {
                    self.tique_de_cron();
                    if sono.is_none() && sono_pendente.is_none() {
                        sono_pendente = self.decidir_sono();
                    }
                }
                _ = tique_checkpoint.tick() => self.checkpoint(),
                _ = tique_manutencao.tick(), if manutencao.is_none() => {
                    manutencao = Some(Box::pin(self.manutencao()));
                }
                _ = esperar_trabalho(&mut manutencao) => manutencao = None,
                _ = tokio::time::sleep_until(proximo_ciclo),
                    if ciclo.is_none() && sono.is_none() && sono_pendente.is_none() => {
                    ciclo = Some(Box::pin(self.um_ciclo_supervisionado()));
                }
                _ = esperar_trabalho(&mut ciclo) => {
                    ciclo = None;
                    let pediu = self.continuacao_pedida.swap(false, Ordering::Relaxed);
                    // Fora das horas ativas o heartbeat espera mais (descanso).
                    let intervalo = ritmo::intervalo_heartbeat(
                        ritmo::fase_agora(&self.config.ritmo),
                        self.config.daemon.heartbeat_segundos,
                        &self.config.ritmo,
                    );
                    let espera = proxima_espera(
                        pediu,
                        &mut continuacoes_seguidas,
                        self.config.daemon.max_continuacoes_seguidas,
                        Duration::from_secs(self.config.daemon.continuacao_segundos),
                        intervalo,
                    );
                    proximo_ciclo = Instant::now() + espera;
                }
                _ = esperar_trabalho(&mut sono) => {
                    sono = None;
                    // Acordou: o primeiro ciclo vem já (e vê o evento `sono`).
                    proximo_ciclo = Instant::now();
                }
                _ = &mut parar => {
                    if sono.is_some() {
                        // Marcado como interrompido ao subir de novo; o que
                        // já foi gravado (propostas + marcas) não se repete.
                        tracing::info!("parada pedida no meio do sono");
                    }
                    if ciclo.is_some() {
                        // O ciclo é largado no próximo ponto de espera: os
                        // eventos dele continuam pendentes para a próxima vez.
                        tracing::info!("parada pedida no meio de um ciclo");
                    }
                    break;
                }
            }
            if ciclo.is_none()
                && sono.is_none()
                && let Some((dia, gatilho)) = sono_pendente.take()
            {
                tracing::info!(
                    "hora de dormir (revisão de {dia}, {})",
                    gatilho.como_texto()
                );
                sono = Some(Box::pin(self.sono_supervisionado(dia, gatilho)));
            }
            ultima_volta_ms.store(agora_ms(), Ordering::Relaxed);
        }
        drop(ciclo);
        drop(manutencao);
        drop(sono);
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

    #[tokio::test]
    async fn supervisao_isola_tempo_e_panico() {
        let curto = Duration::from_millis(200);
        assert_eq!(supervisionar(curto, async { 7 }).await, Desfecho::Ok(7));
        assert_eq!(
            supervisionar(curto, tokio::time::sleep(Duration::from_secs(5))).await,
            Desfecho::TempoEsgotado
        );
        let panico = supervisionar(curto, async {
            if curto.as_millis() > 0 {
                panic!("explodiu de propósito");
            }
        })
        .await;
        assert_eq!(panico, Desfecho::Panico("explodiu de propósito".into()));
        // Pânico com String formatada também vira mensagem.
        let n = 3;
        let formatado: Desfecho<()> =
            supervisionar(curto, async move { panic!("falha {n}") }).await;
        assert_eq!(formatado, Desfecho::Panico("falha 3".into()));
    }

    #[test]
    fn continuacao_tem_limite() {
        let (c, n) = (Duration::from_secs(1), Duration::from_secs(300));
        let mut seguidas = 0;
        assert_eq!(proxima_espera(true, &mut seguidas, 2, c, n), c);
        assert_eq!(proxima_espera(true, &mut seguidas, 2, c, n), c);
        // Terceira seguida: passou do limite, volta ao normal e zera.
        assert_eq!(proxima_espera(true, &mut seguidas, 2, c, n), n);
        assert_eq!(seguidas, 0);
        assert_eq!(proxima_espera(true, &mut seguidas, 2, c, n), c);
        assert_eq!(proxima_espera(false, &mut seguidas, 2, c, n), n);
        assert_eq!(seguidas, 0);
    }

    #[test]
    fn ausencia_por_queda_por_parada_e_curta() {
        let min = 60_000;
        // Primeira execução: nada a comparar.
        assert_eq!(avaliar_ausencia(None, None, None, 100 * min, 10), None);
        // Queda: sem parada depois do início; conta do último sinal de vida.
        let queda = avaliar_ausencia(Some(0), None, Some(30 * min), 100 * min, 10).unwrap();
        assert!(!queda.parada_limpa);
        assert_eq!(queda.desde_ms, 30 * min);
        // Uma parada ANTERIOR ao último início não vale: também é queda.
        let velha = avaliar_ausencia(
            Some(50 * min),
            Some(10 * min),
            Some(60 * min),
            100 * min,
            10,
        );
        assert!(velha.is_some_and(|a| !a.parada_limpa && a.desde_ms == 60 * min));
        // Parada limpa: conta da parada.
        let limpa =
            avaliar_ausencia(Some(0), Some(40 * min), Some(39 * min), 100 * min, 10).unwrap();
        assert!(limpa.parada_limpa);
        assert_eq!(limpa.desde_ms, 40 * min);
        // Ausência curta (um restart rápido): sem aviso.
        assert_eq!(
            avaliar_ausencia(Some(0), Some(95 * min), None, 100 * min, 10),
            None
        );
        let texto = texto_ausencia(&queda, 2, 1);
        assert!(texto.contains("(1 h 10 min); motivo: queda (último sinal de vida em"));
        assert!(texto.contains("2 cron(s) atrasado(s)"));
        assert!(texto.contains("1 sub-agente(s) interrompido(s)"));
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
