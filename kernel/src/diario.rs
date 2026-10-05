//! Diário: base da metacognição (por enquanto SÓ registro).
//!
//! Antes de cada ação autônoma, o kernel grava o que o Abiyss ESPERA que
//! aconteça (a "expectativa", declarada por ele na decisão). Depois, grava
//! o que REALMENTE aconteceu (o "resultado observado", calculado por código).
//! Comparar as duas coisas é o que, no futuro, vai permitir ao Abiyss
//! perceber os próprios erros de previsão. Escalonamento: fora do escopo.

use rusqlite::params;

use crate::db::Banco;
use crate::importacoes;
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
    /// Sinais observados antes de agir (vindos da importação do Hermes).
    pub sinais: Option<String>,
    /// Risco percebido (texto livre).
    pub risco: Option<String>,
    /// Confiança na expectativa, de 0.0 a 1.0.
    pub confianca: Option<f64>,
    /// Campos que o kernel não conhece, preservados em JSON.
    pub extras: Option<String>,
}

/// Uma entrada vinda de outro sistema (ex.: o journal do Hermes).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntradaImportada {
    pub momento_ms: i64,
    /// Ex.: "hermes".
    pub origem: String,
    pub goal_id: Option<i64>,
    pub acao: String,
    pub expectativa: String,
    pub sinais: Option<String>,
    pub risco: Option<String>,
    pub confianca: Option<f64>,
    pub resultado: Option<String>,
    pub resultado_ms: Option<i64>,
    pub extras: Option<String>,
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

/// Grava uma entrada importada e a chave dela, na mesma transação.
/// Devolve `None` se a chave já tinha sido importada (nada é gravado).
pub fn importar(
    banco: &Banco,
    entrada: &EntradaImportada,
    chave: &str,
    original: &str,
) -> anyhow::Result<Option<i64>> {
    if importacoes::ja_importado(banco, chave)? {
        return Ok(None);
    }
    let expectativa = if entrada.expectativa.trim().is_empty() {
        SEM_EXPECTATIVA
    } else {
        entrada.expectativa.trim()
    };
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    transacao.execute(
        "INSERT INTO diario (momento_ms, origem, goal_id, acao, expectativa, resultado,
                             resultado_ms, sinais, risco, confianca, extras)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            entrada.momento_ms,
            entrada.origem,
            entrada.goal_id,
            entrada.acao,
            expectativa,
            entrada.resultado,
            entrada.resultado_ms,
            entrada.sinais,
            entrada.risco,
            entrada.confianca,
            entrada.extras
        ],
    )?;
    let id = transacao.last_insert_rowid();
    importacoes::registrar_em(&transacao, chave, "diario", &id.to_string(), original)?;
    transacao.commit()?;
    Ok(Some(id))
}

/// Quantas entradas há no diário.
pub fn contar(banco: &Banco) -> anyhow::Result<i64> {
    let n = banco
        .conexao()
        .query_row("SELECT COUNT(*) FROM diario", [], |l| l.get(0))?;
    Ok(n)
}

/// Entradas mais recentes primeiro.
pub fn recentes(banco: &Banco, limite: usize) -> anyhow::Result<Vec<EntradaDiario>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT id, momento_ms, origem, goal_id, subagente_id, acao, expectativa, resultado,
                resultado_ms, sinais, risco, confianca, extras
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
                sinais: l.get(9)?,
                risco: l.get(10)?,
                confianca: l.get(11)?,
                extras: l.get(12)?,
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

    #[test]
    fn importa_uma_vez_so_com_campos_novos() {
        let banco = Banco::em_memoria().unwrap();
        let entrada = EntradaImportada {
            momento_ms: 1_000,
            origem: "hermes".into(),
            acao: "ler docs".into(),
            sinais: Some("README; MCP".into()),
            risco: Some("baixo".into()),
            confianca: Some(0.8),
            extras: Some("{\"mood\":\"focado\"}".into()),
            ..Default::default()
        };
        assert!(
            importar(&banco, &entrada, "hermes:j:1", "{}")
                .unwrap()
                .is_some()
        );
        assert!(
            importar(&banco, &entrada, "hermes:j:1", "{}")
                .unwrap()
                .is_none()
        );
        assert_eq!(contar(&banco).unwrap(), 1);
        let lida = &recentes(&banco, 1).unwrap()[0];
        assert_eq!(lida.expectativa, SEM_EXPECTATIVA);
        assert_eq!(lida.confianca, Some(0.8));
        assert_eq!(lida.risco.as_deref(), Some("baixo"));
        assert!(lida.extras.as_deref().unwrap().contains("focado"));
    }
}
