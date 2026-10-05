//! Ponte MCP: o kernel conversa com servidores MCP usando o crate `rmcp`
//! e expõe as ferramentas deles ao modelo. Dois transportes:
//! - **processo filho (stdio)**: servidores em Python, em `recursos/`
//!   (`comando` + `args` no abiyss.toml);
//! - **HTTP "streamable"**: servidores que já estão rodando, locais ou
//!   remotos, como o qmd (`url` no abiyss.toml).
//!
//! - Nome exposto ao modelo: `<servidor>__<ferramenta>`.
//! - `expor = false`: as ferramentas NÃO aparecem para o modelo; só o
//!   kernel usa o servidor (ex.: o qmd por trás de `memoria_buscar`).
//! - Os processos filhos recebem um ambiente LIMPO (as chaves do NIM
//!   não vazam para código que o Abiyss poderá editar no futuro).
//! - Token de servidor HTTP só por variável de ambiente (`token_env`).
//! - Um servidor que não sobe é registrado no log e ignorado: o Abiyss
//!   continua funcionando sem ele.
//! - Supervisão (no daemon, `supervisionar`): servidor que caiu, que
//!   estourou uma chamada (`timeout_segundos`), que passou de
//!   `max_memoria_mb` (árvore de processos inteira) ou de
//!   `max_vida_segundos` é morto (o grupo de processos todo) e sobe de novo.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock, Tool};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::nim::Ferramenta;
use crate::processos;

/// Separador entre o nome do servidor e o da ferramenta.
pub const SEPARADOR: &str = "__";

/// Variáveis de ambiente repassadas aos servidores. Todo o resto
/// (incluindo as chaves do NIM) fica de fora.
const AMBIENTE_PERMITIDO: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LANG",
    "LC_ALL",
    "TZ",
    "TMPDIR",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "REQUESTS_CA_BUNDLE",
];

/// Seção `[mcp]` do abiyss.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConfigMcp {
    /// Tempo máximo para um servidor subir e listar as ferramentas.
    /// (Na primeira vez o `uv` pode precisar baixar dependências.)
    pub timeout_inicio_segundos: u64,
    /// De quanto em quanto tempo o daemon confere os servidores.
    pub supervisao_segundos: u64,
    /// Memória máxima (RSS do processo + descendentes) de um servidor stdio.
    pub max_memoria_mb: u64,
    /// Tempo máximo de vida de um servidor stdio antes de ser reiniciado
    /// (0 = sem limite).
    pub max_vida_segundos: u64,
    /// Resultado de ferramenta maior que isto é cortado (com aviso).
    pub max_bytes_resultado: usize,
    pub servidores: Vec<ConfigServidorMcp>,
}

impl Default for ConfigMcp {
    fn default() -> Self {
        ConfigMcp {
            timeout_inicio_segundos: 120,
            supervisao_segundos: 30,
            max_memoria_mb: 512,
            max_vida_segundos: 0,
            max_bytes_resultado: 1_000_000,
            servidores: Vec::new(),
        }
    }
}

/// Um item `[[mcp.servidores]]`: OU `comando` (processo filho) OU `url` (HTTP).
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigServidorMcp {
    /// Nome curto (letras, números, `_` ou `-`); vira prefixo das ferramentas.
    pub nome: String,
    /// Programa a executar (transporte stdio). Vazio quando há `url`.
    #[serde(default)]
    pub comando: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Pasta onde o processo roda (relativa à raiz do projeto).
    #[serde(default)]
    pub diretorio: Option<String>,
    /// Variáveis extras para este servidor (não coloque segredos do NIM aqui).
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Tempo máximo de cada chamada de ferramenta.
    #[serde(default = "padrao_timeout_chamada")]
    pub timeout_segundos: u64,
    #[serde(default = "padrao_ativo")]
    pub ativo: bool,
    /// Endpoint MCP de um servidor que já está rodando (transporte HTTP
    /// "streamable"), ex.: "http://localhost:8181/mcp".
    #[serde(default)]
    pub url: Option<String>,
    /// NOME da variável de ambiente com o token (enviado como Bearer).
    /// O token em si nunca fica no abiyss.toml.
    #[serde(default)]
    pub token_env: Option<String>,
    /// `false` = as ferramentas não são oferecidas ao modelo (só o kernel usa).
    #[serde(default = "padrao_ativo")]
    pub expor: bool,
}

