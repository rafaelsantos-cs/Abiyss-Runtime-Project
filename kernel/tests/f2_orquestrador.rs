//! F2: orquestrador com dois pools, testado contra o mock do NIM.
//!
//! Os limites aqui são bem maiores que os 40/min reais (ex.: 600/min =
//! uma ficha a cada 100 ms) só para os testes rodarem rápido. A lógica é
//! a mesma.
//!
//! Sem depender de relógio onde dá: o espaçamento é medido nas FICHAS que o
//! próprio balde entrega (um gatilho no banco anota cada uma com o instante
//! do balde), não na chegada ao mock, que soma a latência da rede e do
//! agendador; a ordem entre tarefas é combinada esperando o que dá para
//! observar (tamanho da fila, bloqueio gravado, requisições seguradas no
//! mock), não com `sleep`. Tempo pausado do tokio não serve aqui: o balde
//! usa o relógio de parede (é dividido entre processos pelo SQLite) e, com
//! HTTP de verdade, o avanço automático do tempo dispararia os prazos do
//! cliente no meio de uma resposta.

use std::sync::Arc;
use std::time::{Duration, Instant};

use abiyss::config::{Config, config_de_teste};
use abiyss::db::Banco;
use abiyss::nim::mock::{MockNim, RespostaMock};
use abiyss::nim::{self, Mensagem, PedidoChat};
use abiyss::orquestrador::{
    BALDE_CEREBRO, BALDE_CEREBRO_AUTONOMO, BALDE_CEREBRO_TERCEIROS, BALDE_SUBAGENTES, Nivel,
    Origem, Orquestrador,
};
use rusqlite::params;
use tokio::sync::Semaphore;

fn config(mock: &MockNim) -> Config {
    let mut c = config_de_teste(&mock.base_url(), std::path::Path::new("."));
    // Retentativas rápidas para os testes.
    for r in [
        &mut c.pools.cerebro.retentativas,
        &mut c.pools.subagentes.retentativas,
    ] {
        r.max_tentativas = 4;
        r.backoff_inicial_ms = 100;
        r.backoff_maximo_ms = 400;
    }
    c
}

fn orquestrador(config: &Config) -> Orquestrador {
    Orquestrador::novo(
        config,
        Banco::em_memoria().unwrap(),
        "nvapi-cerebro",
        "nvapi-sub",
    )
    .unwrap()
}

/// Orquestrador com o registro de fichas ligado no banco dele.
fn orquestrador_com_fichas(config: &Config) -> (Orquestrador, Banco) {
    let banco = Banco::em_memoria().unwrap();
    registrar_fichas(&banco);
    let o = Orquestrador::novo(config, banco.clone(), "nvapi-cerebro", "nvapi-sub").unwrap();
    (o, banco)
}

/// Anota cada ficha entregue, com o instante do PRÓPRIO balde: o UPDATE de
/// `baldes.tokens` só acontece quando o balde entrega uma ficha (o 429 mexe
/// só em `bloqueado_ate_ms`).
fn registrar_fichas(banco: &Banco) {
    banco
        .conexao()
        .execute_batch(
            "CREATE TABLE fichas_teste (
                 id INTEGER PRIMARY KEY,
                 balde TEXT NOT NULL,
                 momento_ms INTEGER NOT NULL
             );
             CREATE TRIGGER ficha_entregue AFTER UPDATE OF tokens ON baldes
             BEGIN
                 INSERT INTO fichas_teste (balde, momento_ms) VALUES (NEW.nome, NEW.atualizado_ms);
             END;",
        )
        .unwrap();
}

/// Instantes (ms, relógio do banco) das fichas entregues por um balde.
fn fichas(banco: &Banco, balde: &str) -> Vec<i64> {
    let conexao = banco.conexao();
    let mut consulta = conexao
        .prepare("SELECT momento_ms FROM fichas_teste WHERE balde = ?1 ORDER BY id")
        .unwrap();
    consulta
        .query_map(params![balde], |l| l.get(0))
        .unwrap()
        .map(|m| m.unwrap())
        .collect()
}

