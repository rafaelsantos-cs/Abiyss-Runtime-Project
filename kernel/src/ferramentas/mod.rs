//! Ferramentas que o modelo pode chamar (tool calling).
//!
//! `CaixaDeFerramentas` junta todas as ferramentas disponíveis numa sessão
//! (nativas do workspace, skills e as dos servidores MCP), gera as definições
//! enviadas ao modelo e executa as chamadas. Todo resultado volta
//! ROTULADO como dado (ver `crate::dados`).
//!
//! Cada resultado também diz se trouxe conteúdo EXTERNO para o contexto
//! (`origem_externa`). A conversa guarda essa marca no histórico e a passa
//! adiante (`ContextoChamada`): é assim que o kernel sabe, por código, se
//! uma proposta de memória pode ter vindo de ferramentas ou da web.

pub mod workspace;

use std::sync::Arc;

use serde_json::{Value, json};

use crate::config::Config;
use crate::dados;
use crate::mcp::PonteMcp;
use crate::memoria::nota::{Escopo, EscopoBusca, Fonte};
use crate::memoria::{Memoria, PedidoProposta};
use crate::nim::{ChamadaFerramenta, Ferramenta};
use crate::skills::Skills;
use crate::subagentes::ControleSubagentes;
use workspace::Workspace;

/// Nomes das ferramentas nativas.
pub const LER_ARQUIVO: &str = "ler_arquivo";
pub const LISTAR_ARQUIVOS: &str = "listar_arquivos";
pub const ESCREVER_ARQUIVO: &str = "escrever_arquivo";
pub const LER_SKILL: &str = "ler_skill";
pub const MEMORIA_BUSCAR: &str = "memoria_buscar";
pub const MEMORIA_LER: &str = "memoria_ler";
pub const MEMORIA_PROPOR: &str = "memoria_propor";

/// Resultado de uma ferramenta, pronto para virar mensagem `tool`.
#[derive(Debug, Clone)]
pub struct ResultadoFerramenta {
    /// Texto já rotulado como dado.
    pub texto: String,
    /// A ferramenta falhou?
    pub erro: bool,
    /// `Some(rotulo)` se o resultado traz conteúdo externo (web, MCP,
    /// arquivos do workspace, skills, memória externa...). `None` = só
    /// conteúdo interno (memória interna, confirmações do kernel).
    pub origem_externa: Option<String>,
}

/// O que o kernel sabe sobre o contexto em que o modelo pediu a chamada.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContextoChamada {
    /// `Some("mcp:x, ler_arquivo")` se o contexto tinha conteúdo externo.
    pub origem_externa: Option<String>,
}

impl ContextoChamada {
    /// Contexto sem nada externo.
    pub fn limpo() -> ContextoChamada {
        ContextoChamada::default()
    }

    /// A partir das origens externas presentes no contexto.
    pub fn com_origens(origens: &[String]) -> ContextoChamada {
        ContextoChamada {
            origem_externa: if origens.is_empty() {
                None
            } else {
                Some(origens.join(", "))
            },
        }
    }

    /// Quem chama não rastreia origem (ex.: sub-agentes): por segurança,
    /// tudo conta como externo.
    pub fn sem_rastreio() -> ContextoChamada {
        ContextoChamada {
            origem_externa: Some("contexto sem rastreio de origem".to_string()),
        }
    }
}

/// Ferramentas de orquestração: só o Abiyss principal tem acesso.
pub const DELEGAR: &str = "delegar";
pub const STATUS: &str = "status";
pub const CANCELAR: &str = "cancelar";

/// Ferramentas nativas (as que o kernel implementa em Rust).
const NATIVAS: &[&str] = &[LER_ARQUIVO, LISTAR_ARQUIVOS, ESCREVER_ARQUIVO, LER_SKILL];
const ORQUESTRACAO: &[&str] = &[DELEGAR, STATUS, CANCELAR];
const MEMORIA: &[&str] = &[MEMORIA_BUSCAR, MEMORIA_LER, MEMORIA_PROPOR];