impl ConfigServidorMcp {
    /// "http" ou "stdio".
    pub fn transporte(&self) -> &'static str {
        if self.url.is_some() { "http" } else { "stdio" }
    }
}

fn padrao_timeout_chamada() -> u64 {
    60
}

fn padrao_ativo() -> bool {
    true
}

/// Um servidor MCP em execução.
struct ServidorMcp {
    nome: String,
    /// Config original (para subir de novo ao reiniciar).
    config: ConfigServidorMcp,
    /// Ferramentas vistas na primeira subida (os nomes expostos ficam fixos).
    ferramentas: Vec<Tool>,
    timeout: Duration,
    /// As ferramentas aparecem para o modelo?
    expor: bool,
    transporte: &'static str,
    /// Trocada inteira quando o servidor é reiniciado. As chamadas seguram
    /// a leitura; o reinício espera elas terminarem (no máximo `timeout`).
    conexao: RwLock<Conexao>,
    /// Uma chamada estourou o tempo: o servidor provavelmente travou.
    pedir_reinicio: AtomicBool,
    reinicios: AtomicU32,
}

/// A conexão viva com um servidor.
struct Conexao {
    cliente: RunningService<RoleClient, ()>,
    /// PID do processo filho, que também é o ID do grupo de processos
    /// (`None` no transporte HTTP).
    pid: Option<u32>,
    inicio: Instant,
}

impl Conexao {
    /// Fecha com educação e depois mata o grupo (pega netos que sobrarem).
    async fn encerrar(self) {
        let pid = self.pid;
        if let Err(e) = self.cliente.cancel().await {
            tracing::debug!("erro ao encerrar conexão MCP: {e}");
        }
        if let Some(pid) = pid {
            processos::matar_grupo(pid);
        }
    }
}

/// Resposta de uma ferramenta MCP chamada pelo próprio kernel.
#[derive(Debug, Clone, PartialEq)]
pub struct RespostaMcp {
    /// Texto juntado dos blocos de conteúdo.
    pub texto: String,
    /// `structuredContent`, quando o servidor manda.
    pub estruturado: Option<Value>,
}

/// Onde encontrar uma ferramenta exposta.
#[derive(Debug, Clone)]
struct Endereco {
    indice_servidor: usize,
    nome_original: String,
}

/// Todos os servidores MCP ativos.
#[derive(Default)]
pub struct PonteMcp {
    servidores: Vec<ServidorMcp>,
    /// nome exposto → (servidor, nome original)
    enderecos: HashMap<String, Endereco>,
    /// Raiz do projeto (para resolver `diretorio` ao reiniciar).
    raiz: PathBuf,
    limites: ConfigMcp,
}

impl PonteMcp {
    /// Ponte sem nenhum servidor.
    pub fn vazia() -> PonteMcp {
        PonteMcp::default()
    }

    /// Sobe todos os servidores ativos da config.
    pub async fn iniciar(config: &Config) -> PonteMcp {
        let mut ponte = PonteMcp {
            raiz: config.raiz.clone(),
            limites: ConfigMcp {
                servidores: Vec::new(),
                ..config.mcp.clone()
            },
            ..PonteMcp::default()
        };
        let timeout = Duration::from_secs(config.mcp.timeout_inicio_segundos);
        for item in config.mcp.servidores.iter().filter(|s| s.ativo) {
            match iniciar_servidor(&config.raiz, item, timeout).await {
                Ok(servidor) => {
                    tracing::info!(
                        "MCP '{}' ({}) ativo com {} ferramenta(s){}",
                        servidor.nome,
                        servidor.transporte,
                        servidor.ferramentas.len(),
                        if servidor.expor {
                            ""
                        } else {
                            ", só para o kernel"
                        }
                    );
                    ponte.adicionar(servidor);
                }
                Err(e) => tracing::error!("MCP '{}' não subiu: {e:#}", item.nome),
            }
        }
        ponte
    }

    fn adicionar(&mut self, servidor: ServidorMcp) {
        let indice = self.servidores.len();
        // Servidor só do kernel: as ferramentas não ficam chamáveis pelo modelo.
        let ferramentas = if servidor.expor {
            servidor.ferramentas.as_slice()
        } else {
            &[]
        };
        for ferramenta in ferramentas {
            let exposto = nome_exposto(&servidor.nome, &ferramenta.name);
            if self.enderecos.contains_key(&exposto) {
                tracing::warn!("ferramenta MCP duplicada ignorada: {exposto}");
                continue;
            }
            self.enderecos.insert(
                exposto,
                Endereco {
                    indice_servidor: indice,
                    nome_original: ferramenta.name.to_string(),
                },
            );
        }
        self.servidores.push(servidor);
    }

