//! Skills: pacotes de instruções que o Abiyss consulta quando precisa.
//!
//! Cada skill é uma pasta com um `SKILL.md` (frontmatter YAML com `name` e
//! `description`) e, opcionalmente, uma pasta `references/` com material
//! de apoio:
//!
//! ```text
//! skills/
//!   delegar-bem/
//!     SKILL.md
//!     references/niveis.md
//!   pesquisa/              ← categorias (pastas sem SKILL.md) também valem
//!     arxiv/SKILL.md
//! ```
//!
//! **Revelação progressiva:** só `name` + `description` entram no contexto
//! (no system prompt). O texto completo é lido sob demanda pela ferramenta
//! `ler_skill(nome)`, e cada referência por `ler_skill(nome, referencia)`.
//!
//! As skills são SÓ LEITURA para o Abiyss: nenhuma ferramenta escreve aqui
//! e o workspace é recusado se contiver esta pasta.

use std::path::{Path, PathBuf};

use anyhow::bail;

use crate::caminho_seguro::{ler_texto_limitado, resolver_dentro};
use crate::config::Config;
use crate::dados;
use crate::frontmatter;

/// Nome do arquivo principal de uma skill.
pub const ARQUIVO_SKILL: &str = "SKILL.md";
/// Pasta de material de apoio dentro da skill.
pub const PASTA_REFERENCIAS: &str = "references";

/// Até quantos níveis de pasta procurar (`categoria/skill/SKILL.md` = 2).
const PROFUNDIDADE_MAXIMA: usize = 3;
/// Proteção contra pastas enormes.
const MAX_SKILLS: usize = 500;
/// Tamanho máximo de um `SKILL.md` ou de uma referência.
const MAX_BYTES_ARQUIVO: usize = 256 * 1024;

/// Uma skill válida.
#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub nome: String,
    pub descricao: String,
    /// Pasta da skill (onde está o SKILL.md).
    pub pasta: PathBuf,
}

/// Uma pasta com SKILL.md que não pôde ser carregada, e por quê.
#[derive(Debug, Clone, PartialEq)]
pub struct ProblemaSkill {
    pub caminho: PathBuf,
    pub motivo: String,
}

/// Resultado de uma varredura da pasta de skills.
#[derive(Debug, Clone, Default)]
pub struct Catalogo {
    /// Em ordem alfabética do caminho.
    pub skills: Vec<Skill>,
    pub problemas: Vec<ProblemaSkill>,
}

impl Catalogo {
    pub fn buscar(&self, nome: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.nome == nome.trim())
    }
}

/// Acesso à pasta de skills. A pasta é relida a cada uso: editar ou
/// acrescentar uma skill vale na hora, sem reiniciar.
#[derive(Debug, Clone)]
pub struct Skills {
    raiz: PathBuf,
}

impl Skills {
    pub fn nova(raiz: &Path) -> Skills {
        Skills {
            raiz: raiz.to_path_buf(),
        }
    }

    pub fn da_config(config: &Config) -> Skills {
        Skills::nova(&config.caminho_skills())
    }

    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    /// A pasta existe? (Sem ela, a ferramenta `ler_skill` nem é oferecida.)
    pub fn existe(&self) -> bool {
        self.raiz.is_dir()
    }

    /// Varre a pasta e carrega o frontmatter de cada skill.
    pub fn catalogo(&self) -> Catalogo {
        let mut catalogo = Catalogo::default();
        let mut pastas = Vec::new();
        procurar_pastas_de_skill(&self.raiz, 0, &mut pastas);
        pastas.sort();
        for pasta in pastas {
            if catalogo.skills.len() >= MAX_SKILLS {
                catalogo.problemas.push(ProblemaSkill {
                    caminho: pasta,
                    motivo: format!("ignorada: limite de {MAX_SKILLS} skills"),
                });
                continue;
            }
            match carregar_skill(&pasta) {
                Ok(skill) => {
                    if catalogo.buscar(&skill.nome).is_some() {
                        catalogo.problemas.push(ProblemaSkill {
                            caminho: pasta,
                            motivo: format!("nome '{}' repetido; vale a primeira", skill.nome),
                        });
                    } else {
                        catalogo.skills.push(skill);
                    }
                }
                Err(e) => catalogo.problemas.push(ProblemaSkill {
                    caminho: pasta,
                    motivo: format!("{e:#}"),
                }),
            }
        }
        catalogo
    }

