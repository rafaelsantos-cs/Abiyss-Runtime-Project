//! E1: o loop do daemon nunca trava — crons e sinal de vida continuam
//! andando durante um ciclo longo e durante um checkpoint do WAL que espera
//! outro leitor; ciclo que passa do tempo máximo é interrompido, registrado,
//! e os eventos continuam pendentes.

mod comum;

use std::time::Duration;

use abiyss::daemon::{self, Daemon, OpcoesDaemon};
use abiyss::db::Banco;
use abiyss::eventos;
use abiyss::heartbeat;
use abiyss::nim::mock::RespostaMock;
use comum::Ambiente;
use serde_json::json;

fn sinal_de_vida(amb: &Ambiente) -> i64 {
    daemon::ler_estado(&amb.banco, daemon::CHAVE_SINAL_DE_VIDA)
        .unwrap()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn decisao_vazia() -> RespostaMock {
    RespostaMock::texto(
        json!({"percepcao": "p", "orientacao": "o", "decisao": "d", "acoes": []}).to_string(),
    )
}

fn daemon_de(amb: &Ambiente) -> Daemon {
    Daemon::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
        amb.ferramentas.clone(),
    )
}

#[tokio::test]
async fn sinal_de_vida_anda_durante_um_ciclo_longo() {
    let mut amb = Ambiente::novo().await;
    amb.config.daemon.cron_verificacao_segundos = 1;
    // Só o primeiro ciclo (que roda ao subir) interessa aqui.
    amb.config.daemon.heartbeat_segundos = 3600;
    eventos::publicar(&amb.banco, "teste", "t", "acorde").unwrap();
    amb.mock
        .enfileirar(decisao_vazia().atrasada(Duration::from_millis(3500)));

    let d = daemon_de(&amb);
    let opcoes = OpcoesDaemon::default();
    let (parar, parada) = tokio::sync::oneshot::channel::<()>();
    let observar = async {
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let antes = sinal_de_vida(&amb);
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let depois = sinal_de_vida(&amb);
        // O modelo ainda está "pensando" (resposta atrasada 3,5 s)...
        let ciclos_terminados = heartbeat::ultimo_ciclo(&amb.banco, false).unwrap();
        let _ = parar.send(());
        (antes, depois, ciclos_terminados)
    };
    let (resultado, (antes, depois, ciclos)) = tokio::join!(
        d.rodar_ate(&opcoes, async {
            let _ = parada.await;
        }),
        observar
    );
    resultado.unwrap();
    assert_eq!(
        amb.mock.total_requisicoes(),
        1,
        "o ciclo estava em andamento"
    );
    assert!(ciclos.is_none(), "o ciclo ainda não tinha terminado");
    assert!(antes > 0);
    // ...e mesmo assim o sinal de vida avançou (o loop não ficou parado).
    assert!(depois > antes, "sinal de vida parado: {antes} → {depois}");
    // A parada no meio do ciclo não consumiu o evento.
    assert_eq!(eventos::contar_pendentes(&amb.banco).unwrap(), 1);
    assert!(
        daemon::ler_estado(&amb.banco, daemon::CHAVE_PARADO)
            .unwrap()
            .is_some()
    );
}

/// O checkpoint `TRUNCATE` espera (até o busy_timeout de 5 s) quem ainda lê
/// o WAL, como um `abiyss chat` aberto. Ele roda logo ao subir; antes, rodava
/// no próprio loop e na conexão dele: o loop (e o sinal de vida) parava.
#[tokio::test(flavor = "multi_thread")]
async fn checkpoint_esperando_um_leitor_nao_segura_o_loop() {
    let mut amb = Ambiente::novo().await;
    amb.config.daemon.cron_verificacao_segundos = 1;
    amb.config.daemon.heartbeat_segundos = 3600;
    // Outro processo com uma leitura aberta no WAL...
    let leitor = rusqlite::Connection::open(amb.config.caminho_banco()).unwrap();
    leitor.execute_batch("BEGIN").unwrap();
    let _: i64 = leitor
        .query_row("SELECT count(*) FROM fila_eventos", [], |l| l.get(0))
        .unwrap();
    // ...e escrita depois dela: o checkpoint não pode reiniciar o WAL.
    eventos::publicar(&amb.banco, "teste", "t", "depois do leitor").unwrap();

    // Quem observa fica noutra thread e lê pela sua própria conexão: se o
    // loop parar a thread dele (ou a trava da conexão dele), não para junto.
    let caminho = amb.config.caminho_banco();
    let (parar, parada) = tokio::sync::oneshot::channel::<()>();
    let observador = std::thread::spawn(move || {
        let banco = Banco::abrir(&caminho).unwrap();
        let ler = || {
            daemon::ler_estado(&banco, daemon::CHAVE_SINAL_DE_VIDA)
                .unwrap()
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0)
        };
        // O checkpoint começa logo ao subir; antes, parava o loop por 5 s.
        std::thread::sleep(Duration::from_millis(2000));
        let antes = ler();
        std::thread::sleep(Duration::from_millis(2000));
        let depois = ler();
        let _ = parar.send(());
        (antes, depois)
    });
    daemon_de(&amb)
        .rodar_ate(&OpcoesDaemon::default(), async {
            let _ = parada.await;
        })
        .await
        .unwrap();
    let (antes, depois) = observador.join().unwrap();
    leitor.execute_batch("COMMIT").unwrap();
    assert!(antes > 0, "nenhum sinal de vida nos primeiros 2 s");
    assert!(depois > antes, "sinal de vida parado: {antes} → {depois}");
}

#[tokio::test]
async fn ciclo_que_passa_do_tempo_e_interrompido_e_registrado() {
    let mut amb = Ambiente::novo().await;
    amb.config.daemon.cron_verificacao_segundos = 1;
    amb.config.daemon.heartbeat_segundos = 3600;
    amb.config.daemon.max_duracao_ciclo_segundos = 1;
    eventos::publicar(&amb.banco, "teste", "t", "acorde").unwrap();
    amb.mock
        .enfileirar(decisao_vazia().atrasada(Duration::from_secs(4)));

    let d = daemon_de(&amb);
    let opcoes = OpcoesDaemon::default();
    let (parar, parada) = tokio::sync::oneshot::channel::<()>();
    let observar = async {
        tokio::time::sleep(Duration::from_millis(2000)).await;
        let _ = parar.send(());
    };
    let (resultado, ()) = tokio::join!(
        d.rodar_ate(&opcoes, async {
            let _ = parada.await;
        }),
        observar
    );
    resultado.unwrap();
    let ciclo = heartbeat::ultimo_ciclo(&amb.banco, false).unwrap().unwrap();
    assert!(
        ciclo
            .erro
            .as_deref()
            .unwrap_or("")
            .contains("tempo esgotado"),
        "{ciclo:?}"
    );
    // O evento não foi consumido: será visto no próximo ciclo.
    assert_eq!(eventos::contar_pendentes(&amb.banco).unwrap(), 1);
}
