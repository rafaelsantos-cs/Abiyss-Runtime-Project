//! Infra 6: o roteiro do teste de resistência (`mock-nim --roteiro
//! resistencia`) faz o daemon percorrer o caminho completo — heartbeat
//! decide, delega um sub-agente, o sub-agente devolve relatório.
//! (A execução longa em si é `tools/resistencia.sh`; ver docs/RESISTENCIA.md.)

mod comum;

use std::time::Duration;

use abiyss::daemon::{Daemon, OpcoesDaemon};
use abiyss::goals::{self, NovoGoal};
use abiyss::heartbeat;
use abiyss::nim::mock::roteiro_resistencia;
use abiyss::subagentes::{self, EstadoSubagente};
use comum::Ambiente;

#[tokio::test]
async fn roteiro_de_resistencia_percorre_heartbeat_e_subagente() {
    let amb = Ambiente::novo().await;
    amb.mock
        .definir_roteiro(roteiro_resistencia(Duration::from_millis(10)));
    goals::criar(
        &amb.banco,
        &NovoGoal {
            titulo: "Resistência".into(),
            nucleo: "Manter o Abiyss estável por horas.".into(),
            descricao: String::new(),
            prioridade: 0,
        },
        "usuario",
    )
    .unwrap();

    let daemon = Daemon::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
        amb.ferramentas.clone(),
    );
    daemon.rodar(&OpcoesDaemon { uma_vez: true }).await.unwrap();

    // O 1º ciclo chamou o modelo, entendeu a decisão e delegou (sem erro).
    let ciclo = heartbeat::ultimo_ciclo(&amb.banco, true).unwrap().unwrap();
    assert!(ciclo.erro.is_none(), "{:?}", ciclo.erro);
    assert!(
        ciclo.resultado.as_deref().unwrap_or("").contains("ok"),
        "{:?}",
        ciclo.resultado
    );
    // O sub-agente rodou até o fim com o relatório do roteiro.
    let sub = subagentes::obter(&amb.banco, 1)
        .unwrap()
        .expect("sub-agente #1");
    assert_eq!(sub.estado, EstadoSubagente::Concluido);
    assert_eq!(sub.relatorio.unwrap().status, "concluido");
    let modelos: Vec<String> = amb
        .mock
        .requisicoes()
        .iter()
        .map(|r| r.corpo["model"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(modelos, vec!["teste/cerebro", "teste/low"]);
}
