-- Migrações do banco do runtime do Abiyss, copiadas LITERALMENTE de
-- kernel/src/db.rs (MIGRACOES) no commit 2318c40 da branch
-- claude/determined-carson-6qrkl4. Usadas só nos testes do adaptador
-- SQLite (server/dsr/runtime-sqlite.js). NÃO edite: copie de novo do
-- runtime quando o esquema mudar. Cada bloco é separado por "-- @migracao N".

-- @migracao 1
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
    

-- @migracao 2
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
    

-- @migracao 3
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
    

-- @migracao 4
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
    

-- @migracao 5
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
    

-- @migracao 6
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
    

-- @migracao 7
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
    

-- @migracao 8
    ALTER TABLE chamadas_modelo ADD COLUMN primeiro_token_ms INTEGER;
    ALTER TABLE chamadas_modelo ADD COLUMN stream INTEGER NOT NULL DEFAULT 0;
    CREATE INDEX idx_chamadas_modelo_momento ON chamadas_modelo(modelo, momento_ms);
    

-- @migracao 9
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
    

-- @migracao 10
    ALTER TABLE fila_eventos ADD COLUMN origem_externa TEXT;
    UPDATE fila_eventos SET origem_externa = 'subagente:' || origem
        WHERE tipo = 'subagente';
    