    /// Nomes dos servidores ativos.
    pub fn servidores(&self) -> Vec<String> {
        self.servidores.iter().map(|s| s.nome.clone()).collect()
    }

    /// Servidores ativos com transporte e visibilidade (para listagens).
    pub fn descricao_servidores(&self) -> Vec<String> {
        self.servidores
            .iter()
            .map(|s| {
                let visibilidade = if s.expor { "" } else { ", só para o kernel" };
                format!("{} ({}{visibilidade})", s.nome, s.transporte)
            })
            .collect()
    }

    /// Ferramentas de um servidor (inclusive dos que não são expostos).
    pub fn ferramentas_do_servidor(&self, servidor: &str) -> Option<&[Tool]> {
        self.servidores
            .iter()
            .find(|s| s.nome == servidor)
            .map(|s| s.ferramentas.as_slice())
    }

    /// Chamada feita pelo KERNEL (não pelo modelo) a uma ferramenta pelo
    /// nome original, em qualquer servidor ativo (exposto ou não).
    pub async fn chamar_no_servidor(
        &self,
        servidor: &str,
        ferramenta: &str,
        argumentos: Value,
    ) -> anyhow::Result<RespostaMcp> {
        let indice = self
            .servidores
            .iter()
            .position(|s| s.nome == servidor)
            .with_context(|| format!("servidor MCP '{servidor}' não está ativo"))?;
        let resultado = self.chamar_interno(indice, ferramenta, argumentos).await?;
        let texto = self.limitar(texto_do_resultado(&resultado));
        if resultado.is_error == Some(true) {
            bail!("{texto}");
        }
        Ok(RespostaMcp {
            texto,
            estruturado: resultado.structured_content.clone(),
        })
    }

    /// A ferramenta com este nome exposto existe?
    pub fn tem(&self, nome: &str) -> bool {
        self.enderecos.contains_key(nome)
    }

    /// Definições das ferramentas MCP no formato do NIM (só as expostas).
    pub fn definicoes(&self) -> Vec<Ferramenta> {
        let mut lista = Vec::new();
        for servidor in self.servidores.iter().filter(|s| s.expor) {
            for ferramenta in &servidor.ferramentas {
                let exposto = nome_exposto(&servidor.nome, &ferramenta.name);
                let descricao = ferramenta
                    .description
                    .as_deref()
                    .unwrap_or("(sem descrição)");
                let esquema = Value::Object(ferramenta.input_schema.as_ref().clone());
                lista.push(Ferramenta::nova(
                    exposto,
                    format!("[MCP {}] {descricao}", servidor.nome),
                    esquema,
                ));
            }
        }
        lista
    }

    /// Chama uma ferramenta pelo nome exposto e devolve o texto do resultado.
    pub async fn chamar(&self, nome: &str, argumentos: Value) -> anyhow::Result<String> {
        let endereco = self
            .enderecos
            .get(nome)
            .with_context(|| format!("ferramenta MCP desconhecida: {nome}"))?;
        let resultado = self
            .chamar_interno(
                endereco.indice_servidor,
                &endereco.nome_original,
                argumentos,
            )
            .await?;
        let texto = self.limitar(texto_do_resultado(&resultado));
        if resultado.is_error == Some(true) {
            bail!("{texto}");
        }
        Ok(texto)
    }

    /// Corta resultados maiores que `max_bytes_resultado`.
    fn limitar(&self, texto: String) -> String {
        let maximo = self.limites.max_bytes_resultado;
        if texto.len() <= maximo {
            return texto;
        }
        let mut corte = maximo;
        while !texto.is_char_boundary(corte) {
            corte -= 1;
        }
        format!(
            "{}\n[… resultado cortado: {} de {} bytes (mcp.max_bytes_resultado)]",
            &texto[..corte],
            corte,
            texto.len()
        )
    }

