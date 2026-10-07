//! Infra 2 ligada ao runtime: a tabela de esforço decide os campos de cada
//! chamada — chat (padrão + `aprofundar` até o teto), heartbeat (raso na
//! rotina, profundo por situação), delegação (esforço limitado ao teto do
//! nível do sub-agente) — e níveis a confirmar não mandam os campos deles.
//! Tudo contra o mock do NIM, conferindo o corpo de cada pedido.

mod comum;

use std::sync::Arc;

use abiyss::chat::{APROFUNDAR, SessaoChat};
use abiyss::config::ConfigModelos;
use abiyss::esforco::{self, EscolhaEsforco, NivelEsforco};
use abiyss::eventos;
use abiyss::goals::{self, EstadoGoal, NovoGoal};
use abiyss::heartbeat::{self, Heartbeat, SituacaoCiclo};
use abiyss::nim::mock::RespostaMock;
use abiyss::status;
use abiyss::subagentes::ExecutorSubagentes;
use abiyss::vigilancia;
use comum::Ambiente;
use serde_json::{Value, json};

/// Tabelas de teste: cada nível tem um `max_tokens` diferente, para o
/// pedido mostrar qual nível foi usado.
const MODELOS: &str = r#"
[cerebro]
id = "teste/cerebro"
max_tokens = 1000
extra = { chat_template_kwargs = { enable_thinking = true } }
[cerebro.esforco]
campo_pensar = "chat_template_kwargs.enable_thinking"
campo_orcamento = "chat_template_kwargs.thinking_budget"
[cerebro.esforco.niveis]
minimal = { pensar = false, max_tokens = 100 }
low = { pensar = false, max_tokens = 200 }
medium = { pensar = true, max_tokens = 300 }
high = { pensar = true, orcamento = 4000, max_tokens = 400 }
xhigh = { pensar = true, orcamento = 8000, max_tokens = 500, a_confirmar = "orçamento grande" }
ultra = { pensar = true, extra = { reasoning_effort = "max" }, a_confirmar = "reasoning_effort" }

[sub_ultra]
id = "teste/ultra"
max_tokens = 2000
[sub_ultra.esforco.niveis]
high = { max_tokens = 1500 }
ultra = { usar_modelo = "cerebro", pensar = true, max_tokens = 600 }

[sub_medium]
id = "teste/medium"
[sub_medium.esforco]
campo_pensar = "thinking"
[sub_medium.esforco.niveis]
medium = { pensar = true, max_tokens = 700 }
high = { pensar = true, max_tokens = 800 }

[sub_low]
id = "teste/low"
[sub_low.esforco]
campo_pensar = "chat_template_kwargs.enable_thinking"
[sub_low.esforco.niveis]
low = { pensar = false, max_tokens = 900, a_confirmar = "nome do campo" }
medium = { pensar = true, max_tokens = 950 }
high = { usar_modelo = "sub_medium", pensar = true }
"#;

async fn ambiente() -> Ambiente {
    let mut amb = Ambiente::novo().await;
    let modelos: ConfigModelos = toml::from_str(MODELOS).unwrap();
    esforco::validar(&modelos).unwrap();
    amb.config.modelos = modelos;
    amb
}

fn sessao(amb: &Ambiente) -> SessaoChat {
    SessaoChat::nova(
        amb.config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
        amb.ferramentas.clone(),
    )
    .unwrap()
}

