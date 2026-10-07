//! Mock do NVIDIA NIM: um servidor HTTP local que imita
//! `POST /v1/chat/completions` (com e sem streaming).
//!
//! Serve para os testes NÃO dependerem de rede nem das chaves reais,
//! e também pode ser rodado pela CLI (`abiyss mock-nim`) para
//! experimentar o Abiyss sem gastar cota.
//!
//! Como decidir o que o mock responde:
//! 1. `enfileirar(...)`: respostas usadas uma vez cada, em ordem;
//! 2. `definir_roteiro(...)`: uma função que olha o pedido e escolhe a resposta;
//! 3. se nada disso existir: ecoa a última mensagem do usuário.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::tempo::agora_ms;

/// Uma chamada de ferramenta que o mock vai "pedir".
#[derive(Debug, Clone)]
pub struct ChamadaMock {
    pub nome: String,
    pub argumentos: Value,
}

/// O que o mock responde a uma requisição.
#[derive(Debug, Clone)]
pub enum RespostaMock {
    /// Resposta de texto normal.
    Texto(String),
    /// Texto acompanhado de raciocínio (`reasoning_content`).
    TextoComRaciocinio { raciocinio: String, texto: String },
    /// O "modelo" pede uma ou mais ferramentas.
    Ferramentas(Vec<ChamadaMock>),
    /// Erro HTTP (ex.: 429 com Retry-After, 503...).
    Erro {
        status: u16,
        retry_after: Option<String>,
        corpo: String,
    },
    /// Espera um pouco antes de responder (para testar concorrência e cancelamento).
    Atrasada {
        atraso: Duration,
        resposta: Box<RespostaMock>,
    },
    /// Segura a resposta até o teste liberar uma vaga no `portao`
    /// (`add_permits`): sincronização explícita, sem depender de relógio.
    Segurada {
        portao: Arc<Semaphore>,
        resposta: Box<RespostaMock>,
    },
}

impl RespostaMock {
    pub fn texto(texto: impl Into<String>) -> RespostaMock {
        RespostaMock::Texto(texto.into())
    }

    pub fn ferramenta(nome: impl Into<String>, argumentos: Value) -> RespostaMock {
        RespostaMock::Ferramentas(vec![ChamadaMock {
            nome: nome.into(),
            argumentos,
        }])
    }

    pub fn erro(status: u16, retry_after: Option<&str>) -> RespostaMock {
        RespostaMock::Erro {
            status,
            retry_after: retry_after.map(|s| s.to_string()),
            corpo: format!("{{\"error\":\"erro simulado {status}\"}}"),
        }
    }

    pub fn atrasada(self, atraso: Duration) -> RespostaMock {
        RespostaMock::Atrasada {
            atraso,
            resposta: Box::new(self),
        }
    }

    /// Só responde depois de pegar (e gastar) uma vaga do `portao`.
    pub fn segurada(self, portao: Arc<Semaphore>) -> RespostaMock {
        RespostaMock::Segurada {
            portao,
            resposta: Box::new(self),
        }
    }
}

/// Uma requisição que o mock recebeu (para os testes conferirem).
#[derive(Debug, Clone)]
pub struct RequisicaoRecebida {
    pub corpo: Value,
    pub autorizacao: Option<String>,
    pub momento: Instant,
    /// O mesmo instante no relógio do banco (`tempo::agora_ms`), para
    /// comparar com o que o kernel grava (fichas do token bucket, bloqueios).
    pub momento_ms: i64,
}

/// Função que escolhe a resposta olhando o corpo do pedido.
pub type Roteiro = Arc<dyn Fn(&Value) -> RespostaMock + Send + Sync>;

#[derive(Default)]
struct EstadoMock {
    fila: VecDeque<RespostaMock>,
    roteiro: Option<Roteiro>,
    /// Só as últimas `MAX_RECEBIDAS_GUARDADAS` (o mock pode ficar horas no
    /// ar no teste de resistência); o total fica em `total_recebidas`.
    recebidas: VecDeque<RequisicaoRecebida>,
    total_recebidas: usize,
    em_andamento: usize,
    pico_em_andamento: usize,
}

type Compartilhado = Arc<Mutex<EstadoMock>>;

/// Quantas requisições recebidas o mock guarda para os testes conferirem.
const MAX_RECEBIDAS_GUARDADAS: usize = 1_000;

