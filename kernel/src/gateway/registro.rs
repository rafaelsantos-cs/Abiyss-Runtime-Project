//! O registro das mensagens do gateway (`gateway_mensagens`), que também é
//! a fila nos dois sentidos.
//!
//! Entrada (`direcao = 'entrada'`), por tipo:
//! - `dono`: o dono falando. `pendente` → `processando` → `respondida`
//!   (ou `erro`; `interrompida` se o daemon parou no meio do turno);
//! - `comando`: `/status`, `/pedidos`... respondido na hora;
//! - `resposta_pedido`: "responder" do dono a um pedido (vira a resposta);
//! - `externo`: outra pessoa ou bot no canal permitido (vira evento);
//! - `ignorado`: DM de estranho, outro canal... (o texto NÃO é guardado);
//! - `limitado`: passou do teto por minuto.
//!
//! Saída (`direcao = 'saida'`): `resposta`, `comando`, `pedido`, `resumo`,
//! `aviso`. `transmitindo` (o turno ainda escreve) → `pendente` (esperando
//! o adaptador confirmar a entrega) → `entregue` (ou `falhou` depois de
//! `MAX_TENTATIVAS`). Entrega é "pelo menos uma vez": sem confirmação em
//! `ESPERA_CONFIRMACAO_MS`, manda de novo.

use rusqlite::{OptionalExtension, params};

use crate::db::Banco;

use super::confianca::MensagemDiscord;

/// Sem confirmação do adaptador depois disto, a saída é mandada de novo.
pub const ESPERA_CONFIRMACAO_MS: i64 = 60_000;
/// Tentativas de entrega antes de desistir de uma saída.
pub const MAX_TENTATIVAS: i64 = 5;

/// Uma mensagem do dono esperando (ou recebendo) resposta.
#[derive(Debug, Clone, PartialEq)]
pub struct Entrada {
    pub id: i64,
    pub momento_ms: i64,
    pub canal_id: String,
    pub discord_id: String,
    pub conteudo: String,
}

/// Uma mensagem a entregar.
#[derive(Debug, Clone, PartialEq)]
pub struct Saida {
    pub id: i64,
    pub tipo: String,
    /// `None` = DM do dono.
    pub canal_id: Option<String>,
    pub responde_a: Option<String>,
    pub pedido_id: Option<i64>,
    pub conteudo: String,
    pub tentativas: i64,
}

/// O que gravar de uma mensagem que chegou.
pub struct NovaEntrada<'a> {
    pub mensagem: &'a MensagemDiscord,
    pub tipo: &'a str,
    pub estado: &'a str,
    /// `None` = o texto não é guardado (ex.: DM de estranho).
    pub conteudo: Option<&'a str>,
    pub origem_externa: Option<&'a str>,
    pub pedido_id: Option<i64>,
}

/// Grava a entrada. `None` = essa mensagem do Discord já estava registrada
/// (o adaptador mandou de novo depois de uma reconexão): nada a fazer.
pub fn registrar_entrada(
    banco: &Banco,
    nova: &NovaEntrada<'_>,
    agora: i64,
) -> anyhow::Result<Option<i64>> {
    let m = nova.mensagem;
    let conexao = banco.conexao();
    let n = conexao.execute(
        "INSERT OR IGNORE INTO gateway_mensagens
           (momento_ms, direcao, tipo, estado, canal_id, autor_id, discord_id, responde_a,
            pedido_id, conteudo, origem_externa)
         VALUES (?1, 'entrada', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            agora,
            nova.tipo,
            nova.estado,
            m.canal_id,
            m.autor_id,
            m.id,
            m.responde_a,
            nova.pedido_id,
            nova.conteudo,
            nova.origem_externa
        ],
    )?;
    Ok((n > 0).then(|| conexao.last_insert_rowid()))
}

/// Pega TODAS as mensagens do dono que estão na fila (as que chegaram
/// enquanto ele estava ocupado viram um turno só) e marca como
/// `processando`.
pub fn tomar_lote_do_dono(banco: &Banco) -> anyhow::Result<Vec<Entrada>> {
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    let lote: Vec<Entrada> = {
        let mut consulta = transacao.prepare(
            "SELECT id, momento_ms, canal_id, discord_id, conteudo FROM gateway_mensagens
              WHERE direcao = 'entrada' AND tipo = 'dono' AND estado = 'pendente'
              ORDER BY id",
        )?;
        consulta
            .query_map([], |l| {
                Ok(Entrada {
                    id: l.get(0)?,
                    momento_ms: l.get(1)?,
                    canal_id: l.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    discord_id: l.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    conteudo: l.get::<_, Option<String>>(4)?.unwrap_or_default(),
                })
            })?
            .collect::<Result<_, _>>()?
    };
    for e in &lote {
        transacao.execute(
            "UPDATE gateway_mensagens SET estado = 'processando' WHERE id = ?1",
            params![e.id],
        )?;
    }
    transacao.commit()?;
    Ok(lote)
}

