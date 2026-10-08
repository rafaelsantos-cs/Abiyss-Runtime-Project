//! O registro das mensagens do gateway (`gateway_mensagens`), que também é
//! a fila nos dois sentidos.
//!
//! Entrada (`direcao = 'entrada'`), por tipo:
//! - `dono`: o dono falando. `pendente` → `processando` → `respondida`
//!   (ou `erro`; `interrompida` se o daemon parou no meio do turno);
//! - `terceiro`: pessoa conhecida ou alguém num canal permitido (nível 2),
//!   o mesmo ciclo, numa fila à parte (por canal: DM ou canal de servidor);
//! - `comando`: `/status`, `/pedidos`... respondido na hora;
//! - `resposta_pedido`: "responder" do dono a um pedido (vira a resposta);
//! - `ignorado`: DM de estranho, bot, outro canal, canal sem chamar o
//!   bot, terceiro respondendo a pedido... (o texto NÃO é guardado);
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
    pub tipo: String,
    pub autor_id: String,
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
    /// Caminho real de um arquivo do workspace.
    pub anexo: Option<String>,
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

const COLUNAS_ENTRADA: &str = "id, momento_ms, tipo, autor_id, canal_id, discord_id, conteudo";

fn entrada_da_linha(l: &rusqlite::Row<'_>) -> rusqlite::Result<Entrada> {
    Ok(Entrada {
        id: l.get(0)?,
        momento_ms: l.get(1)?,
        tipo: l.get(2)?,
        autor_id: l.get::<_, Option<String>>(3)?.unwrap_or_default(),
        canal_id: l.get::<_, Option<String>>(4)?.unwrap_or_default(),
        discord_id: l.get::<_, Option<String>>(5)?.unwrap_or_default(),
        conteudo: l.get::<_, Option<String>>(6)?.unwrap_or_default(),
    })
}

