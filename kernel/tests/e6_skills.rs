//! E6: coerência das skills do repositório — todas carregam, seguem as
//! regras de formato e só citam ferramentas, ações e estados que existem.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use abiyss::ferramentas;
use abiyss::frontmatter;
use abiyss::goals::EstadoGoal;
use abiyss::heartbeat::NOMES_ACOES;
use abiyss::skills::{ARQUIVO_SKILL, PASTA_REFERENCIAS, RaizSkills, Skill, Skills};
use serde_json::Value;

const ESPERADAS: [&str; 6] = [
    "registrar-memoria",
    "conduzir-goals",
    "delegar-bem",
    "dormir-bem",
    "planejar-o-dia",
    "sair-de-loops",
];

fn raiz() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../skills")
}

fn skills_do_repositorio() -> Vec<Skill> {
    let skills = Skills::com_raizes(vec![RaizSkills {
        caminho: raiz(),
        confiavel: true,
    }]);
    let catalogo = skills.catalogo();
    assert!(
        catalogo.problemas.is_empty(),
        "skills que não carregaram: {:?}",
        catalogo.problemas
    );
    catalogo.skills
}

/// Nomes que uma seção "Ferramentas e ações" pode citar.
fn nomes_conhecidos(skills: &[Skill]) -> HashSet<String> {
    let mut nomes: HashSet<String> = [
        ferramentas::LER_ARQUIVO,
        ferramentas::LISTAR_ARQUIVOS,
        ferramentas::ESCREVER_ARQUIVO,
        ferramentas::LER_SKILL,
        ferramentas::MEMORIA_BUSCAR,
        ferramentas::MEMORIA_LER,
        ferramentas::MEMORIA_PROPOR,
        ferramentas::DELEGAR,
        ferramentas::STATUS,
        ferramentas::CANCELAR,
    ]
    .iter()
    .map(|n| n.to_string())
    .collect();
    nomes.extend(NOMES_ACOES.iter().map(|n| n.to_string()));
    nomes.extend(EstadoGoal::TODOS.iter().map(|e| e.como_texto().to_string()));
    nomes.extend(skills.iter().map(|s| s.nome.clone()));
    nomes
}

/// Trechos entre crases de uma seção `## <titulo>` (até a próxima `## `).
fn nomes_da_secao(corpo: &str, titulo: &str) -> Option<Vec<String>> {
    let cabecalho = format!("## {titulo}");
    let inicio = corpo.lines().position(|l| l.trim() == cabecalho)?;
    let mut nomes = Vec::new();
    for linha in corpo.lines().skip(inicio + 1) {
        if linha.starts_with("## ") {
            break;
        }
        let partes: Vec<&str> = linha.split('`').collect();
        // Entre crases = posições ímpares.
        nomes.extend(partes.iter().skip(1).step_by(2).map(|s| s.to_string()));
    }
    Some(nomes)
}

