//! Cliente HTTP do NVIDIA NIM (API compatível com a OpenAI).
//!
//! Este cliente faz UMA requisição por chamada. Quem decide QUANDO chamar,
//! espera o rate limit e repete em caso de erro é o orquestrador
//! (`crate::orquestrador`). Separar as duas coisas deixa cada parte simples.

use std::time::Duration;

use futures::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{HeaderMap, RETRY_AFTER};

use super::sse::{AcumuladorStream, LeitorSse};
use super::tipos::{EventoStream, PedacoStream, PedidoChat, RespostaChat, RespostaModelo};

/// Erros que uma chamada ao NIM pode produzir.
#[derive(Debug, thiserror::Error)]
pub enum ErroNim {
    /// HTTP 429: passamos do limite de requisições.
    #[error("limite de requisições do NIM atingido (HTTP 429): {corpo}")]
    LimiteDeTaxa {
        /// Quanto o servidor pediu para esperar (cabeçalho Retry-After).
        retry_after: Option<Duration>,
        corpo: String,
    },

    /// Qualquer outra resposta HTTP de erro.
    #[error("NIM respondeu HTTP {status}: {corpo}")]
    Http {
        status: u16,
        corpo: String,
        retry_after: Option<Duration>,
    },

    /// Não conseguimos falar com o servidor (DNS, conexão, timeout...).
    #[error("falha de rede ao chamar o NIM: {0}")]
    Rede(String),

    /// O stream começou mas caiu no meio. NÃO é repetido automaticamente,
    /// porque parte da resposta já pode ter sido mostrada ao usuário.
    #[error("o stream do NIM foi interrompido: {0}")]
    StreamInterrompido(String),

    /// O servidor respondeu algo que não entendemos.
    #[error("resposta inválida do NIM: {0}")]
    RespostaInvalida(String),

    /// O servidor mandou um objeto `error` dentro do corpo/stream.
    #[error("erro informado pela API do NIM: {0}")]
    Api(String),
}

impl ErroNim {
    /// Vale a pena tentar de novo?
    /// Sim para 429, erros 5xx temporários e falhas de rede.
    pub fn eh_retentavel(&self) -> bool {
        match self {
            ErroNim::LimiteDeTaxa { .. } => true,
            ErroNim::Http { status, .. } => matches!(status, 408 | 500 | 502 | 503 | 504),
            ErroNim::Rede(_) => true,
            ErroNim::StreamInterrompido(_) => false,
            ErroNim::RespostaInvalida(_) => false,
            ErroNim::Api(_) => false,
        }
    }

    /// Tempo de espera sugerido pelo servidor, se houver.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            ErroNim::LimiteDeTaxa { retry_after, .. } => *retry_after,
            ErroNim::Http { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    pub fn eh_limite_de_taxa(&self) -> bool {
        matches!(self, ErroNim::LimiteDeTaxa { .. })
    }
}

/// Cliente de um pool. Cada pool tem o SEU cliente, com a SUA chave.
#[derive(Clone)]
pub struct ClienteNim {
    http: reqwest::Client,
    url_chat: String,
    chave: String,
}

// Implementação manual de Debug para a chave NUNCA aparecer em logs.
impl std::fmt::Debug for ClienteNim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClienteNim")
            .field("url_chat", &self.url_chat)
            .field("chave", &"***")
            .finish()
    }
}

