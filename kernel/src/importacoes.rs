//! Registro de itens importados de outros sistemas (tabela `importacoes`).
//!
//! Cada item importado (uma linha do diário, um goal, uma nota...) ganha
//! uma CHAVE estável, ex.: "hermes:goal:g-001". Antes de importar, o
//! importador confere a chave; depois, grava junto com o item, na mesma
//! transação. Assim, rodar a importação duas vezes não duplica nada.

use rusqlite::{OptionalExtension, Transaction, params};

use crate::db::Banco;
use crate::tempo::agora_ms;

/// Para onde um item já importado foi (`None` = nunca importado).
pub fn destino(banco: &Banco, chave: &str) -> anyhow::Result<Option<String>> {
    let destino = banco
        .conexao()
        .query_row(
            "SELECT destino FROM importacoes WHERE chave = ?1",
            params![chave],
            |l| l.get(0),
        )
        .optional()?;
    Ok(destino)
}

pub fn ja_importado(banco: &Banco, chave: &str) -> anyhow::Result<bool> {
    Ok(destino(banco, chave)?.is_some())
}

/// Registra um item importado dentro de uma transação já aberta.
/// `original` guarda o registro de origem completo (quando faz sentido).
pub fn registrar_em(
    transacao: &Transaction<'_>,
    chave: &str,
    tipo: &str,
    destino: &str,
    original: &str,
) -> anyhow::Result<()> {
    transacao.execute(
        "INSERT INTO importacoes (chave, tipo, destino, original, momento_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![chave, tipo, destino, original, agora_ms()],
    )?;
    Ok(())
}

/// Registra um item importado (fora de transação: notas, memória central).
pub fn registrar(
    banco: &Banco,
    chave: &str,
    tipo: &str,
    destino: &str,
    original: &str,
) -> anyhow::Result<()> {
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    registrar_em(&transacao, chave, tipo, destino, original)?;
    transacao.commit()?;
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn chave_registrada_uma_vez_so() {
        let banco = Banco::em_memoria().unwrap();
        assert!(!ja_importado(&banco, "hermes:x").unwrap());
        registrar(&banco, "hermes:x", "teste", "1", "{}").unwrap();
        assert_eq!(destino(&banco, "hermes:x").unwrap().as_deref(), Some("1"));
        // A chave é única: registrar de novo é erro (o importador confere antes).
        assert!(registrar(&banco, "hermes:x", "teste", "2", "").is_err());
    }
}
