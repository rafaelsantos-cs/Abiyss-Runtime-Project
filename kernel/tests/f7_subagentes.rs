//! F7: sub-agentes assíncronos, contra o mock do NIM.

mod comum;

use std::sync::Arc;
use std::time::{Duration, Instant};

use abiyss::chat::SessaoChat;
use abiyss::daemon::{Daemon, OpcoesDaemon};
use abiyss::eventos;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::goals::{self, NovoGoal};
use abiyss::heartbeat::Heartbeat;
use abiyss::nim::mock::{ChamadaMock, RespostaMock};
use abiyss::orquestrador::Nivel;
use abiyss::subagentes::{
    ControleSubagentes, EstadoSubagente, ExecutorSubagentes, PedidoDelegacao,
};
use comum::{Ambiente, NUCLEO_DE_TESTE};
use serde_json::{Value, json};

fn executor(amb: &Ambiente) -> Arc<ExecutorSubagentes> {
    ExecutorSubagentes::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
        amb.ferramentas.clone(),
    )
}

fn pedido(nivel: Nivel, tarefa: &str, contexto: &str) -> PedidoDelegacao {
    PedidoDelegacao {
        nivel,
        tarefa: tarefa.into(),
        contexto: contexto.into(),
        prazo_segundos: 600,
        goal_id: None,
        esforco: None,
    }
}

fn relatorio_json(resumo: &str) -> String {
    json!({
        "status": "concluido",
        "resumo": resumo,
        "artefatos": ["relatorio.md"],
        "confianca": 0.9,
        "duvidas": []
    })
    .to_string()
}

