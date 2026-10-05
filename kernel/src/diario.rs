//! Diário: base da metacognição (por enquanto SÓ registro).
//!
//! Antes de cada ação autônoma, o kernel grava o que o Abiyss ESPERA que
//! aconteça (a "expectativa", declarada por ele na decisão). Depois, grava
//! o que REALMENTE aconteceu (o "resultado observado", calculado por código).
//! Comparar as duas coisas é o que, no futuro, vai permitir ao Abiyss
//! perceber os próprios erros de previsão. Escalonamento: fora do escopo.

use rusqlite::params;

use crate::db::Banco;
use crate::tempo::agora_ms;

/// Texto usado quando o modelo não declarou expectativa.
pub const SEM_EXPECTATIVA: &str = "(não declarada)";

#[derive(Debug, Clone, PartialEq)]
pub struct EntradaDiario {
    pub id: i64,
    pub momento_ms: i64,
    /// "heartbeat", "chat"...
    pub origem: String,
    pub goal_id: Option<i64>,
    pub subagente_id: Option<i64>,
    pub acao: String,
    pub expectativa: String,
    pub resultado: Option<String>,
    pub resultado_ms: Option<i64>,
}

/// Grava a expectativa ANTES da ação. Devolve o ID da entrada.
pub fn registrar_expectativa(
    banco: &Banco,
    origem: &str,
    goal_id: Option<i64>,
    acao: &str,
    expectativa: &str,
) -> anyhow::Result<i64> {
    let expectativa = if expectativa.trim().is_empty() {
        SEM_EXPECTATIVA
    } else {
        expectativa.trim()
    };
    let conexao = banco.conexao();
    conexao.execute(
        "INSERT INTO diario (momento_ms, origem, goal_id, acao, expectativa)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![agora_ms(), origem, goal_id, acao, expectativa],
    )?;
    Ok(conexao.last_insert_rowid())
}

/// Grava o resultado observado DEPOIS da ação.
pub fn registrar_resultado(banco: &Banco, id: i64, resultado: &str) -> anyhow::Result<()> {
    banco.conexao().execute(
        "UPDATE diario SET resultado = ?1, resultado_ms = ?2 WHERE id = ?3",
        params![resultado, agora_ms(), id],
    )?;
    Ok(())
}

/// Liga a entrada a um sub-agente (o resultado final virá depois).
pub fn vincular_subagente(banco: &Banco, id: i64, subagente_id: i64) -> anyhow::Result<()> {
    banco.conexao().execute(
        "UPDATE diario SET subagente_id = ?1 WHERE id = ?2",
        params![subagente_id, id],
    )?;
    Ok(())
}

/// Quando um sub-agente termina, acrescenta o relatório dele ao resultado
/// das entradas ligadas a ele.
pub fn anexar_resultado_de_subagente(
    banco: &Banco,
    subagente_id: i64,
    texto: &str,
) -> anyhow::Result<()> {
    banco.conexao().execute(
        "UPDATE diario
         SET resultado = COALESCE(resultado || char(10), '') || ?1, resultado_ms = ?2
         WHERE subagente_id = ?3",
        params![texto, agora_ms(), subagente_id],
    )?;
    Ok(())
}

/// Entradas mais recentes primeiro.
pub fn recentes(banco: &Banco, limite: usize) -> anyhow::Result<Vec<EntradaDiario>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT id, momento_ms, origem, goal_id, subagente_id, acao, expectativa, resultado, resultado_ms
         FROM diario ORDER BY id DESC LIMIT ?1",
    )?;
    let lista = consulta
        .query_map(params![limite as i64], |l| {
            Ok(EntradaDiario {
                id: l.get(0)?,
                momento_ms: l.get(1)?,
                origem: l.get(2)?,
                goal_id: l.get(3)?,
                subagente_id: l.get(4)?,
                acao: l.get(5)?,
                expectativa: l.get(6)?,
                resultado: l.get(7)?,
                resultado_ms: l.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lista)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn expectativa_antes_resultado_depois() {
        let banco = Banco::em_memoria().unwrap();
        let id =
            registrar_expectativa(&banco, "heartbeat", Some(1), "delegar resumo", "  ").unwrap();
        let antes = &recentes(&banco, 10).unwrap()[0];
        assert_eq!(antes.expectativa, SEM_EXPECTATIVA);
        assert_eq!(antes.resultado, None);

        registrar_resultado(&banco, id, "ok: sub-agente 3 delegado").unwrap();
        vincular_subagente(&banco, id, 3).unwrap();
        anexar_resultado_de_subagente(&banco, 3, "relatório: concluido").unwrap();
        let depois = &recentes(&banco, 10).unwrap()[0];
        assert_eq!(depois.subagente_id, Some(3));
        assert_eq!(
            depois.resultado.as_deref(),
            Some("ok: sub-agente 3 delegado\nrelatório: concluido")
        );
        assert!(depois.resultado_ms.is_some());
    }
}
