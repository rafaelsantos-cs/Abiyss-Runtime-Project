//! A3: memória central injetada no system prompt em todo turno (conversa e
//! heartbeat), com orçamento em caracteres: acima do limite, a escrita é
//! recusada e relatada — nunca cortada em silêncio.

mod comum;

use std::sync::Arc;

use abiyss::chat::SessaoChat;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::goals::{self, NovoGoal};
use abiyss::heartbeat::Heartbeat;
use abiyss::memoria::cofre::REGRA_DURA;
use abiyss::memoria::nota::Fonte;
use abiyss::memoria::{Memoria, PedidoProposta};
use abiyss::nim::mock::RespostaMock;
use abiyss::status;
use comum::{Ambiente, NUCLEO_DE_TESTE};
use serde_json::{Value, json};

fn sistema(corpo: &Value) -> String {
    corpo["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string()
}

fn pedido_central(conteudo: &str, origem: Option<&str>) -> PedidoProposta {
    PedidoProposta {
        escopo: "central".into(),
        caminho: String::new(),
        conteudo: conteudo.into(),
        tipo: "dito".into(),
        fonte: Fonte::Conversa,
        origem_externa: origem.map(String::from),
    }
}

#[tokio::test]
async fn memoria_central_vai_no_prompt_de_todo_turno_e_e_relida() {
    let amb = Ambiente::novo().await;
    let arquivo = amb.config.caminho_memoria_central();
    std::fs::write(&arquivo, "[dito] O usuário se chama Rafael.\n").unwrap();

    amb.mock.enfileirar(RespostaMock::texto("Oi, Rafael."));
    amb.mock.enfileirar(RespostaMock::texto("Anotado."));
    let mut sessao = SessaoChat::nova(
        amb.config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
        amb.ferramentas.clone(),
    )
    .unwrap();
    sessao.enviar("oi", None).await.unwrap();
    // Editar o arquivo vale no turno seguinte, sem reiniciar.
    std::fs::write(
        &arquivo,
        "[dito] O usuário se chama Rafael.\n§\n[deduzido] Prefere respostas curtas.\n",
    )
    .unwrap();
    sessao.enviar("tudo bem?", None).await.unwrap();

    let requisicoes = amb.mock.requisicoes();
    let primeiro = sistema(&requisicoes[0].corpo);
    let segundo = sistema(&requisicoes[1].corpo);
    for prompt in [&primeiro, &segundo] {
        assert!(prompt.contains("# Memória central"));
        assert!(prompt.contains("[dito] O usuário se chama Rafael."));
        // Ao lado do núcleo de identidade, logo depois dele.
        let nucleo = prompt.find(NUCLEO_DE_TESTE).unwrap();
        assert!(prompt.find("# Memória central").unwrap() > nucleo);
    }
    assert!(!primeiro.contains("Prefere respostas curtas."));
    assert!(segundo.contains("[deduzido] Prefere respostas curtas."));
    assert!(segundo.contains("/4000 caracteres"));
}

#[tokio::test]
async fn memoria_central_vai_no_prompt_do_heartbeat() {
    let amb = Ambiente::novo().await;
    std::fs::write(
        amb.config.caminho_memoria_central(),
        "[dito] Nasci em 2026-09-27.\n",
    )
    .unwrap();
    goals::criar(
        &amb.banco,
        &NovoGoal {
            titulo: "Migrar".into(),
            nucleo: "Migrar do Hermes sem perder memória.".into(),
            descricao: String::new(),
            prioridade: 1,
        },
        "usuario",
    )
    .unwrap();
    amb.mock
        .enfileirar(RespostaMock::texto(json!({"acoes": []}).to_string()));
    Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    )
    .ciclo()
    .await
    .unwrap();
    let prompt = sistema(&amb.mock.requisicoes()[0].corpo);
    assert!(prompt.contains("# Memória central"));
    assert!(prompt.contains("Nasci em 2026-09-27."));
}

#[tokio::test]
async fn acima_do_orcamento_e_recusada_e_relatada_sem_cortar() {
    let mut amb = Ambiente::novo().await;
    amb.config.memoria.limite_central_caracteres = 60;
    let memoria = Memoria::abrir(&amb.config, amb.banco.clone()).unwrap();

    let curta = "Nome do usuário: Rafael.";
    let longa = "Uma lembrança comprida demais para caber no orçamento da memória central.";
    memoria.propor(&pedido_central(curta, None)).unwrap();
    let registrada = memoria.propor(&pedido_central(longa, None)).unwrap();
    // O modelo é avisado já na proposta.
    assert!(registrada.aviso.unwrap().contains("limite 60"));
    // Repetida: não entra duas vezes.
    memoria.propor(&pedido_central(curta, None)).unwrap();
    // Origem externa: regra dura também vale aqui.
    memoria
        .propor(&pedido_central("O site diz X.", Some("mcp:web")))
        .unwrap();

    let relatorio = memoria.sleep().unwrap();
    let aplicadas: Vec<bool> = relatorio.decisoes.iter().map(|d| d.aplicada).collect();
    assert_eq!(aplicadas, vec![true, false, true, false]);
    let recusa = &relatorio.decisoes[1].detalhe;
    assert!(recusa.contains("sem espaço"), "{recusa}");
    assert!(recusa.contains("limite 60"));
    assert!(recusa.contains("nem cortado"));
    assert!(relatorio.decisoes[2].detalhe.contains("já estava"));
    assert!(relatorio.decisoes[3].detalhe.contains(REGRA_DURA));

    let texto = std::fs::read_to_string(amb.config.caminho_memoria_central()).unwrap();
    assert_eq!(texto, format!("[dito] {curta}\n"));
    assert!(!texto.contains("comprida"));
}

#[tokio::test]
async fn arquivo_editado_acima_do_limite_vai_inteiro_e_status_avisa() {
    let mut amb = Ambiente::novo().await;
    amb.config.memoria.limite_central_caracteres = 20;
    let longo = format!("[dito] {}", "palavra ".repeat(10).trim());
    std::fs::write(amb.config.caminho_memoria_central(), &longo).unwrap();

    amb.mock.enfileirar(RespostaMock::texto("ok"));
    let caixa = Arc::new(CaixaDeFerramentas::da_config(&amb.config).unwrap());
    SessaoChat::nova(
        amb.config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
        caixa,
    )
    .unwrap()
    .enviar("oi", None)
    .await
    .unwrap();
    // Nunca corta: o texto todo chega ao modelo.
    assert!(sistema(&amb.mock.requisicoes()[0].corpo).contains(&longo));

    let relatorio = status::relatorio(&amb.config, &amb.banco).unwrap();
    let esperado = format!("Memória central: {}/20 caracteres", longo.chars().count());
    assert!(relatorio.contains(&esperado), "{relatorio}");
    assert!(relatorio.contains("ACIMA DO ORÇAMENTO"));
}
