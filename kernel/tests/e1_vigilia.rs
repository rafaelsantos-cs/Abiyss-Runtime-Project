//! E1: o loop do daemon nunca trava — crons e sinal de vida continuam
//! andando durante um ciclo longo; ciclo que passa do tempo máximo é
//! interrompido, registrado, e os eventos continuam pendentes.

mod comum;

use std::time::Duration;

use abiyss::daemon::{self, Daemon, OpcoesDaemon};
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
