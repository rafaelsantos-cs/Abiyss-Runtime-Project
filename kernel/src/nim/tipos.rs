//! Tipos do formato de chat completions compatível com a OpenAI,
//! que é o formato aceito pelo NVIDIA NIM.
//!
//! Os NOMES DOS CAMPOS ficam em inglês de propósito: eles precisam bater
//! exatamente com o JSON da API (`role`, `content`, `tool_calls`...).
//! Assim dá para comparar com a documentação lado a lado.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Quem "fala" cada mensagem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Papel {
    System,
    User,
    Assistant,
    Tool,
}

impl Papel {
    /// Texto usado no banco de dados e nos logs.
    pub fn como_texto(&self) -> &'static str {
        match self {
            Papel::System => "system",
            Papel::User => "user",
            Papel::Assistant => "assistant",
            Papel::Tool => "tool",
        }
    }

    pub fn de_texto(texto: &str) -> Option<Papel> {
        match texto {
            "system" => Some(Papel::System),
            "user" => Some(Papel::User),
            "assistant" => Some(Papel::Assistant),
            "tool" => Some(Papel::Tool),
            _ => None,
        }
    }
}

/// Uma mensagem da conversa.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mensagem {
    pub role: Papel,

    /// Texto da mensagem. Pode faltar quando o assistente só pede ferramentas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Raciocínio ("thinking") devolvido por modelos com modo de raciocínio.
    /// Alguns servidores usam o nome `reasoning`; o `alias` aceita os dois.
    #[serde(default, alias = "reasoning", skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,

    /// Ferramentas que o assistente pediu para chamar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChamadaFerramenta>>,

    /// Em mensagens `tool`: a qual chamada este resultado responde.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// Em mensagens `tool`: nome da ferramenta (opcional na API).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Mensagem {
    fn vazia(role: Papel) -> Mensagem {
        Mensagem {
            role,
            content: None,
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn sistema(texto: impl Into<String>) -> Mensagem {
        Mensagem {
            content: Some(texto.into()),
            ..Mensagem::vazia(Papel::System)
        }
    }

    pub fn usuario(texto: impl Into<String>) -> Mensagem {
        Mensagem {
            content: Some(texto.into()),
            ..Mensagem::vazia(Papel::User)
        }
    }

    pub fn assistente(texto: impl Into<String>) -> Mensagem {
        Mensagem {
            content: Some(texto.into()),
            ..Mensagem::vazia(Papel::Assistant)
        }
    }

    /// Resultado de uma ferramenta, respondendo à chamada `id_chamada`.
    pub fn resultado_ferramenta(
        id_chamada: impl Into<String>,
        nome: impl Into<String>,
        texto: impl Into<String>,
    ) -> Mensagem {
        Mensagem {
            content: Some(texto.into()),
            tool_call_id: Some(id_chamada.into()),
            name: Some(nome.into()),
            ..Mensagem::vazia(Papel::Tool)
        }
    }

    /// Texto da mensagem, ou "" se não houver.
    pub fn texto(&self) -> &str {
        self.content.as_deref().unwrap_or("")
    }

    /// Lista de chamadas de ferramenta (vazia se não houver).
    pub fn chamadas(&self) -> &[ChamadaFerramenta] {
        self.tool_calls.as_deref().unwrap_or(&[])
    }
}

/// Uma chamada de ferramenta pedida pelo modelo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamadaFerramenta {
    pub id: String,
    #[serde(rename = "type", default = "tipo_function")]
    pub tipo: String,
    pub function: FuncaoChamada,
}