/// Fichas consecutivas nunca mais perto que o período do balde (1 ms de
/// folga só para o arredondamento da conta em ponto flutuante).
fn assert_espacadas(fichas: &[i64], periodo_ms: i64) {
    for par in fichas.windows(2) {
        assert!(
            par[1] - par[0] >= periodo_ms - 1,
            "fichas a {} ms uma da outra (período {periodo_ms} ms): {fichas:?}",
            par[1] - par[0]
        );
    }
}

/// Toda requisição precisa da sua ficha ANTES de sair: em ordem, a k-ésima
/// chegada ao mock não vem antes da k-ésima ficha (mesmo relógio).
fn assert_chegadas_depois_das_fichas(mock: &MockNim, fichas: &[i64]) {
    let mut chegadas: Vec<i64> = mock.requisicoes().iter().map(|r| r.momento_ms).collect();
    chegadas.sort_unstable();
    assert_eq!(chegadas.len(), fichas.len(), "uma ficha por requisição");
    for (chegada, ficha) in chegadas.iter().zip(fichas) {
        assert!(
            chegada >= ficha,
            "requisição chegou ({chegada}) antes da ficha ({ficha})"
        );
    }
}

/// Espera algo observável acontecer (sincronização explícita, sem `sleep`
/// com prazo para dar certo). Só falha se não acontecer em 10 s.
async fn esperar(condicao: impl Fn() -> bool, o_que: &str) {
    let limite = Instant::now() + Duration::from_secs(10);
    while !condicao() {
        assert!(Instant::now() < limite, "não aconteceu: {o_que}");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// O bloqueio (429) gravado no balde, quando aparecer.
async fn esperar_bloqueio(banco: &Banco, balde: &str) -> i64 {
    let ler = || -> i64 {
        banco
            .conexao()
            .query_row(
                "SELECT COALESCE(MAX(bloqueado_ate_ms), 0) FROM baldes WHERE nome = ?1",
                params![balde],
                |l| l.get(0),
            )
            .unwrap()
    };
    esperar(|| ler() > 0, "o 429 virar bloqueio do pool").await;
    ler()
}

fn pedido(config: &Config, nivel: Option<Nivel>, texto: &str) -> PedidoChat {
    let modelo = match nivel {
        None => &config.modelos.cerebro,
        Some(Nivel::Ultra) => &config.modelos.sub_ultra,
        Some(Nivel::Medium) => &config.modelos.sub_medium,
        Some(Nivel::Low) => &config.modelos.sub_low,
    };
    nim::montar_pedido(modelo, vec![Mensagem::usuario(texto)], vec![])
}

/// Intervalos entre as chegadas consecutivas no mock.
fn intervalos(mock: &MockNim) -> Vec<Duration> {
    let momentos: Vec<Instant> = mock.requisicoes().iter().map(|r| r.momento).collect();
    momentos.windows(2).map(|par| par[1] - par[0]).collect()
}

#[tokio::test]
async fn token_bucket_espaca_as_requisicoes() {
    let mock = MockNim::iniciar().await;
    let mut c = config(&mock);
    c.pools.subagentes.requisicoes_por_minuto = 600; // 1 ficha a cada 100 ms
    c.pools.subagentes.rajada = 1;
    c.pools.subagentes.concorrencia.low = 10;
    let (o, banco) = orquestrador_com_fichas(&c);

    let mut tarefas = Vec::new();
    for i in 0..5 {
        let o = o.clone();
        let p = pedido(&c, Some(Nivel::Low), &format!("{i}"));
        tarefas.push(tokio::spawn(async move {
            o.subagentes.chamar(Nivel::Low, &p, None).await.unwrap()
        }));
    }
    for t in tarefas {
        t.await.unwrap();
    }

    assert_eq!(mock.total_requisicoes(), 5);
    let fichas = fichas(&banco, BALDE_SUBAGENTES);
    assert_eq!(fichas.len(), 5);
    assert_espacadas(&fichas, 100);
    assert_chegadas_depois_das_fichas(&mock, &fichas);
}

#[tokio::test]
async fn respeita_retry_after_do_429() {
    let mock = MockNim::iniciar().await;
    let c = config(&mock);
    let o = orquestrador(&c);
    mock.enfileirar(RespostaMock::erro(429, Some("1")));

    let inicio = Instant::now();
    let r = o
        .cerebro
        .chamar(Origem::Conversa, &pedido(&c, None, "oi"), None)
        .await
        .unwrap();

    assert_eq!(r.mensagem.texto(), "mock: oi");
    assert_eq!(mock.total_requisicoes(), 2);
    assert!(
        inicio.elapsed() >= Duration::from_millis(950),
        "deveria ter esperado o Retry-After de 1 s, esperou {:?}",
        inicio.elapsed()
    );
}

#[tokio::test]
async fn retry_after_segura_o_pool_inteiro() {
    let mock = MockNim::iniciar().await;
    let mut c = config(&mock);
    c.pools.subagentes.requisicoes_por_minuto = 600;
    c.pools.subagentes.concorrencia.low = 5;
    let (o, banco) = orquestrador_com_fichas(&c);
    mock.enfileirar(RespostaMock::erro(429, Some("1")));
    let chamar = |texto: &str| {
        let o = o.clone();
        let p = pedido(&c, Some(Nivel::Low), texto);
        tokio::spawn(async move { o.subagentes.chamar(Nivel::Low, &p, None).await.unwrap() })
    };

    // A primeira leva o 429; só DEPOIS que o bloqueio está gravado chegam as
    // outras (antes disso, uma ficha entregue antes do 429 é legítima).
    let mut tarefas = vec![chamar("0")];
    let ate = esperar_bloqueio(&banco, BALDE_SUBAGENTES).await;
    tarefas.extend([chamar("1"), chamar("2")]);
    for t in tarefas {
        t.await.unwrap();
    }

    // Nenhuma ficha (logo, nenhuma requisição) até o fim do bloqueio: nem a
    // nova tentativa da primeira, nem as que chegaram durante ele.
    let fichas = fichas(&banco, BALDE_SUBAGENTES);
    assert_eq!(fichas.len(), 4, "{fichas:?}");
    assert!(fichas[1..].iter().all(|f| *f >= ate), "{fichas:?} < {ate}");
    let recebidas = mock.requisicoes();
    assert_eq!(recebidas.len(), 4);
    for r in &recebidas[1..] {
        assert!(
            r.momento_ms >= ate,
            "uma requisição furou o bloqueio do 429"
        );
    }
    assert_chegadas_depois_das_fichas(&mock, &fichas);
}

#[tokio::test]
async fn backoff_exponencial_em_erros_5xx() {
    let mock = MockNim::iniciar().await;
    let c = config(&mock);
    let o = orquestrador(&c);
    mock.enfileirar(RespostaMock::erro(503, None));
    mock.enfileirar(RespostaMock::erro(502, None));

    let inicio = Instant::now();
    o.cerebro
        .chamar(Origem::Conversa, &pedido(&c, None, "x"), None)
        .await
        .unwrap();

    assert_eq!(mock.total_requisicoes(), 3);
    let gaps = intervalos(&mock);
    // backoff 1 = 50..100 ms; backoff 2 = 100..200 ms.
    assert!(gaps[0] >= Duration::from_millis(45), "{gaps:?}");
    assert!(gaps[1] >= Duration::from_millis(95), "{gaps:?}");
    assert!(inicio.elapsed() >= Duration::from_millis(145));
}

#[tokio::test]
async fn desiste_depois_do_maximo_de_tentativas() {
    let mock = MockNim::iniciar().await;
    let c = config(&mock);
    let o = orquestrador(&c);
    mock.definir_roteiro(|_| RespostaMock::erro(503, None));

    let erro = o
        .cerebro
        .chamar(Origem::Conversa, &pedido(&c, None, "x"), None)
        .await
        .unwrap_err();
    assert_eq!(mock.total_requisicoes(), 4);
    assert!(format!("{erro:#}").contains("503"));
}

#[tokio::test]
async fn erro_400_nao_e_repetido() {
    let mock = MockNim::iniciar().await;
    let c = config(&mock);
    let o = orquestrador(&c);
    mock.enfileirar(RespostaMock::erro(400, None));

    assert!(
        o.cerebro
            .chamar(Origem::Conversa, &pedido(&c, None, "x"), None)
            .await
            .is_err()
    );
    assert_eq!(mock.total_requisicoes(), 1);
}

#[tokio::test]
async fn ultra_passa_na_frente_de_low() {
    let mock = MockNim::iniciar().await;
    let mut c = config(&mock);
    c.pools.subagentes.requisicoes_por_minuto = 120; // 1 ficha a cada 500 ms
    c.pools.subagentes.concorrencia.low = 5;
    let (o, banco) = orquestrador_com_fichas(&c);

    // Gasta a ficha inicial: agora todo mundo vai ter de esperar.
    o.subagentes
        .chamar(Nivel::Low, &pedido(&c, Some(Nivel::Low), "aquece"), None)
        .await
        .unwrap();

    let mut tarefas = Vec::new();
    for i in 0..3 {
        let o = o.clone();
        let p = pedido(&c, Some(Nivel::Low), &format!("low {i}"));
        tarefas.push(tokio::spawn(async move {
            o.subagentes.chamar(Nivel::Low, &p, None).await.unwrap()
        }));
    }
    // O Ultra chega DEPOIS dos três Low (já na fila, esperando a ficha)...
    esperar(|| o.subagentes.na_fila() == 3, "os três low na fila").await;
    let o2 = o.clone();
    let p = pedido(&c, Some(Nivel::Ultra), "ultra");
    tarefas.push(tokio::spawn(async move {
        o2.subagentes.chamar(Nivel::Ultra, &p, None).await.unwrap()
    }));
    esperar(|| o.subagentes.na_fila() == 4, "o ultra na fila").await;
    // ...e antes da próxima ficha (só a do aquecimento saiu até aqui).
    assert_eq!(fichas(&banco, BALDE_SUBAGENTES).len(), 1);
    for t in tarefas {
        t.await.unwrap();
    }

    // ...mas é atendido primeiro.
    let modelos: Vec<String> = mock
        .requisicoes()
        .iter()
        .map(|r| r.corpo["model"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(modelos[1], "teste/ultra", "ordem: {modelos:?}");
}

#[tokio::test]
async fn limite_de_concorrencia_por_nivel() {
    let mock = MockNim::iniciar().await;
    let mut c = config(&mock);
    c.pools.subagentes.requisicoes_por_minuto = 60_000;
    c.pools.subagentes.rajada = 100;
    c.pools.subagentes.concorrencia.low = 2;
    let o = orquestrador(&c);
    // Cada resposta fica segurada no mock até o teste liberar.
    let portao = Arc::new(Semaphore::new(0));
    let p2 = Arc::clone(&portao);
    mock.definir_roteiro(move |_| RespostaMock::texto("ok").segurada(Arc::clone(&p2)));

    let mut tarefas = Vec::new();
    for i in 0..6 {
        let o = o.clone();
        let p = pedido(&c, Some(Nivel::Low), &format!("{i}"));
        tarefas.push(tokio::spawn(async move {
            o.subagentes.chamar(Nivel::Low, &p, None).await.unwrap()
        }));
    }
    // 6 chamadas, 2 por vez: três rodadas, cada uma com duas seguradas no mock.
    for rodada in 1..=3 {
        esperar(
            || mock.total_requisicoes() == 2 * rodada && mock.em_andamento() == 2,
            "duas chamadas seguradas no mock",
        )
        .await;
        portao.add_permits(2);
    }
    for t in tarefas {
        t.await.unwrap();
    }

    assert_eq!(mock.total_requisicoes(), 6);
    // As duas vagas foram usadas, e nunca uma terceira ao mesmo tempo.
    assert_eq!(mock.pico_concorrencia(), 2);
}

#[tokio::test]
async fn um_pool_nunca_usa_a_capacidade_do_outro() {
    let mock = MockNim::iniciar().await;
    let mut c = config(&mock);
    c.pools.subagentes.requisicoes_por_minuto = 6; // 1 ficha a cada 10 s
    let o = orquestrador(&c);

    // Esgota o pool dos sub-agentes.
    o.subagentes
        .chamar(Nivel::Low, &pedido(&c, Some(Nivel::Low), "a"), None)
        .await
        .unwrap();
    let preso = {
        let o = o.clone();
        let p = pedido(&c, Some(Nivel::Low), "b");
        tokio::spawn(async move { o.subagentes.chamar(Nivel::Low, &p, None).await })
    };
    esperar(
        || o.subagentes.na_fila() == 1,
        "o sub-agente esperando ficha",
    )
    .await;

    // O cérebro continua respondendo: a chamada volta enquanto o sub-agente
    // ainda espera a ficha dele (que só vem daqui a 10 s).
    o.cerebro
        .chamar(Origem::Conversa, &pedido(&c, None, "c"), None)
        .await
        .unwrap();
    assert!(!preso.is_finished(), "o sub-agente deveria estar esperando");
    assert_eq!(o.subagentes.na_fila(), 1);
    preso.abort();

    // E cada pool usa a SUA chave.
    let chaves: Vec<String> = mock
        .requisicoes()
        .iter()
        .map(|r| r.autorizacao.clone().unwrap())
        .collect();
    assert_eq!(chaves, vec!["Bearer nvapi-sub", "Bearer nvapi-cerebro"]);
}

#[tokio::test]
async fn conversa_tem_fatia_reservada_no_pool_do_cerebro() {
    let mock = MockNim::iniciar().await;
    let mut c = config(&mock);
    // Total: 1 ficha a cada 100 ms. Autônomo: no máximo 1 a cada 500 ms.
    c.pools.cerebro.requisicoes_por_minuto = 600;
    c.pools.cerebro.reserva_conversa_por_minuto = 480;
    let (o, banco) = orquestrador_com_fichas(&c);

    // Muitas chamadas autônomas ao mesmo tempo...
    let mut tarefas = Vec::new();
    for i in 0..4 {
        let o = o.clone();
        let p = pedido(&c, None, &format!("auto {i}"));
        tarefas.push(tokio::spawn(async move {
            o.cerebro.chamar(Origem::Autonomo, &p, None).await.unwrap()
        }));
    }
    // (a primeira levou a ficha; as outras três esperam na fila)...
    esperar(
        || fichas(&banco, BALDE_CEREBRO_AUTONOMO).len() == 1 && o.cerebro.na_fila() == 3,
        "uma autônoma enviada e três na fila",
    )
    .await;
    // ...e o usuário manda uma mensagem.
    o.cerebro
        .chamar(Origem::Conversa, &pedido(&c, None, "usuario"), None)
        .await
        .unwrap();
    for t in tarefas {
        t.await.unwrap();
    }

    // As autônomas nunca saíram mais rápido que a fatia delas, nem o pool
    // inteiro mais rápido que o total.
    let autonomas = fichas(&banco, BALDE_CEREBRO_AUTONOMO);
    let total = fichas(&banco, BALDE_CEREBRO);
    assert_eq!((autonomas.len(), total.len()), (4, 5));
    assert_espacadas(&autonomas, 500);
    assert_espacadas(&total, 100);
    // A conversa não ficou atrás da fila autônoma: levou a ficha seguinte do
    // balde total (a que não é de autônoma), antes da 2ª autônoma.
    let da_conversa: Vec<i64> = total
        .iter()
        .filter(|f| !autonomas.contains(f))
        .copied()
        .collect();
    assert_eq!(
        da_conversa,
        vec![total[1]],
        "total {total:?}, autônomas {autonomas:?}"
    );
    assert!(total[1] < autonomas[1]);
    assert_chegadas_depois_das_fichas(&mock, &total);
}

#[tokio::test]
async fn processos_diferentes_dividem_o_mesmo_balde() {
    // Dois orquestradores com o MESMO arquivo de banco simulam
    // `abiyss chat` e `abiyss daemon` rodando juntos.
    let mock = MockNim::iniciar().await;
    let mut c = config(&mock);
    c.pools.cerebro.requisicoes_por_minuto = 600;
    c.pools.cerebro.reserva_conversa_por_minuto = 0;
    let pasta = tempfile::tempdir().unwrap();
    let caminho = pasta.path().join("abiyss.db");
    let banco = Banco::abrir(&caminho).unwrap();
    registrar_fichas(&banco);
    let chat = Orquestrador::novo(&c, banco.clone(), "k1", "k2").unwrap();
    let daemon = Orquestrador::novo(&c, Banco::abrir(&caminho).unwrap(), "k1", "k2").unwrap();

    let mut tarefas = Vec::new();
    for (i, o) in [chat.clone(), daemon.clone(), chat, daemon]
        .into_iter()
        .enumerate()
    {
        let p = pedido(&c, None, &format!("{i}"));
        let o = Arc::new(o);
        tarefas.push(tokio::spawn(async move {
            o.cerebro.chamar(Origem::Conversa, &p, None).await.unwrap()
        }));
    }
    for t in tarefas {
        t.await.unwrap();
    }
    // As fichas dos dois "processos" saíram do mesmo balde, no ritmo dele.
    let fichas = fichas(&banco, BALDE_CEREBRO);
    assert_eq!(fichas.len(), 4);
    assert_espacadas(&fichas, 100);
    assert_chegadas_depois_das_fichas(&mock, &fichas);
}

#[tokio::test]
async fn cada_tentativa_fica_registrada_no_banco() {
    let mock = MockNim::iniciar().await;
    let c = config(&mock);
    let banco = Banco::em_memoria().unwrap();
    let o = Orquestrador::novo(&c, banco.clone(), "a", "b").unwrap();
    mock.enfileirar(RespostaMock::erro(429, Some("0")));

    o.cerebro
        .chamar(Origem::Conversa, &pedido(&c, None, "x"), None)
        .await
        .unwrap();

    let linhas: Vec<(String, String, i64)> = {
        let con = banco.conexao();
        let mut consulta = con
            .prepare("SELECT origem, status, tokens_saida FROM chamadas_modelo ORDER BY id")
            .unwrap();
        consulta
            .query_map([], |l| Ok((l.get(0)?, l.get(1)?, l.get(2)?)))
            .unwrap()
            .map(|l| l.unwrap())
            .collect()
    };
    assert_eq!(linhas.len(), 2);
    assert_eq!(linhas[0].1, "limite");
    assert_eq!(linhas[1].0, "conversa");
    assert_eq!(linhas[1].1, "ok");
    assert!(linhas[1].2 > 0);
}

/// Conversa com outras pessoas (gateway): fica atrás do dono E do heartbeat,
/// mesmo chegando antes, e tem o próprio teto por minuto.
#[tokio::test]
async fn terceiros_ficam_atras_do_dono_e_do_heartbeat_e_tem_teto_proprio() {
    let mock = MockNim::iniciar().await;
    let mut c = config(&mock);
    c.pools.cerebro.requisicoes_por_minuto = 600; // 1 ficha a cada 100 ms
    c.pools.cerebro.reserva_conversa_por_minuto = 100;
    c.pools.cerebro.terceiros_por_minuto = 120; // 1 a cada 500 ms
    let (o, banco) = orquestrador_com_fichas(&c);
    o.cerebro
        .chamar(Origem::Conversa, &pedido(&c, None, "aquece"), None)
        .await
        .unwrap();

    let mut tarefas = Vec::new();
    for i in 0..3 {
        let o = o.clone();
        let p = pedido(&c, None, &format!("terceiro {i}"));
        tarefas.push(tokio::spawn(async move {
            o.cerebro.chamar(Origem::Terceiros, &p, None).await.unwrap()
        }));
    }
    esperar(|| o.cerebro.na_fila() == 3, "os três terceiros na fila").await;
    for (origem, texto) in [(Origem::Autonomo, "heartbeat"), (Origem::Conversa, "dono")] {
        let o = o.clone();
        let p = pedido(&c, None, texto);
        tarefas.push(tokio::spawn(async move {
            o.cerebro.chamar(origem, &p, None).await.unwrap()
        }));
    }
    esperar(|| o.cerebro.na_fila() == 5, "dono e heartbeat na fila").await;
    for t in tarefas {
        t.await.unwrap();
    }
    let ordem: Vec<String> = mock
        .requisicoes()
        .iter()
        .map(|r| {
            r.corpo["messages"][0]["content"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        ordem,
        [
            "aquece",
            "dono",
            "heartbeat",
            "terceiro 0",
            "terceiro 1",
            "terceiro 2"
        ],
        "chegaram depois, foram antes"
    );
    // O teto próprio: as fichas dos terceiros ficam a 500 ms umas das outras.
    let proprias = fichas(&banco, BALDE_CEREBRO_TERCEIROS);
    assert_eq!(proprias.len(), 3);
    for par in proprias.windows(2) {
        assert!(par[1] - par[0] >= 499, "fichas de terceiros: {proprias:?}");
    }
    // E saem também da fatia autônoma (nunca da reserva do dono).
    assert_eq!(fichas(&banco, BALDE_CEREBRO_AUTONOMO).len(), 4);
}
