//! A4: ponte MCP com servidores remotos por HTTP ("streamable") e o qmd
//! como motor de `memoria_buscar`, com busca por texto quando ele falta.
//!
//! O qmd de verdade não roda no CI: aqui um servidor MCP HTTP de mentira
//! (feito com o próprio rmcp) faz o papel dele, no formato que o kernel
//! supõe (ver `memoria::qmd`).

mod comum;

use std::sync::{Arc, Mutex};

use abiyss::config::Config;
use abiyss::ferramentas::{CaixaDeFerramentas, ContextoChamada};
use abiyss::frontmatter::Documento;
use abiyss::mcp::{ConfigServidorMcp, PonteMcp};
use abiyss::memoria::Memoria;
use abiyss::memoria::nota::{EscopoBusca, Fonte, Procedencia, Tipo};
use abiyss::nim::ChamadaFerramenta;
use abiyss::nim::tipos::FuncaoChamada;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use comum::Ambiente;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Value, json};

/// Como o qmd de mentira responde.
#[derive(Clone, Copy, PartialEq)]
enum Modo {
    /// JSON com uma lista "results" (o formato suposto).
    Json,
    /// Texto livre, que o kernel não sabe ler.
    Texto,
    /// JSON, mas só com notas que não existem no cofre.
    SoInexistentes,
}

#[derive(Clone)]
struct QmdDeMentira {
    modo: Modo,
    /// Argumentos recebidos em cada chamada de "search".
    recebidos: Arc<Mutex<Vec<Value>>>,
}

impl ServerHandler for QmdDeMentira {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _pedido: Option<PaginatedRequestParams>,
        _contexto: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let esquema = json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer"},
                "collection": {"type": "string"}
            },
            "required": ["query"]
        });
        Ok(ListToolsResult::with_all_items(vec![
            Tool::new(
                "search",
                "Busca BM25",
                Arc::new(esquema.as_object().unwrap().clone()),
            ),
            Tool::new("status", "Estado do índice", Arc::new(Default::default())),
        ]))
    }

    async fn call_tool(
        &self,
        pedido: CallToolRequestParams,
        _contexto: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = Value::Object(pedido.arguments.clone().unwrap_or_default());
        let texto = if pedido.name == "status" {
            "ok".to_string()
        } else {
            self.recebidos.lock().unwrap().push(args.clone());
            match self.modo {
                Modo::Texto => "Found 1 result:\n- pessoas/ana.md (0.9)".to_string(),
                Modo::SoInexistentes => {
                    json!({"results": [{"file": "qmd://cofre/01_internal/nao-existe.md"}]})
                        .to_string()
                }
                Modo::Json => resultados_json(args["collection"].as_str()).to_string(),
            }
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(texto)]).into())
    }
}

/// Resultados no formato suposto do qmd. Com coleção: caminhos relativos à
/// coleção; sem coleção: uma coleção "cofre" com o cofre inteiro.
fn resultados_json(colecao: Option<&str>) -> Value {
    let lista = match colecao {
        Some("abiyss-interno") => json!([
            {"file": "qmd://abiyss-interno/pessoas/ana.md", "score": 0.9, "snippet": "Ana gosta de chá verde."},
            {"file": "qmd://abiyss-interno/pessoas/esquecida.md", "score": 0.8, "snippet": "não existe mais"}
        ]),
        Some("abiyss-externo") => json!([
            {"file": "qmd://abiyss-externo/bebidas/cha.md", "score": 0.7, "snippet": "Mapa de fontes sobre chá."}
        ]),
        _ => json!([
            {"file": "qmd://cofre/02_external/bebidas/cha.md", "score": 0.95, "snippet": "Mapa de fontes sobre chá."},
            {"file": "qmd://cofre/01_internal/pessoas/ana.md", "score": 0.9, "snippet": "Ana gosta de chá verde."}
        ]),
    };
    json!({ "results": lista })
}

/// Exige "Authorization: Bearer abiyss" (ver o teste do token).
async fn exigir_token(
    pedido: axum::extract::Request,
    proximo: axum::middleware::Next,
) -> axum::response::Response {
    let autorizado = pedido
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        == Some("Bearer abiyss");
    if autorizado {
        proximo.run(pedido).await
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

/// Sobe o qmd de mentira em 127.0.0.1 e devolve a URL do endpoint MCP.
async fn subir_qmd(modo: Modo, com_token: bool) -> (String, Arc<Mutex<Vec<Value>>>) {
    let recebidos = Arc::new(Mutex::new(Vec::new()));
    let servidor = QmdDeMentira {
        modo,
        recebidos: recebidos.clone(),
    };
    let servico: StreamableHttpService<QmdDeMentira, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(servidor.clone()),
            Default::default(),
            StreamableHttpServerConfig::default(),
        );
    let mut rotas = axum::Router::new().nest_service("/mcp", servico);
    if com_token {
        rotas = rotas.layer(axum::middleware::from_fn(exigir_token));
    }
    let ouvinte = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endereco = ouvinte.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(ouvinte, rotas).await;
    });
    (format!("http://{endereco}/mcp"), recebidos)
}