fn tipo_function() -> String {
    "function".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuncaoChamada {
    pub name: String,
    /// Argumentos como TEXTO JSON (é assim que a API manda).
    /// Precisa ser interpretado com `serde_json::from_str`.
    #[serde(default)]
    pub arguments: String,
}

/// Definição de uma ferramenta oferecida ao modelo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ferramenta {
    #[serde(rename = "type")]
    pub tipo: String,
    pub function: DefinicaoFuncao,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DefinicaoFuncao {
    pub name: String,
    pub description: String,
    /// JSON Schema dos argumentos.
    pub parameters: Value,
}

impl Ferramenta {
    pub fn nova(nome: impl Into<String>, descricao: impl Into<String>, parametros: Value) -> Self {
        Ferramenta {
            tipo: "function".to_string(),
            function: DefinicaoFuncao {
                name: nome.into(),
                description: descricao.into(),
                parameters: parametros,
            },
        }
    }

    pub fn nome(&self) -> &str {
        &self.function.name
    }
}

/// Corpo de `POST /chat/completions`.
#[derive(Debug, Clone, Serialize)]
pub struct PedidoChat {
    pub model: String,
    pub messages: Vec<Mensagem>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Ferramenta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<OpcoesStream>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Campos extras vindos da config do modelo (ex.: `chat_template_kwargs`).
    /// `flatten` coloca cada chave direto na raiz do JSON.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
    /// Não vai para o NIM: o nível da tabela de esforço com que o pedido foi
    /// montado (o orquestrador grava em `chamadas_modelo`).
    #[serde(skip)]
    pub esforco: Option<crate::esforco::EsforcoAplicado>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpcoesStream {
    /// Pede para o servidor mandar o uso de tokens no último pedaço do stream.
    pub include_usage: bool,
}

/// Contagem de tokens de uma chamada.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Uso {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

impl Uso {
    pub fn somar(&mut self, outro: &Uso) {
        self.prompt_tokens += outro.prompt_tokens;
        self.completion_tokens += outro.completion_tokens;
        self.total_tokens += outro.total_tokens;
    }
}

/// Resposta (sem streaming) de `POST /chat/completions`.
#[derive(Debug, Clone, Deserialize)]
pub struct RespostaChat {
    #[serde(default)]
    pub choices: Vec<Escolha>,
    #[serde(default)]
    pub usage: Option<Uso>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Escolha {
    pub message: Mensagem,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// Um pedaço do stream SSE (`stream: true`).
#[derive(Debug, Clone, Deserialize)]
pub struct PedacoStream {
    #[serde(default)]
    pub choices: Vec<EscolhaStream>,
    #[serde(default)]
    pub usage: Option<Uso>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EscolhaStream {
    #[serde(default)]
    pub delta: Delta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// O "pedacinho" de mensagem que chega em cada evento do stream.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Delta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default, alias = "reasoning")]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<DeltaChamada>>,
}

/// Pedaço de uma chamada de ferramenta. O `index` diz a qual chamada
/// ele pertence; `id` e `name` costumam vir só no primeiro pedaço e
/// `arguments` vem picado em vários pedaços que precisam ser concatenados.
#[derive(Debug, Clone, Deserialize)]
pub struct DeltaChamada {
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<DeltaFuncao>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DeltaFuncao {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

/// Resultado final de uma chamada ao modelo, com ou sem streaming.
#[derive(Debug, Clone)]
pub struct RespostaModelo {
    /// A mensagem do assistente (texto e/ou chamadas de ferramenta).
    pub mensagem: Mensagem,
    /// Por que o modelo parou: "stop", "tool_calls", "length"...
    pub motivo_fim: Option<String>,
    /// Tokens gastos (zerado se o servidor não informar).
    pub uso: Uso,
}

/// O que o cliente avisa enquanto o stream chega (para mostrar na tela).
#[derive(Debug, Clone, PartialEq)]
pub enum EventoStream {
    /// Pedaço de texto da resposta.
    Texto(String),
    /// Pedaço do raciocínio do modelo.
    Raciocinio(String),
    /// O modelo começou a pedir esta ferramenta.
    InicioFerramenta(String),
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn mensagem_de_ferramenta_serializa_no_formato_da_api() {
        let m = Mensagem::resultado_ferramenta("call_1", "ler_arquivo", "conteúdo");
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(
            v,
            json!({"role": "tool", "content": "conteúdo", "tool_call_id": "call_1", "name": "ler_arquivo"})
        );
    }

    #[test]
    fn extra_vai_para_a_raiz_do_pedido() {
        let mut extra = Map::new();
        extra.insert(
            "chat_template_kwargs".into(),
            json!({"enable_thinking": true}),
        );
        let pedido = PedidoChat {
            model: "m".into(),
            messages: vec![Mensagem::usuario("oi")],
            tools: vec![],
            tool_choice: None,
            stream: false,
            stream_options: None,
            max_tokens: Some(10),
            temperature: None,
            top_p: None,
            extra,
            esforco: Some(crate::esforco::EsforcoAplicado {
                nivel: crate::esforco::NivelEsforco::High,
                confirmado: true,
            }),
        };
        let v = serde_json::to_value(&pedido).unwrap();
        assert!(v.get("esforco").is_none(), "o nível não vai para o NIM");
        assert_eq!(v["chat_template_kwargs"]["enable_thinking"], json!(true));
        assert_eq!(v["max_tokens"], json!(10));
        assert!(v.get("tools").is_none(), "lista vazia não deve ser enviada");
        assert!(v.get("temperature").is_none());
    }

    #[test]
    fn resposta_com_tool_calls_e_reasoning_alternativo() {
        let texto = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "reasoning": "pensando",
                    "tool_calls": [{"id": "c1", "type": "function",
                        "function": {"name": "somar", "arguments": "{\"a\":1}"}}]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7}
        }"#;
        let r: RespostaChat = serde_json::from_str(texto).unwrap();
        let m = &r.choices[0].message;
        assert_eq!(m.reasoning_content.as_deref(), Some("pensando"));
        assert_eq!(m.chamadas()[0].function.name, "somar");
        assert_eq!(r.usage.unwrap().total_tokens, 7);
    }
}
