//! F8: base da metacognição — diário (expectativa → resultado) e
//! interocepção no contexto. Só registro, sem escalonamento.

mod comum;

use abiyss::chat::SessaoChat;
use abiyss::diario::{self, SEM_EXPECTATIVA};
use abiyss::goals::{self, NovoGoal};
use abiyss::heartbeat::Heartbeat;
use abiyss::nim::mock::RespostaMock;
use abiyss::status;
use abiyss::subagentes::ExecutorSubagentes;
use comum::Ambiente;
use serde_json::json;

fn criar_goal(amb: &Ambiente) -> i64 {
    goals::criar(
        &amb.banco,
        &NovoGoal {
            titulo: "Organizar notas".into(),
            nucleo: "Notas organizadas por tema.".into(),
            descricao: String::new(),
            prioridade: 1,
        },
        "usuario",
    )
    .unwrap()
    .id
}

#[tokio::test]
async fn diario_guarda_expectativa_antes_e_resultado_depois() {
    let amb = Ambiente::novo().await;
    let goal = criar_goal(&amb);
    amb.mock.enfileirar(RespostaMock::texto(
        json!({"decisao": "comprometer", "acoes": [
            {"tipo": "transicionar_goal", "goal_id": goal, "para": "comprometido",
             "motivo": "faz sentido", "expectativa": "o goal passa a comprometido"},
            {"tipo": "transicionar_goal", "goal_id": goal, "para": "concluido", "motivo": "pressa"}
        ]})
        .to_string(),
    ));
    Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    )
    .ciclo()
    .await
    .unwrap();

    let mut entradas = diario::recentes(&amb.banco, 10).unwrap();
    entradas.reverse();
    assert_eq!(entradas.len(), 2);

    let primeira = &entradas[0];
    assert_eq!(primeira.origem, "heartbeat");
    assert_eq!(primeira.goal_id, Some(goal));
    assert_eq!(primeira.expectativa, "o goal passa a comprometido");
    assert_eq!(
        primeira.resultado.as_deref(),
        Some("ok: goal #1 agora está 'comprometido'")
    );
    assert!(primeira.resultado_ms.unwrap() >= primeira.momento_ms);

    // Sem expectativa declarada e com resultado de erro: tudo registrado.
    let segunda = &entradas[1];
    assert_eq!(segunda.expectativa, SEM_EXPECTATIVA);
    assert!(
        segunda
            .resultado
            .as_deref()
            .unwrap()
            .starts_with("erro: transição inválida")
    );
}

#[tokio::test]
async fn diario_completa_a_delegacao_com_o_relatorio_do_subagente() {
    let amb = Ambiente::novo().await;
    let goal = criar_goal(&amb);
    amb.mock.definir_roteiro(move |corpo| {
        if corpo["model"] == "teste/cerebro" {
            RespostaMock::texto(
                json!({"acoes": [{"tipo": "delegar", "nivel": "low", "tarefa": "liste as notas",
                    "goal_id": goal, "expectativa": "uma lista de notas em até 1 minuto"}]})
                .to_string(),
            )
        } else {
            RespostaMock::texto(
                json!({"status": "concluido", "resumo": "3 notas encontradas", "artefatos": [],
                       "confianca": 0.7, "duvidas": []})
                .to_string(),
            )
        }
    });
    let executor = ExecutorSubagentes::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
        amb.ferramentas.clone(),
    );
    Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    )
    .com_subagentes(executor.controle())
    .ciclo()
    .await
    .unwrap();

    let entrada = &diario::recentes(&amb.banco, 1).unwrap()[0];
    assert_eq!(entrada.subagente_id, Some(1));
    assert_eq!(entrada.expectativa, "uma lista de notas em até 1 minuto");
    assert_eq!(
        entrada.resultado.as_deref(),
        Some("ok: sub-agente 1 (low) delegado")
    );

    executor.executar_pendentes_e_esperar().await.unwrap();
    let entrada = &diario::recentes(&amb.banco, 1).unwrap()[0];
    let resultado = entrada.resultado.as_deref().unwrap();
    assert!(
        resultado.contains("sub-agente 1 terminou (concluido)"),
        "{resultado}"
    );
    assert!(resultado.contains("3 notas encontradas"));
}

#[tokio::test]
async fn interocepcao_e_data_hora_no_contexto_do_heartbeat() {
    let amb = Ambiente::novo().await;
    criar_goal(&amb);
    amb.mock.enfileirar(RespostaMock::texto(r#"{"acoes": []}"#));
    Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    )
    .ciclo()
    .await
    .unwrap();
    let contexto = amb.mock.requisicoes()[0].corpo["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_string();
    for trecho in [
        "## Agora e estado do corpo (medido pelo kernel)",
        "Data e hora: ",
        "CPU: carga",
        "Memória:",
        "Disco (dados):",
        "Pool cerebro: 0/",
        "Pool subagentes: 0/",
    ] {
        assert!(
            contexto.contains(trecho),
            "faltou '{trecho}' em:\n{contexto}"
        );
    }
    let sistema = amb.mock.requisicoes()[0].corpo["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(sistema.contains("\"expectativa\""));
}

#[tokio::test]
async fn interocepcao_no_chat_conta_os_tokens_gastos() {
    let amb = Ambiente::novo().await;
    let mut sessao = SessaoChat::nova(
        amb.config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
        amb.ferramentas.clone(),
    )
    .unwrap();
    sessao.enviar("primeira", None).await.unwrap();
    sessao.enviar("segunda", None).await.unwrap();

    let sistema = |i: usize| {
        amb.mock.requisicoes()[i].corpo["messages"][0]["content"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // Linha do pool do cérebro no bloco de interocepção.
    let linha_cerebro = |texto: &str| {
        texto
            .lines()
            .find(|l| l.starts_with("Pool cerebro:"))
            .unwrap()
            .to_string()
    };
    let antes = sistema(0);
    let depois = sistema(1);
    assert!(antes.contains("# Contexto atual (calculado pelo kernel)"));
    assert!(antes.contains("Data e hora: "));
    assert!(linha_cerebro(&antes).contains("tokens hoje 0 entrada + 0 saída"));
    // Depois da primeira chamada, os tokens gastos aparecem.
    let cerebro = linha_cerebro(&depois);
    assert!(cerebro.starts_with("Pool cerebro: 1/"), "{cerebro}");
    assert!(!cerebro.contains("tokens hoje 0 entrada"), "{cerebro}");
}

#[tokio::test]
async fn status_mostra_a_interocepcao() {
    let amb = Ambiente::novo().await;
    let texto = status::relatorio(&amb.config, &amb.banco).unwrap();
    assert!(texto.contains("Interocepção:"));
    assert!(texto.contains("Data e hora: "));
    assert!(texto.contains("Pool subagentes:"));
}