pub struct CaixaDeFerramentas {
    workspace: Workspace,
    /// Skills (só leitura). `None` = sem `ler_skill`.
    skills: Option<Skills>,
    /// Memória de longo prazo. `None` = sem `memoria_*`.
    memoria: Option<Arc<Memoria>>,
    mcp: Arc<PonteMcp>,
    /// Ferramentas de sub-agentes (`delegar`, `status`, `cancelar`).
    /// `None` em caixas restritas: sub-agente nunca cria sub-agente.
    subagentes: Option<ControleSubagentes>,
    /// Se `Some`, só estas ferramentas aparecem e podem ser executadas.
    /// Aceita curinga no fim: "exemplo__*".
    permitidas: Option<Vec<String>>,
}

impl CaixaDeFerramentas {
    /// Monta a caixa a partir da config (abre e valida o workspace).
    pub fn da_config(config: &Config) -> anyhow::Result<CaixaDeFerramentas> {
        let workspace = Workspace::abrir(
            &config.caminho_workspace(),
            &config.areas_protegidas(),
            config.ferramentas.clone(),
        )?;
        Ok(CaixaDeFerramentas::nova(workspace).com_skills(Skills::da_config(config)))
    }

    pub fn nova(workspace: Workspace) -> CaixaDeFerramentas {
        CaixaDeFerramentas {
            workspace,
            skills: None,
            memoria: None,
            mcp: Arc::new(PonteMcp::vazia()),
            subagentes: None,
            permitidas: None,
        }
    }

    /// Acrescenta `delegar`, `status` e `cancelar`.
    pub fn com_subagentes(mut self, controle: ControleSubagentes) -> CaixaDeFerramentas {
        self.subagentes = Some(controle);
        self
    }

    /// Acrescenta `ler_skill` (só aparece se a pasta de skills existir).
    pub fn com_skills(mut self, skills: Skills) -> CaixaDeFerramentas {
        self.skills = Some(skills);
        self
    }

    /// Acrescenta `memoria_buscar`, `memoria_ler` e `memoria_propor`.
    pub fn com_memoria(mut self, memoria: Arc<Memoria>) -> CaixaDeFerramentas {
        self.memoria = Some(memoria);
        self
    }

    /// Acrescenta as ferramentas dos servidores MCP.
    pub fn com_mcp(mut self, mcp: Arc<PonteMcp>) -> CaixaDeFerramentas {
        self.mcp = mcp;
        self
    }

    /// Cópia desta caixa só com as ferramentas da lista (usado pelos
    /// sub-agentes). Aceita curinga no fim do nome: "exemplo__*".
    pub fn restrita(&self, nomes: &[String]) -> CaixaDeFerramentas {
        CaixaDeFerramentas {
            workspace: self.workspace.clone(),
            skills: self.skills.clone(),
            memoria: self.memoria.clone(),
            mcp: Arc::clone(&self.mcp),
            // Regra do kernel: caixa restrita (sub-agente) não delega.
            subagentes: None,
            permitidas: Some(nomes.to_vec()),
        }
    }

    fn permitida(&self, nome: &str) -> bool {
        match &self.permitidas {
            None => true,
            Some(lista) => lista.iter().any(|padrao| match padrao.strip_suffix('*') {
                Some(prefixo) => nome.starts_with(prefixo),
                None => nome == padrao,
            }),
        }
    }

    /// Skills disponíveis E permitidas nesta caixa?
    fn skills_ativas(&self) -> Option<&Skills> {
        self.skills
            .as_ref()
            .filter(|s| s.existe() && self.permitida(LER_SKILL))
    }

    /// `ler_skill` vai ler uma skill confiável? (Skill inexistente ou de
    /// pasta não confiável = não; na dúvida, conta como externa.)
    fn skill_confiavel(&self, args: &Value) -> bool {
        match (self.skills_ativas(), args["nome"].as_str()) {
            (Some(skills), Some(nome)) => skills.confiavel(nome) == Some(true),
            _ => false,
        }
    }

    /// Índice das skills (nome + descrição) para o system prompt.
    /// `None` se esta caixa não tem `ler_skill` ou não há skills.
    pub fn indice_skills(&self) -> Option<String> {
        self.skills_ativas()?.indice_para_prompt()
    }

