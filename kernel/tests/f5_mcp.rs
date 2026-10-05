//! F5: ponte MCP com o servidor Python de exemplo.
//!
//! Estes testes precisam do `uv` instalado (o CI instala). Sem ele, são
//! pulados com um aviso, para não quebrar quem só quer mexer no kernel.

mod comum;

use std::path::PathBuf;
use std::sync::Arc;

use abiyss::chat::SessaoChat;
use abiyss::config::Config;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::mcp::{ConfigServidorMcp, PonteMcp};
use abiyss::nim::mock::RespostaMock;
use comum::Ambiente;
use serde_json::json;

fn uv_disponivel() -> bool {
    let ok = std::process::Command::new("uv")
        .arg("--version")
        .output()
        .map(|s| s.status.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("AVISO: 'uv' não encontrado; teste MCP pulado");
    }
    ok
}

fn pasta_exemplo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../recursos/mcp/exemplo")
}

fn servidor_exemplo() -> ConfigServidorMcp {
    ConfigServidorMcp {
        nome: "exemplo".into(),
        comando: "uv".into(),
        args: vec![
            "run".into(),
            "--frozen".into(),
            "--quiet".into(),
            "servidor.py".into(),
        ],
        diretorio: Some(pasta_exemplo().to_string_lossy().into_owned()),
        env: Default::default(),
        timeout_segundos: 30,
        ativo: true,
    }
}

fn com_servidores(config: &Config, servidores: Vec<ConfigServidorMcp>) -> Config {
    let mut c = config.clone();
    c.mcp.servidores = servidores;
    c
}

#[tokio::test]
async fn lista_e_chama_ferramentas_do_servidor_exemplo() {
    if !uv_disponivel() {
        return;
    }
    let amb = Ambiente::novo().await;
    let config = com_servidores(&amb.config, vec![servidor_exemplo()]);
    let ponte = PonteMcp::iniciar(&config).await;

    assert_eq!(ponte.servidores(), vec!["exemplo"]);
    let nomes: Vec<String> = ponte
        .definicoes()
        .iter()
        .map(|f| f.function.name.clone())
        .collect();
    for esperado in [
        "exemplo__contar_palavras",
        "exemplo__somar",
        "exemplo__remover_acentos",
        "exemplo__agora_utc",
    ] {
        assert!(
            nomes.contains(&esperado.to_string()),
            "faltou {esperado}: {nomes:?}"
        );
    }
    // O JSON Schema gerado pelo SDK Python chega ao modelo.
    let somar = ponte
        .definicoes()
        .into_iter()
        .find(|f| f.function.name == "exemplo__somar")
        .unwrap();
    assert_eq!(
        somar.function.parameters["properties"]["numeros"]["type"],
        "array"
    );

    let contagem = ponte
        .chamar(
            "exemplo__contar_palavras",
            json!({"texto": "olá mundo bonito"}),
        )
        .await
        .unwrap();
    let contagem: serde_json::Value = serde_json::from_str(&contagem).unwrap();
    assert_eq!(contagem["palavras"], 3);

    let soma = ponte
        .chamar("exemplo__somar", json!({"numeros": [1, 2.5, 3]}))
        .await
        .unwrap();
    assert_eq!(soma.trim(), "6.5");

    // Argumento errado vira erro (is_error), não pânico.
    assert!(
        ponte
            .chamar("exemplo__somar", json!({"numeros": "não é lista"}))
            .await
            .is_err()
    );
    assert!(
        ponte
            .chamar("exemplo__nao_existe", json!({}))
            .await
            .is_err()
    );
    ponte.encerrar().await;
}

#[tokio::test]
async fn modelo_usa_ferramenta_mcp_no_chat() {
    if !uv_disponivel() {
        return;
    }
    let amb = Ambiente::novo().await;
    let config = com_servidores(&amb.config, vec![servidor_exemplo()]);
    let ponte = Arc::new(PonteMcp::iniciar(&config).await);
    let caixa = Arc::new(
        CaixaDeFerramentas::da_config(&config)
            .unwrap()
            .com_mcp(ponte.clone()),
    );

    amb.mock.enfileirar(RespostaMock::ferramenta(
        "exemplo__remover_acentos",
        json!({"texto": "ação rápida"}),
    ));
    amb.mock
        .enfileirar(RespostaMock::texto("Sem acentos: acao rapida"));
    let mut sessao = SessaoChat::nova(
        config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
        caixa,
    )
    .unwrap();
    let r = sessao.enviar("tire os acentos", None).await.unwrap();
    assert_eq!(r.ferramentas_usadas, 1);

    let requisicoes = amb.mock.requisicoes();
    // A ferramenta MCP foi oferecida ao modelo...
    assert!(
        requisicoes[0].corpo["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["function"]["name"] == "exemplo__remover_acentos")
    );
    // ...e o resultado voltou rotulado como dado vindo de MCP.
    let resultado = requisicoes[1].corpo["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "tool")
        .unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(resultado.starts_with("<dados origem=\"mcp:exemplo__remover_acentos\">"));
    assert!(resultado.contains("acao rapida"));
}

#[tokio::test]
async fn servidor_que_nao_sobe_e_ignorado() {
    let amb = Ambiente::novo().await;
    let mut quebrado = servidor_exemplo();
    quebrado.nome = "quebrado".into();
    quebrado.comando = "comando-que-nao-existe-abiyss".into();
    let mut nome_ruim = servidor_exemplo();
    nome_ruim.nome = "nome com espaço".into();
    let config = com_servidores(&amb.config, vec![quebrado, nome_ruim]);

    let ponte = PonteMcp::iniciar(&config).await;
    assert!(ponte.servidores().is_empty());
    assert!(ponte.definicoes().is_empty());
}

#[tokio::test]
async fn servidor_recebe_ambiente_limpo() {
    if !uv_disponivel() {
        return;
    }
    let amb = Ambiente::novo().await;
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_eco_env.py");
    let mut servidor = servidor_exemplo();
    servidor.nome = "eco".into();
    servidor.args = vec![
        "run".into(),
        "--frozen".into(),
        "--quiet".into(),
        "python".into(),
        fixture.to_string_lossy().into_owned(),
    ];
    servidor
        .env
        .insert("ABIYSS_VARIAVEL_DO_SERVIDOR".into(), "ok".into());
    let config = com_servidores(&amb.config, vec![servidor]);
    let ponte = PonteMcp::iniciar(&config).await;
    assert_eq!(ponte.servidores(), vec!["eco"]);

    // O `cargo test` define CARGO_MANIFEST_DIR no nosso processo; o filho não deve ver.
    assert!(std::env::var("CARGO_MANIFEST_DIR").is_ok());
    let vazou = ponte
        .chamar("eco__ler_env", json!({"nome": "CARGO_MANIFEST_DIR"}))
        .await
        .unwrap();
    assert_eq!(vazou, "<ausente>");
    // A variável configurada para o servidor chega.
    let propria = ponte
        .chamar(
            "eco__ler_env",
            json!({"nome": "ABIYSS_VARIAVEL_DO_SERVIDOR"}),
        )
        .await
        .unwrap();
    assert_eq!(propria, "ok");
    // PATH passa (senão nem o python seria encontrado).
    let path = ponte
        .chamar("eco__ler_env", json!({"nome": "PATH"}))
        .await
        .unwrap();
    assert_ne!(path, "<ausente>");
    ponte.encerrar().await;
}
