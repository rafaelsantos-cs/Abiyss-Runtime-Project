//! E7: estagnação (decisão repetida, ação que falha seguidas vezes) e o
//! disjuntor do heartbeat, contra o mock do NIM.

mod comum;

use abiyss::eventos;
use abiyss::goals::{self, NovoGoal};
use abiyss::heartbeat::Heartbeat;
use abiyss::nim::mock::RespostaMock;
use abiyss::status;
use abiyss::tempo::agora_ms;
use abiyss::vigilancia::{self, EstadoDisjuntor};
use comum::Ambiente;
use serde_json::{Value, json};

fn heartbeat(amb: &Ambiente) -> Heartbeat {
    Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    )
}

fn criar_goal(amb: &Ambiente) -> goals::Goal {
    goals::criar(
        &amb.banco,
        &NovoGoal {
            titulo: "VPS".into(),
            nucleo: "Pronto quando houver uma tabela de 3 VPS em workspace/vps.md.".into(),
            descricao: String::new(),
            prioridade: 1,
        },
        "usuario",
    )
    .unwrap()
}

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

fn avisos_de_estagnacao(amb: &Ambiente) -> Vec<String> {
    eventos::pendentes(&amb.banco, 50)
        .unwrap()
        .into_iter()
        .filter(|e| e.tipo == eventos::TIPO_KERNEL && e.origem == vigilancia::ORIGEM_ESTAGNACAO)
        .map(|e| e.conteudo)
        .collect()
}

/// Copia a skill do repositório para o projeto de teste (pasta confiável).
fn instalar_skill(amb: &Ambiente, nome: &str) {
    let origem = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../skills")
        .join(nome)
        .join("SKILL.md");
    let destino = amb.caminho(&format!("skills/{nome}"));
    std::fs::create_dir_all(&destino).unwrap();
    std::fs::copy(origem, destino.join("SKILL.md")).unwrap();
}

#[tokio::test]
async fn tres_decisoes_iguais_trazem_o_aviso_no_quarto_ciclo() {
    let amb = Ambiente::novo().await;
    instalar_skill(&amb, "sair-de-loops");
    let goal = criar_goal(&amb);
    // O goal tem de ser mais antigo que o 1º ciclo: no mesmo milissegundo, o
    // kernel conta como "o goal mudou durante as repetições" e não avisa.
    comum::esperar_o_relogio_passar(goal.atualizado_ms).await;
    let mesma = json!([{
        "tipo": "delegar", "nivel": "low", "goal_id": goal.id,
        "tarefa": "Pesquisar preços de VPS", "expectativa": "tabela completa"
    }]);
    let hb = heartbeat(&amb);
    for i in 0..3 {
        eventos::publicar(&amb.banco, "teste", "t", &format!("novidade {i}")).unwrap();
        amb.mock.enfileirar(decisao(mesma.clone()));
        let r = hb.ciclo().await.unwrap();
        assert!(r.chamou_modelo);
        if i < 2 {
            assert!(avisos_de_estagnacao(&amb).is_empty(), "aviso cedo demais");
        }
    }
    let avisos = avisos_de_estagnacao(&amb);
    assert_eq!(avisos.len(), 1);
    assert!(avisos[0].contains("3 vezes seguidas"), "{}", avisos[0]);

    // O 4º ciclo (acordado pelo próprio aviso) traz o aviso e a skill.
    amb.mock.enfileirar(decisao(mesma.clone()));
    let r = hb.ciclo().await.unwrap();
    assert!(r.chamou_modelo);
    let quarto = texto_da_requisicao(&amb.mock.requisicoes()[3].corpo);
    assert!(quarto.contains("Estagnação: você decidiu a mesma coisa 3 vezes"));
    assert!(quarto.contains("Procedimento sugerido pelo kernel (skill sair-de-loops)"));

    // A revisão periódica do goal fica 2× mais espaçada.
    let estado = vigilancia::ler_estagnacao(&amb.banco).unwrap().unwrap();
    assert_eq!(estado.goal_id, Some(goal.id));
    assert_eq!(estado.multiplicador, 2);
    let relatorio = status::relatorio(&amb.config, &amb.banco).unwrap();
    assert!(relatorio.contains("Estagnação: goal #1 parado; revisão periódica a cada 2×"));

    // Só as repetições DEPOIS do aviso contam: o 4º ciclo igual não avisa de novo.
    assert!(avisos_de_estagnacao(&amb).is_empty());
}

