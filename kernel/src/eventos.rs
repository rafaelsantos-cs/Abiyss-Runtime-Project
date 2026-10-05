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
/// Texto de uma skill pedido pelo heartbeat (`consultar_skill`).
pub const TIPO_SKILL: &str = "skill";
/// Avisos do próprio kernel (estagnação, reinício...).
pub const TIPO_KERNEL: &str = "kernel";
/// Resumo do sono, publicado ao acordar.
pub const TIPO_SONO: &str = "sono";
/// Resposta do usuário a um pedido do Abiyss.
pub const TIPO_USUARIO: &str = "usuario";

#[derive(Debug, Clone, PartialEq)]
pub struct Evento {
    pub id: i64,
    pub momento_ms: i64,
    pub tipo: String,
    /// De onde veio (nome do cron, id do sub-agente...).
    pub origem: String,
    /// Conteúdo livre (texto ou JSON).
    pub conteudo: String,
    /// Calculado pelo kernel ao publicar: `Some(rótulo)` se o conteúdo veio
    /// de fora (relatório de sub-agente, skill não confiável...). Um ciclo
    /// que consome evento externo fica marcado como externo (ver o sono).
    pub origem_externa: Option<String>,
}

impl Evento {
    pub fn eh_externo(&self) -> bool {
        self.origem_externa.is_some()
    }
}

/// Coloca na fila um evento de conteúdo interno (cron, kernel...) e
/// devolve o ID.
pub fn publicar(banco: &Banco, tipo: &str, origem: &str, conteudo: &str) -> anyhow::Result<i64> {
    publicar_com_origem(banco, tipo, origem, conteudo, None)
}

/// Coloca um evento na fila, dizendo se o conteúdo é externo.
pub fn publicar_com_origem(
    banco: &Banco,
    tipo: &str,
    origem: &str,
    conteudo: &str,
    origem_externa: Option<&str>,
) -> anyhow::Result<i64> {
    let conexao = banco.conexao();
    conexao.execute(
        "INSERT INTO fila_eventos (momento_ms, tipo, origem, conteudo, origem_externa)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![agora_ms(), tipo, origem, conteudo, origem_externa],
    )?;
    Ok(conexao.last_insert_rowid())
}

/// Eventos ainda não consumidos, do mais antigo ao mais novo.
pub fn pendentes(banco: &Banco, limite: usize) -> anyhow::Result<Vec<Evento>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT id, momento_ms, tipo, origem, conteudo, origem_externa FROM fila_eventos
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
                origem_externa: l.get(5)?,
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
        let b =
            publicar_com_origem(&banco, TIPO_SUBAGENTE, "7", "{}", Some("subagente:7")).unwrap();
        let lista = pendentes(&banco, 10).unwrap();
        assert_eq!(lista.iter().map(|e| e.id).collect::<Vec<_>>(), vec![a, b]);
        assert_eq!(pendentes(&banco, 1).unwrap().len(), 1);
        marcar_consumidos(&banco, &[a]).unwrap();
        assert_eq!(contar_pendentes(&banco).unwrap(), 1);
        let restante = &pendentes(&banco, 10).unwrap()[0];
        assert_eq!(restante.id, b);
        assert!(restante.eh_externo());
        assert!(!lista[0].eh_externo());
    }
}