    /// Chamada com tempo limite, comum aos dois caminhos acima.
    async fn chamar_interno(
        &self,
        indice: usize,
        ferramenta: &str,
        argumentos: Value,
    ) -> anyhow::Result<CallToolResult> {
        let servidor = &self.servidores[indice];
        let nome = format!("{}{SEPARADOR}{ferramenta}", servidor.nome);
        let mut parametros = CallToolRequestParams::new(ferramenta.to_string());
        if let Value::Object(mapa) = argumentos {
            parametros = parametros.with_arguments(mapa);
        }
        let conexao = servidor.conexao.read().await;
        let chamada = tokio::time::timeout(servidor.timeout, conexao.cliente.call_tool(parametros));
        let Ok(resultado) = chamada.await else {
            // Travou: a supervisão mata e sobe de novo na próxima rodada.
            servidor.pedir_reinicio.store(true, Ordering::Relaxed);
            bail!(
                "ferramenta {nome} passou do tempo limite ({} s)",
                servidor.timeout.as_secs()
            );
        };
        resultado.with_context(|| format!("falha ao chamar {nome}"))
    }

    /// Supervisão contínua (roda no daemon até a tarefa ser abortada).
    pub async fn supervisionar(self: Arc<Self>) {
        let intervalo = Duration::from_secs(self.limites.supervisao_segundos.max(1));
        let mut tique = tokio::time::interval(intervalo);
        tique.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tique.tick().await; // o primeiro tique é imediato: os servidores acabaram de subir
        loop {
            tique.tick().await;
            self.verificar().await;
        }
    }

    /// Uma rodada de supervisão: reinicia quem precisa. Devolve os nomes
    /// dos servidores reiniciados.
    pub async fn verificar(&self) -> Vec<String> {
        let mut reiniciados = Vec::new();
        for servidor in &self.servidores {
            let Some(motivo) = self.motivo_para_reiniciar(servidor).await else {
                continue;
            };
            tracing::warn!("MCP '{}': {motivo}; reiniciando", servidor.nome);
            match self.reiniciar(servidor).await {
                Ok(()) => reiniciados.push(servidor.nome.clone()),
                Err(e) => tracing::error!(
                    "MCP '{}' não subiu de novo (tenta na próxima rodada): {e:#}",
                    servidor.nome
                ),
            }
        }
        reiniciados
    }

    async fn motivo_para_reiniciar(&self, servidor: &ServidorMcp) -> Option<String> {
        if servidor.pedir_reinicio.load(Ordering::Relaxed) {
            return Some("uma chamada passou do tempo limite".to_string());
        }
        let conexao = servidor.conexao.read().await;
        if conexao.cliente.is_closed() || conexao.cliente.peer().is_transport_closed() {
            return Some("o servidor caiu".to_string());
        }
        // Limites de memória e de tempo de vida: só para processos filhos.
        let pid = conexao.pid?;
        let maximo = self.limites.max_memoria_mb * 1024 * 1024;
        if let Some(rss) = processos::rss_da_arvore_bytes(pid)
            && rss > maximo
        {
            return Some(format!(
                "usando {} MiB (limite mcp.max_memoria_mb = {})",
                rss / (1024 * 1024),
                self.limites.max_memoria_mb
            ));
        }
        let vida = self.limites.max_vida_segundos;
        if vida > 0 && conexao.inicio.elapsed() >= Duration::from_secs(vida) {
            return Some(format!("passou de mcp.max_vida_segundos = {vida}"));
        }
        None
    }

