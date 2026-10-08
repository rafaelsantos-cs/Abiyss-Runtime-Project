//! Persistência em SQLite.
//!
//! Um único arquivo (`data/abiyss.db`) guarda tudo: baldes do rate limit,
//! histórico de conversa, goals, eventos, sub-agentes, diário...
//! Vários processos (chat e daemon) podem abrir o mesmo arquivo ao mesmo
//! tempo; o modo WAL e o `busy_timeout` cuidam disso.
//!
//! O esquema evolui por MIGRAÇÕES numeradas: cada fase acrescenta um
//! item no fim de `MIGRACOES`. A versão aplicada fica em `PRAGMA user_version`.

use std::path::{Path, PathBuf};
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
    // 7 (A5) — importação do Hermes: o diário ganha sinais, risco e
    // confiança; diário e goals ganham `extras` (campos desconhecidos
    // preservados, em JSON); `importacoes` garante que importar duas vezes
    // não duplica nada (uma chave por item importado).
    r#"
    ALTER TABLE diario ADD COLUMN sinais TEXT;
    ALTER TABLE diario ADD COLUMN risco TEXT;
    ALTER TABLE diario ADD COLUMN confianca REAL;
    ALTER TABLE diario ADD COLUMN extras TEXT;
    ALTER TABLE goals ADD COLUMN extras TEXT;
    CREATE TABLE importacoes (
        chave      TEXT PRIMARY KEY,
        tipo       TEXT    NOT NULL,
        destino    TEXT    NOT NULL,
        original   TEXT    NOT NULL DEFAULT '',
        momento_ms INTEGER NOT NULL
    );
    "#,
    // 8 (infra v0.2) — latência das chamadas ao modelo: tempo até o primeiro
    // token (NULL quando a chamada falhou antes de qualquer token) e se a
    // chamada foi por streaming. A duração total já existia (`duracao_ms`).
    r#"
    ALTER TABLE chamadas_modelo ADD COLUMN primeiro_token_ms INTEGER;
    ALTER TABLE chamadas_modelo ADD COLUMN stream INTEGER NOT NULL DEFAULT 0;
    CREATE INDEX idx_chamadas_modelo_momento ON chamadas_modelo(modelo, momento_ms);
    "#,
    // 9 (infra v0.2) — retenção: o detalhe antigo de chamadas, eventos
    // consumidos e ciclos vira agregado diário (dia UTC, "AAAA-MM-DD").
    // Só somas, contagens e máximos: dá para somar dois lotes do mesmo dia.
    r#"
    CREATE TABLE chamadas_modelo_diarias (
        dia                     TEXT    NOT NULL,
        pool                    TEXT    NOT NULL,
        modelo                  TEXT    NOT NULL,
        status                  TEXT    NOT NULL,
        chamadas                INTEGER NOT NULL,
        tokens_entrada          INTEGER NOT NULL,
        tokens_saida            INTEGER NOT NULL,
        duracao_soma_ms         INTEGER NOT NULL,
        duracao_max_ms          INTEGER NOT NULL,
        primeiro_token_soma_ms  INTEGER NOT NULL,
        primeiro_token_amostras INTEGER NOT NULL,
        PRIMARY KEY (dia, pool, modelo, status)
    );
    CREATE TABLE eventos_diarios (
        dia     TEXT    NOT NULL,
        tipo    TEXT    NOT NULL,
        origem  TEXT    NOT NULL,
        eventos INTEGER NOT NULL,
        PRIMARY KEY (dia, tipo, origem)
    );
    CREATE TABLE ciclos_diarios (
        dia             TEXT    PRIMARY KEY,
        ciclos          INTEGER NOT NULL,
        com_modelo      INTEGER NOT NULL,
        com_erro        INTEGER NOT NULL,
        tokens          INTEGER NOT NULL,
        duracao_soma_ms INTEGER NOT NULL
    );
    CREATE INDEX idx_fila_momento ON fila_eventos(momento_ms);
    CREATE INDEX idx_ciclos_inicio ON ciclos(inicio_ms);
    "#,
    // 10 (E2) — origem de cada evento da fila, calculada pelo kernel ao
    // publicar: relatórios de sub-agente e skills não confiáveis são
    // conteúdo EXTERNO. Eventos de sub-agente gravados antes desta migração
    // são marcados como externos, por segurança.
    r#"
    ALTER TABLE fila_eventos ADD COLUMN origem_externa TEXT;
    UPDATE fila_eventos SET origem_externa = 'subagente:' || origem
        WHERE tipo = 'subagente';
    "#,
    // 11 (E5) — sono de verdade. `ciclos.origem_externa`: o ciclo consumiu
    // evento externo (o sono não usa o que ele decidiu como material
    // interno). `propostas_memoria.evidencias`: IDs citáveis (m:12, c:40...)
    // que sustentam uma proposta do sono. `sonos`: uma linha por sono, com
    // fase e resultado. `marcas_sono`: até onde cada fonte já foi revisada,
    // por passada (interno/externo) — rodar de novo nunca repete material.
    r#"
    ALTER TABLE ciclos ADD COLUMN origem_externa TEXT;
    ALTER TABLE propostas_memoria ADD COLUMN evidencias TEXT;
    CREATE TABLE sonos (
        id        INTEGER PRIMARY KEY,
        dia       TEXT    NOT NULL,
        gatilho   TEXT    NOT NULL,
        inicio_ms INTEGER NOT NULL,
        fim_ms    INTEGER,
        estado    TEXT    NOT NULL,
        fase      TEXT    NOT NULL,
        chamadas  INTEGER NOT NULL DEFAULT 0,
        tokens    INTEGER NOT NULL DEFAULT 0,
        resumo    TEXT,
        erro      TEXT
    );
    CREATE INDEX idx_sonos_dia ON sonos(dia, id);
    CREATE TABLE marcas_sono (
        chave  TEXT    PRIMARY KEY,
        ate_id INTEGER NOT NULL
    );
    "#,
    // 12 (E7) — impressão digital de cada ciclo que chamou o modelo (goal
    // em foco + ações normalizadas), para detectar estagnação.
    r#"
    ALTER TABLE ciclos ADD COLUMN impressao TEXT;
    "#,
    // 13 (E9) — pedidos ao usuário (caixa de entrada assíncrona). A mesma
    // pergunta pendente não duplica (`chave` normalizada, única entre os
    // pendentes).
    r#"
    CREATE TABLE pedidos_usuario (
        id               INTEGER PRIMARY KEY,
        criado_ms        INTEGER NOT NULL,
        origem           TEXT    NOT NULL,
        goal_id          INTEGER,
        pergunta         TEXT    NOT NULL,
        contexto         TEXT    NOT NULL DEFAULT '',
        urgencia         TEXT    NOT NULL,
        estado           TEXT    NOT NULL,
        resposta         TEXT,
        respondido_ms    INTEGER,
        origem_externa   TEXT,
        resposta_externa TEXT,
        chave            TEXT    NOT NULL
    );
    CREATE INDEX idx_pedidos_estado ON pedidos_usuario(estado, id);
    CREATE UNIQUE INDEX idx_pedidos_chave_pendente
        ON pedidos_usuario(chave) WHERE estado = 'pendente';
    "#,
    // 14 (v0.2, ajustes) — tabela de esforço ligada ao runtime. Em cada
    // chamada: o nível resolvido (NULL = sem tabela, ex.: `testar-nim`, ou
    // chamada anterior a esta migração) e se os campos dele foram enviados
    // (0 = nível a confirmar ou fora da tabela: foram os parâmetros padrão
    // do modelo). Em cada sub-agente: o nível que ele roda (já limitado ao
    // teto do nível do sub-agente; NULL = anterior a esta migração).
    r#"
    ALTER TABLE chamadas_modelo ADD COLUMN nivel_esforco TEXT;
    ALTER TABLE chamadas_modelo ADD COLUMN esforco_confirmado INTEGER;
    ALTER TABLE subagentes ADD COLUMN esforco TEXT;
    "#,
    // 15 (v0.2, gateway) — registro das mensagens do gateway (Discord): uma
    // linha por mensagem lógica, nos dois sentidos. É também a fila: entrada
    // `pendente` espera a vez da conversa; saída `pendente` espera o
    // adaptador confirmar a entrega. `gateway_ids_discord` liga cada
    // mensagem do Discord (uma resposta longa vira várias) à linha lógica:
    // é por ela que um "responder" no Discord acha o pedido certo. A
    // retenção agrega o detalhe velho em `gateway_mensagens_diarias`.
    r#"
    CREATE TABLE gateway_mensagens (
        id             INTEGER PRIMARY KEY,
        momento_ms     INTEGER NOT NULL,
        direcao        TEXT    NOT NULL,
        tipo           TEXT    NOT NULL,
        estado         TEXT    NOT NULL,
        canal_id       TEXT,
        autor_id       TEXT,
        discord_id     TEXT,
        responde_a     TEXT,
        pedido_id      INTEGER,
        conteudo       TEXT,
        origem_externa TEXT,
        tentativas     INTEGER NOT NULL DEFAULT 0,
        tentativa_ms   INTEGER,
        concluido_ms   INTEGER
    );
    CREATE INDEX idx_gateway_fila ON gateway_mensagens(direcao, estado, id);
    CREATE INDEX idx_gateway_momento ON gateway_mensagens(momento_ms);
    CREATE INDEX idx_gateway_pedido ON gateway_mensagens(pedido_id);
    CREATE UNIQUE INDEX idx_gateway_entrada_discord
        ON gateway_mensagens(discord_id) WHERE direcao = 'entrada';
    CREATE TABLE gateway_ids_discord (
        discord_id  TEXT    PRIMARY KEY,
        mensagem_id INTEGER NOT NULL REFERENCES gateway_mensagens(id) ON DELETE CASCADE
    );
    CREATE INDEX idx_gateway_ids_mensagem ON gateway_ids_discord(mensagem_id);
    CREATE TABLE gateway_mensagens_diarias (
        dia       TEXT    NOT NULL,
        direcao   TEXT    NOT NULL,
        tipo      TEXT    NOT NULL,
        estado    TEXT    NOT NULL,
        mensagens INTEGER NOT NULL,
        PRIMARY KEY (dia, direcao, tipo, estado)
    );
    "#,
    // 16 (v0.2, gateway) — anexo de uma saída: caminho REAL de um arquivo
    // dentro do workspace (o kernel confere ao pôr na fila; o adaptador
    // confere de novo ao mandar). Nada de fora do workspace sai.
    r#"
    ALTER TABLE gateway_mensagens ADD COLUMN anexo TEXT;
    "#,
    // 17 (v0.2, gateway em níveis) — chamadas ao modelo gastas para
    // responder a uma entrada (gravadas na última do lote): o orçamento
    // diário por pessoa de quem não é o dono soma esta coluna.
    r#"
    ALTER TABLE gateway_mensagens ADD COLUMN chamadas INTEGER;
    "#,
];

