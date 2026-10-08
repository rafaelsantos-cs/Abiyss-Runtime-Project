//! Infra 3: retenção dos registros detalhados (agregados diários), checkpoint
//! do WAL e vacuum incremental — com dados sintéticos de 60 dias.

use abiyss::db::Banco;
use abiyss::manutencao::{self, ConfigRetencao};
use rusqlite::params;

const DIA_MS: i64 = 24 * 60 * 60 * 1000;
/// 2026-10-05T15:00:00Z
const AGORA: i64 = 1_791_212_400_000;
const DIAS_SINTETICOS: i64 = 60;
const CHAMADAS_POR_DIA: i64 = 40;

/// Preenche as três tabelas com um registro por hora (mais ou menos) de
/// cada tipo nos últimos 60 dias. Metade dos eventos fica pendente nos
/// primeiros 5 dias (não pode ser apagada nunca).
fn preencher(banco: &Banco) {
    let mut conexao = banco.conexao();
    let t = conexao.transaction().unwrap();
    for dia in 0..DIAS_SINTETICOS {
        let base = AGORA - (dia + 1) * DIA_MS;
        for i in 0..CHAMADAS_POR_DIA {
            let momento = base + i * (DIA_MS / CHAMADAS_POR_DIA);
            let status = if i % 10 == 0 { "erro" } else { "ok" };
            let ttft: Option<i64> = (status == "ok").then_some(100 + i);
            t.execute(
                "INSERT INTO chamadas_modelo (momento_ms, pool, origem, modelo, tentativa, status,
                    tokens_entrada, tokens_saida, duracao_ms, primeiro_token_ms, stream)
                 VALUES (?1, ?2, 'autonomo', 'teste/glm', 1, ?3, 1000, 200, ?4, ?5, 1)",
                params![
                    momento,
                    if i % 2 == 0 { "cerebro" } else { "subagentes" },
                    status,
                    1000 + i,
                    ttft
                ],
            )
            .unwrap();
            // Conteúdo grande para o vacuum ter o que devolver.
            let conteudo = "x".repeat(2_000);
            let consumido: Option<i64> = (dia >= 5 || i % 2 == 0).then_some(momento + 1);
            t.execute(
                "INSERT INTO fila_eventos (momento_ms, tipo, origem, conteudo, consumido_ms)
                 VALUES (?1, 'cron', 'lembrete', ?2, ?3)",
                params![momento, conteudo, consumido],
            )
            .unwrap();
            t.execute(
                "INSERT INTO ciclos (inicio_ms, fim_ms, chamou_modelo, motivo, tokens, erro)
                 VALUES (?1, ?2, ?3, 'teste', 50, ?4)",
                params![
                    momento,
                    momento + 500,
                    i % 3 == 0,
                    (i % 7 == 0).then_some("falhou")
                ],
            )
            .unwrap();
        }
    }
    t.commit().unwrap();
}

fn contar(banco: &Banco, sql: &str) -> i64 {
    banco.conexao().query_row(sql, [], |l| l.get(0)).unwrap()
}

