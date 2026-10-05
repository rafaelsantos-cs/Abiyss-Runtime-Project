//! Tudo que fala com o NVIDIA NIM.
//!
//! - `tipos`: o formato JSON de chat completions (compatível com OpenAI);
//! - `sse`: leitura do streaming (Server-Sent Events);
//! - `cliente`: o cliente HTTP (uma requisição por chamada);
//! - `mock`: um NIM de mentira para testes.

pub mod cliente;
pub mod mock;
pub mod sse;
pub mod tipos;

pub use cliente::{ClienteNim, ErroNim};
pub use tipos::{
    ChamadaFerramenta, EventoStream, Ferramenta, Mensagem, Papel, PedidoChat, RespostaModelo, Uso,
};

use crate::config::ConfigModelo;

/// Monta o corpo da requisição a partir da config do modelo.
/// Os parâmetros (`max_tokens`, `extra`...) vêm todos do `abiyss.toml`.
pub fn montar_pedido(
    modelo: &ConfigModelo,
    mensagens: Vec<Mensagem>,
    ferramentas: Vec<Ferramenta>,
) -> PedidoChat {
    PedidoChat {
        model: modelo.id.clone(),
        messages: mensagens,
        tool_choice: if ferramentas.is_empty() {
            None
        } else {
            Some(serde_json::Value::String("auto".to_string()))
        },
        tools: ferramentas,
        stream: false,
        stream_options: None,
        max_tokens: modelo.max_tokens,
        temperature: modelo.temperatura,
        top_p: modelo.top_p,
        extra: modelo.extra.clone(),
    }
}
