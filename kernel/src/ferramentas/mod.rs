//! Ferramentas que o modelo pode chamar (tool calling).
//!
//! `CaixaDeFerramentas` junta todas as ferramentas disponíveis numa sessão
//! (nativas do workspace + as dos servidores MCP), gera as definições
//! enviadas ao modelo e executa as chamadas. Todo resultado volta
//! ROTULADO como dado (ver `crate::dados`).

pub mod workspace;

use std::sync::Arc;

use serde_json::{Value, json};

use crate::config::Config;
use crate::dados;
use crate::mcp::PonteMcp;
use crate::nim::{ChamadaFerramenta, Ferramenta};
use workspace::Workspace;

/// Nomes das ferramentas nativas.
pub const LER_ARQUIVO: &str = "ler_arquivo";
pub const LISTAR_ARQUIVOS: &str = "listar_arquivos";
pub const ESCREVER_ARQUIVO: &str = "escrever_arquivo";

/// Resultado de uma ferramenta, pronto para virar mensagem `tool`.
#[derive(Debug, Clone)]
pub struct ResultadoFerramenta {
    /// Texto já rotulado como dado.
    pub texto: String,
    /// A ferramenta falhou?
    pub erro: bool,
}

/// Ferramentas nativas (as que o kernel implementa em Rust).
const NATIVAS: &[&str] = &[LER_ARQUIVO, LISTAR_ARQUIVOS, ESCREVER_ARQUIVO];

pub struct CaixaDeFerramentas {
    workspace: Workspace,
    mcp: Arc<PonteMcp>,
    /// Se `Some`, só estas ferramentas aparecem e podem ser executadas.
    /// Aceita curinga no fim: "exemplo__*".
    permitidas: Option<Vec<String>>,
}

impl CaixaDeFerramentas {
    /// Monta a caixa a partir da config (abre e valida o workspace).
    pub fn da_config(config: &Config) -> anyhow::Result<CaixaDeFerramentas> {
        let workspace = Workspace::abrir(
            &config.caminho_workspace(),
            &config.areas_protegidas(),
            config.ferramentas.clone(),
        )?;
        Ok(CaixaDeFerramentas::nova(workspace))
    }

    pub fn nova(workspace: Workspace) -> CaixaDeFerramentas {
        CaixaDeFerramentas {
            workspace,
            mcp: Arc::new(PonteMcp::vazia()),
            permitidas: None,
        }
    }

    /// Acrescenta as ferramentas dos servidores MCP.
    pub fn com_mcp(mut self, mcp: Arc<PonteMcp>) -> CaixaDeFerramentas {
        self.mcp = mcp;
        self
    }

    /// Cópia desta caixa só com as ferramentas da lista (usado pelos
    /// sub-agentes). Aceita curinga no fim do nome: "exemplo__*".
    pub fn restrita(&self, nomes: &[String]) -> CaixaDeFerramentas {
        CaixaDeFerramentas {
            workspace: self.workspace.clone(),
            mcp: Arc::clone(&self.mcp),
            permitidas: Some(nomes.to_vec()),
        }
    }

    fn permitida(&self, nome: &str) -> bool {
        match &self.permitidas {
            None => true,
            Some(lista) => lista.iter().any(|padrao| match padrao.strip_suffix('*') {
                Some(prefixo) => nome.starts_with(prefixo),
                None => nome == padrao,
            }),
        }
    }

    /// Definições (nome, descrição, JSON Schema) enviadas ao modelo.
    pub fn definicoes(&self) -> Vec<Ferramenta> {
        let todas = vec![
            Ferramenta::nova(
                LER_ARQUIVO,
                "Lê um arquivo de texto do workspace. Caminho relativo ao workspace.",
                json!({
                    "type": "object",
                    "properties": {
                        "caminho": {"type": "string", "description": "Ex.: notas/ideias.md"}
                    },
                    "required": ["caminho"]
                }),
            ),
            Ferramenta::nova(
                LISTAR_ARQUIVOS,
                "Lista arquivos e pastas do workspace.",
                json!({
                    "type": "object",
                    "properties": {
                        "caminho": {"type": "string", "description": "Pasta relativa ao workspace; vazio = raiz"},
                        "recursivo": {"type": "boolean", "description": "Incluir subpastas"}
                    }
                }),
            ),
            Ferramenta::nova(
                ESCREVER_ARQUIVO,
                "Cria ou altera um arquivo de texto no workspace (só dentro do workspace).",
                json!({
                    "type": "object",
                    "properties": {
                        "caminho": {"type": "string", "description": "Ex.: notas/ideias.md"},
                        "conteudo": {"type": "string"},
                        "acrescentar": {"type": "boolean", "description": "true = adiciona ao fim em vez de sobrescrever"}
                    },
                    "required": ["caminho", "conteudo"]
                }),
            ),
        ];
        todas
            .into_iter()
            .chain(self.mcp.definicoes())
            .filter(|f| self.permitida(f.nome()))
            .collect()
    }