/// O servidor mock. Ao ser destruído (`drop`), o servidor para.
pub struct MockNim {
    endereco: SocketAddr,
    estado: Compartilhado,
    tarefa: tokio::task::JoinHandle<()>,
}

impl Drop for MockNim {
    fn drop(&mut self) {
        self.tarefa.abort();
    }
}

impl MockNim {
    /// Sobe o mock numa porta livre qualquer de 127.0.0.1.
    pub async fn iniciar() -> MockNim {
        MockNim::iniciar_em("127.0.0.1:0")
            .await
            .expect("não consegui abrir porta para o mock")
    }

    /// Sobe o mock num endereço específico (ex.: "127.0.0.1:8089").
    pub async fn iniciar_em(endereco: &str) -> std::io::Result<MockNim> {
        let estado: Compartilhado = Arc::new(Mutex::new(EstadoMock::default()));
        let app = Router::new()
            .route("/v1/chat/completions", post(tratar_chat))
            .route("/v1/models", get(tratar_modelos))
            .with_state(estado.clone());

        let ouvinte = tokio::net::TcpListener::bind(endereco).await?;
        let endereco = ouvinte.local_addr()?;
        let tarefa = tokio::spawn(async move {
            let _ = axum::serve(ouvinte, app).await;
        });
        Ok(MockNim {
            endereco,
            estado,
            tarefa,
        })
    }

    /// URL para colocar em `nim.base_url`.
    pub fn base_url(&self) -> String {
        format!("http://{}/v1", self.endereco)
    }

    pub fn enfileirar(&self, resposta: RespostaMock) {
        self.estado.lock().unwrap().fila.push_back(resposta);
    }

    pub fn definir_roteiro(
        &self,
        roteiro: impl Fn(&Value) -> RespostaMock + Send + Sync + 'static,
    ) {
        self.estado.lock().unwrap().roteiro = Some(Arc::new(roteiro));
    }

    /// As últimas requisições recebidas (no máximo `MAX_RECEBIDAS_GUARDADAS`),
    /// da mais antiga para a mais nova.
    pub fn requisicoes(&self) -> Vec<RequisicaoRecebida> {
        self.estado
            .lock()
            .unwrap()
            .recebidas
            .iter()
            .cloned()
            .collect()
    }

    /// Total de requisições desde que o mock subiu.
    pub fn total_requisicoes(&self) -> usize {
        self.estado.lock().unwrap().total_recebidas
    }

    /// Maior número de requisições atendidas ao mesmo tempo.
    pub fn pico_concorrencia(&self) -> usize {
        self.estado.lock().unwrap().pico_em_andamento
    }

    /// Requisições sendo atendidas agora (ex.: seguradas no portão).
    pub fn em_andamento(&self) -> usize {
        self.estado.lock().unwrap().em_andamento
    }
}

/// Conta requisições em andamento; o `Drop` desconta mesmo se o
/// cliente desistir no meio (a tarefa do handler é cancelada).
struct GuardaAndamento(Compartilhado);

impl GuardaAndamento {
    fn novo(estado: Compartilhado) -> GuardaAndamento {
        {
            let mut e = estado.lock().unwrap();
            e.em_andamento += 1;
            e.pico_em_andamento = e.pico_em_andamento.max(e.em_andamento);
        }
        GuardaAndamento(estado)
    }
}

impl Drop for GuardaAndamento {
    fn drop(&mut self) {
        let mut e = self.0.lock().unwrap();
        e.em_andamento -= 1;
    }
}

async fn tratar_modelos() -> impl IntoResponse {
    axum::Json(json!({"object": "list", "data": [{"id": "mock/modelo", "object": "model"}]}))
}

