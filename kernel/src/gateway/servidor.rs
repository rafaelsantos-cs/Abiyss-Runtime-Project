//! O lado do kernel: um socket Unix local (sem porta de rede) por onde o
//! adaptador do Discord conversa com o daemon.
//!
//! Roda numa tarefa própria do daemon e o loop principal NUNCA espera por
//! ela (nem a vê: o daemon só a cria ao subir e a aborta ao parar). Usa uma
//! conexão própria ao banco, então nem a trava da conexão do loop ela
//! segura. Adaptador fora do ar, travado ou lento não muda nada para o
//! daemon: as entradas e saídas ficam na fila do banco (`registro`).
//!
//! Três trabalhos em paralelo:
//! - `aceitar`: uma conexão por vez (a nova substitui a antiga); cada linha
//!   do adaptador é tratada na hora (gravar, classificar, comandos);
//! - `conversar`: as mensagens do dono, uma conversa por vez, pelo mesmo
//!   caminho do `abiyss chat` (`SessaoChat`, esforço do `[chat]`). O que
//!   chega enquanto um turno roda fica na fila e vira o turno seguinte
//!   (todas juntas);
//! - `entregar`: manda as saídas pendentes ao adaptador e espera a
//!   confirmação (sem confirmação, manda de novo);
//! - `agendar`: pedidos pendentes por DM (com lembretes limitados) e o
//!   resumo da manhã (ver `agenda`).
//!
//! "Responder" (reply do Discord) do dono numa mensagem de pedido é a
//! resposta ÀQUELE pedido: vai direto para `pedidos::responder`, sem
//! passar pelo modelo.

use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, bail};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinSet;

use crate::chat::SessaoChat;
use crate::config::Config;
use crate::daemon::{self, Desfecho};
use crate::db::Banco;
use crate::eventos;
use crate::ferramentas::CaixaDeFerramentas;
use crate::orquestrador::Orquestrador;
use crate::tempo::agora_ms;

use super::agenda;
use super::comandos::{self, CHAVE_CONVERSA};
use super::confianca::{self, MensagemDiscord, Remetente};
use super::protocolo::{DoAdaptador, MAX_LINHA, ParaAdaptador, VERSAO};
use super::registro::{self, NovaEntrada, NovaSaida};

/// Linhas esperando para ir ao adaptador. Cheio (adaptador que não lê), o
/// que não couber fica para depois: as saídas estão no banco.
const FILA_DO_SOCKET: usize = 256;
/// Prazo para o adaptador dizer `ola` depois de conectar.
const PRAZO_OLA: Duration = Duration::from_secs(10);
/// Mesmo sem aviso, a conversa e a entrega olham a fila de tempos em tempos.
const REVISAO_CONVERSA: Duration = Duration::from_secs(30);
const REVISAO_ENTREGA: Duration = Duration::from_secs(5);

/// A conexão atual com o adaptador.
struct Ligacao {
    geracao: u64,
    linhas: mpsc::Sender<String>,
}

pub struct Gateway {
    config: Config,
    /// Conexão PRÓPRIA ao banco (não a do loop do daemon).
    banco: Banco,
    orquestrador: Orquestrador,
    /// As ferramentas da conversa (as mesmas do `abiyss chat`).
    caixa: Arc<CaixaDeFerramentas>,
    ligacao: Mutex<Option<Ligacao>>,
    acordar_conversa: Notify,
    acordar_entrega: Notify,
    ocupado: AtomicBool,
}

/// Apaga o arquivo do socket quando o gateway para.
struct ArquivoSocket(PathBuf);

impl Drop for ArquivoSocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Gateway {
    /// `banco` deve ser uma conexão própria (`Banco::outra_conexao`).
    pub fn novo(
        config: Config,
        banco: Banco,
        orquestrador: Orquestrador,
        caixa: Arc<CaixaDeFerramentas>,
    ) -> Arc<Gateway> {
        Arc::new(Gateway {
            config,
            banco,
            orquestrador,
            caixa,
            ligacao: Mutex::new(None),
            acordar_conversa: Notify::new(),
            acordar_entrega: Notify::new(),
            ocupado: AtomicBool::new(false),
        })
    }

