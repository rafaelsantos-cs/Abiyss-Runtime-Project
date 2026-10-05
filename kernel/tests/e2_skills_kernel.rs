//! E2: skills no kernel.
//!
//! - skills de pasta confiável (versionada no projeto) não marcam o
//!   contexto como externo: a proposta interna seguinte passa;
//! - skills de pasta de fora continuam sendo conteúdo externo;
//! - o heartbeat vê o índice e pode consultar uma skill (o texto chega no
//!   ciclo seguinte, que o daemon antecipa, com limite);
//! - o kernel põe sozinho a skill certa em situações que ele detecta.

mod comum;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use abiyss::chat::SessaoChat;
use abiyss::config::{Config, ConfigRaizSkills};
use abiyss::daemon::{Daemon, OpcoesDaemon};
use abiyss::eventos;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::goals::{self, NovoGoal};
use abiyss::heartbeat::Heartbeat;
use abiyss::memoria::Memoria;
use abiyss::memoria::propostas;
use abiyss::nim::mock::{ChamadaMock, RespostaMock};
use comum::Ambiente;
use serde_json::{Value, json};

const CORPO: &str = "PROCEDIMENTO-DA-SKILL-DE-TESTE";

fn criar_skill(raiz: &Path, nome: &str) {
    let pasta = raiz.join(nome);
    std::fs::create_dir_all(&pasta).unwrap();
    std::fs::write(
        pasta.join("SKILL.md"),
        format!("---\nname: {nome}\ndescription: Skill de teste {nome}.\n---\n# {nome}\n{CORPO}\n"),
    )
    .unwrap();
}

fn caixa_com_memoria(amb: &Ambiente, config: &Config) -> Arc<CaixaDeFerramentas> {
    let memoria = Arc::new(Memoria::abrir(config, amb.banco.clone()).unwrap());
    Arc::new(
        CaixaDeFerramentas::da_config(config)
            .unwrap()
            .com_memoria(memoria),
    )
}

/// Conversa que lê uma skill e, em seguida, propõe uma memória interna.
async fn ler_skill_e_propor(amb: &Ambiente, config: &Config, nome: &str) {
    let caixa = caixa_com_memoria(amb, config);
    amb.mock
        .enfileirar(RespostaMock::ferramenta("ler_skill", json!({"nome": nome})));
    amb.mock
        .enfileirar(RespostaMock::Ferramentas(vec![ChamadaMock {
            nome: "memoria_propor".into(),
            argumentos: json!({
                "escopo": "interno",
                "caminho": "preferencias/tom.md",
                "conteudo": "Prefere respostas diretas.",
                "tipo": "dito"
            }),
        }]));
    amb.mock.enfileirar(RespostaMock::texto("Anotado."));
    SessaoChat::nova(
        config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
        caixa,
    )
    .unwrap()
    .enviar("prefiro respostas diretas; veja a skill antes", None)
    .await
    .unwrap();
}

#[tokio::test]
async fn skill_confiavel_nao_bloqueia_proposta_interna() {
    let amb = Ambiente::novo().await;
    // `skills/` dentro da pasta do projeto = confiável por padrão.
    criar_skill(&amb.config.caminho_skills(), "lembrar");
    assert!(amb.config.raizes_skills()[0].confiavel);
    ler_skill_e_propor(&amb, &amb.config, "lembrar").await;

    let pendentes = propostas::pendentes(&amb.banco).unwrap();
    assert_eq!(pendentes.len(), 1);
    assert_eq!(pendentes[0].origem_externa, None, "{pendentes:?}");
}

#[tokio::test]
async fn skill_de_fora_continua_sendo_externa() {
    let amb = Ambiente::novo().await;
    let fora = tempfile::tempdir().unwrap();
    criar_skill(fora.path(), "lembrar");
    let mut config = amb.config.clone();
    config.skills.raizes = vec![ConfigRaizSkills {
        caminho: fora.path().to_string_lossy().to_string(),
        confiavel: None,
    }];
    assert!(!config.raizes_skills()[0].confiavel);
    ler_skill_e_propor(&amb, &config, "lembrar").await;

    let pendentes = propostas::pendentes(&amb.banco).unwrap();
    assert_eq!(pendentes.len(), 1);
    let origem = pendentes[0].origem_externa.as_deref().unwrap_or("");
    assert!(origem.contains("ler_skill"), "{origem}");
    // E o sleep rejeita, pela regra dura.
    let memoria = Memoria::abrir(&config, amb.banco.clone()).unwrap();
    assert_eq!(memoria.sleep().unwrap().rejeitadas(), 1);
}

#[test]
fn confianca_pelo_lugar_e_explicita() {
    let projeto = tempfile::tempdir().unwrap();
    let mut config = abiyss::config::config_de_teste("http://127.0.0.1:9/v1", projeto.path());
    // Padrão: [caminhos] skills = "skills", dentro do projeto.
    assert!(config.raizes_skills()[0].confiavel);
    config.skills.raizes = vec![
        ConfigRaizSkills {
            caminho: "../fora".into(),
            confiavel: None,
        },
        ConfigRaizSkills {
            caminho: "/opt/skills-de-terceiros".into(),
            confiavel: None,
        },
        ConfigRaizSkills {
            caminho: "/opt/minhas".into(),
            confiavel: Some(true),
        },
    ];
    let raizes = config.raizes_skills();
    assert!(!raizes[0].confiavel, "'..' nunca é confiável por padrão");
    assert!(!raizes[1].confiavel);
    assert!(raizes[2].confiavel);
    // Todas as raízes são áreas protegidas do workspace.
    let protegidas = config.areas_protegidas();
    assert!(raizes.iter().all(|r| protegidas.contains(&r.caminho)));
    assert_eq!(config.caminho_skills(), raizes[0].caminho);
}