#[test]
fn agrega_o_que_passou_da_retencao_e_preserva_o_resto() {
    let pasta = tempfile::tempdir().unwrap();
    let banco = Banco::abrir(&pasta.path().join("abiyss.db")).unwrap();
    preencher(&banco);

    let total_chamadas = contar(&banco, "SELECT COUNT(*) FROM chamadas_modelo");
    let tokens_antes = contar(&banco, "SELECT SUM(tokens_entrada) FROM chamadas_modelo");
    let pendentes_antes = contar(
        &banco,
        "SELECT COUNT(*) FROM fila_eventos WHERE consumido_ms IS NULL",
    );
    let ciclos_antes = contar(&banco, "SELECT COUNT(*) FROM ciclos");
    let ttft_antes = contar(
        &banco,
        "SELECT COUNT(primeiro_token_ms) FROM chamadas_modelo",
    );
    let config = ConfigRetencao::default(); // 30 dias para tudo
    let limite = manutencao::limite_ms(AGORA, 30);
    let chamadas_velhas = contar(
        &banco,
        &format!("SELECT COUNT(*) FROM chamadas_modelo WHERE momento_ms < {limite}"),
    );
    assert!(chamadas_velhas > 0);

    let relatorio = manutencao::aplicar_retencao(&banco, &config, AGORA).unwrap();
    assert_eq!(relatorio.chamadas_apagadas as i64, chamadas_velhas);
    assert!(relatorio.dias_agregados >= 29, "{relatorio:?}");

    // Nada mais velho que o limite sobrou em detalhe (exceto eventos pendentes).
    assert_eq!(
        contar(
            &banco,
            &format!("SELECT COUNT(*) FROM chamadas_modelo WHERE momento_ms < {limite}")
        ),
        0
    );
    assert_eq!(
        contar(
            &banco,
            &format!("SELECT COUNT(*) FROM ciclos WHERE inicio_ms < {limite}")
        ),
        0
    );
    assert_eq!(
        contar(
            &banco,
            &format!(
                "SELECT COUNT(*) FROM fila_eventos WHERE momento_ms < {limite} AND consumido_ms IS NOT NULL"
            )
        ),
        0
    );
    // Pendentes nunca são apagados.
    assert_eq!(
        contar(
            &banco,
            "SELECT COUNT(*) FROM fila_eventos WHERE consumido_ms IS NULL"
        ),
        pendentes_antes
    );
    // As últimas 24 h ficam intactas.
    assert_eq!(
        contar(
            &banco,
            &format!(
                "SELECT COUNT(*) FROM chamadas_modelo WHERE momento_ms >= {}",
                AGORA - DIA_MS
            )
        ),
        CHAMADAS_POR_DIA
    );

    // Detalhe + agregado = total original (nada se perde nas contagens).
    let agregadas = contar(&banco, "SELECT SUM(chamadas) FROM chamadas_modelo_diarias");
    let restantes = contar(&banco, "SELECT COUNT(*) FROM chamadas_modelo");
    assert_eq!(agregadas + restantes, total_chamadas);
    let tokens_depois = contar(
        &banco,
        "SELECT SUM(tokens_entrada) FROM chamadas_modelo_diarias",
    ) + contar(&banco, "SELECT SUM(tokens_entrada) FROM chamadas_modelo");
    assert_eq!(tokens_depois, tokens_antes);
    let ciclos_depois = contar(&banco, "SELECT SUM(ciclos) FROM ciclos_diarios")
        + contar(&banco, "SELECT COUNT(*) FROM ciclos");
    assert_eq!(ciclos_depois, ciclos_antes);
    // Latência sem percentil depois de agregada, mas com soma, contagem e
    // máximo — que também fecham com o original.
    let amostras = contar(
        &banco,
        "SELECT SUM(primeiro_token_amostras) FROM chamadas_modelo_diarias",
    ) + contar(
        &banco,
        "SELECT COUNT(primeiro_token_ms) FROM chamadas_modelo",
    );
    assert_eq!(amostras, ttft_antes);
    assert_eq!(
        contar(
            &banco,
            "SELECT MAX(duracao_max_ms) FROM chamadas_modelo_diarias"
        ),
        1000 + CHAMADAS_POR_DIA - 1
    );

    // Rodar de novo não muda nada.
    let de_novo = manutencao::aplicar_retencao(&banco, &config, AGORA).unwrap();
    assert_eq!(de_novo, Default::default());
    assert_eq!(
        contar(&banco, "SELECT SUM(chamadas) FROM chamadas_modelo_diarias"),
        agregadas
    );

    // Um dia depois, só mais um dia é agregado e soma no que já existe.
    let amanha = manutencao::aplicar_retencao(&banco, &config, AGORA + DIA_MS).unwrap();
    assert_eq!(amanha.chamadas_apagadas as i64, CHAMADAS_POR_DIA);
    assert_eq!(
        contar(&banco, "SELECT SUM(chamadas) FROM chamadas_modelo_diarias"),
        agregadas + CHAMADAS_POR_DIA
    );
}

#[test]
fn vacuum_incremental_e_checkpoint_devolvem_espaco() {
    let pasta = tempfile::tempdir().unwrap();
    let caminho = pasta.path().join("abiyss.db");
    let banco = Banco::abrir(&caminho).unwrap();
    // Banco novo já nasce com vacuum incremental.
    assert!(manutencao::vacuum_incremental_ativo(&banco).unwrap());
    preencher(&banco);
    manutencao::checkpoint_wal(&banco).unwrap();
    let tamanho_cheio = std::fs::metadata(&caminho).unwrap().len();

    let resumo = manutencao::rodada_completa(&banco, &ConfigRetencao::default(), AGORA).unwrap();
    assert!(resumo.contains("vacuum"), "{resumo}");
    let checkpoint = manutencao::checkpoint_wal(&banco).unwrap();
    assert!(!checkpoint.ocupado);
    // O WAL foi truncado.
    let wal = pasta.path().join("abiyss.db-wal");
    assert_eq!(std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0), 0);
    // E o arquivo encolheu (cerca de metade dos eventos de 2 KB foi embora).
    let tamanho_depois = std::fs::metadata(&caminho).unwrap().len();
    assert!(
        tamanho_depois < tamanho_cheio * 3 / 4,
        "antes {tamanho_cheio}, depois {tamanho_depois}"
    );
}