impl ClienteNim {
    /// `base_url` sem barra no final, ex.: "https://integrate.api.nvidia.com/v1".
    pub fn novo(
        base_url: &str,
        chave: &str,
        timeout_conexao: Duration,
        timeout_leitura: Duration,
    ) -> Result<ClienteNim, ErroNim> {
        let http = reqwest::Client::builder()
            .connect_timeout(timeout_conexao)
            // Tempo máximo entre dois pedaços recebidos (não o total):
            // assim um stream longo não é cortado, mas um servidor mudo é.
            .read_timeout(timeout_leitura)
            .user_agent(concat!("abiyss/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| ErroNim::Rede(e.to_string()))?;
        Ok(ClienteNim {
            http,
            url_chat: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            chave: chave.to_string(),
        })
    }

    /// Chamada SEM streaming: espera a resposta inteira.
    pub async fn completar(&self, pedido: &PedidoChat) -> Result<RespostaModelo, ErroNim> {
        let mut pedido = pedido.clone();
        pedido.stream = false;
        pedido.stream_options = None;

        let resposta = self.enviar(&pedido).await?;
        let corpo = resposta
            .text()
            .await
            .map_err(|e| ErroNim::Rede(e.to_string()))?;

        verificar_erro_no_corpo(&corpo)?;
        let dados: RespostaChat = serde_json::from_str(&corpo)
            .map_err(|e| ErroNim::RespostaInvalida(format!("{e}: {}", resumir(&corpo))))?;

        let Some(escolha) = dados.choices.into_iter().next() else {
            return Err(ErroNim::RespostaInvalida(
                "resposta sem nenhuma 'choice'".to_string(),
            ));
        };
        Ok(RespostaModelo {
            mensagem: escolha.message,
            motivo_fim: escolha.finish_reason,
            uso: dados.usage.unwrap_or_default(),
        })
    }

    /// Chamada COM streaming. Cada pedaço de texto/raciocínio é passado
    /// para `ao_receber` assim que chega; no fim devolve a resposta completa.
    pub async fn completar_stream(
        &self,
        pedido: &PedidoChat,
        ao_receber: &mut (dyn FnMut(EventoStream) + Send),
    ) -> Result<RespostaModelo, ErroNim> {
        let mut pedido = pedido.clone();
        pedido.stream = true;
        pedido.stream_options = Some(super::tipos::OpcoesStream {
            include_usage: true,
        });

        let resposta = self.enviar(&pedido).await?;

        let mut bytes = resposta.bytes_stream();
        let mut leitor = LeitorSse::novo();
        let mut acumulador = AcumuladorStream::novo();
        let mut terminou = false;

        while let Some(pedaco) = bytes.next().await {
            let pedaco = pedaco.map_err(|e| ErroNim::StreamInterrompido(e.to_string()))?;
            for dados in leitor.alimentar(&pedaco) {
                if processar_evento(&dados, &mut acumulador, ao_receber)? {
                    terminou = true;
                }
            }
            if terminou {
                break;
            }
        }
        if !terminou && let Some(dados) = leitor.finalizar() {
            processar_evento(&dados, &mut acumulador, ao_receber)?;
        }
        Ok(acumulador.finalizar())
    }

    /// Faz o POST e transforma respostas HTTP de erro em `ErroNim`.
    async fn enviar(&self, pedido: &PedidoChat) -> Result<reqwest::Response, ErroNim> {
        let resposta = self
            .http
            .post(&self.url_chat)
            .bearer_auth(&self.chave)
            .json(pedido)
            .send()
            .await
            .map_err(|e| ErroNim::Rede(e.to_string()))?;

        let status = resposta.status();
        if status.is_success() {
            return Ok(resposta);
        }

        let retry_after = ler_retry_after(resposta.headers());
        let corpo = resposta.text().await.unwrap_or_default();
        let corpo = resumir(&corpo);
        if status == StatusCode::TOO_MANY_REQUESTS {
            Err(ErroNim::LimiteDeTaxa { retry_after, corpo })
        } else {
            Err(ErroNim::Http {
                status: status.as_u16(),
                corpo,
                retry_after,
            })
        }
    }
}

/// Trata um evento SSE. Devolve `true` quando chegou o `[DONE]`.
fn processar_evento(
    dados: &str,
    acumulador: &mut AcumuladorStream,
    ao_receber: &mut (dyn FnMut(EventoStream) + Send),
) -> Result<bool, ErroNim> {
    let dados = dados.trim();
    if dados == "[DONE]" {
        return Ok(true);
    }
    if dados.is_empty() {
        return Ok(false);
    }
    verificar_erro_no_corpo(dados)?;
    let pedaco: PedacoStream = serde_json::from_str(dados)
        .map_err(|e| ErroNim::RespostaInvalida(format!("{e}: {}", resumir(dados))))?;
    for evento in acumulador.aplicar(pedaco) {
        ao_receber(evento);
    }
    Ok(false)
}

/// Algumas APIs respondem 200 com `{"error": {...}}` no corpo ou no stream.
fn verificar_erro_no_corpo(corpo: &str) -> Result<(), ErroNim> {
    if let Ok(valor) = serde_json::from_str::<serde_json::Value>(corpo)
        && let Some(erro) = valor.get("error")
        && !erro.is_null()
    {
        return Err(ErroNim::Api(resumir(&erro.to_string())));
    }
    Ok(())
}

/// Lê o cabeçalho Retry-After, que pode vir em segundos ("30")
/// ou como data HTTP ("Wed, 21 Oct 2026 07:28:00 GMT").
pub fn ler_retry_after(cabecalhos: &HeaderMap) -> Option<Duration> {
    let texto = cabecalhos.get(RETRY_AFTER)?.to_str().ok()?.trim();
    interpretar_retry_after(texto, chrono::Utc::now())
}

/// Parte pura de `ler_retry_after` (recebe "agora" para ser testável).
pub fn interpretar_retry_after(
    texto: &str,
    agora: chrono::DateTime<chrono::Utc>,
) -> Option<Duration> {
    if let Ok(segundos) = texto.parse::<f64>() {
        if segundos.is_finite() && segundos >= 0.0 {
            return Some(Duration::from_secs_f64(segundos));
        }
        return None;
    }
    let data = chrono::DateTime::parse_from_rfc2822(texto).ok()?;
    let diferenca = data.with_timezone(&chrono::Utc) - agora;
    // Data no passado = pode tentar já.
    Some(diferenca.to_std().unwrap_or(Duration::ZERO))
}

/// Corta textos longos (corpos de erro) para não poluir logs.
fn resumir(texto: &str) -> String {
    const LIMITE: usize = 500;
    if texto.chars().count() <= LIMITE {
        texto.to_string()
    } else {
        let curto: String = texto.chars().take(LIMITE).collect();
        format!("{curto}… (cortado)")
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn retry_after_em_segundos_e_em_data_http() {
        let agora = chrono::DateTime::parse_from_rfc3339("2026-10-21T07:28:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            interpretar_retry_after("30", agora),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            interpretar_retry_after("Wed, 21 Oct 2026 07:28:10 GMT", agora),
            Some(Duration::from_secs(10))
        );
        // Data no passado vira zero.
        assert_eq!(
            interpretar_retry_after("Wed, 21 Oct 2026 07:00:00 GMT", agora),
            Some(Duration::ZERO)
        );
        assert_eq!(interpretar_retry_after("-1", agora), None);
        assert_eq!(interpretar_retry_after("lixo", agora), None);
    }

    #[test]
    fn classifica_erros_retentaveis() {
        let e429 = ErroNim::LimiteDeTaxa {
            retry_after: None,
            corpo: String::new(),
        };
        let e503 = ErroNim::Http {
            status: 503,
            corpo: String::new(),
            retry_after: None,
        };
        let e400 = ErroNim::Http {
            status: 400,
            corpo: String::new(),
            retry_after: None,
        };
        assert!(e429.eh_retentavel());
        assert!(e503.eh_retentavel());
        assert!(!e400.eh_retentavel());
        assert!(!ErroNim::StreamInterrompido("x".into()).eh_retentavel());
    }

    #[test]
    fn debug_nao_mostra_a_chave() {
        let cliente = ClienteNim::novo(
            "http://x/v1",
            "nvapi-segredo",
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .unwrap();
        let texto = format!("{cliente:?}");
        assert!(!texto.contains("segredo"));
    }
}