/// Fecha as entradas de um turno (`respondida`, `erro`...).
pub fn concluir_entradas(
    banco: &Banco,
    ids: &[i64],
    estado: &str,
    agora: i64,
) -> anyhow::Result<()> {
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    for id in ids {
        transacao.execute(
            "UPDATE gateway_mensagens SET estado = ?1, concluido_ms = ?2 WHERE id = ?3",
            params![estado, agora, id],
        )?;
    }
    transacao.commit()?;
    Ok(())
}

/// Ao subir: entradas que estavam `processando` (o daemon parou no meio do
/// turno) viram `interrompida` e são devolvidas, para avisar o dono. A
/// mensagem já está no histórico da conversa; repetir o turno sozinho
/// poderia repetir ferramentas (delegar, escrever arquivo...). Saídas que
/// estavam `transmitindo` voltam a `pendente` (o texto parcial é entregue).
pub fn recuperar_interrompidas(banco: &Banco, agora: i64) -> anyhow::Result<Vec<Entrada>> {
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    let lista: Vec<Entrada> = {
        let mut consulta = transacao.prepare(
            "SELECT id, momento_ms, canal_id, discord_id, conteudo FROM gateway_mensagens
              WHERE direcao = 'entrada' AND estado = 'processando' ORDER BY id",
        )?;
        consulta
            .query_map([], |l| {
                Ok(Entrada {
                    id: l.get(0)?,
                    momento_ms: l.get(1)?,
                    canal_id: l.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    discord_id: l.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    conteudo: l.get::<_, Option<String>>(4)?.unwrap_or_default(),
                })
            })?
            .collect::<Result<_, _>>()?
    };
    transacao.execute(
        "UPDATE gateway_mensagens SET estado = 'interrompida', concluido_ms = ?1
          WHERE direcao = 'entrada' AND estado = 'processando'",
        params![agora],
    )?;
    transacao.execute(
        "UPDATE gateway_mensagens SET estado = 'pendente'
          WHERE direcao = 'saida' AND estado = 'transmitindo'",
        [],
    )?;
    transacao.commit()?;
    Ok(lista)
}

/// Uma saída nova.
pub struct NovaSaida<'a> {
    pub tipo: &'a str,
    /// `pendente` (pronta para entregar) ou `transmitindo`.
    pub estado: &'a str,
    pub canal_id: Option<&'a str>,
    pub responde_a: Option<&'a str>,
    pub pedido_id: Option<i64>,
    pub conteudo: &'a str,
}

pub fn nova_saida(banco: &Banco, nova: &NovaSaida<'_>, agora: i64) -> anyhow::Result<i64> {
    let conexao = banco.conexao();
    conexao.execute(
        "INSERT INTO gateway_mensagens
           (momento_ms, direcao, tipo, estado, canal_id, responde_a, pedido_id, conteudo)
         VALUES (?1, 'saida', ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            agora,
            nova.tipo,
            nova.estado,
            nova.canal_id,
            nova.responde_a,
            nova.pedido_id,
            nova.conteudo
        ],
    )?;
    Ok(conexao.last_insert_rowid())
}

/// O turno terminou: a resposta fica `pendente` com o texto final. Se o
/// fim da transmissão já foi mandado ao adaptador (`mandado_em`), isso conta
/// como uma tentativa (a entrega só repete sem confirmação no prazo).
pub fn finalizar_transmissao(
    banco: &Banco,
    id: i64,
    conteudo: &str,
    mandado_em: Option<i64>,
) -> anyhow::Result<()> {
    banco.conexao().execute(
        "UPDATE gateway_mensagens
            SET conteudo = ?1, estado = 'pendente',
                tentativas = tentativas + (?2 IS NOT NULL), tentativa_ms = ?2
          WHERE id = ?3 AND estado = 'transmitindo'",
        params![conteudo, mandado_em, id],
    )?;
    Ok(())
}

