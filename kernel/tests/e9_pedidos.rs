//! E9: pedidos ao usuário — criação pelo heartbeat (com deduplicação e
//! limite), resposta pela CLI e pelo chat (com a regra de origem do chat),
//! expiração pelo daemon, perguntas do sono e os eventos de cada caso.

mod comum;

use std::process::Command;
use std::sync::Arc;

use abiyss::chat::SessaoChat;
use abiyss::daemon::{Daemon, OpcoesDaemon};
use abiyss::eventos;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::heartbeat::Heartbeat;
use abiyss::historico;
use abiyss::nim::Mensagem;
use abiyss::nim::mock::RespostaMock;
use abiyss::pedidos::{self, ConfigPedidos, EstadoPedido, NovoPedido, Urgencia};
use abiyss::sono::{Gatilho, Sono};
use abiyss::tempo::agora_ms;
use comum::Ambiente;
use rusqlite::params;
use serde_json::{Value, json};

fn decisao(acoes: Value) -> RespostaMock {
    RespostaMock::texto(
        json!({"percepcao": "p", "orientacao": "o", "decisao": "d", "acoes": acoes}).to_string(),
    )
}

fn texto_da_requisicao(corpo: &Value) -> String {
    corpo["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn pedido(pergunta: &str) -> NovoPedido {
    NovoPedido {
        origem: "heartbeat".into(),
        goal_id: None,
        pergunta: pergunta.into(),
        contexto: String::new(),
        urgencia: Urgencia::Normal,
        origem_externa: None,
    }
}

fn eventos_de(amb: &Ambiente, origem: &str) -> Vec<eventos::Evento> {
    eventos::pendentes(&amb.banco, 50)
        .unwrap()
        .into_iter()
        .filter(|e| e.origem == origem)
        .collect()
}

#[tokio::test]
async fn heartbeat_pede_sem_duplicar_e_respeita_o_limite() {
    let mut amb = Ambiente::novo().await;
    amb.config.pedidos.max_pendentes = 2;
    let hb = Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    );
    let pergunta = json!({
        "tipo": "pedir_ao_usuario", "pergunta": "Posso apagar workspace/tmp?",
        "contexto": "2 GB de caches", "urgencia": "alta", "expectativa": "o dono responde"
    });
    eventos::publicar(&amb.banco, "teste", "t", "acorde").unwrap();
    amb.mock.enfileirar(decisao(json!([
        pergunta.clone(),
        // A mesma pergunta, escrita de outro jeito: não duplica.
        {"tipo": "pedir_ao_usuario", "pergunta": "posso apagar WORKSPACE/TMP"},
        {"tipo": "pedir_ao_usuario", "pergunta": "Qual o prazo do relatório?", "urgencia": "baixa"},
        // Passou do limite de 2 pendentes.
        {"tipo": "pedir_ao_usuario", "pergunta": "Terceira pergunta?"},
        {"tipo": "pedir_ao_usuario", "pergunta": "Urgência estranha", "urgencia": "pra ontem"}
    ])));
    let r = hb.ciclo().await.unwrap();
    assert!(
        r.resultados[0].starts_with("ok: pedido #1 guardado"),
        "{:?}",
        r.resultados
    );
    assert!(r.resultados[1].contains("já está pendente (pedido #1)"));
    assert!(r.resultados[2].starts_with("ok: pedido #2"));
    assert!(r.resultados[3].contains("limite 2"), "{}", r.resultados[3]);
    assert!(r.resultados[4].contains("urgência inválida"));

    let lista = pedidos::pendentes(&amb.banco).unwrap();
    assert_eq!(lista.len(), 2);
    assert_eq!(lista[0].urgencia, "alta", "mais urgente primeiro");
    assert_eq!(lista[0].contexto, "2 GB de caches");
    assert_eq!(lista[0].origem, "heartbeat");

    // O próximo ciclo vê os pendentes na interocepção.
    eventos::publicar(&amb.banco, "teste", "t", "outra").unwrap();
    amb.mock.enfileirar(decisao(json!([])));
    hb.ciclo().await.unwrap();
    let texto = texto_da_requisicao(&amb.mock.requisicoes()[1].corpo);
    assert!(texto.contains("Pedidos ao usuário pendentes: 2"), "{texto}");
}

