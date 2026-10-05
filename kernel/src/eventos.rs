//! Fila de eventos do Abiyss.
//!
//! Tudo que acontece "fora" do ciclo de pensamento chega aqui: disparos de
//! cron, resultados de sub-agentes, etc. O heartbeat lê os eventos
//! pendentes, coloca no contexto (rotulados como dado) e marca como
//! consumidos depois de uma chamada bem-sucedida ao modelo.
//!
//! Fica no SQLite para funcionar entre processos (ex.: `abiyss chat`
//! delega um sub-agente e o daemon recebe o resultado).

use rusqlite::params;

use crate::db::Banco;
use crate::tempo::agora_ms;

/// Tipos de evento conhecidos.
pub const TIPO_CRON: &str = "cron";
pub const TIPO_SUBAGENTE: &str = "subagente";

#[derive(Debug, Clone, PartialEq)]
pub struct Evento {
    pub id: i64,
    pub momento_ms: i64,
    pub tipo: String,
    /// De onde veio (nome do cron, id do sub-agente...).
    pub origem: String,
    /// Conteúdo livre (texto ou JSON).
    pub conteudo: String,
}

/// Coloca um evento na fila e devolve o ID.
pub fn publicar(banco: &Banco, tipo: &str, origem: &str, conteudo: &str) -> anyhow::Result<i64> {
    let conexao = banco.conexao();
    conexao.execute(
        "INSERT INTO fila_eventos (momento_ms, tipo, origem, conteudo) VALUES (?1, ?2, ?3, ?4)",
        params![agora_ms(), tipo, origem, conteudo],
    )?;
    Ok(conexao.last_insert_rowid())
}

/// Eventos ainda não consumidos, do mais antigo ao mais novo.
pub fn pendentes(banco: &Banco, limite: usize) -> anyhow::Result<Vec<Evento>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT id, momento_ms, tipo, origem, conteudo FROM fila_eventos
         WHERE consumido_ms IS NULL ORDER BY id ASC LIMIT ?1",
    )?;
    let lista = consulta
        .query_map(params![limite as i64], |l| {
            Ok(Evento {
                id: l.get(0)?,
                momento_ms: l.get(1)?,
                tipo: l.get(2)?,
                origem: l.get(3)?,
                conteudo: l.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lista)
}

pub fn contar_pendentes(banco: &Banco) -> anyhow::Result<i64> {
    let n = banco.conexao().query_row(
        "SELECT COUNT(*) FROM fila_eventos WHERE consumido_ms IS NULL",
        [],
        |l| l.get(0),
    )?;
    Ok(n)
}

pub fn marcar_consumidos(banco: &Banco, ids: &[i64]) -> anyhow::Result<()> {
    let agora = agora_ms();
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    for id in ids {
        transacao.execute(
            "UPDATE fila_eventos SET consumido_ms = ?1 WHERE id = ?2",
            params![agora, id],
        )?;
    }
    transacao.commit()?;
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn publica_le_e_consome_em_ordem() {
        let banco = Banco::em_memoria().unwrap();
        let a = publicar(&banco, TIPO_CRON, "bom-dia", "acorde").unwrap();
        let b = publicar(&banco, TIPO_SUBAGENTE, "7", "{}").unwrap();
        let lista = pendentes(&banco, 10).unwrap();
        assert_eq!(lista.iter().map(|e| e.id).collect::<Vec<_>>(), vec![a, b]);
        assert_eq!(pendentes(&banco, 1).unwrap().len(), 1);
        marcar_consumidos(&banco, &[a]).unwrap();
        assert_eq!(contar_pendentes(&banco).unwrap(), 1);
        assert_eq!(pendentes(&banco, 10).unwrap()[0].id, b);
    }
}
