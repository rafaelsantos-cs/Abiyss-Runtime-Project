//! F2: orquestrador com dois pools, testado contra o mock do NIM.
//!
//! Os limites aqui são bem maiores que os 40/min reais (ex.: 600/min =
//! uma ficha a cada 100 ms) só para os testes rodarem rápido. A lógica é
//! a mesma.

use std::sync::Arc;
use std::time::{Duration, Instant};

use abiyss::config::{Config, config_de_teste};
use abiyss::db::Banco;
use abiyss::nim::mock::{MockNim, RespostaMock};
use abiyss::nim::{self, Mensagem, PedidoChat};
use abiyss::orquestrador::{Nivel, Origem, Orquestrador};

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
    let o = orquestrador(&c);

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
    for intervalo in intervalos(&mock) {
        assert!(
            intervalo >= Duration::from_millis(90),
            "requisições próximas demais: {intervalo:?}"
        );
    }
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
    let o = orquestrador(&c);
    mock.enfileirar(RespostaMock::erro(429, Some("1")));

    let mut tarefas = Vec::new();
    for i in 0..3 {
        let o = o.clone();
        let p = pedido(&c, Some(Nivel::Low), &format!("{i}"));
        tarefas.push(tokio::spawn(async move {
            o.subagentes.chamar(Nivel::Low, &p, None).await.unwrap()
        }));
    }
    for t in tarefas {
        t.await.unwrap();
    }

    let recebidas = mock.requisicoes();
    let momento_429 = recebidas[0].momento;
    // Todas as outras chegaram depois do bloqueio de 1 s.
    for r in &recebidas[1..] {
        assert!(
            r.momento - momento_429 >= Duration::from_millis(950),
            "uma requisição furou o bloqueio do 429"
        );
    }
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
    c.pools.subagentes.requisicoes_por_minuto = 200; // 1 ficha a cada 300 ms
    c.pools.subagentes.concorrencia.low = 5;
    let o = orquestrador(&c);

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
    // O Ultra chega DEPOIS dos três Low...
    tokio::time::sleep(Duration::from_millis(50)).await;
    let o2 = o.clone();
    let p = pedido(&c, Some(Nivel::Ultra), "ultra");
    tarefas.push(tokio::spawn(async move {
        o2.subagentes.chamar(Nivel::Ultra, &p, None).await.unwrap()
    }));
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
    mock.definir_roteiro(|_| RespostaMock::texto("ok").atrasada(Duration::from_millis(150)));

    let inicio = Instant::now();
    let mut tarefas = Vec::new();
    for i in 0..6 {
        let o = o.clone();
        let p = pedido(&c, Some(Nivel::Low), &format!("{i}"));
        tarefas.push(tokio::spawn(async move {
            o.subagentes.chamar(Nivel::Low, &p, None).await.unwrap()
        }));
    }
    for t in tarefas {
        t.await.unwrap();
    }

    assert_eq!(mock.pico_concorrencia(), 2);
    // 6 chamadas, 2 por vez, 150 ms cada = pelo menos 3 rodadas.
    assert!(inicio.elapsed() >= Duration::from_millis(440));
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

    // O cérebro continua respondendo na hora.
    let inicio = Instant::now();
    o.cerebro
        .chamar(Origem::Conversa, &pedido(&c, None, "c"), None)
        .await
        .unwrap();
    assert!(inicio.elapsed() < Duration::from_millis(500));
    assert!(!preso.is_finished(), "o sub-agente deveria estar esperando");
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
    // Total: 1 ficha a cada 100 ms. Autônomo: no máximo 1 a cada 200 ms.
    c.pools.cerebro.requisicoes_por_minuto = 600;
    c.pools.cerebro.reserva_conversa_por_minuto = 300;
    let o = orquestrador(&c);

    // Muitas chamadas autônomas ao mesmo tempo...
    let mut tarefas = Vec::new();
    for i in 0..4 {
        let o = o.clone();
        let p = pedido(&c, None, &format!("auto {i}"));
        tarefas.push(tokio::spawn(async move {
            o.cerebro.chamar(Origem::Autonomo, &p, None).await.unwrap()
        }));
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    // ...e o usuário manda uma mensagem.
    let inicio = Instant::now();
    o.cerebro
        .chamar(Origem::Conversa, &pedido(&c, None, "usuario"), None)
        .await
        .unwrap();
    // A conversa não fica atrás da fila autônoma: sai na próxima ficha livre.
    assert!(
        inicio.elapsed() < Duration::from_millis(250),
        "conversa esperou {:?}",
        inicio.elapsed()
    );
    for t in tarefas {
        t.await.unwrap();
    }

    // As autônomas nunca saíram mais rápido que 1 a cada 200 ms.
    let autonomas: Vec<Instant> = mock
        .requisicoes()
        .iter()
        .filter(|r| {
            r.corpo["messages"][0]["content"]
                .as_str()
                .unwrap()
                .starts_with("auto")
        })
        .map(|r| r.momento)
        .collect();
    for par in autonomas.windows(2) {
        assert!(par[1] - par[0] >= Duration::from_millis(185));
    }
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
    let chat = Orquestrador::novo(&c, Banco::abrir(&caminho).unwrap(), "k1", "k2").unwrap();
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
    for intervalo in intervalos(&mock) {
        assert!(intervalo >= Duration::from_millis(90), "{intervalo:?}");
    }
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
