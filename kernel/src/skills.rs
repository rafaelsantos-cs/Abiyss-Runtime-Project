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
//!
//! **Confiança.** Pode haver várias pastas (raízes). Skills de uma raiz
//! confiável (por padrão, as versionadas dentro do projeto) têm a mesma
//! confiança do núcleo: lê-las NÃO marca o contexto como externo. Skills de
//! fora (pasta do Hermes, hubs) são conteúdo externo para a regra dura da
//! memória.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

/// Uma pasta de skills e se ela é confiável.
#[derive(Debug, Clone, PartialEq)]
pub struct RaizSkills {
    pub caminho: PathBuf,
    pub confiavel: bool,
}

/// Uma skill válida.
#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub nome: String,
    pub descricao: String,
    /// Pasta da skill (onde está o SKILL.md).
    pub pasta: PathBuf,
    /// Veio de uma raiz confiável?
    pub confiavel: bool,
}

/// O texto de uma skill e se ele é confiável.
#[derive(Debug, Clone, PartialEq)]
pub struct LeituraSkill {
    pub nome: String,
    pub texto: String,
    pub confiavel: bool,
}

/// Instrução que acompanha o índice no chat (lá existe a ferramenta).
pub const INSTRUCAO_INDICE_CHAT: &str = "Abaixo estão só o nome e a descrição de cada skill. \
Quando uma for útil, leia o texto completo com a ferramenta ler_skill(nome) antes de seguir o procedimento.";

/// Skills automáticas ausentes já avisadas (o aviso sai uma vez por processo).
static AVISADAS: Mutex<Vec<String>> = Mutex::new(Vec::new());

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

/// Acesso às pastas de skills. As pastas são relidas a cada uso: editar
/// ou acrescentar uma skill vale na hora, sem reiniciar.
#[derive(Debug, Clone)]
pub struct Skills {
    raizes: Vec<RaizSkills>,
}

impl Skills {
    /// Uma raiz só, NÃO confiável (o padrão conservador).
    pub fn nova(raiz: &Path) -> Skills {
        Skills::com_raizes(vec![RaizSkills {
            caminho: raiz.to_path_buf(),
            confiavel: false,
        }])
    }

    pub fn com_raizes(raizes: Vec<RaizSkills>) -> Skills {
        Skills { raizes }
    }

    pub fn da_config(config: &Config) -> Skills {
        Skills::com_raizes(config.raizes_skills())
    }

    pub fn raizes(&self) -> &[RaizSkills] {
        &self.raizes
    }

    /// Alguma pasta existe? (Sem nenhuma, a ferramenta `ler_skill` nem é oferecida.)
    pub fn existe(&self) -> bool {
        self.raizes.iter().any(|r| r.caminho.is_dir())
    }

    /// Varre as pastas (em ordem) e carrega o frontmatter de cada skill.
    /// Nome repetido: vale a primeira (a raiz que vem antes ganha).
    pub fn catalogo(&self) -> Catalogo {
        let mut catalogo = Catalogo::default();
        let mut pastas = Vec::new();
        for raiz in &self.raizes {
            let mut desta = Vec::new();
            procurar_pastas_de_skill(&raiz.caminho, 0, &mut desta);
            desta.sort();
            pastas.extend(desta.into_iter().map(|p| (p, raiz.confiavel)));
        }
        for (pasta, confiavel) in pastas {
            if catalogo.skills.len() >= MAX_SKILLS {
                catalogo.problemas.push(ProblemaSkill {
                    caminho: pasta,
                    motivo: format!("ignorada: limite de {MAX_SKILLS} skills"),
                });
                continue;
            }
            match carregar_skill(&pasta, confiavel) {
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

    /// Bloco para o system prompt do chat: só nome e descrição de cada
    /// skill, rotulado como dado. `None` se não houver nenhuma skill.
    pub fn indice_para_prompt(&self) -> Option<String> {
        self.indice_para_prompt_com(INSTRUCAO_INDICE_CHAT)
    }

    /// Como `indice_para_prompt`, com outra instrução no topo (o heartbeat
    /// não tem a ferramenta `ler_skill`: lá é a ação `consultar_skill`).
    pub fn indice_para_prompt_com(&self, instrucao: &str) -> Option<String> {
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
            "{instrucao}\n{}",
            dados::rotular("skills:indice", &linhas.join("\n"))
        ))
    }

    /// A skill existe e é confiável? `None` se não existe.
    pub fn confiavel(&self, nome: &str) -> Option<bool> {
        self.catalogo().buscar(nome).map(|s| s.confiavel)
    }

    /// Texto completo de uma skill CONFIÁVEL, para o kernel colocar no
    /// contexto sozinho (skills automáticas). Skill ausente, quebrada ou
    /// não confiável = `None`, com um aviso no log (uma vez por nome).
    pub fn texto_confiavel(&self, nome: &str) -> Option<String> {
        let nome = nome.trim();
        if nome.is_empty() {
            return None;
        }
        let motivo = match self.ler_detalhado(nome, None) {
            Ok(l) if l.confiavel => return Some(l.texto),
            Ok(_) => "não é de uma pasta confiável".to_string(),
            Err(e) => format!("{e:#}"),
        };
        let mut avisadas = AVISADAS.lock().unwrap_or_else(|e| e.into_inner());
        if !avisadas.iter().any(|n| n == nome) {
            avisadas.push(nome.to_string());
            tracing::warn!("skill automática '{nome}' não será usada: {motivo}");
        }
        None
    }

    /// Texto completo do SKILL.md (ou de uma referência, se `referencia`
    /// for informada). O resultado NÃO é rotulado aqui: quem rotula é a
    /// caixa de ferramentas, como para qualquer ferramenta.
    pub fn ler(&self, nome: &str, referencia: Option<&str>) -> anyhow::Result<String> {
        Ok(self.ler_detalhado(nome, referencia)?.texto)
    }

    /// Como `ler`, dizendo também se a skill é confiável.
    pub fn ler_detalhado(
        &self,
        nome: &str,
        referencia: Option<&str>,
    ) -> anyhow::Result<LeituraSkill> {
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
        let texto = match referencia.map(str::trim).filter(|r| !r.is_empty()) {
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
        }?;
        Ok(LeituraSkill {
            nome: skill.nome.clone(),
            texto,
            confiavel: skill.confiavel,
        })
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

fn carregar_skill(pasta: &Path, confiavel: bool) -> anyhow::Result<Skill> {
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
        confiavel,
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