async fn tratar_chat(
    State(estado): State<Compartilhado>,
    cabecalhos: HeaderMap,
    corpo: String,
) -> Response {
    let _guarda = GuardaAndamento::novo(estado.clone());

    let autorizacao = cabecalhos
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let corpo: Value = match serde_json::from_str(&corpo) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("JSON inválido: {e}")).into_response(),
    };

    // Escolhe a resposta (e registra o pedido) segurando o lock só aqui.
    let resposta = {
        let mut e = estado.lock().unwrap();
        if e.recebidas.len() == MAX_RECEBIDAS_GUARDADAS {
            e.recebidas.pop_front();
        }
        e.recebidas.push_back(RequisicaoRecebida {
            corpo: corpo.clone(),
            autorizacao: autorizacao.clone(),
            momento: Instant::now(),
            momento_ms: agora_ms(),
        });
        e.total_recebidas += 1;
        match e.fila.pop_front() {
            Some(r) => r,
            None => match &e.roteiro {
                Some(roteiro) => roteiro(&corpo),
                None => eco(&corpo),
            },
        }
    };

    // Igual à API real: sem chave, sem resposta.
    if !autorizacao.unwrap_or_default().starts_with("Bearer ") {
        return (StatusCode::UNAUTHORIZED, "{\"error\":\"sem chave\"}").into_response();
    }

    let stream = corpo
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    responder(resposta, &corpo, stream).await
}