fn servidor_http(url: &str, expor: bool) -> ConfigServidorMcp {
    ConfigServidorMcp {
        nome: "qmd".into(),
        comando: String::new(),
        args: vec![],
        diretorio: None,
        env: Default::default(),
        timeout_segundos: 10,
        ativo: true,
        url: Some(url.to_string()),
        token_env: None,
        expor,
    }
}

fn config_com_qmd(base: &Config, servidor: ConfigServidorMcp, colecoes: bool) -> Config {
    let mut c = base.clone();
    c.mcp.timeout_inicio_segundos = 10;
    c.mcp.servidores = vec![servidor];
    if colecoes {
        c.memoria.qmd.colecao_interna = "abiyss-interno".into();
        c.memoria.qmd.colecao_externa = "abiyss-externo".into();
    }
    c
}

/// Cofre com uma nota interna e uma externa que falam de chá.
fn preparar_cofre(memoria: &Memoria) {
    let procedencia = Procedencia {
        fonte: Fonte::Conversa,
        tipo: Tipo::Dito,
        origem_externa: None,
        criado: None,
        rotulo: "teste".into(),
    };
    let cofre = memoria.cofre();
    cofre
        .gravar(
            "01_internal/pessoas/ana.md",
            &Documento::sem_campos("Ana gosta de chá verde."),
            &procedencia,
        )
        .unwrap();
    let externa = abiyss::frontmatter::separar(
        "---\nlinks: [https://exemplo.org/cha]\nnavegador: rapido\nrevalidar_apos: 2026-12-01\n---\nMapa de fontes sobre chá.",
    )
    .unwrap();
    cofre
        .gravar("02_external/bebidas/cha.md", &externa, &procedencia)
        .unwrap();
}

#[tokio::test]
async fn ponte_conecta_por_http_e_esconde_servidor_do_kernel() {
    let amb = Ambiente::novo().await;
    let (url, _) = subir_qmd(Modo::Json, false).await;

    // expor = false: só o kernel usa; o modelo não vê nem consegue chamar.
    let config = config_com_qmd(&amb.config, servidor_http(&url, false), false);
    let ponte = PonteMcp::iniciar(&config).await;
    assert_eq!(ponte.servidores(), vec!["qmd"]);
    assert_eq!(
        ponte.descricao_servidores(),
        vec!["qmd (http, só para o kernel)"]
    );
    assert!(ponte.definicoes().is_empty());
    assert!(!ponte.tem("qmd__search"));
    assert!(ponte.chamar("qmd__status", json!({})).await.is_err());
    let status = ponte
        .chamar_no_servidor("qmd", "status", json!({}))
        .await
        .unwrap();
    assert_eq!(status.texto, "ok");
    ponte.encerrar().await;

    // expor = true: vira ferramenta comum do modelo (qmd__search).
    let config = config_com_qmd(&amb.config, servidor_http(&url, true), false);
    let ponte = PonteMcp::iniciar(&config).await;
    let nomes: Vec<String> = ponte
        .definicoes()
        .iter()
        .map(|f| f.function.name.clone())
        .collect();
    assert_eq!(nomes, vec!["qmd__search", "qmd__status"]);
    assert_eq!(ponte.chamar("qmd__status", json!({})).await.unwrap(), "ok");
    ponte.encerrar().await;
}

#[tokio::test]
async fn memoria_buscar_usa_o_qmd_por_colecao_e_respeita_o_escopo() {
    let amb = Ambiente::novo().await;
    let (url, recebidos) = subir_qmd(Modo::Json, false).await;
    let config = config_com_qmd(&amb.config, servidor_http(&url, false), true);
    let ponte = Arc::new(PonteMcp::iniciar(&config).await);
    let memoria = Memoria::abrir(&config, amb.banco.clone())
        .unwrap()
        .com_mcp(ponte.clone());
    preparar_cofre(&memoria);

    let interno = memoria.buscar("chá", EscopoBusca::Interno).await.unwrap();
    assert_eq!(interno.motor, "qmd");
    let caminhos: Vec<&str> = interno
        .resultados
        .iter()
        .map(|r| r.caminho.as_str())
        .collect();
    // A nota que não existe mais no cofre é descartada.
    assert_eq!(caminhos, vec!["01_internal/pessoas/ana.md"]);
    assert_eq!(interno.resultados[0].trecho, "Ana gosta de chá verde.");
    // Argumentos montados a partir do esquema anunciado pelo qmd.
    assert_eq!(
        recebidos.lock().unwrap()[0],
        json!({"query": "chá", "limit": 8, "collection": "abiyss-interno"})
    );

    let ambos = memoria.buscar("chá", EscopoBusca::Ambos).await.unwrap();
    assert_eq!(ambos.motor, "qmd");
    assert_eq!(ambos.resultados.len(), 2);
    assert_eq!(recebidos.lock().unwrap().len(), 3); // uma chamada por coleção

    // Pela ferramenta do modelo: rotulado como dado e com o motor indicado.
    let caixa = CaixaDeFerramentas::da_config(&config)
        .unwrap()
        .com_memoria(Arc::new(memoria));
    let chamada = ChamadaFerramenta {
        id: "b".into(),
        tipo: "function".into(),
        function: FuncaoChamada {
            name: "memoria_buscar".into(),
            arguments: json!({"consulta": "chá", "escopo": "interno"}).to_string(),
        },
    };
    let r = caixa
        .executar_com(&chamada, &ContextoChamada::limpo())
        .await;
    assert!(r.texto.starts_with("<dados origem=\"memoria_buscar\">"));
    assert!(r.texto.contains("motor: qmd"));
    assert_eq!(r.origem_externa, None);
}

