//! Persistência em SQLite.
//!
//! Um único arquivo (`data/abiyss.db`) guarda tudo: baldes do rate limit,
//! histórico de conversa, goals, eventos, sub-agentes, diário...
//! Vários processos (chat e daemon) podem abrir o mesmo arquivo ao mesmo
//! tempo; o modo WAL e o `busy_timeout` cuidam disso.
//!
//! O esquema evolui por MIGRAÇÕES numeradas: cada fase acrescenta um
//! item no fim de `MIGRACOES`. A versão aplicada fica em `PRAGMA user_version`.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::Context;
use rusqlite::Connection;

/// Lista de migrações. NUNCA edite uma migração já publicada:
/// acrescente uma nova no fim.
const MIGRACOES: &[&str] = &[
    // 1 (F2) — rate limit compartilhado entre processos + registro de chamadas.
    r#"
    CREATE TABLE baldes (
        nome             TEXT PRIMARY KEY,
        tokens           REAL    NOT NULL,
        atualizado_ms    INTEGER NOT NULL,
        bloqueado_ate_ms INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE chamadas_modelo (
        id             INTEGER PRIMARY KEY,
        momento_ms     INTEGER NOT NULL,
        pool           TEXT    NOT NULL,
        origem         TEXT    NOT NULL,
        modelo         TEXT    NOT NULL,
        tentativa      INTEGER NOT NULL,
        status         TEXT    NOT NULL,
        http_status    INTEGER,
        tokens_entrada INTEGER NOT NULL DEFAULT 0,
        tokens_saida   INTEGER NOT NULL DEFAULT 0,
        duracao_ms     INTEGER NOT NULL
    );
    CREATE INDEX idx_chamadas_momento ON chamadas_modelo(momento_ms);
    "#,
    // 2 (F3) — histórico de conversa.
    r#"
    CREATE TABLE conversas (
        id        INTEGER PRIMARY KEY,
        criada_ms INTEGER NOT NULL
    );
    CREATE TABLE mensagens (
        id              INTEGER PRIMARY KEY,
        conversa_id     INTEGER NOT NULL REFERENCES conversas(id),
        momento_ms      INTEGER NOT NULL,
        papel           TEXT    NOT NULL,
        conteudo        TEXT,
        raciocinio      TEXT,
        chamadas_json   TEXT,
        id_chamada      TEXT,
        nome_ferramenta TEXT
    );
    CREATE INDEX idx_mensagens_conversa ON mensagens(conversa_id, id);
    "#,
];

/// Acesso ao banco. `Clone` é barato: todos os clones usam a mesma conexão.
///
/// A conexão fica atrás de um `Mutex` comum (não o do tokio). Isso é de
/// propósito: as consultas são rápidas e o compilador PROÍBE segurar a
/// trava através de um `.await` dentro de tarefas do tokio, o que evita
/// travamentos difíceis de achar.
#[derive(Clone)]
pub struct Banco {
    conexao: Arc<Mutex<Connection>>,
}

impl Banco {
    /// Abre (ou cria) o banco no caminho indicado e aplica as migrações.
    pub fn abrir(caminho: &Path) -> anyhow::Result<Banco> {
        if let Some(pasta) = caminho.parent() {
            std::fs::create_dir_all(pasta)
                .with_context(|| format!("não consegui criar {}", pasta.display()))?;
        }
        let conexao = Connection::open(caminho)
            .with_context(|| format!("não consegui abrir {}", caminho.display()))?;
        // WAL permite um processo escrever enquanto outro lê.
        conexao.pragma_update(None, "journal_mode", "WAL")?;
        Banco::preparar(conexao)
    }

    /// Banco temporário em memória (para testes).
    pub fn em_memoria() -> anyhow::Result<Banco> {
        Banco::preparar(Connection::open_in_memory()?)
    }

    fn preparar(conexao: Connection) -> anyhow::Result<Banco> {
        // Se outro processo estiver escrevendo, espera até 5 s em vez de falhar.
        conexao.busy_timeout(Duration::from_secs(5))?;
        conexao.pragma_update(None, "foreign_keys", "ON")?;
        let banco = Banco {
            conexao: Arc::new(Mutex::new(conexao)),
        };
        banco.migrar()?;
        Ok(banco)
    }

    /// Pega a conexão para fazer consultas. Solte a trava (fim do escopo)
    /// antes de qualquer `.await`.
    pub fn conexao(&self) -> MutexGuard<'_, Connection> {
        // Se outra thread entrou em pânico segurando a trava, seguimos mesmo
        // assim: a conexão do SQLite continua utilizável.
        self.conexao.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn migrar(&self) -> anyhow::Result<()> {
        let mut conexao = self.conexao();
        // (O rusqlite trabalha com i64 para inteiros do SQLite.)
        let versao: i64 = conexao.query_row("PRAGMA user_version", [], |linha| linha.get(0))?;
        for (indice, sql) in MIGRACOES.iter().enumerate().skip(versao as usize) {
            let numero = (indice + 1) as i64;
            let transacao = conexao.transaction()?;
            transacao
                .execute_batch(sql)
                .with_context(|| format!("falha na migração {numero}"))?;
            transacao.pragma_update(None, "user_version", numero)?;
            transacao.commit()?;
            tracing::debug!("migração {numero} aplicada");
        }
        Ok(())
    }

    /// Versão atual do esquema (número de migrações aplicadas).
    pub fn versao(&self) -> anyhow::Result<usize> {
        let versao: i64 = self
            .conexao()
            .query_row("PRAGMA user_version", [], |linha| linha.get(0))?;
        Ok(versao as usize)
    }
}

/// Quantas migrações existem (para testes e para o `abiyss status`).
pub fn total_migracoes() -> usize {
    MIGRACOES.len()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn migracoes_aplicam_e_sao_idempotentes() {
        let pasta = tempfile::tempdir().unwrap();
        let caminho = pasta.path().join("sub/abiyss.db");
        let banco = Banco::abrir(&caminho).unwrap();
        assert_eq!(banco.versao().unwrap(), total_migracoes());
        drop(banco);
        // Abrir de novo não reaplica nada.
        let banco = Banco::abrir(&caminho).unwrap();
        assert_eq!(banco.versao().unwrap(), total_migracoes());
    }
}
