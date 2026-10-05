//! F6: daemon, heartbeat e goals, contra o mock do NIM.

mod comum;

use abiyss::cron;
use abiyss::daemon::{self, Daemon, OpcoesDaemon};
use abiyss::eventos;
use abiyss::goals::{self, EstadoGoal, NovoGoal};
use abiyss::heartbeat::{self, Heartbeat};
use abiyss::nim::mock::RespostaMock;
use abiyss::status;
use comum::Ambiente;
use serde_json::json;

fn heartbeat(amb: &Ambiente) -> Heartbeat {
    Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    )
}

fn criar_goal(amb: &Ambiente, titulo: &str) -> goals::Goal {
    goals::criar(
        &amb.banco,
        &NovoGoal {
            titulo: titulo.into(),
            nucleo: format!("NÚCLEO-{titulo}: pronto quando houver um resumo salvo."),
            descricao: String::new(),
            prioridade: 1,
        },
        "usuario",
    )
    .unwrap()
}

fn decisao(acoes: serde_json::Value) -> RespostaMock {
    RespostaMock::texto(
        json!({
            "percepcao": "vi o goal",
            "orientacao": "dá para começar",
            "decisao": "seguir",
            "acoes": acoes
        })
        .to_string(),
    )
}

#[tokio::test]
async fn sem_goal_e_sem_evento_nao_chama_o_modelo() {
    let amb = Ambiente::novo().await;
    let r = heartbeat(&amb).ciclo().await.unwrap();
    assert!(!r.chamou_modelo);
    assert_eq!(amb.mock.total_requisicoes(), 0);
    let ciclo = heartbeat::ultimo_ciclo(&amb.banco, false).unwrap().unwrap();
    assert!(!ciclo.chamou_modelo);
}

#[tokio::test]
async fn ciclo_faz_uma_chamada_e_transiciona_o_goal() {
    let amb = Ambiente::novo().await;
    let g = criar_goal(&amb, "pesquisa");
    amb.mock.enfileirar(decisao(json!([
        {"tipo": "transicionar_goal", "goal_id": g.id, "para": "comprometido", "motivo": "parece viável"}
    ])));

    let r = heartbeat(&amb).ciclo().await.unwrap();
    assert!(r.chamou_modelo);
    assert_eq!(amb.mock.total_requisicoes(), 1, "UMA chamada por ciclo");
    assert_eq!(r.resultados, vec!["ok: goal #1 agora está 'comprometido'"]);

    // A transição foi registrada como evento, com motivo e autor.
    let historico = goals::eventos(&amb.banco, g.id).unwrap();
    assert_eq!(historico.last().unwrap().para, EstadoGoal::Comprometido);
    assert_eq!(historico.last().unwrap().motivo, "parece viável");
    assert_eq!(historico.last().unwrap().autor, "abiyss");

    // Contexto: núcleo do goal no início E no fim da mensagem.
    let corpo = &amb.mock.requisicoes()[0].corpo;
    let contexto = corpo["messages"][1]["content"].as_str().unwrap();
    assert!(contexto.starts_with("### NÚCLEO DO GOAL EM FOCO — #1"));
    let ultima_ocorrencia = contexto.rfind("NÚCLEO-pesquisa").unwrap();
    assert!(ultima_ocorrencia > contexto.find("## Eventos novos").unwrap());
    // Sem ferramentas: a decisão vem em JSON e o kernel executa.
    assert!(corpo.get("tools").is_none());
    let sistema = corpo["messages"][0]["content"].as_str().unwrap();
    assert!(sistema.contains("Modo heartbeat"));
    assert!(sistema.contains("Seu nome é Abiyss"));
}

