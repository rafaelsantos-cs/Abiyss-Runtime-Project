//! Infra 4: runtime limitado — fila dos pools, tamanho das respostas do NIM,
//! resultados MCP e supervisão dos processos filhos MCP.
//!
//! Os testes de supervisão precisam do `uv` (como os da F5); sem ele, são pulados.

mod comum;

use std::path::PathBuf;
use std::time::Duration;

use abiyss::config::Config;
use abiyss::mcp::{ConfigServidorMcp, PonteMcp};
use abiyss::nim::cliente::LimitesCliente;
use abiyss::nim::mock::{MockNim, RespostaMock};
use abiyss::nim::{self, ClienteNim, EventoStream, Mensagem};
use abiyss::orquestrador::{Nivel, Orquestrador};
use abiyss::processos;
use comum::Ambiente;
use serde_json::json;

#[tokio::test]
async fn fila_do_pool_cheia_recusa_na_hora() {
    let mut amb = Ambiente::novo().await;
    // 1 ficha por minuto: a 1ª chamada passa, a 2ª fica esperando na fila
    // (que só tem 1 lugar) e a 3ª é recusada sem esperar.
    amb.config.pools.subagentes.requisicoes_por_minuto = 1;
    amb.config.pools.subagentes.rajada = 1;
    amb.config.pools.subagentes.max_na_fila = 1;
    amb.config.pools.subagentes.concorrencia.low = 5;
    let o = Orquestrador::novo(&amb.config, amb.banco.clone(), "a", "b").unwrap();
    let pedido = nim::montar_pedido(
        &amb.config.modelos.sub_low,
        vec![Mensagem::usuario("oi")],
        vec![],
    );

    o.subagentes
        .chamar(Nivel::Low, &pedido, None)
        .await
        .unwrap();
    let o2 = o.clone();
    let p2 = pedido.clone();
    let esperando = tokio::spawn(async move { o2.subagentes.chamar(Nivel::Low, &p2, None).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!esperando.is_finished());

    let inicio = std::time::Instant::now();
    let erro = o
        .subagentes
        .chamar(Nivel::Low, &pedido, None)
        .await
        .unwrap_err();
    assert!(inicio.elapsed() < Duration::from_secs(1));
    assert!(
        format!("{erro:#}").contains("fila do pool cheia"),
        "{erro:#}"
    );
    esperando.abort();
}

#[tokio::test]
async fn resposta_do_nim_maior_que_o_limite_falha() {
    let mock = MockNim::iniciar().await;
    let limites = LimitesCliente {
        max_bytes_resposta: 64 * 1024,
        max_conexoes_ociosas: 1,
    };
    let cliente = ClienteNim::novo_com_limites(
        &mock.base_url(),
        "k",
        Duration::from_secs(5),
        Duration::from_secs(5),
        limites,
    )
    .unwrap();
    let pedido = nim::montar_pedido(
        &abiyss::config::config_de_teste(&mock.base_url(), std::path::Path::new("."))
            .modelos
            .cerebro,
        vec![Mensagem::usuario("oi")],
        vec![],
    );
    let grande = "a".repeat(100 * 1024);

    mock.enfileirar(RespostaMock::texto(grande.clone()));
    let erro = cliente.completar(&pedido).await.unwrap_err();
    assert!(erro.to_string().contains("max_bytes_resposta"), "{erro}");

    mock.enfileirar(RespostaMock::texto(grande));
    let mut ignorar = |_: EventoStream| {};
    let erro = cliente
        .completar_stream(&pedido, &mut ignorar)
        .await
        .unwrap_err();
    assert!(erro.to_string().contains("max_bytes_resposta"), "{erro}");

    // Respostas normais continuam passando.
    mock.enfileirar(RespostaMock::texto("ok"));
    assert!(cliente.completar(&pedido).await.is_ok());
}

// ---------------------------------------------------------------------------
// Supervisão MCP
// ---------------------------------------------------------------------------

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

fn servidor_exemplo(timeout_segundos: u64) -> ConfigServidorMcp {
    let pasta = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../recursos/mcp/exemplo");
    ConfigServidorMcp {
        nome: "exemplo".into(),
        comando: "uv".into(),
        args: vec![
            "run".into(),
            "--frozen".into(),
            "--quiet".into(),
            "servidor.py".into(),
        ],
        diretorio: Some(pasta.to_string_lossy().into_owned()),
        env: Default::default(),
        timeout_segundos,
        ativo: true,
        url: None,
        token_env: None,
        expor: true,
    }
}

fn config_mcp(base: &Config, ajustar: impl FnOnce(&mut Config)) -> Config {
    let mut c = base.clone();
    c.mcp.servidores = vec![servidor_exemplo(30)];
    ajustar(&mut c);
    c
}

async fn somar_funciona(ponte: &PonteMcp) {
    let soma = ponte
        .chamar("exemplo__somar", json!({"numeros": [2, 3]}))
        .await
        .unwrap();
    assert_eq!(soma.trim(), "5.0");
}

/// O processo morreu (ou virou zumbi, sem memória)?
async fn esperar_morrer(pid: u32) {
    for _ in 0..50 {
        if processos::rss_da_arvore_bytes(pid).is_none() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("processo {pid} continua vivo");
}

#[tokio::test]
async fn servidor_que_cai_e_reiniciado() {
    if !uv_disponivel() {
        return;
    }
    let amb = Ambiente::novo().await;
    let ponte = PonteMcp::iniciar(&config_mcp(&amb.config, |_| {})).await;
    somar_funciona(&ponte).await;
    // Saudável: nada a fazer.
    assert!(ponte.verificar().await.is_empty());

    let pid = ponte.pid("exemplo").await.unwrap();
    processos::matar_grupo(pid);
    esperar_morrer(pid).await;
    // O transporte percebe a queda em seguida.
    let mut reiniciados = Vec::new();
    for _ in 0..50 {
        reiniciados = ponte.verificar().await;
        if !reiniciados.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(reiniciados, vec!["exemplo"]);
    assert_eq!(ponte.reinicios("exemplo"), Some(1));
    assert_ne!(ponte.pid("exemplo").await, Some(pid));
    somar_funciona(&ponte).await;

    // Encerrar não deixa processo para trás (nem o Python, neto do uv).
    let pid = ponte.pid("exemplo").await.unwrap();
    ponte.encerrar().await;
    esperar_morrer(pid).await;
}

#[tokio::test]
async fn servidor_acima_do_limite_de_memoria_e_reiniciado() {
    if !uv_disponivel() {
        return;
    }
    let amb = Ambiente::novo().await;
    // 1 MiB: qualquer Python passa disso.
    let ponte = PonteMcp::iniciar(&config_mcp(&amb.config, |c| c.mcp.max_memoria_mb = 1)).await;
    let pid = ponte.pid("exemplo").await.unwrap();
    let rss = processos::rss_da_arvore_bytes(pid).unwrap();
    assert!(rss > 1024 * 1024, "árvore com {rss} bytes");

    assert_eq!(ponte.verificar().await, vec!["exemplo"]);
    esperar_morrer(pid).await;
    somar_funciona(&ponte).await;
    ponte.encerrar().await;
}

#[tokio::test]
async fn servidor_velho_demais_ou_travado_e_reiniciado() {
    if !uv_disponivel() {
        return;
    }
    let amb = Ambiente::novo().await;
    let ponte = PonteMcp::iniciar(&config_mcp(&amb.config, |c| c.mcp.max_vida_segundos = 1)).await;
    assert!(ponte.verificar().await.is_empty());
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    assert_eq!(ponte.verificar().await, vec!["exemplo"]);
    ponte.encerrar().await;

    // Chamada que estoura o tempo limite (0 s = estoura sempre) pede reinício.
    let ponte = PonteMcp::iniciar(&config_mcp(&amb.config, |c| {
        c.mcp.servidores = vec![servidor_exemplo(0)]
    }))
    .await;
    let erro = ponte
        .chamar("exemplo__somar", json!({"numeros": [1]}))
        .await
        .unwrap_err();
    assert!(erro.to_string().contains("tempo limite"), "{erro}");
    assert_eq!(ponte.verificar().await, vec!["exemplo"]);
    // Depois do reinício, nada pendente.
    assert!(ponte.verificar().await.is_empty());
    ponte.encerrar().await;
}

#[tokio::test]
async fn resultado_mcp_grande_e_cortado() {
    if !uv_disponivel() {
        return;
    }
    let amb = Ambiente::novo().await;
    let ponte = PonteMcp::iniciar(&config_mcp(&amb.config, |c| {
        c.mcp.max_bytes_resultado = 100
    }))
    .await;
    let texto = ponte
        .chamar(
            "exemplo__remover_acentos",
            json!({"texto": "ação ".repeat(100)}),
        )
        .await
        .unwrap();
    assert!(texto.contains("resultado cortado"), "{texto}");
    assert!(texto.len() < 300);
    ponte.encerrar().await;
}