    /// Caminho do socket.
    pub fn caminho_socket(config: &Config) -> PathBuf {
        config.resolver(&config.gateway.socket)
    }

    /// Roda até ser abortado. Nunca devolve erro: falhas vão para o log e
    /// o daemon segue sem gateway.
    pub async fn rodar(self: Arc<Self>) {
        let caminho = Gateway::caminho_socket(&self.config);
        let ouvinte = match abrir_socket(&caminho) {
            Ok(o) => o,
            Err(e) => {
                tracing::error!(
                    "gateway: não consegui abrir o socket: {e:#}; seguindo sem gateway"
                );
                return;
            }
        };
        let _arquivo = ArquivoSocket(caminho.clone());
        tracing::info!("gateway: esperando o adaptador em {}", caminho.display());
        self.recuperar();
        tokio::join!(
            Arc::clone(&self).aceitar(ouvinte),
            self.conversar(),
            self.entregar(),
            self.agendar()
        );
    }

    /// Ao subir: avisa o dono das mensagens que ficaram no meio de um turno.
    fn recuperar(&self) {
        let interrompidas = match registro::recuperar_interrompidas(&self.banco, agora_ms()) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("gateway: não consegui recuperar a fila: {e:#}");
                return;
            }
        };
        for e in &interrompidas {
            let aviso = "Reiniciei no meio da resposta a esta mensagem. Ela já está no \
                         histórico da conversa; mande \"continue\" se ainda quiser a resposta.";
            let destino = self.destino(&e.canal_id);
            if let Err(erro) = registro::nova_saida(
                &self.banco,
                &NovaSaida {
                    tipo: "aviso",
                    estado: "pendente",
                    canal_id: destino.as_deref(),
                    responde_a: Some(&e.discord_id),
                    pedido_id: None,
                    conteudo: aviso,
                },
                agora_ms(),
            ) {
                tracing::warn!("gateway: não consegui avisar a interrupção: {erro:#}");
            }
        }
    }

    /// Onde responder a uma mensagem do dono: no canal permitido, se veio
    /// de lá; senão, na DM dele (`None`).
    fn destino(&self, canal_id: &str) -> Option<String> {
        (self.config.gateway.canal() == Some(canal_id)).then(|| canal_id.to_string())
    }

    /// Manda uma linha ao adaptador, se houver um conectado e a fila dele
    /// tiver espaço. Nunca espera.
    fn mandar(&self, mensagem: &ParaAdaptador) -> bool {
        let ligacao = self.ligacao.lock().unwrap_or_else(|e| e.into_inner());
        match ligacao.as_ref() {
            Some(l) => l.linhas.try_send(mensagem.linha()).is_ok(),
            None => false,
        }
    }

    /// Há um adaptador conectado (e já apresentado)?
    pub fn conectado(&self) -> bool {
        self.ligacao
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    // -----------------------------------------------------------------------
    // Conexões
    // -----------------------------------------------------------------------

    async fn aceitar(self: Arc<Self>, ouvinte: UnixListener) {
        // Abortar o gateway solta este JoinSet, que aborta as conexões.
        let mut conexoes = JoinSet::new();
        let mut geracao = 0u64;
        loop {
            tokio::select! {
                aceita = ouvinte.accept() => match aceita {
                    Ok((socket, _)) => {
                        geracao += 1;
                        // Uma conexão por vez: a nova (ex.: o adaptador
                        // reiniciou antes de a antiga cair) substitui a antiga.
                        conexoes.abort_all();
                        *self.ligacao.lock().unwrap_or_else(|e| e.into_inner()) = None;
                        conexoes.spawn(Arc::clone(&self).atender(socket, geracao));
                    }
                    Err(e) => {
                        tracing::warn!("gateway: erro ao aceitar conexão: {e}");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                },
                Some(_) = conexoes.join_next(), if !conexoes.is_empty() => {}
            }
        }
    }

    async fn atender(self: Arc<Self>, socket: UnixStream, geracao: u64) {
        let (leitura, mut escrita) = socket.into_split();
        let mut leitor = BufReader::new(leitura);
        // Primeiro o `ola`, com a versão do protocolo.
        match tokio::time::timeout(PRAZO_OLA, ler_linha(&mut leitor)).await {
            Ok(Ok(Some(linha))) => match serde_json::from_str::<DoAdaptador>(&linha) {
                Ok(DoAdaptador::Ola { versao }) if versao == VERSAO => {}
                Ok(DoAdaptador::Ola { versao }) => {
                    tracing::error!(
                        "gateway: adaptador com protocolo v{versao}, o kernel fala v{VERSAO}; \
                         atualize os dois juntos"
                    );
                    return;
                }
                _ => {
                    tracing::warn!("gateway: conexão sem `ola`; fechada");
                    return;
                }
            },
            _ => {
                tracing::warn!("gateway: conexão sem `ola` no prazo; fechada");
                return;
            }
        }
        let ola = match self.ola() {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!("gateway: não consegui montar o `ola`: {e:#}");
                return;
            }
        };
        if escrever(&mut escrita, &ola.linha()).await.is_err() {
            return;
        }
        let (linhas, mut pendentes) = mpsc::channel::<String>(FILA_DO_SOCKET);
        *self.ligacao.lock().unwrap_or_else(|e| e.into_inner()) = Some(Ligacao { geracao, linhas });
        tracing::info!("gateway: adaptador conectado");
        self.acordar_entrega.notify_one();

        let enviar = async {
            while let Some(linha) = pendentes.recv().await {
                if escrever(&mut escrita, &linha).await.is_err() {
                    break;
                }
            }
        };
        let receber = async {
            loop {
                match ler_linha(&mut leitor).await {
                    Ok(Some(linha)) => self.tratar_linha(&linha),
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!("gateway: conexão com o adaptador: {e}");
                        break;
                    }
                }
            }
        };
        tokio::select! {
            _ = enviar => {}
            _ = receber => {}
        }
        let mut ligacao = self.ligacao.lock().unwrap_or_else(|e| e.into_inner());
        if ligacao.as_ref().is_some_and(|l| l.geracao == geracao) {
            *ligacao = None;
        }
        tracing::info!("gateway: adaptador desconectado");
    }

    fn ola(&self) -> anyhow::Result<ParaAdaptador> {
        let g = &self.config.gateway;
        let (ultimo_dm, ultimo_canal) =
            registro::ultimos_recebidos(&self.banco, &g.dono_discord_id, g.canal())?;
        Ok(ParaAdaptador::Ola {
            versao: VERSAO,
            dono_id: g.dono_discord_id.clone(),
            canal_id: g.canal().map(str::to_string),
            ultimo_dm,
            ultimo_canal,
        })
    }

    fn tratar_linha(&self, linha: &str) {
        let mensagem = match serde_json::from_str::<DoAdaptador>(linha) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("gateway: linha inválida do adaptador: {e}");
                return;
            }
        };
        let agora = agora_ms();
        let resultado = match mensagem {
            DoAdaptador::Ola { .. } => Ok(()),
            DoAdaptador::Mensagem { mensagem } => self.receber(&mensagem).map(|estado| {
                self.mandar(&ParaAdaptador::Recebido {
                    id: mensagem.id.clone(),
                    estado: estado.to_string(),
                });
            }),
            DoAdaptador::Enviado { referencia, ids } => {
                registro::confirmar_entrega(&self.banco, referencia, &ids, agora)
            }
            DoAdaptador::Falhou { referencia, erro } => {
                tracing::warn!("gateway: o adaptador não entregou a saída #{referencia}: {erro}");
                registro::registrar_falha(&self.banco, referencia)
            }
        };
        if let Err(e) = resultado {
            tracing::warn!("gateway: {e:#}");
        }
    }

    // -----------------------------------------------------------------------
    // Entrada
    // -----------------------------------------------------------------------

    /// Grava e encaminha uma mensagem do Discord. Devolve o estado que vai
    /// no `recebido`.
    pub fn receber(&self, m: &MensagemDiscord) -> anyhow::Result<&'static str> {
        let agora = agora_ms();
        let nova = |tipo, estado, conteudo, origem_externa| NovaEntrada {
            mensagem: m,
            tipo,
            estado,
            conteudo,
            origem_externa,
            pedido_id: None,
        };
        match confianca::classificar(&self.config.gateway, m) {
            Remetente::Ignorado(motivo) => {
                // O texto de quem não é o dono (fora do canal) não é guardado.
                let gravada = registro::registrar_entrada(
                    &self.banco,
                    &nova("ignorado", "ignorada", None, None),
                    agora,
                )?;
                tracing::debug!(
                    "gateway: mensagem {} ignorada ({})",
                    m.id,
                    motivo.como_texto()
                );
                Ok(if gravada.is_some() {
                    "ignorado"
                } else {
                    "duplicado"
                })
            }
            Remetente::Externo { origem } => {
                let gravada = registro::registrar_entrada(
                    &self.banco,
                    &nova("externo", "externa", Some(&m.texto), Some(&origem)),
                    agora,
                )?;
                if gravada.is_none() {
                    return Ok("duplicado");
                }
                // Para o heartbeat, rotulado como conteúdo externo.
                eventos::publicar_com_origem(
                    &self.banco,
                    eventos::TIPO_DISCORD,
                    &format!("canal:{}", m.canal_id),
                    &format!(
                        "Mensagem no canal do Discord, de {} (id {}{}): {}",
                        if m.autor_nome.is_empty() {
                            "?"
                        } else {
                            &m.autor_nome
                        },
                        m.autor_id,
                        if m.autor_bot { ", bot" } else { "" },
                        m.texto
                    ),
                    Some(&origem),
                )?;
                Ok("externo")
            }
            Remetente::Dono => self.receber_do_dono(m, agora),
        }
    }

    fn receber_do_dono(&self, m: &MensagemDiscord, agora: i64) -> anyhow::Result<&'static str> {
        let nova = |tipo, estado| NovaEntrada {
            mensagem: m,
            tipo,
            estado,
            conteudo: Some(&m.texto),
            origem_externa: None,
            pedido_id: None,
        };
        // "Responder" numa mensagem de pedido: é a resposta àquele pedido.
        if let Some(citada) = &m.responde_a
            && let Some(saida) = registro::saida_do_discord(&self.banco, citada)?
            && let Some(pedido) = saida.pedido_id
        {
            let entrada = NovaEntrada {
                pedido_id: Some(pedido),
                ..nova("resposta_pedido", "respondida")
            };
            if registro::registrar_entrada(&self.banco, &entrada, agora)?.is_none() {
                return Ok("duplicado");
            }
            // A resposta é do dono, direto (sem janela de conversa no meio).
            let texto = match crate::pedidos::responder(&self.banco, pedido, &m.texto, None) {
                Ok(p) => format!(
                    "✅ Pedido #{} respondido. O Abiyss vê a resposta no próximo ciclo.",
                    p.id
                ),
                Err(e) => format!("Não registrei a resposta: {e:#}"),
            };
            self.responder(m, "comando", &texto)?;
            return Ok("resposta_pedido");
        }
        if let Some(comando) = comandos::interpretar(&m.texto) {
            if registro::registrar_entrada(&self.banco, &nova("comando", "respondida"), agora)?
                .is_none()
            {
                return Ok("duplicado");
            }
            let texto = comandos::executar(comando, &self.config, &self.banco)
                .unwrap_or_else(|e| format!("Não consegui executar o comando: {e:#}"));
            self.responder(m, "comando", &texto)?;
            return Ok("comando");
        }
        if registro::registrar_entrada(&self.banco, &nova("dono", "pendente"), agora)?.is_none() {
            return Ok("duplicado");
        }
        self.acordar_conversa.notify_one();
        Ok(if self.ocupado.load(Ordering::Relaxed) {
            "na_fila"
        } else {
            "recebida"
        })
    }

    /// Põe na fila de saída uma resposta direta a `m` (no mesmo lugar).
    fn responder(&self, m: &MensagemDiscord, tipo: &str, texto: &str) -> anyhow::Result<i64> {
        let destino = self.destino(&m.canal_id);
        let id = registro::nova_saida(
            &self.banco,
            &NovaSaida {
                tipo,
                estado: "pendente",
                canal_id: destino.as_deref(),
                responde_a: Some(&m.id),
                pedido_id: None,
                conteudo: texto,
            },
            agora_ms(),
        )?;
        self.acordar_entrega.notify_one();
        Ok(id)
    }

    // -----------------------------------------------------------------------
    // Conversa
    // -----------------------------------------------------------------------

    async fn conversar(&self) {
        loop {
            let lote = registro::tomar_lote_do_dono(&self.banco).unwrap_or_else(|e| {
                tracing::warn!("gateway: não consegui ler a fila de entrada: {e:#}");
                Vec::new()
            });
            if lote.is_empty() {
                tokio::select! {
                    _ = self.acordar_conversa.notified() => {}
                    _ = tokio::time::sleep(REVISAO_CONVERSA) => {}
                }
                continue;
            }
            self.ocupado.store(true, Ordering::Relaxed);
            if let Err(e) = self.turno(&lote).await {
                tracing::error!("gateway: turno da conversa: {e:#}");
            }
            self.ocupado.store(false, Ordering::Relaxed);
        }
    }

    /// Um turno com todas as mensagens do lote (as que chegaram enquanto o
    /// anterior rodava vêm juntas, na ordem).
    async fn turno(&self, lote: &[registro::Entrada]) -> anyhow::Result<()> {
        let ultima = lote.last().context("lote vazio")?;
        let texto = lote
            .iter()
            .map(|e| e.conteudo.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let (resposta, estado) = match self.pensar(&texto).await {
            Ok(r) => (r, "respondida"),
            Err(e) => (format!("⚠ Não consegui responder: {e:#}"), "erro"),
        };
        let ids: Vec<i64> = lote.iter().map(|e| e.id).collect();
        registro::concluir_entradas(&self.banco, &ids, estado, agora_ms())?;
        let destino = self.destino(&ultima.canal_id);
        registro::nova_saida(
            &self.banco,
            &NovaSaida {
                tipo: "resposta",
                estado: "pendente",
                canal_id: destino.as_deref(),
                responde_a: Some(&ultima.discord_id),
                pedido_id: None,
                conteudo: &resposta,
            },
            agora_ms(),
        )?;
        self.acordar_entrega.notify_one();
        Ok(())
    }

    /// A conversa do gateway: a de sempre (entre reinícios) ou uma nova.
    fn sessao(&self) -> anyhow::Result<SessaoChat> {
        let (config, orq, banco, caixa) = (
            self.config.clone(),
            self.orquestrador.clone(),
            self.banco.clone(),
            Arc::clone(&self.caixa),
        );
        if let Some(id) = comandos::conversa_atual(&self.banco)?
            && let Ok(s) = SessaoChat::retomar(
                config.clone(),
                orq.clone(),
                banco.clone(),
                caixa.clone(),
                id,
            )
        {
            return Ok(s);
        }
        let sessao = SessaoChat::nova(config, orq, banco, caixa)?;
        daemon::gravar_estado(&self.banco, CHAVE_CONVERSA, &sessao.conversa.to_string())?;
        Ok(sessao)
    }

    /// O turno de conversa, com tempo máximo e pânico isolado.
    async fn pensar(&self, texto: &str) -> anyhow::Result<String> {
        let mut sessao = self.sessao()?;
        let limite = Duration::from_secs(self.config.gateway.max_duracao_turno_segundos);
        match daemon::supervisionar(limite, sessao.enviar(texto, None)).await {
            Desfecho::Ok(Ok(r)) if r.texto.trim().is_empty() => Ok("(resposta vazia)".into()),
            Desfecho::Ok(Ok(r)) => Ok(r.texto),
            Desfecho::Ok(Err(e)) => Err(e),
            Desfecho::TempoEsgotado => bail!("a resposta passou de {} s", limite.as_secs()),
            Desfecho::Panico(m) => bail!("pânico no turno: {m}"),
        }
    }

    // -----------------------------------------------------------------------
    // Agenda
    // -----------------------------------------------------------------------

    async fn agendar(&self) {
        let intervalo = Duration::from_secs(self.config.gateway.verificacao_segundos);
        loop {
            let g = &self.config.gateway;
            let pedidos = agenda::agendar_pedidos(&self.banco, g, agora_ms());
            let resumo = agenda::agendar_resumo(&self.banco, g);
            match (pedidos, resumo) {
                (Ok(0), Ok(false)) => {}
                (Ok(_), Ok(_)) => self.acordar_entrega.notify_one(),
                (p, r) => {
                    for e in [p.err(), r.err()].into_iter().flatten() {
                        tracing::warn!("gateway: agenda: {e:#}");
                    }
                }
            }
            tokio::time::sleep(intervalo).await;
        }
    }

    // -----------------------------------------------------------------------
    // Entrega
    // -----------------------------------------------------------------------

    async fn entregar(&self) {
        loop {
            if self.conectado()
                && let Err(e) = self.entregar_pendentes()
            {
                tracing::warn!("gateway: entrega: {e:#}");
            }
            tokio::select! {
                _ = self.acordar_entrega.notified() => {}
                _ = tokio::time::sleep(REVISAO_ENTREGA) => {}
            }
        }
    }

    fn entregar_pendentes(&self) -> anyhow::Result<()> {
        let agora = agora_ms();
        for s in registro::saidas_a_entregar(&self.banco, agora, 50)? {
            if let Some(pedido) = s.pedido_id
                && !agenda::pedido_ainda_pendente(&self.banco, pedido)?
            {
                registro::cancelar_saida(&self.banco, s.id, agora)?;
                continue;
            }
            let mensagem = ParaAdaptador::Enviar {
                referencia: s.id,
                canal_id: s.canal_id,
                responder_a: s.responde_a,
                texto: s.conteudo,
            };
            if !self.mandar(&mensagem) {
                break;
            }
            registro::marcar_tentativa(&self.banco, s.id, agora)?;
        }
        Ok(())
    }
}

