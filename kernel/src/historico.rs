//! Histórico de conversas no SQLite.
//!
//! Cada conversa tem várias mensagens (usuário, assistente e resultados de
//! ferramentas). O system prompt NÃO é guardado: ele é montado de novo a
//! cada turno, para refletir o núcleo de identidade e o contexto atuais.

use std::collections::HashSet;

use anyhow::Context;
use rusqlite::{OptionalExtension, params};

use crate::db::Banco;
use crate::nim::{ChamadaFerramenta, Mensagem, Papel};
use crate::tempo::agora_ms;

/// Cria uma conversa nova e devolve o ID.
pub fn criar_conversa(banco: &Banco) -> anyhow::Result<i64> {
    let conexao = banco.conexao();
    conexao.execute(
        "INSERT INTO conversas (criada_ms) VALUES (?1)",
        params![agora_ms()],
    )?;
    Ok(conexao.last_insert_rowid())
}

/// ID da conversa mais recente, se houver alguma.
pub fn ultima_conversa(banco: &Banco) -> anyhow::Result<Option<i64>> {
    let id = banco
        .conexao()
        .query_row("SELECT MAX(id) FROM conversas", [], |l| l.get(0))
        .optional()?
        .flatten();
    Ok(id)
}

pub fn conversa_existe(banco: &Banco, id: i64) -> anyhow::Result<bool> {
    let achou: Option<i64> = banco
        .conexao()
        .query_row("SELECT id FROM conversas WHERE id = ?1", params![id], |l| {
            l.get(0)
        })
        .optional()?;
    Ok(achou.is_some())
}

/// Grava uma mensagem no fim da conversa (sem conteúdo externo).
pub fn adicionar(banco: &Banco, conversa: i64, mensagem: &Mensagem) -> anyhow::Result<()> {
    adicionar_com_origem(banco, conversa, mensagem, None)
}

/// Grava uma mensagem marcando se ela traz (ou deriva de) conteúdo externo.
/// `origem_externa = Some("mcp:x")` em resultados de ferramentas externas e
/// em respostas do modelo escritas logo depois deles (ver `chat`).
pub fn adicionar_com_origem(
    banco: &Banco,
    conversa: i64,
    mensagem: &Mensagem,
    origem_externa: Option<&str>,
) -> anyhow::Result<()> {
    let chamadas = match &mensagem.tool_calls {
        Some(lista) => Some(serde_json::to_string(lista)?),
        None => None,
    };
    banco.conexao().execute(
        "INSERT INTO mensagens
           (conversa_id, momento_ms, papel, conteudo, raciocinio, chamadas_json,
            id_chamada, nome_ferramenta, origem_externa)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            conversa,
            agora_ms(),
            mensagem.role.como_texto(),
            mensagem.content,
            mensagem.reasoning_content,
            chamadas,
            mensagem.tool_call_id,
            mensagem.name,
            origem_externa
        ],
    )?;
    Ok(())
}

/// Origens externas presentes nas últimas `limite` mensagens (a mesma
/// janela que `carregar` coloca no contexto), sem repetição.
/// Lista vazia = o contexto não tem conteúdo externo.
pub fn origens_externas(
    banco: &Banco,
    conversa: i64,
    limite: usize,
) -> anyhow::Result<Vec<String>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT origem_externa FROM
           (SELECT id, origem_externa FROM mensagens WHERE conversa_id = ?1
            ORDER BY id DESC LIMIT ?2)
         WHERE origem_externa IS NOT NULL ORDER BY id ASC",
    )?;
    let mut origens: Vec<String> = Vec::new();
    for origem in consulta.query_map(params![conversa, limite as i64], |l| l.get::<_, String>(0))? {
        // Respostas derivadas guardam várias origens separadas por ", ".
        for parte in origem?.split(", ") {
            if !origens.iter().any(|o| o == parte) {
                origens.push(parte.to_string());
            }
        }
    }
    Ok(origens)
}