/// Saídas prontas para (re)entregar: pendentes nunca tentadas ou cuja última
/// tentativa ficou sem confirmação por `ESPERA_CONFIRMACAO_MS`.
pub fn saidas_a_entregar(banco: &Banco, agora: i64, limite: usize) -> anyhow::Result<Vec<Saida>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT id, tipo, canal_id, responde_a, pedido_id, conteudo, tentativas
           FROM gateway_mensagens
          WHERE direcao = 'saida' AND estado = 'pendente'
            AND (tentativa_ms IS NULL OR tentativa_ms <= ?1)
          ORDER BY id LIMIT ?2",
    )?;
    let lista = consulta
        .query_map(params![agora - ESPERA_CONFIRMACAO_MS, limite as i64], |l| {
            Ok(Saida {
                id: l.get(0)?,
                tipo: l.get(1)?,
                canal_id: l.get(2)?,
                responde_a: l.get(3)?,
                pedido_id: l.get(4)?,
                conteudo: l.get::<_, Option<String>>(5)?.unwrap_or_default(),
                tentativas: l.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(lista)
}

/// Uma tentativa de entrega começou. Passou de `MAX_TENTATIVAS`, desiste.
pub fn marcar_tentativa(banco: &Banco, id: i64, agora: i64) -> anyhow::Result<()> {
    banco.conexao().execute(
        "UPDATE gateway_mensagens
            SET tentativas = tentativas + 1, tentativa_ms = ?1,
                estado = CASE WHEN tentativas + 1 > ?2 THEN 'falhou' ELSE estado END,
                concluido_ms = CASE WHEN tentativas + 1 > ?2 THEN ?1 ELSE concluido_ms END
          WHERE id = ?3",
        params![agora, MAX_TENTATIVAS, id],
    )?;
    Ok(())
}

/// O adaptador entregou: guarda os IDs das mensagens do Discord (a
/// primeira na própria linha; todas no mapa, para o "responder").
pub fn confirmar_entrega(banco: &Banco, id: i64, ids: &[String], agora: i64) -> anyhow::Result<()> {
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    transacao.execute(
        "UPDATE gateway_mensagens SET estado = 'entregue', concluido_ms = ?1,
                discord_id = COALESCE(?2, discord_id)
          WHERE id = ?3 AND direcao = 'saida'",
        params![agora, ids.first(), id],
    )?;
    for discord in ids {
        transacao.execute(
            "INSERT OR IGNORE INTO gateway_ids_discord (discord_id, mensagem_id) VALUES (?1, ?2)",
            params![discord, id],
        )?;
    }
    transacao.commit()?;
    Ok(())
}

/// A entrega falhou do lado do adaptador: tenta de novo depois de
/// `ESPERA_CONFIRMACAO_MS` (as tentativas já foram contadas).
pub fn registrar_falha(banco: &Banco, id: i64) -> anyhow::Result<()> {
    banco.conexao().execute(
        "UPDATE gateway_mensagens
            SET estado = CASE WHEN tentativas >= ?1 THEN 'falhou' ELSE estado END
          WHERE id = ?2 AND direcao = 'saida' AND estado = 'pendente'",
        params![MAX_TENTATIVAS, id],
    )?;
    Ok(())
}

/// Uma saída que não deve mais ir (ex.: pedido já respondido por outro
/// caminho antes de a DM ser entregue).
pub fn cancelar_saida(banco: &Banco, id: i64, agora: i64) -> anyhow::Result<()> {
    banco.conexao().execute(
        "UPDATE gateway_mensagens SET estado = 'cancelada', concluido_ms = ?1
          WHERE id = ?2 AND direcao = 'saida' AND estado = 'pendente'",
        params![agora, id],
    )?;
    Ok(())
}

/// A saída (lógica) a que pertence uma mensagem do Discord.
pub fn saida_do_discord(banco: &Banco, discord_id: &str) -> anyhow::Result<Option<Saida>> {
    let s = banco
        .conexao()
        .query_row(
            "SELECT g.id, g.tipo, g.canal_id, g.responde_a, g.pedido_id, g.conteudo, g.tentativas
               FROM gateway_ids_discord i JOIN gateway_mensagens g ON g.id = i.mensagem_id
              WHERE i.discord_id = ?1",
            params![discord_id],
            |l| {
                Ok(Saida {
                    id: l.get(0)?,
                    tipo: l.get(1)?,
                    canal_id: l.get(2)?,
                    responde_a: l.get(3)?,
                    pedido_id: l.get(4)?,
                    conteudo: l.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    tentativas: l.get(6)?,
                })
            },
        )
        .optional()?;
    Ok(s)
}

/// Maior ID do Discord já recebido do dono fora do canal permitido (a DM)
/// e no canal permitido: o adaptador busca o que chegou depois disso
/// enquanto ele estava fora do ar.
pub fn ultimos_recebidos(
    banco: &Banco,
    dono: &str,
    canal: Option<&str>,
) -> anyhow::Result<(Option<String>, Option<String>)> {
    let conexao = banco.conexao();
    let canal = canal.unwrap_or("");
    let maximo = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> rusqlite::Result<Option<String>> {
        conexao
            .query_row(sql, p, |l| l.get::<_, Option<i64>>(0))
            .map(|v| v.map(|n| n.to_string()))
    };
    let dm = maximo(
        "SELECT MAX(CAST(discord_id AS INTEGER)) FROM gateway_mensagens
          WHERE direcao = 'entrada' AND autor_id = ?1 AND canal_id <> ?2",
        &[&dono, &canal],
    )?;
    let no_canal = maximo(
        "SELECT MAX(CAST(discord_id AS INTEGER)) FROM gateway_mensagens
          WHERE direcao = 'entrada' AND canal_id = ?1",
        &[&canal],
    )?;
    Ok((dm, no_canal))
}

#[cfg(test)]
mod testes {
    use super::*;

    fn msg(id: &str, texto: &str) -> MensagemDiscord {
        MensagemDiscord {
            id: id.into(),
            canal_id: "10".into(),
            dm: true,
            autor_id: "7".into(),
            autor_nome: String::new(),
            autor_bot: false,
            menciona_bot: false,
            responde_a: None,
            responde_ao_bot: false,
            texto: texto.into(),
        }
    }

    fn entrada<'a>(m: &'a MensagemDiscord) -> NovaEntrada<'a> {
        NovaEntrada {
            mensagem: m,
            tipo: "dono",
            estado: "pendente",
            conteudo: Some(&m.texto),
            origem_externa: None,
            pedido_id: None,
        }
    }

    #[test]
    fn fila_de_entrada_sem_duplicar_e_em_lote() {
        let banco = Banco::em_memoria().unwrap();
        let (a, b) = (msg("100", "oi"), msg("101", "tudo bem?"));
        let id_a = registrar_entrada(&banco, &entrada(&a), 1).unwrap();
        assert!(id_a.is_some());
        // O adaptador mandou de novo (reconexão): nada duplica.
        assert_eq!(registrar_entrada(&banco, &entrada(&a), 2).unwrap(), None);
        registrar_entrada(&banco, &entrada(&b), 3).unwrap();
        let lote = tomar_lote_do_dono(&banco).unwrap();
        assert_eq!(
            lote.iter().map(|e| e.conteudo.as_str()).collect::<Vec<_>>(),
            ["oi", "tudo bem?"]
        );
        // Já tomadas: o próximo lote vem vazio.
        assert!(tomar_lote_do_dono(&banco).unwrap().is_empty());
        // Parou no meio: interrompidas, e não voltam para a fila.
        let interrompidas = recuperar_interrompidas(&banco, 9).unwrap();
        assert_eq!(interrompidas.len(), 2);
        assert!(tomar_lote_do_dono(&banco).unwrap().is_empty());
        assert_eq!(
            ultimos_recebidos(&banco, "7", None).unwrap(),
            (Some("101".into()), None)
        );
    }

    #[test]
    fn saida_entrega_confirma_e_desiste() {
        let banco = Banco::em_memoria().unwrap();
        let nova = NovaSaida {
            tipo: "resposta",
            estado: "pendente",
            canal_id: None,
            responde_a: Some("100"),
            pedido_id: Some(3),
            conteudo: "olá",
        };
        let id = nova_saida(&banco, &nova, 1_000_000).unwrap();
        let agora = 2_000_000;
        assert_eq!(saidas_a_entregar(&banco, agora, 10).unwrap().len(), 1);
        marcar_tentativa(&banco, id, agora).unwrap();
        // Esperando confirmação: não manda de novo antes da hora.
        assert!(saidas_a_entregar(&banco, agora + 1, 10).unwrap().is_empty());
        let depois = agora + ESPERA_CONFIRMACAO_MS;
        assert_eq!(saidas_a_entregar(&banco, depois, 10).unwrap().len(), 1);
        confirmar_entrega(&banco, id, &["900".into(), "901".into()], depois).unwrap();
        assert!(
            saidas_a_entregar(&banco, depois * 2, 10)
                .unwrap()
                .is_empty()
        );
        // Qualquer pedaço da resposta leva à linha lógica.
        let s = saida_do_discord(&banco, "901").unwrap().unwrap();
        assert_eq!((s.id, s.pedido_id), (id, Some(3)));
        assert_eq!(saida_do_discord(&banco, "999").unwrap(), None);

        // Sem confirmação nunca: desiste depois de MAX_TENTATIVAS.
        let outra = nova_saida(&banco, &nova, 1).unwrap();
        for i in 0..=MAX_TENTATIVAS {
            marcar_tentativa(&banco, outra, 10 + i).unwrap();
        }
        let estado: String = banco
            .conexao()
            .query_row(
                "SELECT estado FROM gateway_mensagens WHERE id = ?1",
                params![outra],
                |l| l.get(0),
            )
            .unwrap();
        assert_eq!(estado, "falhou");
    }
}