fn decisao(acoes: Value) -> RespostaMock {
    RespostaMock::texto(
        json!({"percepcao": "p", "orientacao": "o", "decisao": "d", "acoes": acoes}).to_string(),
    )
}

fn consultar(nome: &str) -> RespostaMock {
    decisao(json!([{"tipo": "consultar_skill", "nome": nome, "motivo": "preciso do procedimento"}]))
}

fn criar_goal(amb: &Ambiente) {
    goals::criar(
        &amb.banco,
        &NovoGoal {
            titulo: "organizar".into(),
            nucleo: "organizar as notas".into(),
            descricao: String::new(),
            prioridade: 1,
        },
        "usuario",
    )
    .unwrap();
}

fn contexto(amb: &Ambiente, i: usize) -> String {
    amb.mock.requisicoes()[i].corpo["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn heartbeat_ve_o_indice_e_consulta_a_skill() {
    let amb = Ambiente::novo().await;
    criar_skill(&amb.config.caminho_skills(), "organizar-notas");
    criar_goal(&amb);
    let hb = Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    );

    amb.mock.enfileirar(consultar("organizar-notas"));
    let r = hb.ciclo().await.unwrap();
    assert!(r.pedir_continuacao);
    assert!(
        r.resultados[0].contains("chega no próximo ciclo"),
        "{:?}",
        r.resultados
    );
    let sistema = amb.mock.requisicoes()[0].corpo["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(sistema.contains("- organizar-notas: Skill de teste organizar-notas."));
    assert!(sistema.contains("consultar_skill"));
    assert!(!sistema.contains(CORPO), "só nome e descrição no prompt");

    // O texto chega como evento interno (pasta confiável)...
    let pendentes = eventos::pendentes(&amb.banco, 10).unwrap();
    assert_eq!(pendentes[0].tipo, eventos::TIPO_SKILL);
    assert!(!pendentes[0].eh_externo());

    // ...e entra, rotulado como dado, no ciclo seguinte.
    amb.mock.enfileirar(decisao(json!([])));
    let r = hb.ciclo().await.unwrap();
    assert!(!r.pedir_continuacao);
    let ctx = contexto(&amb, 1);
    assert!(ctx.contains("<dados origem=\"skill:organizar-notas\">"));
    assert!(ctx.contains(CORPO));
}

#[tokio::test]
async fn skill_de_pasta_nao_confiavel_vira_evento_externo() {
    let amb = Ambiente::novo().await;
    let fora = tempfile::tempdir().unwrap();
    criar_skill(fora.path(), "de-fora");
    let mut config = amb.config.clone();
    config.skills.raizes = vec![ConfigRaizSkills {
        caminho: fora.path().to_string_lossy().to_string(),
        confiavel: None,
    }];
    criar_goal(&amb);
    let hb = Heartbeat::novo(config, amb.banco.clone(), amb.orquestrador.clone());
    amb.mock.enfileirar(consultar("de-fora"));
    hb.ciclo().await.unwrap();
    let pendentes = eventos::pendentes(&amb.banco, 10).unwrap();
    assert_eq!(
        pendentes[0].origem_externa.as_deref(),
        Some("skill:de-fora")
    );
}

#[tokio::test]
async fn kernel_injeta_a_skill_de_estagnacao() {
    let amb = Ambiente::novo().await;
    criar_skill(&amb.config.caminho_skills(), "sair-de-loops");
    eventos::publicar(
        &amb.banco,
        eventos::TIPO_KERNEL,
        "estagnacao",
        "mesma decisão 3 vezes",
    )
    .unwrap();
    let hb = Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    );
    amb.mock.enfileirar(decisao(json!([])));
    hb.ciclo().await.unwrap();
    let ctx = contexto(&amb, 0);
    assert!(ctx.contains("Procedimento sugerido pelo kernel (skill sair-de-loops)"));
    assert!(ctx.contains(CORPO));
}

#[tokio::test]
async fn daemon_antecipa_o_ciclo_de_continuacao_com_limite() {
    let mut amb = Ambiente::novo().await;
    criar_skill(&amb.config.caminho_skills(), "organizar-notas");
    criar_goal(&amb);
    amb.config.daemon.heartbeat_segundos = 3600;
    amb.config.daemon.continuacao_segundos = 1;
    amb.config.daemon.max_continuacoes_seguidas = 2;
    for _ in 0..5 {
        amb.mock.enfileirar(consultar("organizar-notas"));
    }
    let d = Daemon::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
        amb.ferramentas.clone(),
    );
    let opcoes = OpcoesDaemon::default();
    d.rodar_ate(&opcoes, tokio::time::sleep(Duration::from_millis(3600)))
        .await
        .unwrap();
    // Ciclo ao subir + 2 continuações; a 3ª continuação passaria do limite
    // e o próximo ciclo só viria depois do intervalo normal (1 h).
    assert_eq!(amb.mock.total_requisicoes(), 3);
}