/// Resposta padrão: repete a última mensagem do usuário.
fn eco(corpo: &Value) -> RespostaMock {
    let ultima = corpo["messages"]
        .as_array()
        .and_then(|lista| lista.iter().rev().find(|m| m["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("")
        .to_string();
    RespostaMock::Texto(format!("mock: {ultima}"))
}

async fn responder(resposta: RespostaMock, corpo: &Value, stream: bool) -> Response {
    // Desembrulha atrasos e portões (podem estar aninhados).
    let mut resposta = resposta;
    let resposta = loop {
        resposta = match resposta {
            RespostaMock::Atrasada {
                atraso,
                resposta: r,
            } => {
                tokio::time::sleep(atraso).await;
                *r
            }
            RespostaMock::Segurada {
                portao,
                resposta: r,
            } => {
                if let Ok(vaga) = portao.acquire().await {
                    vaga.forget();
                }
                *r
            }
            outra => break outra,
        };
    };

    if let RespostaMock::Erro {
        status,
        retry_after,
        corpo,
    } = resposta
    {
        let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut r = (status, corpo).into_response();
        if let Some(valor) = retry_after
            && let Ok(v) = valor.parse()
        {
            r.headers_mut().insert(header::RETRY_AFTER, v);
        }
        return r;
    }

    let (raciocinio, texto, chamadas) = match resposta {
        RespostaMock::Texto(t) => (None, Some(t), vec![]),
        RespostaMock::TextoComRaciocinio { raciocinio, texto } => {
            (Some(raciocinio), Some(texto), vec![])
        }
        RespostaMock::Ferramentas(c) => (None, None, c),
        // Já tratados acima.
        RespostaMock::Erro { .. }
        | RespostaMock::Atrasada { .. }
        | RespostaMock::Segurada { .. } => unreachable!(),
    };

    let chamadas_json: Vec<Value> = chamadas
        .iter()
        .map(|c| {
            json!({
                "id": novo_id_chamada(),
                "type": "function",
                "function": {"name": c.nome, "arguments": c.argumentos.to_string()}
            })
        })
        .collect();
    let motivo = if chamadas_json.is_empty() {
        "stop"
    } else {
        "tool_calls"
    };
    let uso = calcular_uso(corpo, texto.as_deref().unwrap_or(""));

    if stream {
        let eventos = eventos_stream(raciocinio, texto, &chamadas_json, motivo, uso);
        let corpo_sse = futures::stream::iter(eventos.into_iter().map(Ok::<String, Infallible>));
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(corpo_sse))
            .unwrap();
    }

    let mut mensagem = json!({"role": "assistant", "content": texto});
    if let Some(r) = raciocinio {
        mensagem["reasoning_content"] = json!(r);
    }
    if !chamadas_json.is_empty() {
        mensagem["tool_calls"] = json!(chamadas_json);
    }
    axum::Json(json!({
        "id": "mock-1",
        "object": "chat.completion",
        "model": corpo["model"],
        "choices": [{"index": 0, "message": mensagem, "finish_reason": motivo}],
        "usage": uso,
    }))
    .into_response()
}

/// Monta os eventos SSE, picando o texto e os argumentos em pedaços
/// pequenos, como um servidor real faria.
fn eventos_stream(
    raciocinio: Option<String>,
    texto: Option<String>,
    chamadas: &[Value],
    motivo: &str,
    uso: Value,
) -> Vec<String> {
    let mut eventos = Vec::new();
    let mut evento = |delta: Value, fim: Option<&str>| {
        let pedaco = json!({
            "id": "mock-1",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": delta, "finish_reason": fim}],
        });
        eventos.push(format!("data: {pedaco}\n\n"));
    };

    evento(json!({"role": "assistant"}), None);
    if let Some(r) = raciocinio {
        for parte in picar(&r, 5) {
            evento(json!({"reasoning_content": parte}), None);
        }
    }
    if let Some(t) = texto {
        for parte in picar(&t, 5) {
            evento(json!({"content": parte}), None);
        }
    }
    for (indice, chamada) in chamadas.iter().enumerate() {
        let argumentos = chamada["function"]["arguments"].as_str().unwrap_or("");
        let partes = picar(argumentos, 4);
        // Primeiro pedaço: id e nome. Os seguintes: só argumentos.
        evento(
            json!({"tool_calls": [{"index": indice, "id": chamada["id"], "type": "function",
                "function": {"name": chamada["function"]["name"], "arguments": ""}}]}),
            None,
        );
        for parte in partes {
            evento(
                json!({"tool_calls": [{"index": indice, "function": {"arguments": parte}}]}),
                None,
            );
        }
    }
    evento(json!({}), Some(motivo));
    eventos.push(format!(
        "data: {}\n\n",
        json!({"id": "mock-1", "choices": [], "usage": uso})
    ));
    eventos.push("data: [DONE]\n\n".to_string());
    eventos
}

/// Roteiro do teste de resistência (`abiyss mock-nim --roteiro resistencia`):
/// o mock responde como um Abiyss bem-comportado, para o daemon exercitar
/// o caminho completo (heartbeat → ações → sub-agentes → eventos).
///
/// - modelo com "cerebro" no ID: decisão válida do heartbeat; a cada 4
///   chamadas delega um sub-agente `low`, nas outras aguarda;
/// - qualquer outro modelo (sub-agentes): relatório final "concluido".
///
/// As respostas têm alguns KB (texto + raciocínio) para os buffers
/// trabalharem, e chegam depois de `atraso` (latência de mentira).
pub fn roteiro_resistencia(atraso: Duration) -> impl Fn(&Value) -> RespostaMock + Send + Sync {
    let contador = AtomicU64::new(0);
    move |corpo: &Value| {
        let n = contador.fetch_add(1, Ordering::Relaxed);
        let modelo = corpo["model"].as_str().unwrap_or("");
        let enchimento = "observação de teste ".repeat(80);
        let texto = if modelo.contains("cerebro") {
            let acao = if n.is_multiple_of(4) {
                json!({"tipo": "delegar", "nivel": "low",
                       "tarefa": format!("tarefa de resistência {n}"), "prazo_segundos": 60})
            } else {
                json!({"tipo": "aguardar", "motivo": "nada novo"})
            };
            json!({
                "percepcao": enchimento,
                "orientacao": "seguir o goal",
                "decisao": "uma ação por ciclo",
                "acoes": [acao]
            })
        } else {
            json!({
                "status": "concluido",
                "resumo": enchimento,
                "artefatos": [],
                "confianca": 0.8,
                "duvidas": []
            })
        };
        RespostaMock::TextoComRaciocinio {
            raciocinio: "pensando no teste de resistência ".repeat(40),
            texto: texto.to_string(),
        }
        .atrasada(atraso)
    }
}

/// Divide um texto em pedaços de `n` caracteres (sem quebrar UTF-8).
fn picar(texto: &str, n: usize) -> Vec<String> {
    let caracteres: Vec<char> = texto.chars().collect();
    caracteres
        .chunks(n.max(1))
        .map(|c| c.iter().collect())
        .collect()
}

/// Contagem de tokens de mentira, mas determinística (~4 caracteres por token).
fn calcular_uso(corpo: &Value, resposta: &str) -> Value {
    let entrada: usize = corpo["messages"]
        .as_array()
        .map(|lista| {
            lista
                .iter()
                .map(|m| m["content"].as_str().unwrap_or("").len())
                .sum()
        })
        .unwrap_or(0);
    let prompt = (entrada / 4 + 1) as u64;
    let completion = (resposta.len() / 4 + 1) as u64;
    json!({"prompt_tokens": prompt, "completion_tokens": completion, "total_tokens": prompt + completion})
}

fn novo_id_chamada() -> String {
    static CONTADOR: AtomicU64 = AtomicU64::new(1);
    format!("call_mock_{}", CONTADOR.fetch_add(1, Ordering::Relaxed))
}