#[tokio::test]
async fn mesma_acao_falhando_tres_vezes_e_estagnacao() {
    let amb = Ambiente::novo().await;
    let goal = criar_goal(&amb);
    let hb = heartbeat(&amb);
    for i in 0..3 {
        eventos::publicar(&amb.banco, "teste", "t", &format!("novidade {i}")).unwrap();
        // A transição é sempre recusada (proposto não vai direto a concluído);
        // a outra ação muda a cada ciclo, então a decisão não se repete.
        amb.mock.enfileirar(decisao(json!([
            {"tipo": "transicionar_goal", "goal_id": goal.id, "para": "concluido", "motivo": "acabei"},
            {"tipo": "consultar_skill", "nome": format!("skill-{i}")}
        ])));
        hb.ciclo().await.unwrap();
    }
    let avisos = avisos_de_estagnacao(&amb);
    assert_eq!(avisos.len(), 1, "{avisos:?}");
    assert!(
        avisos[0].contains("falhou 3 vezes seguidas"),
        "{}",
        avisos[0]
    );
    assert!(avisos[0].contains("transicionar_goal:goal=1:concluido"));
    assert!(avisos[0].contains("transição inválida"), "{}", avisos[0]);
}

#[tokio::test]
async fn disjuntor_abre_depois_de_tres_falhas_e_um_sucesso_fecha() {
    let amb = Ambiente::novo().await;
    let hb = heartbeat(&amb);
    eventos::publicar(&amb.banco, "teste", "t", "acorde").unwrap();

    // Duas chamadas que falham (o evento continua na fila)...
    for _ in 0..2 {
        amb.mock.enfileirar(RespostaMock::erro(400, None));
        assert!(hb.ciclo().await.is_err());
    }
    assert_eq!(
        vigilancia::situacao(&vigilancia::ler_disjuntor(&amb.banco).unwrap(), agora_ms()),
        vigilancia::Situacao::Fechado
    );
    // ...e uma resposta fora do formato: a terceira falha abre.
    amb.mock
        .enfileirar(RespostaMock::texto("não sei responder em JSON"));
    let r = hb.ciclo().await.unwrap();
    assert!(r.erro.is_some());
    let estado = vigilancia::ler_disjuntor(&amb.banco).unwrap();
    assert_eq!(estado.falhas_seguidas, 3);
    assert!(estado.aberto_ate_ms.is_some_and(|ate| ate > agora_ms()));
    let feitas = amb.mock.total_requisicoes();

    // Aberto: o ciclo seguinte NÃO chama o modelo (o evento espera na fila).
    eventos::publicar(&amb.banco, "teste", "t", "outra novidade").unwrap();
    let r = hb.ciclo().await.unwrap();
    assert!(!r.chamou_modelo);
    assert!(r.motivo.contains("disjuntor aberto"), "{}", r.motivo);
    assert_eq!(amb.mock.total_requisicoes(), feitas);
    assert_eq!(eventos::contar_pendentes(&amb.banco).unwrap(), 1);
    let relatorio = status::relatorio(&amb.config, &amb.banco).unwrap();
    assert!(
        relatorio.contains("Disjuntor do heartbeat: ABERTO"),
        "{relatorio}"
    );
    assert!(relatorio.contains("ATENÇÃO: disjuntor do heartbeat ABERTO"));

    // A espera passou: meio-aberto, uma tentativa; deu certo, fecha.
    vigilancia::gravar_disjuntor(
        &amb.banco,
        &EstadoDisjuntor {
            aberto_ate_ms: Some(agora_ms() - 1),
            ..estado
        },
    )
    .unwrap();
    amb.mock.enfileirar(decisao(json!([])));
    let r = hb.ciclo().await.unwrap();
    assert!(r.chamou_modelo);
    assert_eq!(amb.mock.total_requisicoes(), feitas + 1);
    assert_eq!(
        vigilancia::ler_disjuntor(&amb.banco).unwrap(),
        EstadoDisjuntor::default()
    );
    assert_eq!(eventos::contar_pendentes(&amb.banco).unwrap(), 0);
}