#[test]
fn banco_antigo_e_convertido_para_vacuum_incremental() {
    let pasta = tempfile::tempdir().unwrap();
    let caminho = pasta.path().join("abiyss.db");
    {
        // Simula um banco criado antes desta versão (auto_vacuum desligado).
        let antigo = rusqlite::Connection::open(&caminho).unwrap();
        antigo
            .execute_batch("PRAGMA auto_vacuum = NONE; CREATE TABLE velha (x);")
            .unwrap();
    }
    let banco = Banco::abrir(&caminho).unwrap();
    assert!(!manutencao::vacuum_incremental_ativo(&banco).unwrap());

    let mut config = ConfigRetencao {
        converter_auto_vacuum: false,
        ..Default::default()
    };
    manutencao::rodada_completa(&banco, &config, AGORA).unwrap();
    assert!(!manutencao::vacuum_incremental_ativo(&banco).unwrap());

    config.converter_auto_vacuum = true;
    let resumo = manutencao::rodada_completa(&banco, &config, AGORA).unwrap();
    assert!(resumo.contains("convertido"), "{resumo}");
    assert!(manutencao::vacuum_incremental_ativo(&banco).unwrap());
}

/// Mensagens do gateway (Discord): o que terminou vira agregado diário; o
/// que ainda está na fila e o que liga a um pedido pendente ficam.
#[test]
fn mensagens_do_gateway_entram_na_retencao() {
    let banco = Banco::em_memoria().unwrap();
    let velho = AGORA - 40 * DIA_MS;
    let pedido = |estado: &str| -> i64 {
        let c = banco.conexao();
        c.execute(
            "INSERT INTO pedidos_usuario (criado_ms, origem, pergunta, urgencia, estado, chave)
             VALUES (?1, 'heartbeat', 'p?', 'normal', ?2, ?3)",
            params![velho, estado, format!("{estado}-{}", sequencia())],
        )
        .unwrap();
        c.last_insert_rowid()
    };
    let pendente = pedido("pendente");
    let respondido = pedido("respondido");
    let inserir = |momento: i64,
                   direcao: &str,
                   tipo: &str,
                   estado: &str,
                   pedido: Option<i64>,
                   discord: &str|
     -> i64 {
        let c = banco.conexao();
        c.execute(
            "INSERT INTO gateway_mensagens (momento_ms, direcao, tipo, estado, pedido_id, discord_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![momento, direcao, tipo, estado, pedido, discord],
        )
        .unwrap();
        let id = c.last_insert_rowid();
        c.execute(
            "INSERT INTO gateway_ids_discord (discord_id, mensagem_id) VALUES (?1, ?2)",
            params![discord, id],
        )
        .unwrap();
        id
    };
    // Velhas e terminadas: vão embora.
    inserir(velho, "entrada", "dono", "respondida", None, "1");
    inserir(velho + 1, "saida", "resposta", "entregue", None, "2");
    inserir(
        velho + 2,
        "saida",
        "pedido",
        "entregue",
        Some(respondido),
        "3",
    );
    // Velhas, mas ainda servem: ficam.
    let fila = inserir(velho + 3, "entrada", "dono", "pendente", None, "4");
    let saida = inserir(velho + 4, "saida", "resposta", "pendente", None, "5");
    let do_pedido = inserir(
        velho + 5,
        "saida",
        "pedido",
        "entregue",
        Some(pendente),
        "6",
    );
    // Novas: ficam.
    let nova = inserir(AGORA - DIA_MS, "entrada", "dono", "respondida", None, "7");

    let r = manutencao::aplicar_retencao(&banco, &ConfigRetencao::default(), AGORA).unwrap();
    assert_eq!(r.gateway_apagadas, 3, "{r:?}");
    assert!(r.como_texto().contains("3 mensagem(ns) do gateway"));
    let restantes: Vec<i64> = {
        let c = banco.conexao();
        let mut q = c
            .prepare("SELECT id FROM gateway_mensagens ORDER BY id")
            .unwrap();
        q.query_map([], |l| l.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(restantes, vec![fila, saida, do_pedido, nova]);
    // Os ids do Discord das apagadas saíram junto (o "responder" não acha
    // mais nada); os das que ficaram continuam.
    assert_eq!(
        contar(&banco, "SELECT COUNT(*) FROM gateway_ids_discord"),
        4
    );
    assert_eq!(
        contar(
            &banco,
            "SELECT SUM(mensagens) FROM gateway_mensagens_diarias"
        ),
        3
    );
    // De novo: nada muda.
    let de_novo = manutencao::aplicar_retencao(&banco, &ConfigRetencao::default(), AGORA).unwrap();
    assert_eq!(de_novo.gateway_apagadas, 0);
}

/// Chave única por pedido de teste (a chave pendente é única no banco).
fn sequencia() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}