    /// Mata o servidor (o grupo inteiro) e sobe de novo.
    async fn reiniciar(&self, servidor: &ServidorMcp) -> anyhow::Result<()> {
        // Espera as chamadas em andamento (no máximo o `timeout` delas).
        let mut conexao = servidor.conexao.write().await;
        // Mata ANTES de subir o novo, para não ficar com dois na memória.
        if let Some(pid) = conexao.pid {
            processos::matar_grupo(pid);
        }
        let timeout = Duration::from_secs(self.limites.timeout_inicio_segundos);
        let (nova, ferramentas) = conectar(&self.raiz, &servidor.config, timeout).await?;
        let nomes = |lista: &[Tool]| lista.iter().map(|f| f.name.to_string()).collect::<Vec<_>>();
        if nomes(&ferramentas) != nomes(&servidor.ferramentas) {
            tracing::warn!(
                "MCP '{}' voltou com outras ferramentas; os nomes expostos continuam os da primeira subida",
                servidor.nome
            );
        }
        let velha = std::mem::replace(&mut *conexao, nova);
        drop(conexao);
        velha.encerrar().await;
        servidor.pedir_reinicio.store(false, Ordering::Relaxed);
        servidor.reinicios.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Quantas vezes o servidor foi reiniciado pela supervisão.
    pub fn reinicios(&self, servidor: &str) -> Option<u32> {
        self.servidores
            .iter()
            .find(|s| s.nome == servidor)
            .map(|s| s.reinicios.load(Ordering::Relaxed))
    }

    /// PID (= grupo) do processo filho de um servidor stdio.
    pub async fn pid(&self, servidor: &str) -> Option<u32> {
        let servidor = self.servidores.iter().find(|s| s.nome == servidor)?;
        servidor.conexao.read().await.pid
    }

    /// Encerra todos os servidores com educação (fecha o stdio e espera)
    /// e mata o que sobrar de cada grupo de processos.
    pub async fn encerrar(self) {
        for servidor in self.servidores {
            servidor.conexao.into_inner().encerrar().await;
        }
    }
}

async fn iniciar_servidor(
    raiz: &Path,
    item: &ConfigServidorMcp,
    timeout: Duration,
) -> anyhow::Result<ServidorMcp> {
    if item.nome.is_empty()
        || !item
            .nome
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!(
            "nome de servidor inválido: '{}' (use letras, números, _ ou -)",
            item.nome
        );
    }
    if item.url.is_some() && !item.comando.is_empty() {
        bail!("use 'comando' OU 'url', não os dois");
    }
    let (conexao, ferramentas) = conectar(raiz, item, timeout).await?;
    Ok(ServidorMcp {
        nome: item.nome.clone(),
        config: item.clone(),
        ferramentas,
        timeout: Duration::from_secs(item.timeout_segundos),
        expor: item.expor,
        transporte: item.transporte(),
        conexao: RwLock::new(conexao),
        pedir_reinicio: AtomicBool::new(false),
        reinicios: AtomicU32::new(0),
    })
}

/// Sobe (stdio) ou conecta (HTTP) e lista as ferramentas.
async fn conectar(
    raiz: &Path,
    item: &ConfigServidorMcp,
    timeout: Duration,
) -> anyhow::Result<(Conexao, Vec<Tool>)> {
    match &item.url {
        Some(url) => conectar_http(item, url, timeout).await,
        None => subir_processo(raiz, item, timeout).await,
    }
}

/// Transporte stdio: sobe o servidor como processo filho com ambiente limpo,
/// num grupo de processos próprio (para matar a árvore inteira depois).
async fn subir_processo(
    raiz: &Path,
    item: &ConfigServidorMcp,
    timeout: Duration,
) -> anyhow::Result<(Conexao, Vec<Tool>)> {
    if item.comando.is_empty() {
        bail!("falta 'comando' (processo filho) ou 'url' (HTTP)");
    }
    let diretorio = match item.diretorio.as_deref() {
        Some(d) if Path::new(d).is_absolute() => PathBuf::from(d),
        Some(d) => raiz.join(d),
        None => raiz.to_path_buf(),
    };

    let comando = tokio::process::Command::new(&item.comando).configure(|cmd| {
        cmd.args(&item.args).current_dir(&diretorio).env_clear();
        #[cfg(unix)]
        cmd.process_group(0);
        for nome in AMBIENTE_PERMITIDO {
            if let Ok(valor) = std::env::var(nome) {
                cmd.env(nome, valor);
            }
        }
        // Variáveis do próprio uv (cache, índice...) também passam.
        for (nome, valor) in std::env::vars() {
            if nome.starts_with("UV_") {
                cmd.env(nome, valor);
            }
        }
        cmd.envs(&item.env);
    });
    let transporte = TokioChildProcess::new(comando)
        .with_context(|| format!("não consegui executar '{}'", item.comando))?;
    let pid = transporte.id();

    let subir = async {
        let cliente = ().serve(transporte).await.context("handshake MCP falhou")?;
        let ferramentas = cliente
            .list_all_tools()
            .await
            .context("não consegui listar as ferramentas")?;
        anyhow::Ok((cliente, ferramentas))
    };
    let resultado = tokio::time::timeout(timeout, subir).await;
    let pronto = match resultado {
        Ok(pronto) => pronto,
        Err(_) => Err(anyhow::anyhow!(
            "servidor não respondeu em {} s",
            timeout.as_secs()
        )),
    };
    let (cliente, ferramentas) = match pronto {
        Ok(pronto) => pronto,
        Err(e) => {
            // Não subiu: não deixa o processo (nem netos) para trás.
            if let Some(pid) = pid {
                processos::matar_grupo(pid);
            }
            return Err(e);
        }
    };
    let conexao = Conexao {
        cliente,
        pid,
        inicio: Instant::now(),
    };
    Ok((conexao, ferramentas))
}