/// Cache de páginas do SQLite por conexão, em KiB (o padrão do SQLite é
/// ~2 MB, implícito). O daemon usa `[banco] cache_kib` do abiyss.toml.
pub const CACHE_PADRAO_KIB: u32 = 2048;

/// Teto do arquivo WAL depois de cada checkpoint (o SQLite trunca o que
/// passar disto). Sem teto, o WAL fica do tamanho do maior pico.
const LIMITE_WAL_BYTES: i64 = 64 * 1024 * 1024;

/// Quanto um comando espera outro processo (ou conexão) soltar o banco
/// antes de falhar (`busy_timeout`).
pub const ESPERA_PADRAO: Duration = Duration::from_secs(5);

/// Acesso ao banco. `Clone` é barato: todos os clones usam a mesma conexão.
///
/// A conexão fica atrás de um `Mutex` comum (não o do tokio). Isso é de
/// propósito: as consultas são rápidas e o compilador PROÍBE segurar a
/// trava através de um `.await` dentro de tarefas do tokio, o que evita
/// travamentos difíceis de achar.
#[derive(Clone)]
pub struct Banco {
    conexao: Arc<Mutex<Connection>>,
    /// Arquivo do banco (`None` em memória). Guardado ao abrir: pedir à
    /// conexão exigiria a trava dela.
    caminho: Option<PathBuf>,
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
        ligar_vacuum_incremental_se_novo(&conexao)?;
        // WAL permite um processo escrever enquanto outro lê.
        conexao.pragma_update(None, "journal_mode", "WAL")?;
        conexao.pragma_update(None, "journal_size_limit", LIMITE_WAL_BYTES)?;
        Banco::preparar(conexao, Some(caminho.to_path_buf()))
    }

    /// Banco temporário em memória (para testes).
    pub fn em_memoria() -> anyhow::Result<Banco> {
        Banco::preparar(Connection::open_in_memory()?, None)
    }

    fn preparar(conexao: Connection, caminho: Option<PathBuf>) -> anyhow::Result<Banco> {
        // Se outro processo estiver escrevendo, espera até 5 s em vez de falhar.
        conexao.busy_timeout(ESPERA_PADRAO)?;
        conexao.pragma_update(None, "foreign_keys", "ON")?;
        conexao.pragma_update(None, "cache_size", -(CACHE_PADRAO_KIB as i64))?;
        let banco = Banco {
            conexao: Arc::new(Mutex::new(conexao)),
            caminho,
        };
        banco.migrar()?;
        Ok(banco)
    }

    /// Uma conexão NOVA ao mesmo arquivo (com os mesmos pragmas e `espera`
    /// como `busy_timeout`), para trabalho longo que não pode segurar a trava
    /// desta: a manutenção (o VACUUM completo da conversão leva minutos num
    /// banco grande) e o checkpoint do WAL. Quem usa esta aqui espera no
    /// máximo o `busy_timeout` do SQLite, em vez de esperar a trava pelo
    /// trabalho inteiro. Banco em memória não tem arquivo: devolve um clone
    /// (a mesma conexão, sem mexer nela).
    pub fn outra_conexao(&self, espera: Duration) -> anyhow::Result<Banco> {
        match &self.caminho {
            Some(caminho) => {
                let banco = Banco::abrir(caminho)?;
                banco.conexao().busy_timeout(espera)?;
                Ok(banco)
            }
            None => Ok(self.clone()),
        }
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

    /// Muda o teto do cache de páginas desta conexão (KiB). Valor negativo
    /// no pragma = tamanho em KiB (positivo seria em páginas).
    pub fn limitar_cache(&self, kib: u32) -> anyhow::Result<()> {
        self.conexao()
            .pragma_update(None, "cache_size", -(kib as i64))?;
        Ok(())
    }

    /// Teto atual do cache de páginas, em KiB.
    pub fn cache_kib(&self) -> anyhow::Result<u32> {
        let valor: i64 = self
            .conexao()
            .query_row("PRAGMA cache_size", [], |l| l.get(0))?;
        // Negativo = KiB; positivo = páginas (convertidas com o tamanho da página).
        if valor < 0 {
            return Ok((-valor) as u32);
        }
        let pagina: i64 = self
            .conexao()
            .query_row("PRAGMA page_size", [], |l| l.get(0))?;
        Ok((valor * pagina / 1024) as u32)
    }

    /// Versão atual do esquema (número de migrações aplicadas).
    pub fn versao(&self) -> anyhow::Result<usize> {
        let versao: i64 = self
            .conexao()
            .query_row("PRAGMA user_version", [], |linha| linha.get(0))?;
        Ok(versao as usize)
    }
}