/// Pega TODAS as mensagens do dono que estão na fila (as que chegaram
/// enquanto ele estava ocupado viram um turno só) e marca como
/// `processando`.
pub fn tomar_lote_do_dono(banco: &Banco) -> anyhow::Result<Vec<Entrada>> {
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    let lote: Vec<Entrada> = {
        let mut consulta = transacao.prepare(&format!(
            "SELECT {COLUNAS_ENTRADA} FROM gateway_mensagens
              WHERE direcao = 'entrada' AND tipo = 'dono' AND estado = 'pendente'
              ORDER BY id"
        ))?;
        consulta
            .query_map([], entrada_da_linha)?
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

/// O próximo lote de outra pessoa: as mensagens pendentes do MESMO autor
/// no MESMO canal (DM ou canal de servidor) da mais antiga da fila, marcadas
/// `processando`. Canal com um turno rodando fica de fora (uma conversa,
/// um turno por vez; outro canal pode rodar em paralelo, até o teto).
pub fn tomar_lote_terceiro(banco: &Banco) -> anyhow::Result<Vec<Entrada>> {
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    let primeira: Option<(String, String)> = transacao
        .query_row(
            "SELECT canal_id, autor_id FROM gateway_mensagens AS g
              WHERE direcao = 'entrada' AND tipo = 'terceiro' AND estado = 'pendente'
                AND NOT EXISTS (
                    SELECT 1 FROM gateway_mensagens AS r
                     WHERE r.direcao = 'entrada' AND r.tipo = 'terceiro'
                       AND r.estado = 'processando' AND r.canal_id = g.canal_id)
              ORDER BY id LIMIT 1",
            [],
            |l| Ok((l.get(0)?, l.get(1)?)),
        )
        .optional()?;
    let Some((canal, autor)) = primeira else {
        return Ok(Vec::new());
    };
    let lote: Vec<Entrada> = {
        let mut consulta = transacao.prepare(&format!(
            "SELECT {COLUNAS_ENTRADA} FROM gateway_mensagens
              WHERE direcao = 'entrada' AND tipo = 'terceiro' AND estado = 'pendente'
                AND canal_id = ?1 AND autor_id = ?2
              ORDER BY id"
        ))?;
        consulta
            .query_map(params![canal, autor], entrada_da_linha)?
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
        let mut consulta = transacao.prepare(&format!(
            "SELECT {COLUNAS_ENTRADA} FROM gateway_mensagens
              WHERE direcao = 'entrada' AND estado = 'processando' ORDER BY id"
        ))?;
        consulta
            .query_map([], entrada_da_linha)?
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
    pub anexo: Option<&'a str>,
}

pub fn nova_saida(banco: &Banco, nova: &NovaSaida<'_>, agora: i64) -> anyhow::Result<i64> {
    let conexao = banco.conexao();
    conexao.execute(
        "INSERT INTO gateway_mensagens
           (momento_ms, direcao, tipo, estado, canal_id, responde_a, pedido_id, conteudo, anexo)
         VALUES (?1, 'saida', ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            agora,
            nova.tipo,
            nova.estado,
            nova.canal_id,
            nova.responde_a,
            nova.pedido_id,
            nova.conteudo,
            nova.anexo
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
        "SELECT id, tipo, canal_id, responde_a, pedido_id, conteudo, tentativas, anexo
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
                anexo: l.get(7)?,
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
            "SELECT g.id, g.tipo, g.canal_id, g.responde_a, g.pedido_id, g.conteudo, g.tentativas,
                    g.anexo
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
                    anexo: l.get(7)?,
                })
            },
        )
        .optional()?;
    Ok(s)
}

/// Entradas destes tipos desde `desde` (para os tetos por minuto).
pub fn entradas_desde(banco: &Banco, tipos: &[&str], desde: i64) -> anyhow::Result<usize> {
    let marcas = (0..tipos.len())
        .map(|i| format!("?{}", i + 2))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT COUNT(*) FROM gateway_mensagens
          WHERE direcao = 'entrada' AND momento_ms >= ?1 AND tipo IN ({marcas})"
    );
    let mut valores: Vec<&dyn rusqlite::ToSql> = vec![&desde];
    valores.extend(tipos.iter().map(|t| t as &dyn rusqlite::ToSql));
    let n: i64 = banco
        .conexao()
        .query_row(&sql, valores.as_slice(), |l| l.get(0))?;
    Ok(n as usize)
}

/// Entradas destes tipos de UM autor desde `desde`.
pub fn entradas_do_autor_desde(
    banco: &Banco,
    autor: &str,
    tipos: &[&str],
    desde: i64,
) -> anyhow::Result<usize> {
    let marcas = (0..tipos.len())
        .map(|i| format!("?{}", i + 3))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT COUNT(*) FROM gateway_mensagens
          WHERE direcao = 'entrada' AND autor_id = ?1 AND momento_ms >= ?2 AND tipo IN ({marcas})"
    );
    let mut valores: Vec<&dyn rusqlite::ToSql> = vec![&autor, &desde];
    valores.extend(tipos.iter().map(|t| t as &dyn rusqlite::ToSql));
    let n: i64 = banco
        .conexao()
        .query_row(&sql, valores.as_slice(), |l| l.get(0))?;
    Ok(n as usize)
}

/// Saídas mandadas ao adaptador desde `desde` (teto por minuto).
pub fn entregas_desde(banco: &Banco, desde: i64) -> anyhow::Result<usize> {
    let n: i64 = banco.conexao().query_row(
        "SELECT COUNT(*) FROM gateway_mensagens WHERE direcao = 'saida' AND tentativa_ms >= ?1",
        params![desde],
        |l| l.get(0),
    )?;
    Ok(n as usize)
}

/// (último ID da DM do dono, [(canal, último ID)]).
pub type UltimosRecebidos = (Option<String>, Vec<(String, String)>);