/// O mock já recebeu algum resultado de ferramenta nesta conversa?
fn tem_resultado_de_ferramenta(corpo: &Value) -> bool {
    corpo["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["role"] == "tool")
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

#[tokio::test]
async fn delegar_pelo_chat_devolve_id_na_hora() {
    let amb = Ambiente::novo().await;
    let controle = ControleSubagentes::novo(amb.config.clone(), amb.banco.clone(), None);
    let caixa = Arc::new(
        CaixaDeFerramentas::da_config(&amb.config)
            .unwrap()
            .com_subagentes(controle.clone()),
    );
    amb.mock.enfileirar(RespostaMock::ferramenta(
        "delegar",
        json!({"nivel": "low", "tarefa": "contar palavras de notas.md", "prazo": 120}),
    ));
    amb.mock
        .enfileirar(RespostaMock::texto("Deleguei; aviso quando terminar."));

    let mut sessao = SessaoChat::nova(
        amb.config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
        caixa,
    )
    .unwrap();
    let inicio = Instant::now();
    sessao.enviar("conte as palavras", None).await.unwrap();
    assert!(
        inicio.elapsed() < Duration::from_secs(2),
        "não pode esperar o sub-agente"
    );

    // O resultado da ferramenta trouxe o ID e o estado pendente.
    let resultado = amb.mock.requisicoes()[1].corpo["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "tool")
        .unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(resultado.contains("\"id\":1"), "{resultado}");
    assert!(resultado.contains("pendente"));
    assert_eq!(
        controle.status(1).unwrap().estado,
        EstadoSubagente::Pendente
    );
    // O chat oferece as três ferramentas de orquestração.
    let nomes = nomes_das_tools(&amb.mock.requisicoes()[0].corpo);
    for n in ["delegar", "status", "cancelar"] {
        assert!(nomes.contains(&n.to_string()));
    }
}

#[tokio::test]
async fn subagente_roda_com_contexto_limpo_tools_do_nivel_e_relatorio() {
    let amb = Ambiente::novo().await;
    amb.mock.definir_roteiro(|corpo| {
        if !tem_resultado_de_ferramenta(corpo) {
            RespostaMock::ferramenta(
                "escrever_arquivo",
                json!({"caminho": "relatorio.md", "conteudo": "# Resumo"}),
            )
        } else {
            RespostaMock::texto(relatorio_json("escrevi o resumo"))
        }
    });
    let ex = executor(&amb);
    let id = ex
        .controle()
        .delegar(
            &pedido(Nivel::Medium, "Escreva um resumo", "texto do contexto"),
            "teste",
        )
        .unwrap();
    ex.executar_pendentes_e_esperar().await.unwrap();

    let info = ex.controle().status(id).unwrap();
    assert_eq!(info.estado, EstadoSubagente::Concluido);
    let rel = info.relatorio.unwrap();
    assert_eq!(rel.status, "concluido");
    assert_eq!(rel.artefatos, vec!["relatorio.md"]);
    assert_eq!(info.rodadas, 2);
    assert!(info.tokens > 0);
    assert_eq!(
        std::fs::read_to_string(amb.caminho("workspace/relatorio.md")).unwrap(),
        "# Resumo"
    );

    let primeira = &amb.mock.requisicoes()[0];
    assert_eq!(primeira.corpo["model"], "teste/medium");
    assert_eq!(primeira.autorizacao.as_deref(), Some("Bearer nvapi-sub"));
    let msgs = primeira.corpo["messages"].as_array().unwrap();
    // Contexto limpo: só system + tarefa, sem identidade nem histórico do Abiyss.
    assert_eq!(msgs.len(), 2);
    let sistema = msgs[0]["content"].as_str().unwrap();
    assert!(sistema.contains("sub-agente de nível medium"));
    assert!(sistema.contains("não pode criar outros sub-agentes"));
    assert!(!sistema.contains(NUCLEO_DE_TESTE));
    let tarefa = msgs[1]["content"].as_str().unwrap();
    assert!(tarefa.contains("Escreva um resumo"));
    assert!(tarefa.contains("<dados origem=\"contexto_da_delegacao\">"));
    // Ferramentas do nível medium, sem as de orquestração.
    let nomes = nomes_das_tools(&primeira.corpo);
    assert!(nomes.contains(&"escrever_arquivo".to_string()));
    for proibida in ["delegar", "status", "cancelar"] {
        assert!(!nomes.contains(&proibida.to_string()));
    }

    // O resultado chegou como evento na fila do Abiyss.
    let evento = &eventos::pendentes(&amb.banco, 10).unwrap()[0];
    assert_eq!(evento.tipo, eventos::TIPO_SUBAGENTE);
    assert_eq!(evento.origem, id.to_string());
    assert!(evento.conteudo.contains("escrevi o resumo"));
}

#[tokio::test]
async fn low_nao_escreve_e_ninguem_delega() {
    let amb = Ambiente::novo().await;
    amb.mock.definir_roteiro(|corpo| {
        if !tem_resultado_de_ferramenta(corpo) {
            RespostaMock::Ferramentas(vec![
                ChamadaMock {
                    nome: "escrever_arquivo".into(),
                    argumentos: json!({"caminho": "x.md", "conteudo": "x"}),
                },
                ChamadaMock {
                    nome: "delegar".into(),
                    argumentos: json!({"nivel": "ultra", "tarefa": "me ajude"}),
                },
            ])
        } else {
            RespostaMock::texto(relatorio_json("não consegui escrever"))
        }
    });
    let ex = executor(&amb);
    let id = ex
        .controle()
        .delegar(&pedido(Nivel::Low, "tente escrever", ""), "teste")
        .unwrap();
    ex.executar_pendentes_e_esperar().await.unwrap();

    let nomes = nomes_das_tools(&amb.mock.requisicoes()[0].corpo);
    assert!(!nomes.contains(&"escrever_arquivo".to_string()));
    assert!(!nomes.contains(&"delegar".to_string()));
    let resultados: Vec<String> = amb.mock.requisicoes()[1].corpo["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(resultados.len(), 2);
    assert!(resultados.iter().all(|r| r.contains("não está disponível")));
    assert!(!amb.caminho("workspace/x.md").exists());
    // Nenhum sub-agente novo foi criado.
    assert!(ex.controle().status(id + 1).is_err());
}

#[tokio::test]
async fn orcamento_de_tokens_interrompe_com_relatorio_parcial() {
    let mut amb = Ambiente::novo().await;
    amb.config.subagentes.low.max_tokens = 5;
    amb.mock
        .definir_roteiro(|_| RespostaMock::ferramenta("listar_arquivos", json!({})));
    let ex = executor(&amb);
    let id = ex
        .controle()
        .delegar(&pedido(Nivel::Low, "liste tudo para sempre", ""), "teste")
        .unwrap();
    ex.executar_pendentes_e_esperar().await.unwrap();

    let info = ex.controle().status(id).unwrap();
    let rel = info.relatorio.unwrap();
    assert_eq!(rel.status, "parcial");
    assert!(rel.resumo.contains("orçamento"));
    // A resposta pedida nunca pode passar do que sobra do orçamento.
    assert!(
        amb.mock.requisicoes()[0].corpo["max_tokens"]
            .as_u64()
            .unwrap()
            <= 5
    );
    assert!(amb.mock.total_requisicoes() < 3);
}

#[tokio::test]
async fn prazo_esgotado_vira_expirado() {
    let mut amb = Ambiente::novo().await;
    amb.config.subagentes.low.max_segundos = 1;
    amb.mock
        .definir_roteiro(|_| RespostaMock::texto("demorei").atrasada(Duration::from_secs(5)));
    let ex = executor(&amb);
    let id = ex
        .controle()
        .delegar(&pedido(Nivel::Low, "algo lento", ""), "teste")
        .unwrap();
    let inicio = Instant::now();
    ex.executar_pendentes_e_esperar().await.unwrap();
    assert!(inicio.elapsed() < Duration::from_secs(3));
    let info = ex.controle().status(id).unwrap();
    assert_eq!(info.estado, EstadoSubagente::Expirado);
    assert!(info.relatorio.unwrap().resumo.contains("prazo"));
}

#[tokio::test]
async fn cancelar_interrompe_subagente_em_execucao() {
    let amb = Ambiente::novo().await;
    amb.mock
        .definir_roteiro(|_| RespostaMock::texto("lento").atrasada(Duration::from_secs(10)));
    let ex = executor(&amb);
    let controle = ex.controle();
    let id = controle
        .delegar(&pedido(Nivel::Medium, "algo demorado", ""), "teste")
        .unwrap();
    let rodando = {
        let ex = ex.clone();
        tokio::spawn(async move { ex.executar_pendentes_e_esperar().await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        controle.status(id).unwrap().estado,
        EstadoSubagente::Executando
    );
    assert!(controle.cancelar(id).unwrap().contains("solicitado"));

    tokio::time::timeout(Duration::from_secs(4), rodando)
        .await
        .expect("o cancelamento deveria interromper em ~1 s")
        .unwrap()
        .unwrap();
    assert_eq!(
        controle.status(id).unwrap().estado,
        EstadoSubagente::Cancelado
    );
    let evento = &eventos::pendentes(&amb.banco, 10).unwrap()[0];
    assert!(evento.conteudo.contains("cancelado"));
}

#[tokio::test]
async fn limite_de_subagentes_simultaneos() {
    let mut amb = Ambiente::novo().await;
    amb.config.subagentes.max_simultaneos = 1;
    amb.mock.definir_roteiro(|_| {
        RespostaMock::texto(relatorio_json("ok")).atrasada(Duration::from_millis(150))
    });
    let ex = executor(&amb);
    for i in 0..3 {
        ex.controle()
            .delegar(&pedido(Nivel::Low, &format!("tarefa {i}"), ""), "teste")
            .unwrap();
    }
    ex.executar_pendentes_e_esperar().await.unwrap();
    assert_eq!(amb.mock.total_requisicoes(), 3);
    assert_eq!(amb.mock.pico_concorrencia(), 1);
}

#[tokio::test]
async fn heartbeat_delega_e_recebe_o_resultado_como_evento() {
    let amb = Ambiente::novo().await;
    let goal = goals::criar(
        &amb.banco,
        &NovoGoal {
            titulo: "Resumo semanal".into(),
            nucleo: "Ter um resumo das notas da semana.".into(),
            descricao: String::new(),
            prioridade: 1,
        },
        "usuario",
    )
    .unwrap();
    let goal_id = goal.id;
    amb.mock
        .definir_roteiro(move |corpo| match corpo["model"].as_str().unwrap() {
            // Cérebro: no 1º ciclo delega; no 2º só aguarda.
            "teste/cerebro" => {
                let contexto = corpo["messages"][1]["content"].as_str().unwrap();
                if contexto.contains("<dados origem=\"subagente:") {
                    RespostaMock::texto(r#"{"decisao": "recebi", "acoes": []}"#)
                } else {
                    RespostaMock::texto(
                        json!({"decisao": "delegar", "acoes": [{
                            "tipo": "delegar", "nivel": "low", "tarefa": "resuma as notas",
                            "prazo_segundos": 60, "goal_id": goal_id
                        }]})
                        .to_string(),
                    )
                }
            }
            _ => RespostaMock::texto(relatorio_json("resumo pronto")),
        });

    let ex = executor(&amb);
    let hb = Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    )
    .com_subagentes(ex.controle());
    let r = hb.ciclo().await.unwrap();
    assert_eq!(
        r.resultados,
        vec!["ok: sub-agente 1 (low, esforço low) delegado"]
    );
    let info = ex.controle().status(1).unwrap();
    assert_eq!(info.origem, "heartbeat");
    assert_eq!(info.goal_id, Some(goal_id));

    ex.executar_pendentes_e_esperar().await.unwrap();
    assert_eq!(eventos::contar_pendentes(&amb.banco).unwrap(), 1);

    // Próximo ciclo: o evento do sub-agente faz o modelo ser chamado e
    // o relatório entra no contexto como dado.
    let r2 = hb.ciclo().await.unwrap();
    assert!(r2.chamou_modelo);
    let ultima = amb
        .mock
        .requisicoes()
        .into_iter()
        .rev()
        .find(|r| r.corpo["model"] == "teste/cerebro")
        .unwrap();
    let contexto = ultima.corpo["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(contexto.contains("<dados origem=\"subagente:1\">"));
    assert!(contexto.contains("resumo pronto"));
    assert_eq!(eventos::contar_pendentes(&amb.banco).unwrap(), 0);
}

#[tokio::test]
async fn daemon_uma_vez_roda_os_subagentes_delegados() {
    let amb = Ambiente::novo().await;
    let controle = ControleSubagentes::novo(amb.config.clone(), amb.banco.clone(), None);
    // Pedido feito "por outro processo" (ex.: abiyss chat).
    let id = controle
        .delegar(&pedido(Nivel::Ultra, "pesquise", ""), "chat")
        .unwrap();
    amb.mock
        .definir_roteiro(|_| RespostaMock::texto(relatorio_json("pesquisa feita")));
    let d = Daemon::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
        amb.ferramentas.clone(),
    );
    d.rodar(&OpcoesDaemon { uma_vez: true }).await.unwrap();
    assert_eq!(
        controle.status(id).unwrap().estado,
        EstadoSubagente::Concluido
    );
    assert_eq!(amb.mock.requisicoes()[0].corpo["model"], "teste/ultra");
}
