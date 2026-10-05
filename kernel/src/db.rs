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
    // 3 (F6) — goals, fila de eventos, crons, ciclos do heartbeat e estado do daemon.
    r#"
    CREATE TABLE goals (
        id            INTEGER PRIMARY KEY,
        titulo        TEXT    NOT NULL,
        nucleo        TEXT    NOT NULL,
        descricao     TEXT    NOT NULL DEFAULT '',
        prioridade    INTEGER NOT NULL DEFAULT 0,
        estado        TEXT    NOT NULL,
        criado_ms     INTEGER NOT NULL,
        atualizado_ms INTEGER NOT NULL
    );
    CREATE TABLE eventos_goal (
        id         INTEGER PRIMARY KEY,
        goal_id    INTEGER NOT NULL REFERENCES goals(id),
        momento_ms INTEGER NOT NULL,
        de         TEXT,
        para       TEXT    NOT NULL,
        motivo     TEXT    NOT NULL,
        autor      TEXT    NOT NULL
    );
    CREATE INDEX idx_eventos_goal ON eventos_goal(goal_id, id);
    CREATE TABLE fila_eventos (
        id           INTEGER PRIMARY KEY,
        momento_ms   INTEGER NOT NULL,
        tipo         TEXT    NOT NULL,
        origem       TEXT    NOT NULL,
        conteudo     TEXT    NOT NULL,
        consumido_ms INTEGER
    );
    CREATE INDEX idx_fila_pendentes ON fila_eventos(consumido_ms, id);
    CREATE TABLE crons (
        id         INTEGER PRIMARY KEY,
        nome       TEXT    NOT NULL UNIQUE,
        expressao  TEXT    NOT NULL,
        mensagem   TEXT    NOT NULL,
        ativo      INTEGER NOT NULL DEFAULT 1,
        proximo_ms INTEGER NOT NULL,
        ultimo_ms  INTEGER,
        criado_ms  INTEGER NOT NULL
    );
    CREATE TABLE ciclos (
        id             INTEGER PRIMARY KEY,
        inicio_ms      INTEGER NOT NULL,
        fim_ms         INTEGER,
        chamou_modelo  INTEGER NOT NULL,
        motivo         TEXT    NOT NULL,
        goal_foco      INTEGER,
        resposta       TEXT,
        resultado      TEXT,
        erro           TEXT,
        tokens         INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE estado_daemon (
        chave TEXT PRIMARY KEY,
        valor TEXT NOT NULL
    );
    "#,
    // 4 (F7) — sub-agentes assíncronos.
    r#"
    CREATE TABLE subagentes (
        id             INTEGER PRIMARY KEY,
        nivel          TEXT    NOT NULL,
        tarefa         TEXT    NOT NULL,
        contexto       TEXT    NOT NULL DEFAULT '',
        prazo_segundos INTEGER NOT NULL,
        goal_id        INTEGER,
        origem         TEXT    NOT NULL,
        estado         TEXT    NOT NULL,
        criado_ms      INTEGER NOT NULL,
        iniciado_ms    INTEGER,
        terminado_ms   INTEGER,
        cancelar       INTEGER NOT NULL DEFAULT 0,
        relatorio      TEXT,
        tokens         INTEGER NOT NULL DEFAULT 0,
        rodadas        INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_subagentes_estado ON subagentes(estado, id);
    "#,
    // 5 (F8) — diário da metacognição (expectativa antes, resultado depois).
    r#"
    CREATE TABLE diario (
        id           INTEGER PRIMARY KEY,
        momento_ms   INTEGER NOT NULL,
        origem       TEXT    NOT NULL,
        goal_id      INTEGER,
        subagente_id INTEGER,
        acao         TEXT    NOT NULL,
        expectativa  TEXT    NOT NULL,
        resultado    TEXT,
        resultado_ms INTEGER
    );
    CREATE INDEX idx_diario_subagente ON diario(subagente_id);
    "#,
    // 6 (A2) — memória: fila de propostas, registro de operações e a origem
    // de cada mensagem (para a regra "conteúdo externo nunca entra direto
    // em 01_internal"). Resultados de ferramenta gravados antes desta
    // migração são marcados como externos, por segurança.
    r#"
    ALTER TABLE mensagens ADD COLUMN origem_externa TEXT;
    UPDATE mensagens SET origem_externa = 'ferramenta (anterior ao rastreio de origem)'
        WHERE papel = 'tool';
    CREATE TABLE propostas_memoria (
        id             INTEGER PRIMARY KEY,
        criado_ms      INTEGER NOT NULL,
        escopo         TEXT    NOT NULL,
        caminho        TEXT    NOT NULL,
        conteudo       TEXT    NOT NULL,
        tipo           TEXT    NOT NULL,
        fonte          TEXT    NOT NULL,
        origem_externa TEXT,
        estado         TEXT    NOT NULL DEFAULT 'pendente',
        motivo         TEXT,
        decidido_ms    INTEGER
    );
    CREATE INDEX idx_propostas_estado ON propostas_memoria(estado, id);
    CREATE TABLE registro_memoria (
        id         INTEGER PRIMARY KEY,
        momento_ms INTEGER NOT NULL,
        acao       TEXT    NOT NULL,
        caminho    TEXT    NOT NULL,
        detalhe    TEXT    NOT NULL DEFAULT ''
    );
    CREATE INDEX idx_registro_caminho ON registro_memoria(caminho, id);
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