    /// Definições (nome, descrição, JSON Schema) enviadas ao modelo.
    pub fn definicoes(&self) -> Vec<Ferramenta> {
        let skills = if self.skills_ativas().is_some() {
            vec![definicao_ler_skill()]
        } else {
            vec![]
        };
        let todas = vec![
            Ferramenta::nova(
                LER_ARQUIVO,
                "Lê um arquivo de texto do workspace. Caminho relativo ao workspace.",
                json!({
                    "type": "object",
                    "properties": {
                        "caminho": {"type": "string", "description": "Ex.: notas/ideias.md"}
                    },
                    "required": ["caminho"]
                }),
            ),
            Ferramenta::nova(
                LISTAR_ARQUIVOS,
                "Lista arquivos e pastas do workspace.",
                json!({
                    "type": "object",
                    "properties": {
                        "caminho": {"type": "string", "description": "Pasta relativa ao workspace; vazio = raiz"},
                        "recursivo": {"type": "boolean", "description": "Incluir subpastas"}
                    }
                }),
            ),
            Ferramenta::nova(
                ESCREVER_ARQUIVO,
                "Cria ou altera um arquivo de texto no workspace (só dentro do workspace).",
                json!({
                    "type": "object",
                    "properties": {
                        "caminho": {"type": "string", "description": "Ex.: notas/ideias.md"},
                        "conteudo": {"type": "string"},
                        "acrescentar": {"type": "boolean", "description": "true = adiciona ao fim em vez de sobrescrever"}
                    },
                    "required": ["caminho", "conteudo"]
                }),
            ),
        ];
        let memoria = if self.memoria.is_some() {
            definicoes_memoria()
        } else {
            vec![]
        };
        let orquestracao = if self.subagentes.is_some() {
            definicoes_orquestracao()
        } else {
            vec![]
        };
        todas
            .into_iter()
            .chain(skills)
            .chain(memoria)
            .chain(orquestracao)
            .chain(self.mcp.definicoes())
            .filter(|f| self.permitida(f.nome()))
            .collect()
    }

    /// Executa uma chamada sem saber de onde veio o contexto (sub-agentes):
    /// por segurança, conta como contexto com conteúdo externo.
    pub async fn executar(&self, chamada: &ChamadaFerramenta) -> ResultadoFerramenta {
        self.executar_com(chamada, &ContextoChamada::sem_rastreio())
            .await
    }

    /// Executa uma chamada pedida pelo modelo. Nunca devolve `Err`: erros
    /// viram texto para o modelo ler e se corrigir.
    pub async fn executar_com(
        &self,
        chamada: &ChamadaFerramenta,
        contexto: &ContextoChamada,
    ) -> ResultadoFerramenta {
        let nome = chamada.function.name.as_str();
        let eh_mcp = self.mcp.tem(nome);
        let mut origem_externa = None;
        let resultado = if !self.permitida(nome) {
            Err(anyhow::anyhow!(
                "ferramenta '{nome}' não está disponível aqui"
            ))
        } else {
            match interpretar_argumentos(&chamada.function.arguments) {
                Err(e) => Err(e),
                Ok(args) => {
                    origem_externa = classificar_origem(nome, &args, eh_mcp);
                    // Skill de pasta confiável (versionada no projeto) tem a
                    // confiança do núcleo: não é conteúdo externo.
                    if nome == LER_SKILL && self.skill_confiavel(&args) {
                        origem_externa = None;
                    }
                    if NATIVAS.contains(&nome) {
                        self.executar_nativa(nome, &args)
                    } else if MEMORIA.contains(&nome) {
                        self.executar_memoria(nome, &args, contexto).await
                    } else if ORQUESTRACAO.contains(&nome) {
                        self.executar_orquestracao(nome, &args)
                    } else if eh_mcp {
                        self.mcp.chamar(nome, args).await
                    } else {
                        origem_externa = None;
                        Err(anyhow::anyhow!("ferramenta desconhecida: '{nome}'"))
                    }
                }
            }
        };

        // Resultados de MCP levam o prefixo "mcp:" na origem do rótulo.
        let origem = if eh_mcp {
            format!("mcp:{nome}")
        } else {
            nome.to_string()
        };
        match resultado {
            Ok(texto) => ResultadoFerramenta {
                texto: dados::rotular(&origem, &texto),
                erro: false,
                origem_externa,
            },
            Err(e) => {
                tracing::debug!("ferramenta {nome} falhou: {e:#}");
                ResultadoFerramenta {
                    texto: dados::rotular(&origem, &format!("ERRO: {e:#}")),
                    erro: true,
                    origem_externa,
                }
            }
        }
    }