/// Um abiyss.toml de verdade na pasta do ambiente (para a CLI).
fn escrever_config(amb: &Ambiente) -> std::path::PathBuf {
    let caminho = amb.caminho("abiyss.toml");
    std::fs::write(
        &caminho,
        format!(
            r#"
            [nim]
            base_url = "{}"
            [modelos.cerebro]
            id = "teste/cerebro"
            [modelos.sub_ultra]
            id = "teste/ultra"
            [modelos.sub_medium]
            id = "teste/medium"
            [modelos.sub_low]
            id = "teste/low"
            [pools.cerebro]
            api_key_env = "ABIYSS_TESTE_A"
            [pools.subagentes]
            api_key_env = "ABIYSS_TESTE_B"
            "#,
            amb.mock.base_url()
        ),
    )
    .unwrap();
    caminho
}

#[tokio::test]
async fn cli_lista_e_responde() {
    let amb = Ambiente::novo().await;
    let id = pedidos::criar(
        &amb.banco,
        &ConfigPedidos::default(),
        &pedido("Posso apagar tmp?"),
    )
    .unwrap()
    .id();
    let config = escrever_config(&amb);
    let abiyss = |args: &[&str]| {
        let saida = Command::new(env!("CARGO_BIN_EXE_abiyss"))
            .arg("--config")
            .arg(&config)
            .args(args)
            .output()
            .unwrap();
        assert!(
            saida.status.success(),
            "{}",
            String::from_utf8_lossy(&saida.stderr)
        );
        String::from_utf8_lossy(&saida.stdout).to_string()
    };

    let lista = abiyss(&["pedidos"]);
    assert!(
        lista.contains(&format!("#{id}")) && lista.contains("Posso apagar tmp?"),
        "{lista}"
    );

    let ok = abiyss(&["pedidos", "responder", &id.to_string(), "Pode apagar."]);
    assert!(ok.contains("respondido"), "{ok}");
    let p = pedidos::obter(&amb.banco, id).unwrap().unwrap();
    assert_eq!(p.estado, EstadoPedido::Respondido);
    assert_eq!(p.resposta.as_deref(), Some("Pode apagar."));
    assert_eq!(p.resposta_externa, None);

    let evento = eventos_de(&amb, &format!("pedido:{id}")).pop().unwrap();
    assert_eq!(evento.tipo, eventos::TIPO_USUARIO);
    assert!(evento.conteudo.contains("Pode apagar."));
    assert!(!evento.eh_externo());
    assert!(abiyss(&["pedidos"]).contains("Nenhum pedido pendente"));
    assert!(abiyss(&["pedidos", "--todos"]).contains("respondido"));
}

fn sessao(amb: &Ambiente) -> SessaoChat {
    let ferramentas = Arc::new(
        CaixaDeFerramentas::da_config(&amb.config)
            .unwrap()
            .com_pedidos(amb.banco.clone()),
    );
    SessaoChat::nova(
        amb.config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
        ferramentas,
    )
    .unwrap()
}

