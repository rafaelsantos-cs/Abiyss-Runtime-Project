//! A1: carregador de skills com revelação progressiva.
//!
//! - só nome + descrição entram no system prompt;
//! - `ler_skill(nome)` devolve o SKILL.md inteiro (rotulado como dado);
//! - `ler_skill(nome, referencia)` lê de references/ sem sair da skill;
//! - skills são só leitura (o workspace não pode englobar a pasta).

mod comum;

use std::path::Path;

use abiyss::chat::SessaoChat;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::ferramentas::workspace::Workspace;
use abiyss::nim::mock::RespostaMock;
use abiyss::skills::Skills;
use comum::Ambiente;
use serde_json::{Value, json};

const CORPO_SECRETO: &str = "PASSO-A-PASSO-QUE-SO-APARECE-NO-LER-SKILL";

fn criar_skills(raiz: &Path) {
    let pesquisa = raiz.join("pesquisa/arxiv");
    std::fs::create_dir_all(pesquisa.join("references")).unwrap();
    std::fs::write(
        pesquisa.join("SKILL.md"),
        format!(
            "---\nname: arxiv\ndescription: Busca artigos no arXiv.\nversion: 1.0.0\n---\n# arXiv\n{CORPO_SECRETO}\n"
        ),
    )
    .unwrap();
    std::fs::write(pesquisa.join("references/api.md"), "GET /api/query").unwrap();
    let outra = raiz.join("revisar");
    std::fs::create_dir_all(&outra).unwrap();
    std::fs::write(
        outra.join("SKILL.md"),
        "---\nname: revisar\ndescription: Revisa um texto em português.\n---\nCorpo.",
    )
    .unwrap();
    // Sem descrição: ignorada (aparece em `abiyss skills` como problema).
    let quebrada = raiz.join("quebrada");
    std::fs::create_dir_all(&quebrada).unwrap();
    std::fs::write(quebrada.join("SKILL.md"), "---\nname: quebrada\n---\n").unwrap();
}

fn sessao(amb: &Ambiente, ferramentas: std::sync::Arc<CaixaDeFerramentas>) -> SessaoChat {
    SessaoChat::nova(
        amb.config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
        ferramentas,
    )
    .unwrap()
}

fn texto_do_sistema(corpo: &Value) -> String {
    corpo["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn so_nome_e_descricao_vao_para_o_prompt_e_ler_skill_traz_o_resto() {
    let amb = Ambiente::novo().await;
    criar_skills(&amb.config.caminho_skills());
    let ferramentas = std::sync::Arc::new(CaixaDeFerramentas::da_config(&amb.config).unwrap());

    amb.mock.enfileirar(RespostaMock::ferramenta(
        "ler_skill",
        json!({"nome": "arxiv"}),
    ));
    amb.mock.enfileirar(RespostaMock::ferramenta(
        "ler_skill",
        json!({"nome": "arxiv", "referencia": "api.md"}),
    ));
    amb.mock.enfileirar(RespostaMock::texto("Pronto."));
    sessao(&amb, ferramentas)
        .enviar("procure artigos", None)
        .await
        .unwrap();

    let requisicoes = amb.mock.requisicoes();
    let sistema = texto_do_sistema(&requisicoes[0].corpo);
    // Índice: nome + descrição, rotulado como dado.
    assert!(sistema.contains("# Skills disponíveis"));
    assert!(sistema.contains("<dados origem=\"skills:indice\">"));
    assert!(sistema.contains("- arxiv: Busca artigos no arXiv."));
    assert!(sistema.contains("- revisar: Revisa um texto em português."));
    assert!(!sistema.contains("quebrada"));
    // O corpo da skill NÃO está no prompt...
    assert!(!sistema.contains(CORPO_SECRETO));
    // ...e a ferramenta foi oferecida.
    assert!(
        requisicoes[0].corpo["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["function"]["name"] == "ler_skill")
    );

    // O corpo chega pela ferramenta, rotulado como dado.
    let mensagens = requisicoes[2].corpo["messages"].as_array().unwrap();
    let resultados: Vec<&str> = mensagens
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    assert_eq!(resultados.len(), 2);
    assert!(resultados[0].starts_with("<dados origem=\"ler_skill\">"));
    assert!(resultados[0].contains(CORPO_SECRETO));
    assert!(resultados[0].contains("arquivos em references/: api.md"));
    assert!(resultados[1].contains("GET /api/query"));
}

#[tokio::test]
async fn sem_pasta_de_skills_nada_muda() {
    let amb = Ambiente::novo().await;
    amb.mock.enfileirar(RespostaMock::texto("oi"));
    sessao(&amb, amb.ferramentas.clone())
        .enviar("oi", None)
        .await
        .unwrap();
    let requisicao = &amb.mock.requisicoes()[0];
    assert!(!texto_do_sistema(&requisicao.corpo).contains("Skills disponíveis"));
    assert!(
        !requisicao.corpo["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["function"]["name"] == "ler_skill")
    );
}

#[test]
fn referencia_nao_sai_da_pasta_da_skill() {
    let pasta = tempfile::tempdir().unwrap();
    criar_skills(pasta.path());
    std::fs::write(pasta.path().join("segredo.txt"), "não pode").unwrap();
    let skills = Skills::nova(pasta.path());

    assert!(skills.ler("arxiv", Some("../SKILL.md")).is_err());
    assert!(skills.ler("arxiv", Some("../../../segredo.txt")).is_err());
    assert!(skills.ler("arxiv", Some("/etc/passwd")).is_err());
    assert!(skills.ler("arxiv", Some("nao-existe.md")).is_err());
    // Skill sem references/.
    assert!(skills.ler("revisar", Some("x.md")).is_err());
    // Nome desconhecido lista as disponíveis.
    let erro = skills.ler("nao-existe", None).unwrap_err().to_string();
    assert!(erro.contains("arxiv"));
    assert!(erro.contains("revisar"));

    #[cfg(unix)]
    {
        let refs = pasta.path().join("pesquisa/arxiv/references");
        std::os::unix::fs::symlink(pasta.path().join("segredo.txt"), refs.join("link.md")).unwrap();
        assert!(skills.ler("arxiv", Some("link.md")).is_err());
    }
}

#[test]
fn skills_sao_area_protegida_do_workspace() {
    let amb_pasta = tempfile::tempdir().unwrap();
    let config = abiyss::config::config_de_teste("http://127.0.0.1:9/v1", amb_pasta.path());
    let protegidas = config.areas_protegidas();
    assert!(protegidas.contains(&config.caminho_skills()));
    // Workspace dentro da pasta de skills: recusado.
    let dentro = config.caminho_skills().join("ws");
    assert!(Workspace::abrir(&dentro, &protegidas, Default::default()).is_err());
}

#[test]
fn skill_de_exemplo_do_repositorio_carrega() {
    let raiz = Path::new(env!("CARGO_MANIFEST_DIR")).join("../skills");
    let catalogo = Skills::nova(&raiz).catalogo();
    assert!(catalogo.problemas.is_empty(), "{:?}", catalogo.problemas);
    let skill = catalogo.buscar("delegar-bem").expect("skill de exemplo");
    assert!(skill.descricao.contains("sub-agente"));
    let texto = Skills::nova(&raiz).ler("delegar-bem", None).unwrap();
    assert!(texto.contains("references/: niveis.md"));
}