/// Lê as últimas `limite` mensagens da conversa, em ordem cronológica,
/// já saneadas (ver `sanear`).
pub fn carregar(banco: &Banco, conversa: i64, limite: usize) -> anyhow::Result<Vec<Mensagem>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT papel, conteudo, raciocinio, chamadas_json, id_chamada, nome_ferramenta
         FROM mensagens WHERE conversa_id = ?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let linhas = consulta.query_map(params![conversa, limite as i64], |l| {
        Ok((
            l.get::<_, String>(0)?,
            l.get::<_, Option<String>>(1)?,
            l.get::<_, Option<String>>(2)?,
            l.get::<_, Option<String>>(3)?,
            l.get::<_, Option<String>>(4)?,
            l.get::<_, Option<String>>(5)?,
        ))
    })?;

    let mut mensagens = Vec::new();
    for linha in linhas {
        let (papel, conteudo, raciocinio, chamadas, id_chamada, nome) = linha?;
        let role = Papel::de_texto(&papel).with_context(|| format!("papel inválido: {papel}"))?;
        let tool_calls: Option<Vec<ChamadaFerramenta>> = match chamadas {
            Some(json) => Some(serde_json::from_str(&json)?),
            None => None,
        };
        mensagens.push(Mensagem {
            role,
            content: conteudo,
            reasoning_content: raciocinio,
            tool_calls,
            tool_call_id: id_chamada,
            name: nome,
        });
    }
    // Veio do mais novo para o mais velho; invertemos.
    mensagens.reverse();
    Ok(sanear(mensagens))
}