    /// Bloco para o system prompt: só nome e descrição de cada skill,
    /// rotulado como dado. `None` se não houver nenhuma skill.
    pub fn indice_para_prompt(&self) -> Option<String> {
        let catalogo = self.catalogo();
        if catalogo.skills.is_empty() {
            return None;
        }
        let linhas: Vec<String> = catalogo
            .skills
            .iter()
            .map(|s| format!("- {}: {}", s.nome, uma_linha(&s.descricao)))
            .collect();
        Some(format!(
            "Abaixo estão só o nome e a descrição de cada skill. Quando uma for útil, \
leia o texto completo com a ferramenta ler_skill(nome) antes de seguir o procedimento.\n{}",
            dados::rotular("skills:indice", &linhas.join("\n"))
        ))
    }

    /// Texto completo do SKILL.md (ou de uma referência, se `referencia`
    /// for informada). O resultado NÃO é rotulado aqui: quem rotula é a
    /// caixa de ferramentas, como para qualquer ferramenta.
    pub fn ler(&self, nome: &str, referencia: Option<&str>) -> anyhow::Result<String> {
        let catalogo = self.catalogo();
        let Some(skill) = catalogo.buscar(nome) else {
            let nomes: Vec<&str> = catalogo.skills.iter().map(|s| s.nome.as_str()).collect();
            if nomes.is_empty() {
                bail!("não há nenhuma skill disponível");
            }
            bail!(
                "skill '{}' não existe. Disponíveis: {}",
                nome.trim(),
                nomes.join(", ")
            );
        };

        let pasta_refs = skill.pasta.join(PASTA_REFERENCIAS);
        match referencia.map(str::trim).filter(|r| !r.is_empty()) {
            Some(arquivo) => {
                if !pasta_refs.is_dir() {
                    bail!(
                        "a skill '{}' não tem pasta {PASTA_REFERENCIAS}/",
                        skill.nome
                    );
                }
                let raiz = pasta_refs.canonicalize()?;
                let caminho = resolver_dentro(&raiz, arquivo, "references/ da skill")?;
                ler_texto_limitado(&caminho, MAX_BYTES_ARQUIVO)
            }
            None => {
                let mut texto =
                    ler_texto_limitado(&skill.pasta.join(ARQUIVO_SKILL), MAX_BYTES_ARQUIVO)?;
                let refs = listar_referencias(&pasta_refs);
                if !refs.is_empty() {
                    texto.push_str(&format!(
                        "\n\n[arquivos em {PASTA_REFERENCIAS}/: {} — leia com ler_skill(nome, referencia)]",
                        refs.join(", ")
                    ));
                }
                Ok(texto)
            }
        }
    }
}

/// Procura pastas que contêm um SKILL.md. Não entra em pastas ocultas
/// (`.git`, `.hub`...), não segue links simbólicos e não procura dentro
/// de uma skill (as subpastas dela são material da própria skill).
fn procurar_pastas_de_skill(pasta: &Path, profundidade: usize, achadas: &mut Vec<PathBuf>) {
    if profundidade > PROFUNDIDADE_MAXIMA {
        return;
    }
    if pasta.join(ARQUIVO_SKILL).is_file() && profundidade > 0 {
        achadas.push(pasta.to_path_buf());
        return;
    }
    let Ok(entradas) = std::fs::read_dir(pasta) else {
        return;
    };
    for entrada in entradas.filter_map(Result::ok) {
        let oculta = entrada.file_name().to_string_lossy().starts_with('.');
        // `file_type` da entrada NÃO segue links: um link para pasta não é pasta aqui.
        let eh_pasta = entrada.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if eh_pasta && !oculta {
            procurar_pastas_de_skill(&entrada.path(), profundidade + 1, achadas);
        }
    }
}

