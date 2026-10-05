//! F3: `abiyss chat` — identidade no system prompt e histórico no SQLite.

mod comum;

use abiyss::chat::SessaoChat;
use abiyss::historico;
use abiyss::nim::mock::RespostaMock;
use comum::{Ambiente, NUCLEO_DE_TESTE};

fn sessao(amb: &Ambiente) -> SessaoChat {
    SessaoChat::nova(
        amb.config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
    )
    .unwrap()
}

#[tokio::test]
async fn system_prompt_tem_regras_do_kernel_e_nucleo() {
    let amb = Ambiente::novo().await;
    let mut s = sessao(&amb);
    let r = s.enviar("quem é você?", None).await.unwrap();
    assert_eq!(r.texto, "mock: quem é você?");

    let corpo = &amb.mock.requisicoes()[0].corpo;
    let sistema = corpo["messages"][0]["content"].as_str().unwrap();
    assert_eq!(corpo["messages"][0]["role"], "system");
    assert!(sistema.contains("Seu nome é Abiyss"));
    assert!(sistema.contains("Hermes"));
    assert!(sistema.contains(NUCLEO_DE_TESTE));
    assert_eq!(corpo["model"], "teste/cerebro");
}

#[tokio::test]
async fn historico_vai_no_contexto_e_sobrevive_entre_sessoes() {
    let amb = Ambiente::novo().await;
    let mut s = sessao(&amb);
    amb.mock.enfileirar(RespostaMock::texto("Prazer, Rafael."));
    s.enviar("meu nome é Rafael", None).await.unwrap();
    s.enviar("qual é o meu nome?", None).await.unwrap();

    let segunda = &amb.mock.requisicoes()[1].corpo["messages"];
    let textos: Vec<&str> = segunda
        .as_array()
        .unwrap()
        .iter()
        .skip(1) // pula o system
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    assert_eq!(
        textos,
        vec!["meu nome é Rafael", "Prazer, Rafael.", "qual é o meu nome?"]
    );

    // Uma sessão nova retomando a mesma conversa enxerga tudo.
    let id = s.conversa;
    drop(s);
    let mut retomada = SessaoChat::retomar(
        amb.config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
        id,
    )
    .unwrap();
    retomada.enviar("e agora?", None).await.unwrap();
    let terceira = &amb.mock.requisicoes()[2].corpo["messages"];
    assert_eq!(terceira.as_array().unwrap().len(), 1 + 5);

    assert_eq!(historico::ultima_conversa(&amb.banco).unwrap(), Some(id));
}

#[tokio::test]
async fn limite_do_historico_e_respeitado() {
    let mut amb = Ambiente::novo().await;
    amb.config.chat.historico_max_mensagens = 3;
    let mut s = sessao(&amb);
    for i in 0..4 {
        s.enviar(&format!("msg {i}"), None).await.unwrap();
    }
    let ultima = &amb.mock.requisicoes()[3].corpo["messages"];
    let lista = ultima.as_array().unwrap();
    // system + no máximo 3 mensagens, começando por uma do usuário.
    assert!(lista.len() <= 4);
    assert_eq!(lista[1]["role"], "user");
    assert_eq!(lista.last().unwrap()["content"], "msg 3");
}

#[tokio::test]
async fn sem_nucleo_continua_funcionando() {
    let amb = Ambiente::novo().await;
    std::fs::remove_file(amb.caminho("identity/nucleo.md")).unwrap();
    let mut s = sessao(&amb);
    s.enviar("oi", None).await.unwrap();
    let sistema = amb.mock.requisicoes()[0].corpo["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(sistema.contains("Seu nome é Abiyss"));
    assert!(!sistema.contains(NUCLEO_DE_TESTE));
}

#[tokio::test]
async fn erro_do_nim_nao_corrompe_o_historico() {
    let amb = Ambiente::novo().await;
    let mut s = sessao(&amb);
    amb.mock.enfileirar(RespostaMock::erro(400, None));
    assert!(s.enviar("primeira", None).await.is_err());
    // A próxima mensagem segue normalmente.
    let r = s.enviar("segunda", None).await.unwrap();
    assert_eq!(r.texto, "mock: segunda");
}

#[tokio::test]
async fn resposta_em_streaming_chega_em_pedacos() {
    let amb = Ambiente::novo().await;
    let mut s = sessao(&amb);
    let mut pedacos = Vec::new();
    let mut ao_receber = |e: abiyss::nim::EventoStream| {
        if let abiyss::nim::EventoStream::Texto(t) = e {
            pedacos.push(t);
        }
    };
    let r = s
        .enviar("uma mensagem comprida", Some(&mut ao_receber))
        .await
        .unwrap();
    assert!(pedacos.len() > 1);
    assert_eq!(pedacos.concat(), r.texto);
}