/// Deixa o histórico num formato que a API aceita:
/// - começa sempre numa mensagem do usuário (o corte por `limite` pode
///   ter deixado resultados de ferramenta "órfãos" no início);
/// - remove pedidos de ferramenta cujos resultados não estão todos lá
///   (ex.: o processo caiu no meio de uma chamada) e resultados sem pedido.
pub fn sanear(mensagens: Vec<Mensagem>) -> Vec<Mensagem> {
    // 1. Pula tudo até a primeira mensagem do usuário.
    let inicio = mensagens
        .iter()
        .position(|m| m.role == Papel::User)
        .unwrap_or(mensagens.len());
    let mensagens: Vec<Mensagem> = mensagens.into_iter().skip(inicio).collect();

    // 2. Percorre mantendo só sequências completas de ferramenta.
    let mut saida: Vec<Mensagem> = Vec::new();
    let mut i = 0;
    while i < mensagens.len() {
        let atual = &mensagens[i];
        if atual.role == Papel::Assistant && !atual.chamadas().is_empty() {
            let pedidos: HashSet<&str> = atual.chamadas().iter().map(|c| c.id.as_str()).collect();
            // Junta os resultados de ferramenta logo em seguida.
            let mut j = i + 1;
            let mut respondidos = HashSet::new();
            while j < mensagens.len() && mensagens[j].role == Papel::Tool {
                if let Some(id) = mensagens[j].tool_call_id.as_deref() {
                    respondidos.insert(id);
                }
                j += 1;
            }
            if pedidos.iter().all(|id| respondidos.contains(id)) {
                // Sequência completa: mantém o pedido e só os resultados que casam.
                saida.push(atual.clone());
                for resultado in &mensagens[i + 1..j] {
                    if let Some(id) = resultado.tool_call_id.as_deref()
                        && pedidos.contains(id)
                    {
                        saida.push(resultado.clone());
                    }
                }
            }
            i = j;
        } else if atual.role == Papel::Tool {
            // Resultado sem pedido logo antes: descarta.
            i += 1;
        } else {
            saida.push(atual.clone());
            i += 1;
        }
    }
    saida
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::nim::tipos::FuncaoChamada;

    fn pedido_ferramenta(ids: &[&str]) -> Mensagem {
        let mut m = Mensagem::assistente("");
        m.content = None;
        m.tool_calls = Some(
            ids.iter()
                .map(|id| ChamadaFerramenta {
                    id: id.to_string(),
                    tipo: "function".into(),
                    function: FuncaoChamada {
                        name: "f".into(),
                        arguments: "{}".into(),
                    },
                })
                .collect(),
        );
        m
    }

    #[test]
    fn sanear_remove_orfaos_e_sequencias_incompletas() {
        let lista = vec![
            Mensagem::resultado_ferramenta("x", "f", "órfão do corte"),
            Mensagem::assistente("resposta antiga"),
            Mensagem::usuario("pergunta 1"),
            pedido_ferramenta(&["a", "b"]),
            Mensagem::resultado_ferramenta("a", "f", "ra"),
            Mensagem::resultado_ferramenta("b", "f", "rb"),
            Mensagem::assistente("resposta 1"),
            Mensagem::usuario("pergunta 2"),
            pedido_ferramenta(&["c", "d"]),
            Mensagem::resultado_ferramenta("c", "f", "rc"), // falta o "d"
        ];
        let limpa = sanear(lista);
        let papeis: Vec<Papel> = limpa.iter().map(|m| m.role).collect();
        assert_eq!(
            papeis,
            vec![
                Papel::User,
                Papel::Assistant,
                Papel::Tool,
                Papel::Tool,
                Papel::Assistant,
                Papel::User
            ]
        );
    }

    #[test]
    fn origem_externa_vale_so_dentro_da_janela() {
        let banco = Banco::em_memoria().unwrap();
        let c = criar_conversa(&banco).unwrap();
        adicionar(&banco, c, &Mensagem::usuario("leia o site")).unwrap();
        adicionar_com_origem(&banco, c, &pedido_ferramenta(&["k"]), None).unwrap();
        let resultado = Mensagem::resultado_ferramenta("k", "f", "texto do site");
        adicionar_com_origem(&banco, c, &resultado, Some("mcp:web")).unwrap();
        let resposta = Mensagem::assistente("o site diz X");
        adicionar_com_origem(&banco, c, &resposta, Some("mcp:web, ler_skill")).unwrap();
        assert_eq!(
            origens_externas(&banco, c, 10).unwrap(),
            vec!["mcp:web", "ler_skill"]
        );
        // A resposta derivada continua "contaminada" mesmo sem o resultado na janela.
        assert_eq!(origens_externas(&banco, c, 1).unwrap().len(), 2);
        adicionar(&banco, c, &Mensagem::usuario("outra coisa")).unwrap();
        adicionar(&banco, c, &Mensagem::assistente("ok")).unwrap();
        assert!(origens_externas(&banco, c, 2).unwrap().is_empty());
    }

    #[test]
    fn grava_e_le_com_ferramentas_e_limite() {
        let banco = Banco::em_memoria().unwrap();
        let c = criar_conversa(&banco).unwrap();
        adicionar(&banco, c, &Mensagem::usuario("um")).unwrap();
        adicionar(&banco, c, &Mensagem::assistente("dois")).unwrap();
        adicionar(&banco, c, &Mensagem::usuario("três")).unwrap();
        adicionar(&banco, c, &pedido_ferramenta(&["k"])).unwrap();
        adicionar(&banco, c, &Mensagem::resultado_ferramenta("k", "f", "ok")).unwrap();

        let tudo = carregar(&banco, c, 100).unwrap();
        assert_eq!(tudo.len(), 5);
        assert_eq!(tudo[3].chamadas()[0].id, "k");
        assert_eq!(tudo[4].tool_call_id.as_deref(), Some("k"));

        // Limite 4 corta o "um"; o "dois" (assistente) fica órfão e sai.
        let recorte = carregar(&banco, c, 4).unwrap();
        assert_eq!(recorte[0].texto(), "três");
        assert_eq!(recorte.len(), 3);

        assert_eq!(ultima_conversa(&banco).unwrap(), Some(c));
        assert!(origens_externas(&banco, c, 100).unwrap().is_empty());
        assert!(conversa_existe(&banco, c).unwrap());
        assert!(!conversa_existe(&banco, c + 1).unwrap());
    }
}
