//! Leitura de Server-Sent Events (SSE) e montagem da resposta em streaming.
//!
//! Formato SSE, em resumo: o servidor manda linhas de texto. Linhas que
//! começam com `data:` carregam o conteúdo; uma linha EM BRANCO encerra
//! um evento. Com `stream: true`, cada evento traz um JSON `PedacoStream`
//! e o último evento é o texto literal `[DONE]`.

use std::collections::BTreeMap;

use super::tipos::{
    ChamadaFerramenta, EventoStream, FuncaoChamada, Mensagem, Papel, PedacoStream, RespostaModelo,
    Uso,
};

/// Junta os bytes que chegam da rede e devolve os eventos completos.
///
/// Guardamos BYTES (e não texto) porque um caractere UTF-8 de vários
/// bytes (como "ç") pode chegar partido entre dois pacotes. Só
/// convertemos para texto quando uma linha inteira chegou.
#[derive(Debug, Default)]
pub struct LeitorSse {
    pendente: Vec<u8>,
    dados_do_evento: Vec<String>,
}

impl LeitorSse {
    pub fn novo() -> LeitorSse {
        LeitorSse::default()
    }

    /// Recebe mais bytes e devolve o campo `data` de cada evento que terminou.
    pub fn alimentar(&mut self, bytes: &[u8]) -> Vec<String> {
        self.pendente.extend_from_slice(bytes);
        let mut prontos = Vec::new();

        // Enquanto houver uma linha completa (terminada em \n) no buffer...
        while let Some(pos) = self.pendente.iter().position(|b| *b == b'\n') {
            let linha_bytes: Vec<u8> = self.pendente.drain(..=pos).collect();
            let linha = String::from_utf8_lossy(&linha_bytes);
            let linha = linha.trim_end_matches(['\n', '\r']);

            if linha.is_empty() {
                // Linha em branco: fim do evento.
                if !self.dados_do_evento.is_empty() {
                    prontos.push(self.dados_do_evento.join("\n"));
                    self.dados_do_evento.clear();
                }
            } else if let Some(resto) = linha.strip_prefix("data:") {
                // O espaço depois de "data:" é opcional no padrão SSE.
                let resto = resto.strip_prefix(' ').unwrap_or(resto);
                self.dados_do_evento.push(resto.to_string());
            }
            // Outras linhas (comentários ":", "event:", "id:", "retry:") são ignoradas.
        }
        prontos
    }

    /// Chamado quando a conexão termina: entrega um evento que ficou sem
    /// a linha em branco final (alguns servidores fazem isso).
    pub fn finalizar(&mut self) -> Option<String> {
        if !self.pendente.is_empty() {
            let resto = std::mem::take(&mut self.pendente);
            let resto = String::from_utf8_lossy(&resto).to_string();
            let _ = self.alimentar(format!("{resto}\n").as_bytes());
        }
        if self.dados_do_evento.is_empty() {
            None
        } else {
            let dados = self.dados_do_evento.join("\n");
            self.dados_do_evento.clear();
            Some(dados)
        }
    }
}

/// Uma chamada de ferramenta sendo montada pedaço por pedaço.
#[derive(Debug, Default)]
struct ChamadaEmMontagem {
    id: String,
    nome: String,
    argumentos: String,
}

/// Vai somando os pedaços do stream até formar a resposta completa.
#[derive(Debug, Default)]
pub struct AcumuladorStream {
    texto: String,
    raciocinio: String,
    /// Chave = `index` da chamada. BTreeMap mantém a ordem dos índices.
    chamadas: BTreeMap<u32, ChamadaEmMontagem>,
    motivo_fim: Option<String>,
    uso: Uso,
}

impl AcumuladorStream {
    pub fn novo() -> AcumuladorStream {
        AcumuladorStream::default()
    }