/// Arquivos citados como `references/<arquivo>`.
fn referencias_citadas(texto: &str) -> Vec<String> {
    let marca = format!("{PASTA_REFERENCIAS}/");
    texto
        .match_indices(&marca)
        .map(|(i, _)| {
            texto[i + marca.len()..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
                .collect::<String>()
                .trim_end_matches('.')
                .to_string()
        })
        .filter(|n| !n.is_empty())
        .collect()
}

#[test]
fn skills_do_repositorio_sao_coerentes() {
    let skills = skills_do_repositorio();
    let nomes: Vec<&str> = skills.iter().map(|s| s.nome.as_str()).collect();
    for esperada in ESPERADAS {
        assert!(nomes.contains(&esperada), "falta a skill {esperada}");
    }
    let conhecidos = nomes_conhecidos(&skills);

    for skill in &skills {
        let nome = &skill.nome;
        // name: igual à pasta, minúsculas/números/hífens, até 64.
        let pasta = skill.pasta.file_name().unwrap().to_string_lossy();
        assert_eq!(&pasta, nome, "name diferente da pasta");
        assert!(nome.len() <= 64, "{nome}: name longo demais");
        assert!(
            nome.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{nome}: só minúsculas, números e hífens"
        );
        // description: até 1.024 caracteres.
        let tamanho = skill.descricao.chars().count();
        assert!(
            tamanho > 0 && tamanho <= 1024,
            "{nome}: description com {tamanho} caracteres"
        );

        // Corpo: até 500 linhas.
        let texto = std::fs::read_to_string(skill.pasta.join(ARQUIVO_SKILL)).unwrap();
        let doc = frontmatter::separar(&texto).unwrap();
        let linhas = doc.corpo.lines().count();
        assert!(linhas <= 500, "{nome}: corpo com {linhas} linhas");

        // Referências citadas existem; as longas começam com índice.
        for referencia in referencias_citadas(&doc.corpo) {
            let arquivo = skill.pasta.join(PASTA_REFERENCIAS).join(&referencia);
            assert!(
                arquivo.is_file(),
                "{nome}: referência {referencia} não existe"
            );
        }
        if let Ok(entradas) = std::fs::read_dir(skill.pasta.join(PASTA_REFERENCIAS)) {
            for entrada in entradas.flatten() {
                let texto = std::fs::read_to_string(entrada.path()).unwrap();
                if texto.lines().count() > 100 {
                    assert!(
                        texto
                            .lines()
                            .take(20)
                            .any(|l| l.to_lowercase().contains("índice")),
                        "{nome}: referência longa sem índice: {}",
                        entrada.path().display()
                    );
                }
            }
        }

        // Nomes citados em "Ferramentas e ações" existem no kernel.
        if let Some(citados) = nomes_da_secao(&doc.corpo, "Ferramentas e ações") {
            assert!(!citados.is_empty(), "{nome}: seção de ferramentas vazia");
            for citado in citados {
                assert!(
                    conhecidos.contains(&citado),
                    "{nome}: `{citado}` não é ferramenta, ação, estado de goal nem skill"
                );
            }
        }

        // evals.json: 3 cenários com situação e comportamento esperado.
        let evals = std::fs::read_to_string(skill.pasta.join("evals.json"))
            .unwrap_or_else(|_| panic!("{nome}: falta evals.json"));
        let evals: Value = serde_json::from_str(&evals).unwrap();
        let lista = evals.as_array().expect("evals.json é uma lista");
        assert_eq!(lista.len(), 3, "{nome}: evals.json precisa de 3 cenários");
        for cenario in lista {
            for campo in ["situacao", "esperado"] {
                assert!(
                    cenario[campo]
                        .as_str()
                        .is_some_and(|t| !t.trim().is_empty()),
                    "{nome}: cenário sem '{campo}'"
                );
            }
        }
    }
}

#[test]
fn skills_automaticas_padrao_existem_e_sao_confiaveis() {
    let skills = Skills::com_raizes(vec![RaizSkills {
        caminho: raiz(),
        confiavel: true,
    }]);
    let padrao = abiyss::config::ConfigSkillsAutomaticas::default();
    for nome in [&padrao.estagnacao, &padrao.sono, &padrao.despertar] {
        assert!(
            skills.texto_confiavel(nome).is_some(),
            "a skill automática {nome} não existe ou não é confiável"
        );
    }
}

#[test]
fn verificador_pega_nome_inventado() {
    let corpo = "# X\n\n## Ferramentas e ações\n\n- `memoria_propor`: ok\n- `voar`: não existe\n\n## Outra\n\n`ignorado`\n";
    assert_eq!(
        nomes_da_secao(corpo, "Ferramentas e ações").unwrap(),
        vec!["memoria_propor", "voar"]
    );
    assert_eq!(
        referencias_citadas("veja `references/niveis.md`. E references/nota-externa.md."),
        vec!["niveis.md", "nota-externa.md"]
    );
}