#[tokio::test]
async fn transicao_invalida_e_recusada_e_relatada_no_proximo_ciclo() {
    let amb = Ambiente::novo().await;
    let g = criar_goal(&amb, "x");
    amb.mock.enfileirar(decisao(json!([
        {"tipo": "transicionar_goal", "goal_id": g.id, "para": "concluido", "motivo": "pulei etapas"},
        {"tipo": "acao_que_nao_existe"},
        {"tipo": "aguardar", "motivo": "nada"}
    ])));
    let r = heartbeat(&amb).ciclo().await.unwrap();
    assert!(r.resultados[0].starts_with("erro: transição inválida: proposto → concluido"));
    assert!(r.resultados[1].starts_with("erro: ação não reconhecida"));
    assert!(r.resultados[2].starts_with("ok: aguardando"));
    assert_eq!(
        goals::obter(&amb.banco, g.id).unwrap().estado,
        EstadoGoal::Proposto
    );

    // Força nova chamada com um evento: o erro aparece no contexto.
    eventos::publicar(&amb.banco, "teste", "t", "cutucada").unwrap();
    amb.mock.enfileirar(decisao(json!([])));
    heartbeat(&amb).ciclo().await.unwrap();
    let contexto = amb.mock.requisicoes()[1].corpo["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(contexto.contains("transição inválida"));
}

#[tokio::test]
async fn sem_novidade_nao_repete_a_chamada() {
    let amb = Ambiente::novo().await;
    criar_goal(&amb, "y");
    amb.mock
        .enfileirar(decisao(json!([{"tipo": "aguardar", "motivo": "esperar"}])));
    let hb = heartbeat(&amb);
    assert!(hb.ciclo().await.unwrap().chamou_modelo);
    // O goal não mudou e a revisão mínima (30 min) não passou.
    let r = hb.ciclo().await.unwrap();
    assert!(!r.chamou_modelo, "{}", r.motivo);
    assert_eq!(amb.mock.total_requisicoes(), 1);
}

#[tokio::test]
async fn evento_de_cron_entra_como_dado_e_e_consumido() {
    let amb = Ambiente::novo().await;
    let c = cron::adicionar(&amb.banco, "bom-dia", "* * * * *", "Revise os goals").unwrap();
    // Simula o tempo passando até o disparo.
    let disparados = cron::disparar_vencidos(&amb.banco, c.proximo_ms).unwrap();
    assert_eq!(disparados, vec!["bom-dia"]);

    amb.mock.enfileirar(decisao(json!([])));
    let r = heartbeat(&amb).ciclo().await.unwrap();
    assert!(r.chamou_modelo);
    assert!(r.motivo.contains("evento"));
    let contexto = amb.mock.requisicoes()[0].corpo["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(contexto.contains("<dados origem=\"cron:bom-dia\">"));
    assert!(contexto.contains("Revise os goals"));
    assert_eq!(eventos::contar_pendentes(&amb.banco).unwrap(), 0);
}

#[tokio::test]
async fn falha_do_modelo_nao_consome_eventos() {
    let amb = Ambiente::novo().await;
    eventos::publicar(&amb.banco, "teste", "t", "importante").unwrap();
    amb.mock.enfileirar(RespostaMock::erro(400, None));
    assert!(heartbeat(&amb).ciclo().await.is_err());
    assert_eq!(eventos::contar_pendentes(&amb.banco).unwrap(), 1);
    let ciclo = heartbeat::ultimo_ciclo(&amb.banco, true).unwrap().unwrap();
    assert!(ciclo.erro.unwrap().contains("400"));
}

#[tokio::test]
async fn resposta_fora_do_formato_fica_registrada() {
    let amb = Ambiente::novo().await;
    criar_goal(&amb, "z");
    amb.mock
        .enfileirar(RespostaMock::texto("Hmm, deixa eu pensar... sem JSON."));
    let r = heartbeat(&amb).ciclo().await.unwrap();
    assert!(r.decisao.is_none());
    let ciclo = heartbeat::ultimo_ciclo(&amb.banco, true).unwrap().unwrap();
    assert!(ciclo.erro.unwrap().contains("fora do formato"));
}

#[tokio::test]
async fn heartbeat_usa_a_fatia_autonoma_do_pool_do_cerebro() {
    let amb = Ambiente::novo().await;
    criar_goal(&amb, "w");
    amb.mock.enfileirar(decisao(json!([])));
    heartbeat(&amb).ciclo().await.unwrap();
    let origem: String = amb
        .banco
        .conexao()
        .query_row("SELECT origem FROM chamadas_modelo", [], |l| l.get(0))
        .unwrap();
    assert_eq!(origem, "autonomo");
    assert_eq!(
        amb.mock.requisicoes()[0].autorizacao.as_deref(),
        Some("Bearer nvapi-cerebro")
    );
}

#[tokio::test]
async fn daemon_uma_vez_e_status() {
    let amb = Ambiente::novo().await;
    let g = criar_goal(&amb, "relatório semanal");
    amb.mock.enfileirar(decisao(json!([
        {"tipo": "transicionar_goal", "goal_id": g.id, "para": "comprometido", "motivo": "ok"}
    ])));
    let d = Daemon::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
        amb.ferramentas.clone(),
    );
    d.rodar(&OpcoesDaemon { uma_vez: true }).await.unwrap();

    assert_eq!(
        goals::obter(&amb.banco, g.id).unwrap().estado,
        EstadoGoal::Comprometido
    );
    assert!(
        daemon::ler_estado(&amb.banco, daemon::CHAVE_SINAL_DE_VIDA)
            .unwrap()
            .is_some()
    );

    let texto = status::relatorio(&amb.config, &amb.banco).unwrap();
    assert!(texto.contains("Daemon: parado"));
    assert!(texto.contains("comprometido: 1"));
    assert!(texto.contains("em foco: #1"));
    assert!(texto.contains("Último ciclo"));
    assert!(texto.contains("Pool cerebro: 1 requisição"));
}
