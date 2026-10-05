//! Busca no cofre pelo qmd (servidor MCP de busca em Markdown, via HTTP).
//!
//! O formato exato das respostas do qmd não pôde ser conferido durante o
//! desenvolvimento, então esta parte é TOLERANTE e desconfiada:
//! - a ferramenta é escolhida pelo nome configurado ou, se vazio, pela
//!   primeira que existir entre `FERRAMENTAS_PREFERIDAS`;
//! - os argumentos seguem o JSON Schema que o próprio qmd anuncia
//!   (`query`/`q`, `limit`/`n`, `collection`...);
//! - a resposta é lida como JSON (structuredContent ou texto) e cada
//!   resultado precisa apontar para uma nota que EXISTE no cofre e é do
//!   escopo pedido. O resto é descartado (é isso que garante que uma busca
//!   "interna" nunca traga conteúdo de 02_external).
//!
//! Se nada disso der certo, `Memoria::buscar` cai na busca por texto.

use anyhow::{Context, bail};
use rmcp::model::Tool;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::busca::ResultadoBusca;
use super::cofre::Cofre;
use super::nota::{Escopo, EscopoBusca, PASTA_EXTERNA, PASTA_INTERNA};
use crate::mcp::PonteMcp;

/// Ferramentas de busca do qmd, em ordem de preferência (a primeira é a
/// busca por palavras, rápida e sem modelo). A CONFIRMAR na VM.
pub const FERRAMENTAS_PREFERIDAS: &[&str] = &[
    "search",
    "qmd_search",
    "query",
    "qmd_query",
    "vsearch",
    "qmd_vsearch",
];
const CHAVES_CONSULTA: &[&str] = &["query", "q", "text", "consulta"];
const CHAVES_LIMITE: &[&str] = &["limit", "n", "max_results", "top_k", "k"];
const CHAVES_COLECAO: &[&str] = &["collection", "collections", "index"];
const CHAVES_LISTA: &[&str] = &["results", "documents", "items", "hits", "matches"];
const CHAVES_CAMINHO: &[&str] = &["file", "path", "filepath", "uri", "document", "docid", "id"];
const CHAVES_TRECHO: &[&str] = &["snippet", "context", "excerpt", "text", "content", "body"];
const CHAVES_PONTUACAO: &[&str] = &["score", "relevance"];
/// Tamanho máximo do trecho mostrado por resultado.
const MAX_CARACTERES_TRECHO: usize = 300;

/// Seção `[memoria.qmd]` do abiyss.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConfigQmd {
    /// Nome do servidor em `[[mcp.servidores]]` que é o qmd. Vazio = não usar.
    pub servidor: String,
    /// Ferramenta de busca. Vazio = escolher entre `FERRAMENTAS_PREFERIDAS`.
    pub ferramenta: String,
    /// Coleção do qmd que indexa `01_internal/` (opcional).
    pub colecao_interna: String,
    /// Coleção do qmd que indexa `02_external/` (opcional).
    pub colecao_externa: String,
}

impl Default for ConfigQmd {
    fn default() -> Self {
        ConfigQmd {
            servidor: "qmd".to_string(),
            ferramenta: String::new(),
            colecao_interna: String::new(),
            colecao_externa: String::new(),
        }
    }
}

impl ConfigQmd {
    fn colecao(&self, escopo: Escopo) -> Option<&str> {
        let nome = match escopo {
            Escopo::Interno => self.colecao_interna.as_str(),
            Escopo::Externo => self.colecao_externa.as_str(),
        };
        Some(nome.trim()).filter(|n| !n.is_empty())
    }
}