    async fn executar_memoria(
        &self,
        nome: &str,
        args: &Value,
        contexto: &ContextoChamada,
    ) -> anyhow::Result<String> {
        let Some(memoria) = &self.memoria else {
            anyhow::bail!("ferramenta '{nome}' não está disponível aqui");
        };
        match nome {
            MEMORIA_BUSCAR => {
                let consulta = texto_obrigatorio(args, "consulta")?;
                let texto_escopo = args["escopo"].as_str().unwrap_or("ambos");
                let escopo = EscopoBusca::de_texto(texto_escopo).ok_or_else(|| {
                    anyhow::anyhow!(
                        "escopo '{texto_escopo}' inválido: use interno, externo ou ambos"
                    )
                })?;
                let resposta = memoria.buscar(&consulta, escopo).await?;
                if resposta.resultados.is_empty() {
                    return Ok(format!(
                        "Nada encontrado para \"{consulta}\" (escopo: {}, motor: {}).",
                        escopo.como_texto(),
                        resposta.motor
                    ));
                }
                let mut texto = format!(
                    "{} resultado(s) para \"{consulta}\" (escopo: {}, motor: {}):",
                    resposta.resultados.len(),
                    escopo.como_texto(),
                    resposta.motor
                );
                for r in &resposta.resultados {
                    texto.push_str(&format!(
                        "\n- {} [{}]: {}",
                        r.caminho,
                        r.escopo.como_texto(),
                        r.trecho
                    ));
                }
                Ok(texto)
            }
            MEMORIA_LER => {
                let leitura = memoria.ler(&texto_obrigatorio(args, "caminho")?)?;
                let nota = &leitura.nota;
                let mut texto = format!(
                    "Nota: {} (escopo {})\n\n{}",
                    nota.caminho,
                    nota.escopo.como_texto(),
                    nota.texto.trim_end()
                );
                if !leitura.links.is_empty() {
                    texto.push_str("\n\nLinks:");
                    for (alvo, achada) in &leitura.links {
                        let destino = match achada {
                            Some(c) => match Escopo::do_caminho(c) {
                                Some(e) => format!("{c} ({})", e.como_texto()),
                                None => c.clone(),
                            },
                            None => "(nota não encontrada)".to_string(),
                        };
                        texto.push_str(&format!("\n- [[{alvo}]] → {destino}"));
                    }
                }
                Ok(texto)
            }
            MEMORIA_PROPOR => {
                let pedido = PedidoProposta {
                    escopo: texto_obrigatorio(args, "escopo")?,
                    caminho: texto_obrigatorio(args, "caminho")?,
                    conteudo: texto_obrigatorio(args, "conteudo")?,
                    tipo: texto_obrigatorio(args, "tipo")?,
                    fonte: Fonte::Conversa,
                    // A origem vem do KERNEL (histórico), nunca dos argumentos.
                    origem_externa: contexto.origem_externa.clone(),
                };
                let registrada = memoria.propor(&pedido)?;
                let mut texto = format!(
                    "Proposta #{} registrada para {} (pendente: será aplicada ou rejeitada no próximo sleep).",
                    registrada.id, registrada.caminho
                );
                if let Some(aviso) = registrada.aviso {
                    texto.push_str(&format!("\nATENÇÃO: {aviso}"));
                }
                Ok(texto)
            }
            _ => anyhow::bail!("ferramenta desconhecida: '{nome}'"),
        }
    }