#[tokio::test]
async fn sem_colecoes_o_filtro_por_caminho_separa_os_escopos() {
    let amb = Ambiente::novo().await;
    let (url, recebidos) = subir_qmd(Modo::Json, false).await;
    let config = config_com_qmd(&amb.config, servidor_http(&url, false), false);
    let ponte = Arc::new(PonteMcp::iniciar(&config).await);
    let memoria = Memoria::abrir(&config, amb.banco.clone())
        .unwrap()
        .com_mcp(ponte);
    preparar_cofre(&memoria);

    // O qmd devolve uma nota externa com pontuação MAIOR, mas a busca é interna.
    let r = memoria.buscar("chá", EscopoBusca::Interno).await.unwrap();
    assert_eq!(r.motor, "qmd");
    let caminhos: Vec<&str> = r.resultados.iter().map(|r| r.caminho.as_str()).collect();
    assert_eq!(caminhos, vec!["01_internal/pessoas/ana.md"]);
    assert_eq!(
        recebidos.lock().unwrap()[0],
        json!({"query": "chá", "limit": 8})
    );
}

#[tokio::test]
async fn sem_qmd_ou_com_resposta_estranha_cai_na_busca_por_texto() {
    let amb = Ambiente::novo().await;

    // 1. qmd configurado, mas fora do ar (porta fechada).
    let porta_fechada = {
        let ouvinte = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        ouvinte.local_addr().unwrap().port()
    };
    let fora = format!("http://127.0.0.1:{porta_fechada}/mcp");
    let config = config_com_qmd(&amb.config, servidor_http(&fora, false), true);
    let ponte = Arc::new(PonteMcp::iniciar(&config).await);
    assert!(ponte.servidores().is_empty());
    let memoria = Memoria::abrir(&config, amb.banco.clone())
        .unwrap()
        .com_mcp(ponte);
    preparar_cofre(&memoria);
    let r = memoria.buscar("chá", EscopoBusca::Interno).await.unwrap();
    assert_eq!(r.motor, "texto (qmd indisponível)");
    assert_eq!(r.resultados[0].caminho, "01_internal/pessoas/ana.md");

    // 2. qmd responde texto livre / 3. só notas que não existem: "falhou".
    for modo in [Modo::Texto, Modo::SoInexistentes] {
        let (url, _) = subir_qmd(modo, false).await;
        let config = config_com_qmd(&amb.config, servidor_http(&url, false), false);
        let ponte = Arc::new(PonteMcp::iniciar(&config).await);
        let memoria = Memoria::abrir(&config, amb.banco.clone())
            .unwrap()
            .com_mcp(ponte);
        let r = memoria.buscar("chá", EscopoBusca::Interno).await.unwrap();
        assert_eq!(r.motor, "texto (qmd falhou)");
        assert_eq!(r.resultados.len(), 1);
    }

    // 4. Sem servidor qmd configurado: texto, sem motivo.
    let mut config = amb.config.clone();
    config.memoria.qmd.servidor = String::new();
    let memoria = Memoria::abrir(&config, amb.banco.clone()).unwrap();
    assert_eq!(
        memoria
            .buscar("chá", EscopoBusca::Interno)
            .await
            .unwrap()
            .motor,
        "texto"
    );
}

#[tokio::test]
async fn token_vem_so_de_variavel_de_ambiente() {
    let amb = Ambiente::novo().await;
    let (url, _) = subir_qmd(Modo::Json, true).await;

    // Sem token: o servidor recusa (401) e a ponte segue sem ele.
    let config = config_com_qmd(&amb.config, servidor_http(&url, false), false);
    assert!(PonteMcp::iniciar(&config).await.servidores().is_empty());

    // Variável inexistente: erro claro, sem mostrar valor nenhum.
    let mut servidor = servidor_http(&url, false);
    servidor.token_env = Some("ABIYSS_TESTE_VARIAVEL_QUE_NAO_EXISTE".into());
    let config = config_com_qmd(&amb.config, servidor, false);
    assert!(PonteMcp::iniciar(&config).await.servidores().is_empty());

    // Com o token certo. O `cargo test` define CARGO_PKG_NAME=abiyss no
    // processo de teste: usamos essa variável como "token" (evita mexer no
    // ambiente do processo, o que é `unsafe` no Rust 2024).
    let mut servidor = servidor_http(&url, false);
    servidor.token_env = Some("CARGO_PKG_NAME".into());
    let config = config_com_qmd(&amb.config, servidor, false);
    let ponte = PonteMcp::iniciar(&config).await;
    assert_eq!(ponte.servidores(), vec!["qmd"]);
    ponte.encerrar().await;
}
