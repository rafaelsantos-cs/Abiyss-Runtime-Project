//! F1: cliente do NIM contra o mock (nenhuma rede externa, nenhuma chave real).

use std::time::Duration;

use abiyss::nim::mock::{ChamadaMock, MockNim, RespostaMock};
use abiyss::nim::{ClienteNim, ErroNim, EventoStream, Ferramenta, Mensagem, PedidoChat};
use serde_json::{Map, json};

fn cliente(mock: &MockNim) -> ClienteNim {
    ClienteNim::novo(
        &mock.base_url(),
        "nvapi-teste",
        Duration::from_secs(5),
        Duration::from_secs(5),
    )
    .unwrap()
}

fn pedido(texto: &str) -> PedidoChat {
    let mut extra = Map::new();
    extra.insert(
        "chat_template_kwargs".into(),
        json!({"enable_thinking": true, "clear_thinking": false}),
    );
    PedidoChat {
        model: "teste/modelo".into(),
        messages: vec![Mensagem::usuario(texto)],
        tools: vec![Ferramenta::nova(
            "somar",
            "Soma dois números",
            json!({"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}}),
        )],
        tool_choice: Some(json!("auto")),
        stream: false,
        stream_options: None,
        max_tokens: Some(100),
        temperature: None,
        top_p: None,
        extra,
    }
}

#[tokio::test]
async fn chamada_sem_stream_envia_chave_modelo_extra_e_tools() {
    let mock = MockNim::iniciar().await;
    let r = cliente(&mock).completar(&pedido("olá")).await.unwrap();

    assert_eq!(r.mensagem.texto(), "mock: olá");
    assert_eq!(r.motivo_fim.as_deref(), Some("stop"));
    assert!(r.uso.total_tokens > 0);

    let recebida = &mock.requisicoes()[0];
    assert_eq!(recebida.autorizacao.as_deref(), Some("Bearer nvapi-teste"));
    assert_eq!(recebida.corpo["model"], "teste/modelo");
    assert_eq!(recebida.corpo["stream"], false);
    assert_eq!(
        recebida.corpo["chat_template_kwargs"]["enable_thinking"],
        true
    );
    assert_eq!(recebida.corpo["tools"][0]["function"]["name"], "somar");
}

#[tokio::test]
async fn stream_entrega_pedacos_e_monta_resposta() {
    let mock = MockNim::iniciar().await;
    mock.enfileirar(RespostaMock::TextoComRaciocinio {
        raciocinio: "vou cumprimentar".into(),
        texto: "Olá, eu sou o Abiyss.".into(),
    });

    let mut textos = Vec::new();
    let mut raciocinios = Vec::new();
    let mut ao_receber = |e: EventoStream| match e {
        EventoStream::Texto(t) => textos.push(t),
        EventoStream::Raciocinio(r) => raciocinios.push(r),
        EventoStream::InicioFerramenta(_) => {}
    };
    let r = cliente(&mock)
        .completar_stream(&pedido("oi"), &mut ao_receber)
        .await
        .unwrap();

    assert!(textos.len() > 1, "o texto deveria chegar em vários pedaços");
    assert_eq!(textos.concat(), "Olá, eu sou o Abiyss.");
    assert_eq!(raciocinios.concat(), "vou cumprimentar");
    assert_eq!(r.mensagem.texto(), "Olá, eu sou o Abiyss.");
    assert!(r.uso.total_tokens > 0, "uso vem no último pedaço");

    let corpo = &mock.requisicoes()[0].corpo;
    assert_eq!(corpo["stream"], true);
    assert_eq!(corpo["stream_options"]["include_usage"], true);
}

#[tokio::test]
async fn tool_calling_com_e_sem_stream() {
    let mock = MockNim::iniciar().await;
    let chamadas = RespostaMock::Ferramentas(vec![
        ChamadaMock {
            nome: "somar".into(),
            argumentos: json!({"a": 2, "b": 3}),
        },
        ChamadaMock {
            nome: "ler_arquivo".into(),
            argumentos: json!({"caminho": "notas/ação.md"}),
        },
    ]);
    mock.enfileirar(chamadas.clone());
    mock.enfileirar(chamadas);

    let c = cliente(&mock);
    let sem_stream = c.completar(&pedido("some")).await.unwrap();
    let mut nada = |_e: EventoStream| {};
    let com_stream = c
        .completar_stream(&pedido("some"), &mut nada)
        .await
        .unwrap();

    for r in [sem_stream, com_stream] {
        assert_eq!(r.motivo_fim.as_deref(), Some("tool_calls"));
        let lista = r.mensagem.chamadas();
        assert_eq!(lista.len(), 2);
        assert_eq!(lista[0].function.name, "somar");
        let args: serde_json::Value = serde_json::from_str(&lista[0].function.arguments).unwrap();
        assert_eq!(args, json!({"a": 2, "b": 3}));
        let args: serde_json::Value = serde_json::from_str(&lista[1].function.arguments).unwrap();
        assert_eq!(args["caminho"], "notas/ação.md");
        assert!(!lista[0].id.is_empty());
    }
}

#[tokio::test]
async fn erro_429_traz_retry_after() {
    let mock = MockNim::iniciar().await;
    mock.enfileirar(RespostaMock::erro(429, Some("7")));
    let erro = cliente(&mock).completar(&pedido("x")).await.unwrap_err();
    assert!(erro.eh_limite_de_taxa());
    assert!(erro.eh_retentavel());
    assert_eq!(erro.retry_after(), Some(Duration::from_secs(7)));
}

#[tokio::test]
async fn erro_400_nao_e_retentavel() {
    let mock = MockNim::iniciar().await;
    mock.enfileirar(RespostaMock::erro(400, None));
    let erro = cliente(&mock).completar(&pedido("x")).await.unwrap_err();
    assert!(matches!(erro, ErroNim::Http { status: 400, .. }));
    assert!(!erro.eh_retentavel());
}

#[tokio::test]
async fn servidor_fora_do_ar_e_erro_de_rede() {
    // Porta 9 (discard) em localhost: ninguém ouvindo.
    let c = ClienteNim::novo(
        "http://127.0.0.1:9/v1",
        "x",
        Duration::from_secs(2),
        Duration::from_secs(2),
    )
    .unwrap();
    let erro = c.completar(&pedido("x")).await.unwrap_err();
    assert!(matches!(erro, ErroNim::Rede(_)));
    assert!(erro.eh_retentavel());
}