    fn executar_orquestracao(&self, nome: &str, args: &Value) -> anyhow::Result<String> {
        let Some(controle) = &self.subagentes else {
            anyhow::bail!("ferramenta '{nome}' não está disponível aqui");
        };
        match nome {
            DELEGAR => {
                let pedido = ControleSubagentes::pedido_de_argumentos(args)?;
                let id = controle.delegar(&pedido, "chat")?;
                Ok(json!({
                    "id": id,
                    "estado": "pendente",
                    "observacao": "o sub-agente roda em segundo plano (no daemon); o resultado chega como evento na fila e pode ser consultado com status(id)"
                })
                .to_string())
            }
            STATUS => {
                let id = args["id"]
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("falta o 'id' numérico"))?;
                Ok(serde_json::to_string_pretty(
                    &controle.status(id)?.como_json(),
                )?)
            }
            CANCELAR => {
                let id = args["id"]
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("falta o 'id' numérico"))?;
                controle.cancelar(id)
            }
            _ => anyhow::bail!("ferramenta desconhecida: '{nome}'"),
        }
    }

    fn executar_nativa(&self, nome: &str, args: &Value) -> anyhow::Result<String> {
        match nome {
            LER_ARQUIVO => self.workspace.ler(&texto_obrigatorio(args, "caminho")?),
            LISTAR_ARQUIVOS => {
                let caminho = args["caminho"].as_str().unwrap_or("");
                let recursivo = args["recursivo"].as_bool().unwrap_or(false);
                self.workspace.listar(caminho, recursivo)
            }
            ESCREVER_ARQUIVO => {
                let caminho = texto_obrigatorio(args, "caminho")?;
                let conteudo = texto_obrigatorio(args, "conteudo")?;
                let acrescentar = args["acrescentar"].as_bool().unwrap_or(false);
                self.workspace.escrever(&caminho, &conteudo, acrescentar)
            }
            LER_SKILL => {
                let Some(skills) = self.skills_ativas() else {
                    anyhow::bail!("não há skills disponíveis");
                };
                let nome = texto_obrigatorio(args, "nome")?;
                skills.ler(&nome, args["referencia"].as_str())
            }
            _ => anyhow::bail!("ferramenta desconhecida: '{nome}'"),
        }
    }
}

/// Este resultado traz conteúdo externo para o contexto? Devolve o rótulo
/// da origem (ex.: "mcp:exemplo__somar") ou `None` se for só conteúdo
/// interno. Na dúvida, conta como externo.
fn classificar_origem(nome: &str, args: &Value, eh_mcp: bool) -> Option<String> {
    match nome {
        // Só confirmam o que o kernel fez: não trazem texto de fora.
        ESCREVER_ARQUIVO | DELEGAR | CANCELAR | MEMORIA_PROPOR => None,
        // A memória interna é do próprio Abiyss (e só entra lá o que
        // passou pela regra dura); a externa conta como conteúdo externo.
        MEMORIA_BUSCAR => match EscopoBusca::de_texto(args["escopo"].as_str().unwrap_or("ambos")) {
            Some(EscopoBusca::Interno) => None,
            _ => Some("memoria_buscar(externo)".to_string()),
        },
        MEMORIA_LER => {
            let caminho = args["caminho"].as_str().unwrap_or("").trim();
            match Escopo::do_caminho(caminho.trim_start_matches("./")) {
                Some(Escopo::Interno) => None,
                _ => Some(format!("memoria_ler({caminho})")),
            }
        }
        _ if eh_mcp => Some(format!("mcp:{nome}")),
        // Arquivos do workspace, skills (as confiáveis são liberadas por quem
        // chama), relatórios de sub-agentes...
        _ => Some(nome.to_string()),
    }
}