/// Abre o socket: só o usuário do daemon alcança (pasta 0700 se fomos nós
/// que a criamos, socket 0600). Um socket velho (queda) é apagado; outro
/// tipo de arquivo no lugar, não.
fn abrir_socket(caminho: &Path) -> anyhow::Result<UnixListener> {
    if let Some(pasta) = caminho.parent()
        && !pasta.exists()
    {
        std::fs::create_dir_all(pasta)
            .with_context(|| format!("não consegui criar {}", pasta.display()))?;
        std::fs::set_permissions(pasta, std::fs::Permissions::from_mode(0o700))?;
    }
    match std::fs::symlink_metadata(caminho) {
        Ok(meta) if meta.file_type().is_socket() => std::fs::remove_file(caminho)?,
        Ok(_) => bail!(
            "{} existe e não é um socket (não vou apagar)",
            caminho.display()
        ),
        Err(_) => {}
    }
    let ouvinte = UnixListener::bind(caminho)
        .with_context(|| format!("não consegui abrir {}", caminho.display()))?;
    std::fs::set_permissions(caminho, std::fs::Permissions::from_mode(0o600))?;
    Ok(ouvinte)
}

/// Uma linha (sem o `\n`); `None` no fim da conexão. Linha maior que
/// `MAX_LINHA` é erro (a conexão é fechada).
async fn ler_linha<R: AsyncBufRead + Unpin>(leitor: &mut R) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let n = leitor
        .take(MAX_LINHA as u64 + 1)
        .read_until(b'\n', &mut bytes)
        .await?;
    if n == 0 {
        return Ok(None);
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    } else if bytes.len() > MAX_LINHA {
        return Err(std::io::Error::other("linha grande demais"));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| std::io::Error::other("linha que não é UTF-8"))
}

async fn escrever<W: AsyncWriteExt + Unpin>(escrita: &mut W, linha: &str) -> std::io::Result<()> {
    escrita.write_all(linha.as_bytes()).await?;
    escrita.write_all(b"\n").await
}
