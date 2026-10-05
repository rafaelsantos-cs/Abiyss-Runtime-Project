//! Ponte MCP: o kernel sobe servidores MCP (em Python, em `recursos/`)
//! como processos filhos, conversa com eles por stdio usando o crate
//! `rmcp` e expõe as ferramentas deles ao modelo.
//!
//! - Nome exposto ao modelo: `<servidor>__<ferramenta>`.
//! - Os processos filhos recebem um ambiente LIMPO (as chaves do NIM
//!   não vazam para código que o Abiyss poderá editar no futuro).
//! - Um servidor que não sobe é registrado no log e ignorado: o Abiyss
//!   continua funcionando sem ele.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use anyhow::{Context, bail};
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock, Tool};
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde::Deserialize;
use serde_json::Value;

use crate::config::Config;
use crate::nim::Ferramenta;

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
    pub servidores: Vec<ConfigServidorMcp>,
}

impl Default for ConfigMcp {
    fn default() -> Self {
        ConfigMcp {
            timeout_inicio_segundos: 120,
            servidores: Vec::new(),
        }
    }
}

/// Um item `[[mcp.servidores]]`.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigServidorMcp {
    /// Nome curto (letras, números, `_` ou `-`); vira prefixo das ferramentas.
    pub nome: String,
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
    cliente: RunningService<RoleClient, ()>,
    ferramentas: Vec<Tool>,
    timeout: Duration,
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
}

impl PonteMcp {
    /// Ponte sem nenhum servidor.
    pub fn vazia() -> PonteMcp {
        PonteMcp::default()
    }

    /// Sobe todos os servidores ativos da config.
    pub async fn iniciar(config: &Config) -> PonteMcp {
        let mut ponte = PonteMcp::vazia();
        let timeout = Duration::from_secs(config.mcp.timeout_inicio_segundos);
        for item in config.mcp.servidores.iter().filter(|s| s.ativo) {
            match iniciar_servidor(config, item, timeout).await {
                Ok(servidor) => {
                    tracing::info!(
                        "MCP '{}' ativo com {} ferramenta(s)",
                        servidor.nome,
                        servidor.ferramentas.len()
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
        for ferramenta in &servidor.ferramentas {
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

    /// A ferramenta com este nome exposto existe?
    pub fn tem(&self, nome: &str) -> bool {
        self.enderecos.contains_key(nome)
    }

    /// Definições das ferramentas MCP no formato do NIM.
    pub fn definicoes(&self) -> Vec<Ferramenta> {
        let mut lista = Vec::new();
        for servidor in &self.servidores {
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
        let servidor = &self.servidores[endereco.indice_servidor];

        let mut parametros = CallToolRequestParams::new(endereco.nome_original.clone());
        if let Value::Object(mapa) = argumentos {
            parametros = parametros.with_arguments(mapa);
        }
        let resultado =
            tokio::time::timeout(servidor.timeout, servidor.cliente.call_tool(parametros))
                .await
                .with_context(|| {
                    format!(
                        "ferramenta {nome} passou do tempo limite ({} s)",
                        servidor.timeout.as_secs()
                    )
                })?
                .with_context(|| format!("falha ao chamar {nome}"))?;

        let texto = texto_do_resultado(&resultado);
        if resultado.is_error == Some(true) {
            bail!("{texto}");
        }
        Ok(texto)
    }

    /// Encerra todos os servidores com educação (fecha o stdio e espera).
    pub async fn encerrar(self) {
        for servidor in self.servidores {
            let nome = servidor.nome.clone();
            if let Err(e) = servidor.cliente.cancel().await {
                tracing::debug!("erro ao encerrar MCP '{nome}': {e}");
            }
        }
    }
}

async fn iniciar_servidor(
    config: &Config,
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
    let diretorio = item
        .diretorio
        .as_deref()
        .map(|d| config.resolver(d))
        .unwrap_or_else(|| config.raiz.clone());

    let comando = tokio::process::Command::new(&item.comando).configure(|cmd| {
        cmd.args(&item.args).current_dir(&diretorio).env_clear();
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

    let subir = async {
        let cliente = ().serve(transporte).await.context("handshake MCP falhou")?;
        let ferramentas = cliente
            .list_all_tools()
            .await
            .context("não consegui listar as ferramentas")?;
        anyhow::Ok((cliente, ferramentas))
    };
    let (cliente, ferramentas) = tokio::time::timeout(timeout, subir)
        .await
        .with_context(|| format!("servidor não respondeu em {} s", timeout.as_secs()))??;

    Ok(ServidorMcp {
        nome: item.nome.clone(),
        cliente,
        ferramentas,
        timeout: Duration::from_secs(item.timeout_segundos),
    })
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
        assert_eq!(ConfigMcp::default().servidores.len(), 0);
    }
}