fn carregar_skill(pasta: &Path) -> anyhow::Result<Skill> {
    let texto = ler_texto_limitado(&pasta.join(ARQUIVO_SKILL), MAX_BYTES_ARQUIVO)?;
    let doc = frontmatter::separar(&texto)?;
    if !doc.tem_frontmatter {
        bail!("SKILL.md sem frontmatter (precisa de 'name' e 'description')");
    }
    let Some(nome) = doc.texto("name") else {
        bail!("falta o campo 'name' no frontmatter");
    };
    let Some(descricao) = doc.texto("description") else {
        bail!("falta o campo 'description' no frontmatter");
    };
    let nome_pasta = pasta
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if nome != nome_pasta {
        tracing::debug!("skill '{nome}' está na pasta '{nome_pasta}' (nomes diferentes)");
    }
    Ok(Skill {
        nome,
        descricao,
        pasta: pasta.to_path_buf(),
    })
}

/// Arquivos da pasta references/ (só o primeiro nível, sem links).
fn listar_referencias(pasta: &Path) -> Vec<String> {
    let Ok(entradas) = std::fs::read_dir(pasta) else {
        return Vec::new();
    };
    let mut nomes: Vec<String> = entradas
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    nomes.sort();
    nomes
}

/// Junta as linhas de uma descrição numa linha só (para o índice).
fn uma_linha(texto: &str) -> String {
    texto.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod testes {
    use super::*;

    fn criar(raiz: &Path, pasta: &str, conteudo: &str) {
        let p = raiz.join(pasta);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join(ARQUIVO_SKILL), conteudo).unwrap();
    }

    #[test]
    fn carrega_skills_e_relata_problemas() {
        let pasta = tempfile::tempdir().unwrap();
        let raiz = pasta.path();
        criar(
            raiz,
            "b-skill",
            "---\nname: b-skill\ndescription: Faz B.\n---\n# B\n",
        );
        criar(
            raiz,
            "categoria/a-skill",
            "---\nname: a-skill\ndescription: |\n  Faz A\n  em duas linhas.\n---\ncorpo A",
        );
        criar(raiz, "sem-frontmatter", "# só texto");
        criar(raiz, "sem-descricao", "---\nname: x\n---\n");
        criar(
            raiz,
            "repetida",
            "---\nname: b-skill\ndescription: outra\n---\n",
        );
        criar(
            raiz,
            ".oculta/c",
            "---\nname: c\ndescription: nunca vista\n---\n",
        );

        let catalogo = Skills::nova(raiz).catalogo();
        let nomes: Vec<&str> = catalogo.skills.iter().map(|s| s.nome.as_str()).collect();
        assert_eq!(nomes, vec!["b-skill", "a-skill"]);
        assert_eq!(catalogo.problemas.len(), 3);
        assert!(
            catalogo
                .problemas
                .iter()
                .any(|p| p.motivo.contains("repetido"))
        );

        let indice = Skills::nova(raiz).indice_para_prompt().unwrap();
        assert!(indice.contains("- a-skill: Faz A em duas linhas."));
        assert!(!indice.contains("corpo A"));
    }

    #[test]
    fn pasta_inexistente_e_catalogo_vazio() {
        let skills = Skills::nova(Path::new("/caminho/que/nao/existe"));
        assert!(!skills.existe());
        assert!(skills.catalogo().skills.is_empty());
        assert!(skills.indice_para_prompt().is_none());
        assert!(skills.ler("x", None).is_err());
    }
}