/// Definições de `memoria_buscar`, `memoria_ler` e `memoria_propor`.
fn definicoes_memoria() -> Vec<Ferramenta> {
    vec![
        Ferramenta::nova(
            MEMORIA_BUSCAR,
            "Busca na sua memória de longo prazo (o cofre). Escopos: 'interno' (pessoas, preferências, \
             auto-modelo, diário, procedimentos), 'externo' (mapa de fontes: links canônicos e resumos \
             datados) ou 'ambos'. Resultados do escopo externo contam como conteúdo externo.",
            json!({
                "type": "object",
                "properties": {
                    "consulta": {"type": "string", "description": "Palavras a procurar"},
                    "escopo": {"type": "string", "enum": ["interno", "externo", "ambos"]}
                },
                "required": ["consulta", "escopo"]
            }),
        ),
        Ferramenta::nova(
            MEMORIA_LER,
            "Lê uma nota do cofre pelo caminho (ex.: 01_internal/pessoas/ana.md), com os [[wikilinks]] resolvidos.",
            json!({
                "type": "object",
                "properties": {
                    "caminho": {"type": "string", "description": "Relativo ao cofre, começando com 01_internal/ ou 02_external/"}
                },
                "required": ["caminho"]
            }),
        ),
        Ferramenta::nova(
            MEMORIA_PROPOR,
            "Propõe gravar algo na memória. NÃO grava na hora: vai para uma fila aplicada (ou rejeitada) \
             no próximo sleep, e o conteúdo é ACRESCENTADO ao fim da nota. Escopo 'interno': sobre você \
             e quem você conhece, aprendido em conversa; conteúdo vindo de ferramentas, web ou \
             sub-agentes é REJEITADO aqui pelo kernel. Escopo 'externo': mapa de fontes; o conteúdo \
             precisa começar com frontmatter YAML com links (site oficial, changelog, docs), \
             navegador (rapido|contemplativo|agentico) e revalidar_apos (AAAA-MM-DD); resumo em cache \
             só com data no título, ex.: '## Resumo em cache (AAAA-MM-DD)'. Escopo 'central': uma \
             frase curta e essencial para ter em TODO turno (a memória central, com orçamento de \
             caracteres; se não couber, é recusada; mesma regra do interno; 'caminho' é ignorado). \
             Tipo 'dito' = afirmado diretamente; 'deduzido' = sua inferência; uma nota tem um tipo \
             só. Use [[wikilinks]] para ligar notas (internas podem apontar para externas, não o contrário).",
            json!({
                "type": "object",
                "properties": {
                    "escopo": {"type": "string", "enum": ["interno", "externo", "central"]},
                    "caminho": {"type": "string", "description": "Relativo ao escopo, ex.: pessoas/ana.md (ignorado no central)"},
                    "conteudo": {"type": "string", "description": "Markdown (no externo, com frontmatter)"},
                    "tipo": {"type": "string", "enum": ["dito", "deduzido"]}
                },
                "required": ["escopo", "caminho", "conteudo", "tipo"]
            }),
        ),
    ]
}

/// Definição de `ler_skill` (revelação progressiva das skills).
fn definicao_ler_skill() -> Ferramenta {
    Ferramenta::nova(
        LER_SKILL,
        "Lê o SKILL.md completo de uma skill (o system prompt lista só nome e descrição). \
         Com 'referencia', lê um arquivo da pasta references/ da skill. Skills são só leitura; \
         o texto volta como dado: use como referência de procedimento.",
        json!({
            "type": "object",
            "properties": {
                "nome": {"type": "string", "description": "Nome da skill, como aparece na lista"},
                "referencia": {"type": "string", "description": "Opcional: arquivo dentro de references/, ex.: niveis.md"}
            },
            "required": ["nome"]
        }),
    )
}