/// `(nivel_esforco, esforco_confirmado)` de cada chamada, em ordem.
fn niveis_gravados(amb: &Ambiente) -> Vec<(Option<String>, Option<bool>)> {
    let conexao = amb.banco.conexao();
    let mut consulta = conexao
        .prepare("SELECT nivel_esforco, esforco_confirmado FROM chamadas_modelo ORDER BY id")
        .unwrap();
    consulta
        .query_map([], |l| Ok((l.get(0)?, l.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

fn gravado(nivel: &str, confirmado: bool) -> (Option<String>, Option<bool>) {
    (Some(nivel.to_string()), Some(confirmado))
}

fn nomes_das_tools(corpo: &Value) -> Vec<String> {
    corpo["tools"]
        .as_array()
        .map(|lista| {
            lista
                .iter()
                .map(|t| t["function"]["name"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn decisao(acoes: Value) -> String {
    json!({"percepcao": "p", "orientacao": "o", "decisao": "d", "acoes": acoes}).to_string()
}

fn eh_sub_agente(corpo: &Value) -> bool {
    corpo["messages"][0]["content"]
        .as_str()
        .is_some_and(|s| s.starts_with("Você é um sub-agente"))
}

#[tokio::test]
async fn chat_usa_o_padrao_e_aprofundar_vale_so_para_esta_resposta() {
    let amb = ambiente().await;
    let mut s = sessao(&amb);
    amb.mock.enfileirar(RespostaMock::ferramenta(
        APROFUNDAR,
        json!({"nivel": "ultra"}),
    ));
    amb.mock.enfileirar(RespostaMock::texto("resposta pensada"));

    let r = s.enviar("uma pergunta difícil", None).await.unwrap();
    assert_eq!(r.texto, "resposta pensada");
    s.enviar("e agora, uma simples", None).await.unwrap();

    let pedidos: Vec<Value> = amb
        .mock
        .requisicoes()
        .into_iter()
        .map(|r| r.corpo)
        .collect();
    assert_eq!(pedidos.len(), 3);
    // 1ª rodada: o padrão (medium), com `aprofundar` oferecida até o teto (high).
    assert_eq!(pedidos[0]["model"], "teste/cerebro");
    assert_eq!(pedidos[0]["max_tokens"], 300);
    assert_eq!(
        pedidos[0]["chat_template_kwargs"],
        json!({"enable_thinking": true})
    );
    let aprofundar = pedidos[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["function"]["name"] == APROFUNDAR)
        .expect("o chat oferece aprofundar");
    assert_eq!(
        aprofundar["function"]["parameters"]["properties"]["nivel"]["enum"],
        json!(["high"])
    );
    // 2ª rodada: pediu ultra, foi limitado a high (o teto): os campos do high.
    assert_eq!(pedidos[1]["max_tokens"], 400);
    assert_eq!(
        pedidos[1]["chat_template_kwargs"],
        json!({"enable_thinking": true, "thinking_budget": 4000})
    );
    let resultado = pedidos[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "tool")
        .unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        resultado.contains("esforço desta resposta: high (pedido ultra: o máximo é high)"),
        "{resultado}"
    );
    // A mensagem seguinte do dono volta ao padrão.
    assert_eq!(pedidos[2]["max_tokens"], 300);
    assert!(
        pedidos[2]["chat_template_kwargs"]
            .get("thinking_budget")
            .is_none()
    );

    assert_eq!(
        niveis_gravados(&amb),
        vec![
            gravado("medium", true),
            gravado("high", true),
            gravado("medium", true)
        ]
    );
}

#[tokio::test]
async fn chat_sem_teto_acima_do_padrao_nao_oferece_aprofundar() {
    let mut amb = ambiente().await;
    amb.config.chat.esforco_padrao = NivelEsforco::Low;
    amb.config.chat.esforco_maximo = NivelEsforco::Low;
    let mut s = sessao(&amb);
    s.enviar("oi", None).await.unwrap();
    let corpo = &amb.mock.requisicoes()[0].corpo;
    assert_eq!(corpo["max_tokens"], 200);
    assert_eq!(
        corpo["chat_template_kwargs"],
        json!({"enable_thinking": false})
    );
    assert!(!nomes_das_tools(corpo).contains(&APROFUNDAR.to_string()));
}

#[tokio::test]
async fn nivel_a_confirmar_no_chat_vai_sem_os_campos_e_o_status_mostra() {
    let mut amb = ambiente().await;
    amb.config.chat.esforco_maximo = NivelEsforco::Ultra;
    let mut s = sessao(&amb);
    amb.mock.enfileirar(RespostaMock::ferramenta(
        APROFUNDAR,
        json!({"nivel": "ultra"}),
    ));
    amb.mock.enfileirar(RespostaMock::texto("ok"));
    s.enviar("pense no máximo", None).await.unwrap();

    let ultra = &amb.mock.requisicoes()[1].corpo;
    // Placeholder: nada do nível (nem reasoning_effort, nem o orçamento);
    // só o modelo com os parâmetros padrão.
    assert_eq!(ultra["model"], "teste/cerebro");
    assert_eq!(ultra["max_tokens"], 1000);
    assert!(ultra.get("reasoning_effort").is_none());
    assert_eq!(
        ultra["chat_template_kwargs"],
        json!({"enable_thinking": true})
    );
    assert_eq!(
        niveis_gravados(&amb),
        vec![gravado("medium", true), gravado("ultra", false)]
    );

    // `abiyss status`: latência por nível, com o placeholder separado.
    let relatorio = status::relatorio(&amb.config, &amb.banco).unwrap();
    assert!(relatorio.contains("teste/cerebro [cerebro]"), "{relatorio}");
    assert!(
        relatorio.contains("    esforço medium: 1º token"),
        "{relatorio}"
    );
    assert!(
        relatorio.contains("    esforço ultra [a confirmar: parâmetros padrão]: 1º token"),
        "{relatorio}"
    );
}

fn criar_goal(amb: &Ambiente) -> goals::Goal {
    goals::criar(
        &amb.banco,
        &NovoGoal {
            titulo: "Livro".into(),
            nucleo: "Capítulo 1 pronto até sexta.".into(),
            descricao: String::new(),
            prioridade: 1,
        },
        "usuario",
    )
    .unwrap()
}

#[tokio::test]
async fn heartbeat_raso_na_rotina_e_profundo_por_situacao() {
    let mut amb = ambiente().await;
    // Toda volta sem evento é revisão periódica (sem esperar).
    amb.config.daemon.revisao_minima_segundos = 0;
    // Estagnação configurada num nível a confirmar: vai sem os campos.
    amb.config.daemon.esforco.estagnacao = EscolhaEsforco::Nivel(NivelEsforco::Ultra);
    amb.mock
        .definir_roteiro(|_| RespostaMock::texto(decisao(json!([]))));
    let hb = Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    );

    // Primeiro ciclo com um goal (nenhum ciclo antes) → revisão: profundo (high).
    let goal = criar_goal(&amb);
    let r = hb.ciclo().await.unwrap();
    assert!(r.motivo.starts_with("primeiro ciclo"), "{}", r.motivo);
    assert_eq!(
        r.esforco,
        Some((NivelEsforco::High, SituacaoCiclo::RevisaoGoal))
    );

    // Rotina: um cron → raso (medium).
    eventos::publicar(&amb.banco, eventos::TIPO_CRON, "lembrete", "beber água").unwrap();
    let r = hb.ciclo().await.unwrap();
    assert_eq!(
        r.esforco,
        Some((NivelEsforco::Medium, SituacaoCiclo::Rotina))
    );

    // Revisão periódica do goal → profundo.
    let r = hb.ciclo().await.unwrap();
    assert!(r.motivo.starts_with("revisão periódica"), "{}", r.motivo);
    assert_eq!(
        r.esforco,
        Some((NivelEsforco::High, SituacaoCiclo::RevisaoGoal))
    );

    // O goal mudou (o dono comprometeu) → rotina. A mudança tem de cair num
    // milissegundo depois do início do último ciclo (no mesmo, não conta).
    let ultimo = heartbeat::ultimo_ciclo(&amb.banco, true).unwrap().unwrap();
    comum::esperar_o_relogio_passar(ultimo.inicio_ms).await;
    goals::transicionar(
        &amb.banco,
        goal.id,
        EstadoGoal::Comprometido,
        "vamos",
        "usuario",
    )
    .unwrap();
    let r = hb.ciclo().await.unwrap();
    assert!(r.motivo.contains("mudou"), "{}", r.motivo);
    assert_eq!(
        r.esforco,
        Some((NivelEsforco::Medium, SituacaoCiclo::Rotina))
    );

    // Estagnação → ultra (a confirmar), mesmo junto de um cron.
    eventos::publicar(&amb.banco, eventos::TIPO_CRON, "lembrete", "x").unwrap();
    eventos::publicar(
        &amb.banco,
        eventos::TIPO_KERNEL,
        vigilancia::ORIGEM_ESTAGNACAO,
        "a mesma decisão 3 vezes",
    )
    .unwrap();
    let r = hb.ciclo().await.unwrap();
    assert_eq!(
        r.esforco,
        Some((NivelEsforco::Ultra, SituacaoCiclo::Estagnacao))
    );

    // Despertar (reinício) → profundo.
    eventos::publicar(
        &amb.banco,
        eventos::TIPO_KERNEL,
        eventos::ORIGEM_REINICIO,
        "fiquei fora do ar",
    )
    .unwrap();
    let r = hb.ciclo().await.unwrap();
    assert_eq!(
        r.esforco,
        Some((NivelEsforco::High, SituacaoCiclo::Despertar))
    );

    let pedidos: Vec<Value> = amb
        .mock
        .requisicoes()
        .into_iter()
        .map(|r| r.corpo)
        .collect();
    assert_eq!(pedidos.len(), 6);
    let campos = |p: &Value| (p["max_tokens"].clone(), p["chat_template_kwargs"].clone());
    let medium = (json!(300), json!({"enable_thinking": true}));
    let high = (
        json!(400),
        json!({"enable_thinking": true, "thinking_budget": 4000}),
    );
    let padrao = (json!(1000), json!({"enable_thinking": true}));
    assert_eq!(campos(&pedidos[0]), high);
    assert_eq!(campos(&pedidos[1]), medium);
    assert_eq!(campos(&pedidos[2]), high);
    assert_eq!(campos(&pedidos[3]), medium);
    assert_eq!(campos(&pedidos[4]), padrao);
    assert!(pedidos[4].get("reasoning_effort").is_none());
    assert_eq!(campos(&pedidos[5]), high);
    assert_eq!(
        niveis_gravados(&amb),
        vec![
            gravado("high", true),
            gravado("medium", true),
            gravado("high", true),
            gravado("medium", true),
            gravado("ultra", false),
            gravado("high", true),
        ]
    );
}

#[tokio::test]
async fn delegacao_limita_o_esforco_ao_teto_do_nivel_do_sub_agente() {
    let amb = ambiente().await;
    amb.mock.definir_roteiro(|corpo| {
        if eh_sub_agente(corpo) {
            return RespostaMock::texto(
                json!({"status": "concluido", "resumo": "feito", "confianca": 0.9}).to_string(),
            );
        }
        RespostaMock::texto(decisao(json!([
            // low pede ultra: o teto do low é medium.
            {"tipo": "delegar", "nivel": "low", "tarefa": "a", "esforco": "ultra"},
            // ultra pede ultra: permitido (usa o cérebro, como diz a tabela).
            {"tipo": "delegar", "nivel": "ultra", "tarefa": "b", "esforco": "ultra"},
            // medium sem esforço: o padrão do nível.
            {"tipo": "delegar", "nivel": "medium", "tarefa": "c"},
            // low sem esforço: o padrão (low) está a confirmar.
            {"tipo": "delegar", "nivel": "low", "tarefa": "d"},
            // Esforço que não existe: a ação falha e diz por quê.
            {"tipo": "delegar", "nivel": "low", "tarefa": "e", "esforco": "max"},
        ])))
    });
    let executor = ExecutorSubagentes::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
        amb.ferramentas.clone(),
    );
    let hb = Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    )
    .com_subagentes(executor.controle());
    eventos::publicar(
        &amb.banco,
        eventos::TIPO_CRON,
        "pesquisa",
        "hora de delegar",
    )
    .unwrap();

    let r = hb.ciclo().await.unwrap();
    assert_eq!(
        r.resultados,
        vec![
            "ok: sub-agente 1 (low, esforço medium; pedido ultra limitado ao teto do nível) delegado",
            "ok: sub-agente 2 (ultra, esforço ultra) delegado",
            "ok: sub-agente 3 (medium, esforço medium) delegado",
            "ok: sub-agente 4 (low, esforço low) delegado",
        ]
        .into_iter()
        .map(String::from)
        .chain(std::iter::once(
            "erro: esforço inválido 'max' (use minimal, low, medium, high, xhigh, ultra)".to_string()
        ))
        .collect::<Vec<_>>()
    );
    Arc::clone(&executor)
        .executar_pendentes_e_esperar()
        .await
        .unwrap();

    // O pedido de cada sub-agente, pela tarefa.
    let pedidos: Vec<Value> = amb
        .mock
        .requisicoes()
        .into_iter()
        .map(|r| r.corpo)
        .filter(eh_sub_agente)
        .collect();
    assert_eq!(pedidos.len(), 4);
    let da_tarefa = |tarefa: &str| {
        pedidos
            .iter()
            .find(|p| p["messages"][1]["content"] == format!("Tarefa:\n{tarefa}\n"))
            .unwrap_or_else(|| panic!("sem pedido da tarefa {tarefa}"))
            .clone()
    };
    let a = da_tarefa("a");
    assert_eq!(a["model"], "teste/low");
    assert_eq!(a["max_tokens"], 950);
    assert_eq!(a["chat_template_kwargs"], json!({"enable_thinking": true}));
    let b = da_tarefa("b");
    assert_eq!(b["model"], "teste/cerebro", "ultra.ultra usa o cérebro");
    assert_eq!(b["max_tokens"], 600);
    assert_eq!(b["chat_template_kwargs"], json!({"enable_thinking": true}));
    let c = da_tarefa("c");
    assert_eq!(c["model"], "teste/medium");
    assert_eq!(c["max_tokens"], 700);
    assert_eq!(c["thinking"], json!(true));
    // Placeholder: nem o `pensar` nem o max_tokens do nível; o max_tokens é
    // o que sobra do orçamento do sub-agente.
    let d = da_tarefa("d");
    assert_eq!(d["model"], "teste/low");
    assert!(d.get("chat_template_kwargs").is_none());
    assert_eq!(d["max_tokens"], amb.config.subagentes.low.max_tokens);

    // Gravado por sub-agente e por chamada (as dos sub-agentes no pool deles).
    let conexao = amb.banco.conexao();
    let esforcos: Vec<(i64, String)> = conexao
        .prepare("SELECT id, esforco FROM subagentes ORDER BY id")
        .unwrap()
        .query_map([], |l| Ok((l.get(0)?, l.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        esforcos,
        vec![
            (1, "medium".to_string()),
            (2, "ultra".to_string()),
            (3, "medium".to_string()),
            (4, "low".to_string())
        ]
    );
    let mut sub: Vec<(String, String, bool)> = conexao
        .prepare(
            "SELECT modelo, nivel_esforco, esforco_confirmado FROM chamadas_modelo
              WHERE pool = 'subagentes'",
        )
        .unwrap()
        .query_map([], |l| Ok((l.get(0)?, l.get(1)?, l.get(2)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    sub.sort();
    assert_eq!(
        sub,
        vec![
            ("teste/cerebro".to_string(), "ultra".to_string(), true),
            ("teste/low".to_string(), "low".to_string(), false),
            ("teste/low".to_string(), "medium".to_string(), true),
            ("teste/medium".to_string(), "medium".to_string(), true),
        ]
    );
}