/// Busca pelo qmd. Erro = o chamador deve cair na busca por texto.
pub async fn buscar(
    ponte: &PonteMcp,
    config: &ConfigQmd,
    cofre: &Cofre,
    consulta: &str,
    escopo: EscopoBusca,
    max_resultados: usize,
) -> anyhow::Result<Vec<ResultadoBusca>> {
    let ferramentas = ponte
        .ferramentas_do_servidor(&config.servidor)
        .with_context(|| format!("servidor '{}' não está ativo", config.servidor))?;
    let ferramenta = escolher_ferramenta(ferramentas, &config.ferramenta)?;
    let propriedades = ferramenta
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let chave_colecao = primeira_chave(&propriedades, CHAVES_COLECAO);

    // Uma chamada por escopo quando cada escopo tem sua coleção; senão, uma
    // chamada só (e o filtro por caminho separa os escopos).
    let escopos = escopo.escopos();
    let por_colecao: Vec<(Escopo, &str)> = escopos
        .iter()
        .filter_map(|e| config.colecao(*e).map(|c| (*e, c)))
        .collect();
    let chamadas: Vec<Option<&str>> =
        if chave_colecao.is_some() && por_colecao.len() == escopos.len() {
            por_colecao.iter().map(|(_, c)| Some(*c)).collect()
        } else {
            vec![None]
        };

    let notas = cofre.listar(None);
    let mut brutos = 0;
    let mut resultados: Vec<ResultadoBusca> = Vec::new();
    for colecao in chamadas {
        let args = montar_argumentos(
            &propriedades,
            consulta,
            max_resultados,
            chave_colecao,
            colecao,
        );
        let resposta = ponte
            .chamar_no_servidor(&config.servidor, &ferramenta.name, args)
            .await?;
        let itens = extrair_itens(resposta.estruturado.as_ref(), &resposta.texto)?;
        brutos += itens.len();
        for item in &itens {
            if let Some(r) = mapear(item, colecao, config, cofre, &notas, escopo)
                && !resultados.iter().any(|j| j.caminho == r.caminho)
            {
                resultados.push(r);
            }
        }
    }
    if brutos > 0 && resultados.is_empty() {
        bail!(
            "o qmd devolveu {brutos} resultado(s), mas nenhum aponta para uma nota existente do escopo pedido"
        );
    }
    resultados.sort_by(|a, b| {
        b.pontuacao
            .total_cmp(&a.pontuacao)
            .then_with(|| a.caminho.cmp(&b.caminho))
    });
    resultados.truncate(max_resultados);
    Ok(resultados)
}

fn escolher_ferramenta<'a>(ferramentas: &'a [Tool], configurada: &str) -> anyhow::Result<&'a Tool> {
    let nomes: Vec<&str> = ferramentas.iter().map(|f| f.name.as_ref()).collect();
    let procurar = |nome: &str| ferramentas.iter().find(|f| f.name == nome);
    if !configurada.trim().is_empty() {
        return procurar(configurada.trim()).with_context(|| {
            format!(
                "ferramenta '{configurada}' não existe no qmd (tem: {})",
                nomes.join(", ")
            )
        });
    }
    FERRAMENTAS_PREFERIDAS
        .iter()
        .find_map(|n| procurar(n))
        .with_context(|| {
            format!(
                "nenhuma ferramenta de busca conhecida no qmd (tem: {}); configure [memoria.qmd] ferramenta",
                nomes.join(", ")
            )
        })
}

fn primeira_chave(
    propriedades: &Map<String, Value>,
    opcoes: &[&'static str],
) -> Option<&'static str> {
    opcoes
        .iter()
        .copied()
        .find(|c| propriedades.contains_key(*c))
}

/// Argumentos seguindo o esquema anunciado pelo qmd.
fn montar_argumentos(
    propriedades: &Map<String, Value>,
    consulta: &str,
    limite: usize,
    chave_colecao: Option<&str>,
    colecao: Option<&str>,
) -> Value {
    let mut args = Map::new();
    let chave_consulta = primeira_chave(propriedades, CHAVES_CONSULTA).unwrap_or("query");
    args.insert(chave_consulta.to_string(), json!(consulta));
    if let Some(chave) = primeira_chave(propriedades, CHAVES_LIMITE) {
        args.insert(chave.to_string(), json!(limite));
    }
    if let (Some(chave), Some(nome)) = (chave_colecao, colecao) {
        let valor = match propriedades[chave]["type"].as_str() {
            Some("array") => json!([nome]),
            _ => json!(nome),
        };
        args.insert(chave.to_string(), valor);
    }
    Value::Object(args)
}