/// Transporte HTTP "streamable": conecta num servidor que já está rodando.
async fn conectar_http(
    item: &ConfigServidorMcp,
    url: &str,
    timeout: Duration,
) -> anyhow::Result<(Conexao, Vec<Tool>)> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        bail!("url precisa começar com http:// ou https:// (veio '{url}')");
    }
    let mut config_http = StreamableHttpClientTransportConfig::with_uri(url.to_string());
    if let Some(variavel) = &item.token_env {
        // A mensagem nunca mostra o valor do token.
        let token = std::env::var(variavel)
            .ok()
            .filter(|t| !t.trim().is_empty())
            .with_context(|| format!("variável {variavel} (token do servidor) não definida"))?;
        config_http = config_http.auth_header(token.trim().to_string());
    }
    let transporte = StreamableHttpClientTransport::from_config(config_http);

    let conectar = async {
        let cliente = ()
            .serve(transporte)
            .await
            .with_context(|| format!("handshake MCP em {url} falhou"))?;
        let ferramentas = cliente
            .list_all_tools()
            .await
            .context("não consegui listar as ferramentas")?;
        anyhow::Ok((cliente, ferramentas))
    };
    let (cliente, ferramentas) = tokio::time::timeout(timeout, conectar)
        .await
        .with_context(|| format!("{url} não respondeu em {} s", timeout.as_secs()))??;
    let conexao = Conexao {
        cliente,
        pid: None,
        inicio: Instant::now(),
    };
    Ok((conexao, ferramentas))
}

/// Nome exposto ao modelo: `servidor__ferramenta`, só com caracteres que a
/// API aceita (`[a-zA-Z0-9_-]`) e no máximo 64 caracteres.
pub fn nome_exposto(servidor: &str, ferramenta: &str) -> String {
    let bruto = format!("{servidor}{SEPARADOR}{ferramenta}");
    bruto
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

/// Junta o conteúdo de um resultado MCP num texto só.
fn texto_do_resultado(resultado: &CallToolResult) -> String {
    let mut partes = Vec::new();
    for bloco in &resultado.content {
        match bloco {
            ContentBlock::Text(t) => partes.push(t.text.clone()),
            ContentBlock::Image(_) => partes.push("[imagem omitida]".to_string()),
            ContentBlock::Audio(_) => partes.push("[áudio omitido]".to_string()),
            ContentBlock::Resource(_) => partes.push("[recurso embutido omitido]".to_string()),
            ContentBlock::ResourceLink(r) => partes.push(format!("[link de recurso: {}]", r.uri)),
            // O enum é `non_exhaustive`: versões futuras podem trazer novos tipos.
            _ => partes.push("[conteúdo de tipo desconhecido omitido]".to_string()),
        }
    }
    // Sem texto? Usa o resultado estruturado, se houver.
    if partes.is_empty()
        && let Some(estruturado) = &resultado.structured_content
    {
        partes.push(estruturado.to_string());
    }
    if partes.is_empty() {
        "(resultado vazio)".to_string()
    } else {
        partes.join("\n")
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn nome_exposto_e_seguro() {
        assert_eq!(nome_exposto("exemplo", "somar"), "exemplo__somar");
        assert_eq!(nome_exposto("ex", "a.b c/d"), "ex__a_b_c_d");
        assert_eq!(nome_exposto("x", &"y".repeat(100)).len(), 64);
    }

    #[test]
    fn config_http_sem_comando() {
        let texto = r#"
            [[servidores]]
            nome = "qmd"
            url = "http://localhost:8181/mcp"
            expor = false
        "#;
        let c: ConfigMcp = toml::from_str(texto).unwrap();
        let qmd = &c.servidores[0];
        assert_eq!(qmd.transporte(), "http");
        assert!(qmd.comando.is_empty());
        assert!(!qmd.expor);
        assert_eq!(qmd.token_env, None);
    }

    #[test]
    fn config_mcp_padrao_e_parse() {
        let texto = r#"
            timeout_inicio_segundos = 30
            [[servidores]]
            nome = "exemplo"
            comando = "uv"
            args = ["run", "servidor.py"]
            diretorio = "recursos/mcp/exemplo"
        "#;
        let c: ConfigMcp = toml::from_str(texto).unwrap();
        assert_eq!(c.servidores[0].timeout_segundos, 60);
        assert!(c.servidores[0].ativo);
        assert!(c.servidores[0].expor);
        assert_eq!(c.servidores[0].transporte(), "stdio");
        assert_eq!(ConfigMcp::default().servidores.len(), 0);
    }
}