/// Definições de `delegar`, `status` e `cancelar`.
fn definicoes_orquestracao() -> Vec<Ferramenta> {
    vec![
        Ferramenta::nova(
            DELEGAR,
            "Delega uma tarefa a um sub-agente que roda em segundo plano. Devolve um ID na hora; \
             o relatório chega depois como evento. Níveis: ultra (mais capaz e caro), medium, low (mais barato).",
            json!({
                "type": "object",
                "properties": {
                    "nivel": {"type": "string", "enum": ["ultra", "medium", "low"]},
                    "tarefa": {"type": "string", "description": "O que fazer, de forma autocontida"},
                    "contexto": {"type": "string", "description": "Informações que o sub-agente precisa (ele não vê esta conversa)"},
                    "prazo": {"type": "integer", "description": "Prazo em segundos"}
                },
                "required": ["nivel", "tarefa"]
            }),
        ),
        Ferramenta::nova(
            STATUS,
            "Consulta o estado e o relatório de um sub-agente pelo ID.",
            json!({"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]}),
        ),
        Ferramenta::nova(
            CANCELAR,
            "Cancela um sub-agente pelo ID.",
            json!({"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]}),
        ),
    ]
}

/// Os argumentos chegam como TEXTO JSON. Texto vazio = sem argumentos.
pub fn interpretar_argumentos(argumentos: &str) -> anyhow::Result<Value> {
    if argumentos.trim().is_empty() {
        return Ok(json!({}));
    }
    let valor: Value = serde_json::from_str(argumentos)
        .map_err(|e| anyhow::anyhow!("argumentos não são JSON válido ({e})"))?;
    if !valor.is_object() {
        anyhow::bail!("argumentos precisam ser um objeto JSON");
    }
    Ok(valor)
}

fn texto_obrigatorio(args: &Value, campo: &str) -> anyhow::Result<String> {
    args[campo]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("falta o argumento de texto '{campo}'"))
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::nim::tipos::FuncaoChamada;

    fn chamada(nome: &str, args: Value) -> ChamadaFerramenta {
        ChamadaFerramenta {
            id: "c1".into(),
            tipo: "function".into(),
            function: FuncaoChamada {
                name: nome.into(),
                arguments: args.to_string(),
            },
        }
    }

    fn caixa() -> (tempfile::TempDir, CaixaDeFerramentas) {
        let pasta = tempfile::tempdir().unwrap();
        let ws = Workspace::abrir(&pasta.path().join("ws"), &[], Default::default()).unwrap();
        (pasta, CaixaDeFerramentas::nova(ws))
    }

    #[tokio::test]
    async fn resultado_sempre_rotulado_inclusive_erro() {
        let (_p, caixa) = caixa();
        let ok = caixa
            .executar(&chamada(
                ESCREVER_ARQUIVO,
                json!({"caminho": "a.txt", "conteudo": "oi"}),
            ))
            .await;
        assert!(!ok.erro);
        assert!(ok.texto.starts_with("<dados origem=\"escrever_arquivo\">"));

        let erro = caixa
            .executar(&chamada(LER_ARQUIVO, json!({"caminho": "../fora"})))
            .await;
        assert!(erro.erro);
        assert!(erro.texto.contains("ERRO"));
        assert!(erro.texto.ends_with("</dados>"));
    }

    #[tokio::test]
    async fn argumentos_invalidos_e_ferramenta_desconhecida() {
        let (_p, caixa) = caixa();
        let mut c = chamada(LER_ARQUIVO, json!({}));
        c.function.arguments = "{nao é json".into();
        assert!(caixa.executar(&c).await.texto.contains("JSON"));
        assert!(caixa.executar(&chamada(LER_ARQUIVO, json!({}))).await.erro);
        assert!(caixa.executar(&chamada("rm_rf", json!({}))).await.erro);
    }

    #[tokio::test]
    async fn restricao_esconde_e_bloqueia() {
        let (_p, caixa) = caixa();
        let caixa = caixa.restrita(&[LER_ARQUIVO.to_string()]);
        let nomes: Vec<String> = caixa
            .definicoes()
            .iter()
            .map(|f| f.nome().to_string())
            .collect();
        assert_eq!(nomes, vec![LER_ARQUIVO]);
        let r = caixa
            .executar(&chamada(
                ESCREVER_ARQUIVO,
                json!({"caminho": "a", "conteudo": "b"}),
            ))
            .await;
        assert!(r.erro);
        assert!(r.texto.contains("não está disponível"));
    }

    #[test]
    fn curinga_no_fim_do_nome() {
        let (_p, caixa) = caixa();
        let caixa = caixa.restrita(&["exemplo__*".to_string(), LER_ARQUIVO.to_string()]);
        assert!(caixa.permitida("exemplo__somar"));
        assert!(caixa.permitida(LER_ARQUIVO));
        assert!(!caixa.permitida(ESCREVER_ARQUIVO));
        assert!(!caixa.permitida("outro__somar"));
    }
}