    /// Incorpora um pedaço e devolve o que vale a pena mostrar na tela.
    pub fn aplicar(&mut self, pedaco: PedacoStream) -> Vec<EventoStream> {
        let mut eventos = Vec::new();

        if let Some(uso) = pedaco.usage {
            self.uso = uso;
        }

        // Usamos só a primeira escolha (sempre pedimos n = 1).
        let Some(escolha) = pedaco.choices.into_iter().next() else {
            return eventos;
        };
        if escolha.finish_reason.is_some() {
            self.motivo_fim = escolha.finish_reason;
        }

        let delta = escolha.delta;
        if let Some(texto) = delta.reasoning_content
            && !texto.is_empty()
        {
            self.raciocinio.push_str(&texto);
            eventos.push(EventoStream::Raciocinio(texto));
        }
        if let Some(texto) = delta.content
            && !texto.is_empty()
        {
            self.texto.push_str(&texto);
            eventos.push(EventoStream::Texto(texto));
        }
        for parte in delta.tool_calls.unwrap_or_default() {
            let chamada = self.chamadas.entry(parte.index).or_default();
            if let Some(id) = parte.id
                && !id.is_empty()
            {
                chamada.id = id;
            }
            if let Some(funcao) = parte.function {
                if let Some(nome) = funcao.name
                    && !nome.is_empty()
                {
                    // Primeira vez que vemos o nome: avisa quem está assistindo.
                    if chamada.nome.is_empty() {
                        eventos.push(EventoStream::InicioFerramenta(nome.clone()));
                    }
                    chamada.nome = nome;
                }
                if let Some(argumentos) = funcao.arguments {
                    chamada.argumentos.push_str(&argumentos);
                }
            }
        }
        eventos
    }

    /// Monta a `RespostaModelo` final.
    pub fn finalizar(self) -> RespostaModelo {
        let chamadas: Vec<ChamadaFerramenta> = self
            .chamadas
            .into_iter()
            .map(|(indice, c)| ChamadaFerramenta {
                // Se o servidor não mandou id, criamos um estável.
                id: if c.id.is_empty() {
                    format!("chamada_{indice}")
                } else {
                    c.id
                },
                tipo: "function".to_string(),
                function: FuncaoChamada {
                    name: c.nome,
                    arguments: c.argumentos,
                },
            })
            .collect();

        let mensagem = Mensagem {
            role: Papel::Assistant,
            content: if self.texto.is_empty() {
                None
            } else {
                Some(self.texto)
            },
            reasoning_content: if self.raciocinio.is_empty() {
                None
            } else {
                Some(self.raciocinio)
            },
            tool_calls: if chamadas.is_empty() {
                None
            } else {
                Some(chamadas)
            },
            tool_call_id: None,
            name: None,
        };
        RespostaModelo {
            mensagem,
            motivo_fim: self.motivo_fim,
            uso: self.uso,
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn junta_eventos_partidos_e_utf8_partido() {
        let mut leitor = LeitorSse::novo();
        // "ção" partido no meio de um caractere de 2 bytes.
        let completo = "data: {\"a\":\"ação\"}\n\n".as_bytes();
        let (a, b) = completo.split_at(14);
        assert!(leitor.alimentar(a).is_empty());
        let eventos = leitor.alimentar(b);
        assert_eq!(eventos, vec!["{\"a\":\"ação\"}".to_string()]);
    }

    #[test]
    fn ignora_comentarios_e_aceita_crlf_e_varias_linhas_data() {
        let mut leitor = LeitorSse::novo();
        let eventos =
            leitor.alimentar(b": ping\r\ndata: linha1\r\ndata:linha2\r\n\r\ndata: [DONE]\n\n");
        assert_eq!(
            eventos,
            vec!["linha1\nlinha2".to_string(), "[DONE]".to_string()]
        );
    }

    #[test]
    fn finalizar_entrega_evento_sem_linha_em_branco() {
        let mut leitor = LeitorSse::novo();
        assert!(leitor.alimentar(b"data: ultimo").is_empty());
        assert_eq!(leitor.finalizar(), Some("ultimo".to_string()));
    }

    #[test]
    fn acumula_texto_raciocinio_e_ferramentas_picadas() {
        let pedacos = [
            r#"{"choices":[{"delta":{"reasoning_content":"pen"}}]}"#,
            r#"{"choices":[{"delta":{"reasoning_content":"sando"}}]}"#,
            r#"{"choices":[{"delta":{"content":"Olá"}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"somar","arguments":"{\"a\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"1}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}"#,
        ];
        let mut acc = AcumuladorStream::novo();
        let mut eventos = Vec::new();
        for p in pedacos {
            eventos.extend(acc.aplicar(serde_json::from_str(p).unwrap()));
        }
        assert!(eventos.contains(&EventoStream::InicioFerramenta("somar".into())));
        let r = acc.finalizar();
        assert_eq!(r.mensagem.texto(), "Olá");
        assert_eq!(r.mensagem.reasoning_content.as_deref(), Some("pensando"));
        assert_eq!(r.mensagem.chamadas()[0].function.arguments, "{\"a\":1}");
        assert_eq!(r.mensagem.chamadas()[0].id, "c1");
        assert_eq!(r.motivo_fim.as_deref(), Some("tool_calls"));
        assert_eq!(r.uso.total_tokens, 7);
    }
}