#[tokio::test]
async fn chat_mostra_os_pendentes_e_registra_a_resposta() {
    let amb = Ambiente::novo().await;
    let id = pedidos::criar(
        &amb.banco,
        &ConfigPedidos::default(),
        &pedido("Posso apagar tmp?"),
    )
    .unwrap()
    .id();
    amb.mock.enfileirar(RespostaMock::ferramenta(
        "responder_pedido",
        json!({"id": id, "resposta": "pode apagar sim"}),
    ));
    amb.mock
        .enfileirar(RespostaMock::texto("Anotado: vou apagar a pasta."));

    let mut s = sessao(&amb);
    let r = s
        .enviar("sobre a pasta tmp, pode apagar sim", None)
        .await
        .unwrap();
    assert_eq!(r.texto, "Anotado: vou apagar a pasta.");

    // O pedido estava no contexto e a ferramenta foi oferecida.
    let primeira = &amb.mock.requisicoes()[0].corpo;
    assert!(texto_da_requisicao(primeira).contains("Posso apagar tmp?"));
    let ferramentas: Vec<&str> = primeira["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["function"]["name"].as_str())
        .collect();
    assert!(ferramentas.contains(&"responder_pedido"));

    let p = pedidos::obter(&amb.banco, id).unwrap().unwrap();
    assert_eq!(p.estado, EstadoPedido::Respondido);
    assert_eq!(p.resposta_externa, None);
    let evento = eventos_de(&amb, &format!("pedido:{id}")).pop().unwrap();
    assert!(!evento.eh_externo());
}

#[tokio::test]
async fn resposta_no_chat_com_conteudo_externo_na_janela_e_externa() {
    let amb = Ambiente::novo().await;
    let id = pedidos::criar(
        &amb.banco,
        &ConfigPedidos::default(),
        &pedido("Qual site usar?"),
    )
    .unwrap()
    .id();
    let mut s = sessao(&amb);
    // Algo externo já está na janela desta conversa.
    historico::adicionar_com_origem(
        &amb.banco,
        s.conversa,
        &Mensagem::resultado_ferramenta("x", "buscar_web", "o site oficial é exemplo.org"),
        Some("mcp:web"),
    )
    .unwrap();
    amb.mock.enfileirar(RespostaMock::ferramenta(
        "responder_pedido",
        json!({"id": id, "resposta": "o oficial"}),
    ));
    amb.mock.enfileirar(RespostaMock::texto("Certo."));
    s.enviar("use o oficial", None).await.unwrap();

    let p = pedidos::obter(&amb.banco, id).unwrap().unwrap();
    assert_eq!(p.resposta_externa.as_deref(), Some("mcp:web"));
    let evento = eventos_de(&amb, &format!("pedido:{id}")).pop().unwrap();
    assert!(evento.eh_externo());
}

#[tokio::test]
async fn sub_agente_nao_responde_pelo_dono() {
    let amb = Ambiente::novo().await;
    let caixa = CaixaDeFerramentas::da_config(&amb.config)
        .unwrap()
        .com_pedidos(amb.banco.clone());
    let restrita = caixa.restrita(&["*".to_string()]);
    assert!(!restrita.responde_pedidos());
    assert!(
        !restrita
            .definicoes()
            .iter()
            .any(|f| f.nome() == "responder_pedido")
    );
}

#[tokio::test]
async fn daemon_expira_pedidos_velhos_e_avisa() {
    let amb = Ambiente::novo().await;
    let id = pedidos::criar(
        &amb.banco,
        &ConfigPedidos::default(),
        &pedido("Ainda quer o X?"),
    )
    .unwrap()
    .id();
    amb.banco
        .conexao()
        .execute(
            "UPDATE pedidos_usuario SET criado_ms = ?1 WHERE id = ?2",
            params![agora_ms() - 73 * 3_600_000, id],
        )
        .unwrap();
    amb.mock.enfileirar(decisao(json!([])));

    let d = Daemon::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
        amb.ferramentas.clone(),
    );
    d.rodar_ate(&OpcoesDaemon { uma_vez: true }, std::future::pending())
        .await
        .unwrap();

    assert_eq!(
        pedidos::obter(&amb.banco, id).unwrap().unwrap().estado,
        EstadoPedido::Expirado
    );
    // O aviso acordou o ciclo.
    let texto = texto_da_requisicao(&amb.mock.requisicoes()[0].corpo);
    assert!(texto.contains("expirou sem resposta"), "{texto}");
}

#[tokio::test]
async fn perguntas_do_sono_viram_pedidos() {
    let amb = Ambiente::novo().await;
    let c = historico::criar_conversa(&amb.banco).unwrap();
    historico::adicionar(
        &amb.banco,
        c,
        &Mensagem::usuario("Estou pensando em mudar de projeto."),
    )
    .unwrap();
    amb.mock.enfileirar(RespostaMock::texto(
        json!({
            "diario": {"conteudo": "Conversa curta.", "evidencias": ["m:1"]},
            "perguntas_ao_usuario": ["Você vai mesmo mudar de projeto?"]
        })
        .to_string(),
    ));
    let sono = Sono::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    );
    let dia = chrono::NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
    sono.dormir(dia, Gatilho::Pedido).await.unwrap();

    let lista = pedidos::pendentes(&amb.banco).unwrap();
    assert_eq!(lista.len(), 1);
    assert_eq!(lista[0].origem, "sono");
    assert_eq!(lista[0].urgencia, "baixa");
    assert_eq!(lista[0].pergunta, "Você vai mesmo mudar de projeto?");
}