/// Banco novo (arquivo ainda vazio): liga o vacuum incremental. Precisa vir
/// ANTES de qualquer escrita, inclusive a troca para WAL — depois disso o
/// modo só muda com um VACUUM completo
/// (ver `manutencao::converter_para_vacuum_incremental`).
fn ligar_vacuum_incremental_se_novo(conexao: &Connection) -> anyhow::Result<()> {
    let paginas: i64 = conexao.query_row("PRAGMA page_count", [], |l| l.get(0))?;
    if paginas == 0 {
        conexao.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    }
    Ok(())
}

/// Quantas migrações existem (para testes e para o `abiyss status`).
pub fn total_migracoes() -> usize {
    MIGRACOES.len()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn cache_de_paginas_tem_teto_explicito() {
        let banco = Banco::em_memoria().unwrap();
        assert_eq!(banco.cache_kib().unwrap(), CACHE_PADRAO_KIB);
        banco.limitar_cache(512).unwrap();
        assert_eq!(banco.cache_kib().unwrap(), 512);
    }

    #[test]
    fn outra_conexao_nao_depende_da_trava_desta() {
        let pasta = tempfile::tempdir().unwrap();
        let banco = Banco::abrir(&pasta.path().join("abiyss.db")).unwrap();
        // A trava desta conexão presa (como ficava com a manutenção rodando)...
        let trava = banco.conexao();
        let outra = banco.outra_conexao(ESPERA_PADRAO).unwrap();
        // ...e a outra conexão trabalha no mesmo arquivo, sem esperar por ela.
        std::thread::spawn(move || {
            outra
                .conexao()
                .execute(
                    "INSERT INTO estado_daemon (chave, valor) VALUES ('x', '1')",
                    [],
                )
                .unwrap();
        })
        .join()
        .unwrap();
        let valor: String = trava
            .query_row(
                "SELECT valor FROM estado_daemon WHERE chave = 'x'",
                [],
                |l| l.get(0),
            )
            .unwrap();
        assert_eq!(valor, "1");
        drop(trava);

        // Em memória não há arquivo: é a mesma conexão.
        let memoria = Banco::em_memoria().unwrap();
        memoria
            .conexao()
            .execute(
                "INSERT INTO estado_daemon (chave, valor) VALUES ('y', '2')",
                [],
            )
            .unwrap();
        let clone = memoria.outra_conexao(ESPERA_PADRAO).unwrap();
        let n: i64 = clone
            .conexao()
            .query_row("SELECT count(*) FROM estado_daemon", [], |l| l.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

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