/// Até onde o kernel já recebeu: o maior ID do Discord do dono fora dos
/// canais (a DM dele) e o de cada canal permitido. O adaptador busca o que
/// chegou depois disso enquanto ele estava fora do ar.
pub fn ultimos_recebidos(
    banco: &Banco,
    dono: &str,
    canais: &[&str],
) -> anyhow::Result<UltimosRecebidos> {
    let conexao = banco.conexao();
    let maximo = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> rusqlite::Result<Option<String>> {
        conexao
            .query_row(sql, p, |l| l.get::<_, Option<i64>>(0))
            .map(|v| v.map(|n| n.to_string()))
    };
    let mut dm = None;
    // DM do dono: mensagens dele cujo canal não é nenhum dos permitidos.
    let marcas = (0..canais.len())
        .map(|i| format!("?{}", i + 2))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT MAX(CAST(discord_id AS INTEGER)) FROM gateway_mensagens
          WHERE direcao = 'entrada' AND autor_id = ?1 {}",
        if canais.is_empty() {
            String::new()
        } else {
            format!("AND canal_id NOT IN ({marcas})")
        }
    );
    let mut valores: Vec<&dyn rusqlite::ToSql> = vec![&dono];
    valores.extend(canais.iter().map(|c| c as &dyn rusqlite::ToSql));
    if let Some(v) = maximo(&sql, &valores)? {
        dm = Some(v);
    }
    let mut por_canal = Vec::new();
    for canal in canais {
        if let Some(v) = maximo(
            "SELECT MAX(CAST(discord_id AS INTEGER)) FROM gateway_mensagens
              WHERE direcao = 'entrada' AND canal_id = ?1",
            &[canal],
        )? {
            por_canal.push((canal.to_string(), v));
        }
    }
    Ok((dm, por_canal))
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
            ultimos_recebidos(&banco, "7", &[]).unwrap(),
            (Some("101".into()), vec![])
        );
        assert_eq!(
            ultimos_recebidos(&banco, "7", &["10"]).unwrap(),
            (None, vec![("10".into(), "101".into())])
        );
    }

    #[test]
    fn fila_de_terceiros_por_autor_e_canal_um_turno_por_canal() {
        let banco = Banco::em_memoria().unwrap();
        let entrar = |id: &str, canal: &str, autor: &str| {
            let mut m = msg(id, id);
            m.canal_id = canal.into();
            m.autor_id = autor.into();
            let nova = NovaEntrada {
                mensagem: &m,
                tipo: "terceiro",
                estado: "pendente",
                conteudo: Some(&m.texto),
                origem_externa: None,
                pedido_id: None,
            };
            registrar_entrada(&banco, &nova, 1).unwrap();
        };
        entrar("1", "canal", "ana");
        entrar("2", "canal", "bia");
        entrar("3", "canal", "ana");
        entrar("4", "dm-caio", "caio");
        // A mais antiga manda: as da Ana no canal, juntas.
        let lote = tomar_lote_terceiro(&banco).unwrap();
        assert_eq!(
            lote.iter().map(|e| e.conteudo.as_str()).collect::<Vec<_>>(),
            ["1", "3"]
        );
        assert!(
            lote.iter()
                .all(|e| e.tipo == "terceiro" && e.autor_id == "ana")
        );
        // O canal está ocupado: a Bia espera; o Caio (outra conversa) não.
        let lote = tomar_lote_terceiro(&banco).unwrap();
        assert_eq!(lote[0].conteudo, "4");
        assert!(tomar_lote_terceiro(&banco).unwrap().is_empty());
        concluir_entradas(&banco, &[1, 3], "respondida", 2).unwrap();
        assert_eq!(tomar_lote_terceiro(&banco).unwrap()[0].autor_id, "bia");
        // O dono não sai daqui (nem os terceiros de lá).
        assert!(tomar_lote_do_dono(&banco).unwrap().is_empty());
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
            anexo: None,
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