    /// Executa uma chamada pedida pelo modelo. Nunca devolve `Err`: erros
    /// viram texto para o modelo ler e se corrigir.
    pub async fn executar(&self, chamada: &ChamadaFerramenta) -> ResultadoFerramenta {
        let nome = chamada.function.name.as_str();
        let eh_mcp = self.mcp.tem(nome);
        let resultado = if !self.permitida(nome) {
            Err(anyhow::anyhow!(
                "ferramenta '{nome}' não está disponível aqui"
            ))
        } else if NATIVAS.contains(&nome) {
            self.executar_nativa(nome, &chamada.function.arguments)
        } else if eh_mcp {
            match interpretar_argumentos(&chamada.function.arguments) {
                Ok(args) => self.mcp.chamar(nome, args).await,
                Err(e) => Err(e),
            }
        } else {
            Err(anyhow::anyhow!("ferramenta desconhecida: '{nome}'"))
        };

        // Resultados de MCP levam o prefixo "mcp:" na origem do rótulo.
        let origem = if eh_mcp {
            format!("mcp:{nome}")
        } else {
            nome.to_string()
        };
        match resultado {
            Ok(texto) => ResultadoFerramenta {
                texto: dados::rotular(&origem, &texto),
                erro: false,
            },
            Err(e) => {
                tracing::debug!("ferramenta {nome} falhou: {e:#}");
                ResultadoFerramenta {
                    texto: dados::rotular(&origem, &format!("ERRO: {e:#}")),
                    erro: true,
                }
            }
        }
    }

    fn executar_nativa(&self, nome: &str, argumentos: &str) -> anyhow::Result<String> {
        let args = interpretar_argumentos(argumentos)?;
        match nome {
            LER_ARQUIVO => self.workspace.ler(&texto_obrigatorio(&args, "caminho")?),
            LISTAR_ARQUIVOS => {
                let caminho = args["caminho"].as_str().unwrap_or("");
                let recursivo = args["recursivo"].as_bool().unwrap_or(false);
                self.workspace.listar(caminho, recursivo)
            }
            ESCREVER_ARQUIVO => {
                let caminho = texto_obrigatorio(&args, "caminho")?;
                let conteudo = texto_obrigatorio(&args, "conteudo")?;
                let acrescentar = args["acrescentar"].as_bool().unwrap_or(false);
                self.workspace.escrever(&caminho, &conteudo, acrescentar)
            }
            _ => anyhow::bail!("ferramenta desconhecida: '{nome}'"),
        }
    }
}

/// Os argumentos chegam como TEXTO JSON. Texto vazio = sem argumentos.
pub fn interpretar_argumentos(argumentos: &str) -> anyhow::Result<Value> {
    if argumentos.trim().is_empty() {
        return Ok(json!({}));
    }
    let valor: Value = serde_json::from_str(argumentos)
        .map_err(|e| anyhow::anyhow!("argumentos não são JSON válido ({e})"))?;
    if !valor.is_object() {
        anyhow::bail!("argumentos precisam ser um objeto JSON");
    }
    Ok(valor)
}

fn texto_obrigatorio(args: &Value, campo: &str) -> anyhow::Result<String> {
    args[campo]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("falta o argumento de texto '{campo}'"))
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::nim::tipos::FuncaoChamada;

    fn chamada(nome: &str, args: Value) -> ChamadaFerramenta {
        ChamadaFerramenta {
            id: "c1".into(),
            tipo: "function".into(),
            function: FuncaoChamada {
                name: nome.into(),
                arguments: args.to_string(),
            },
        }
    }

    fn caixa() -> (tempfile::TempDir, CaixaDeFerramentas) {
        let pasta = tempfile::tempdir().unwrap();
        let ws = Workspace::abrir(&pasta.path().join("ws"), &[], Default::default()).unwrap();
        (pasta, CaixaDeFerramentas::nova(ws))
    }

    #[tokio::test]
    async fn resultado_sempre_rotulado_inclusive_erro() {
        let (_p, caixa) = caixa();
        let ok = caixa
            .executar(&chamada(
                ESCREVER_ARQUIVO,
                json!({"caminho": "a.txt", "conteudo": "oi"}),
            ))
            .await;
        assert!(!ok.erro);
        assert!(ok.texto.starts_with("<dados origem=\"escrever_arquivo\">"));

        let erro = caixa
            .executar(&chamada(LER_ARQUIVO, json!({"caminho": "../fora"})))
            .await;
        assert!(erro.erro);
        assert!(erro.texto.contains("ERRO"));
        assert!(erro.texto.ends_with("</dados>"));
    }

    #[tokio::test]
    async fn argumentos_invalidos_e_ferramenta_desconhecida() {
        let (_p, caixa) = caixa();
        let mut c = chamada(LER_ARQUIVO, json!({}));
        c.function.arguments = "{nao é json".into();
        assert!(caixa.executar(&c).await.texto.contains("JSON"));
        assert!(caixa.executar(&chamada(LER_ARQUIVO, json!({}))).await.erro);
        assert!(caixa.executar(&chamada("rm_rf", json!({}))).await.erro);
    }

    #[tokio::test]
    async fn restricao_esconde_e_bloqueia() {
        let (_p, caixa) = caixa();
        let caixa = caixa.restrita(&[LER_ARQUIVO.to_string()]);
        let nomes: Vec<String> = caixa
            .definicoes()
            .iter()
            .map(|f| f.nome().to_string())
            .collect();
        assert_eq!(nomes, vec![LER_ARQUIVO]);
        let r = caixa
            .executar(&chamada(
                ESCREVER_ARQUIVO,
                json!({"caminho": "a", "conteudo": "b"}),
            ))
            .await;
        assert!(r.erro);
        assert!(r.texto.contains("não está disponível"));
    }

    #[test]
    fn curinga_no_fim_do_nome() {
        let (_p, caixa) = caixa();
        let caixa = caixa.restrita(&["exemplo__*".to_string(), LER_ARQUIVO.to_string()]);
        assert!(caixa.permitida("exemplo__somar"));
        assert!(caixa.permitida(LER_ARQUIVO));
        assert!(!caixa.permitida(ESCREVER_ARQUIVO));
        assert!(!caixa.permitida("outro__somar"));
    }
}