/// Lista de resultados de uma resposta do qmd (JSON estruturado ou texto JSON).
pub fn extrair_itens(
    estruturado: Option<&Value>,
    texto: &str,
) -> anyhow::Result<Vec<Map<String, Value>>> {
    let valor = match estruturado {
        Some(v) => v.clone(),
        None => serde_json::from_str::<Value>(texto.trim())
            .context("a resposta do qmd não é JSON (nem structuredContent)")?,
    };
    let lista = match &valor {
        Value::Array(itens) => itens.clone(),
        Value::Object(mapa) => match CHAVES_LISTA.iter().find_map(|c| mapa.get(*c)?.as_array()) {
            Some(itens) => itens.clone(),
            None => bail!(
                "a resposta do qmd não tem uma lista de resultados ({})",
                CHAVES_LISTA.join("/")
            ),
        },
        _ => bail!("a resposta do qmd não é uma lista nem um objeto"),
    };
    Ok(lista
        .into_iter()
        .filter_map(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .collect())
}

/// Liga um resultado do qmd a uma nota do cofre. `None` = descartado.
fn mapear(
    item: &Map<String, Value>,
    colecao: Option<&str>,
    config: &ConfigQmd,
    cofre: &Cofre,
    notas: &[String],
    escopo: EscopoBusca,
) -> Option<ResultadoBusca> {
    let bruto = CHAVES_CAMINHO.iter().find_map(|c| item.get(*c)?.as_str())?;
    let candidato = caminho_no_cofre(bruto, colecao, config, cofre)?;
    let caminho = achar_nota(&candidato, notas)?;
    let (escopo_nota, caminho) = cofre.normalizar(&caminho).ok()?;
    if !escopo.inclui(escopo_nota) {
        return None;
    }
    let trecho = CHAVES_TRECHO
        .iter()
        .find_map(|c| item.get(*c)?.as_str())
        .map(|t| {
            let linha = t.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut curto: String = linha.chars().take(MAX_CARACTERES_TRECHO).collect();
            if linha.chars().count() > MAX_CARACTERES_TRECHO {
                curto.push('…');
            }
            curto
        })
        .unwrap_or_default();
    let pontuacao = CHAVES_PONTUACAO
        .iter()
        .find_map(|c| item.get(*c)?.as_f64())
        .unwrap_or(0.0);
    Some(ResultadoBusca {
        caminho,
        escopo: escopo_nota,
        trecho,
        pontuacao,
    })
}

/// Converte o caminho do qmd ("qmd://colecao/x.md", absoluto, relativo à
/// coleção...) num caminho relativo ao cofre.
fn caminho_no_cofre(
    bruto: &str,
    colecao: Option<&str>,
    config: &ConfigQmd,
    cofre: &Cofre,
) -> Option<String> {
    let mut colecao = colecao.map(String::from);
    let mut resto = bruto.trim().replace('\\', "/");
    if let Some(sem_esquema) = resto.strip_prefix("qmd://") {
        let (c, r) = sem_esquema.split_once('/')?;
        colecao = Some(c.to_string());
        resto = r.to_string();
    }
    let raiz = cofre.raiz().to_string_lossy().replace('\\', "/");
    if let Some(r) = resto.strip_prefix(&raiz) {
        resto = r.to_string();
    }
    let resto = resto
        .trim_start_matches('/')
        .trim_start_matches("./")
        .to_string();
    // Já começa (ou contém, depois de uma "/") a pasta de um escopo?
    for pasta in [PASTA_INTERNA, PASTA_EXTERNA] {
        let marca = format!("{pasta}/");
        if resto.starts_with(&marca) {
            return Some(resto);
        }
        if let Some(pos) = resto.find(&format!("/{marca}")) {
            return Some(resto[pos + 1..].to_string());
        }
    }
    // Relativo à coleção de um escopo.
    let colecao = colecao?;
    if config.colecao(Escopo::Interno) == Some(colecao.as_str()) {
        Some(format!("{PASTA_INTERNA}/{resto}"))
    } else if config.colecao(Escopo::Externo) == Some(colecao.as_str()) {
        Some(format!("{PASTA_EXTERNA}/{resto}"))
    } else {
        None
    }
}

/// Acha a nota no cofre: exata, ou com o nome "simplificado" (o qmd pode
/// guardar caminhos em minúsculas e com hífens no lugar de espaços).
fn achar_nota(candidato: &str, notas: &[String]) -> Option<String> {
    let candidato = if candidato.ends_with(".md") {
        candidato.to_string()
    } else {
        format!("{candidato}.md")
    };
    if notas.contains(&candidato) {
        return Some(candidato);
    }
    let chave = simplificar(&candidato);
    let parecidas: Vec<&String> = notas.iter().filter(|n| simplificar(n) == chave).collect();
    match parecidas.as_slice() {
        [unica] => Some((*unica).clone()),
        _ => None,
    }
}

fn simplificar(caminho: &str) -> String {
    caminho
        .to_lowercase()
        .chars()
        .map(|c| if c == ' ' || c == '_' { '-' } else { c })
        .collect()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn le_resultados_em_varios_formatos() {
        let texto = r#"{"results": [{"file": "qmd://a/x.md", "score": 0.5}, 3]}"#;
        assert_eq!(extrair_itens(None, texto).unwrap().len(), 1);
        let lista = json!([{"path": "01_internal/x.md"}]);
        assert_eq!(extrair_itens(Some(&lista), "ignorado").unwrap().len(), 1);
        assert!(extrair_itens(None, "Found 2 results:\n- x.md").is_err());
        assert!(extrair_itens(None, r#"{"outra": 1}"#).is_err());
    }

    #[test]
    fn caminhos_do_qmd_viram_caminhos_do_cofre() {
        let pasta = tempfile::tempdir().unwrap();
        let cofre = Cofre::abrir(pasta.path()).unwrap();
        let config = ConfigQmd {
            colecao_interna: "abiyss-interno".into(),
            colecao_externa: "abiyss-externo".into(),
            ..Default::default()
        };
        let raiz = cofre.raiz().display().to_string();
        let casos = [
            (
                "qmd://abiyss-interno/pessoas/ana.md",
                None,
                Some("01_internal/pessoas/ana.md"),
            ),
            ("qmd://abiyss-externo/x.md", None, Some("02_external/x.md")),
            (
                "qmd://cofre/01_internal/y.md",
                None,
                Some("01_internal/y.md"),
            ),
            (
                "pessoas/ana.md",
                Some("abiyss-interno"),
                Some("01_internal/pessoas/ana.md"),
            ),
            ("pessoas/ana.md", None, None),
            ("qmd://outra/pessoas/ana.md", None, None),
        ];
        for (bruto, colecao, esperado) in casos {
            assert_eq!(
                caminho_no_cofre(bruto, colecao, &config, &cofre).as_deref(),
                esperado,
                "{bruto}"
            );
        }
        let absoluto = format!("{raiz}/02_external/z.md");
        assert_eq!(
            caminho_no_cofre(&absoluto, None, &config, &cofre).as_deref(),
            Some("02_external/z.md")
        );
        let notas = vec!["01_internal/pessoas/Ana Souza.md".to_string()];
        assert_eq!(
            achar_nota("01_internal/pessoas/ana-souza.md", &notas).as_deref(),
            Some("01_internal/pessoas/Ana Souza.md")
        );
        assert_eq!(achar_nota("01_internal/outra.md", &notas), None);
    }

    #[test]
    fn argumentos_seguem_o_esquema() {
        let props = json!({"q": {"type": "string"}, "n": {"type": "integer"}, "collections": {"type": "array"}});
        let props = props.as_object().unwrap();
        let args = montar_argumentos(
            props,
            "café",
            5,
            Some("collections"),
            Some("abiyss-interno"),
        );
        assert_eq!(
            args,
            json!({"q": "café", "n": 5, "collections": ["abiyss-interno"]})
        );
        let vazio = Map::new();
        assert_eq!(
            montar_argumentos(&vazio, "x", 5, None, None),
            json!({"query": "x"})
        );
    }
}
