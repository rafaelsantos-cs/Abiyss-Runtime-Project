//! Frontmatter YAML no início de arquivos Markdown.
//!
//! É o formato das skills (`SKILL.md`) e das notas do Obsidian:
//!
//! ```text
//! ---
//! name: exemplo
//! description: Uma linha explicando para que serve.
//! ---
//! # Corpo em Markdown
//! ```
//!
//! Os campos ficam num `Mapping` do `serde_yaml_ng`, que GUARDA A ORDEM
//! em que aparecem. Assim, campos que o kernel não conhece são
//! preservados (e reescritos no mesmo lugar) em vez de sumirem.

use anyhow::{Context, bail};
pub use serde_yaml_ng::{Mapping, Value};

/// Um arquivo Markdown dividido em frontmatter + corpo.
#[derive(Debug, Clone, PartialEq)]
pub struct Documento {
    /// Campos do frontmatter, na ordem do arquivo (vazio se não houver).
    pub campos: Mapping,
    /// Texto depois do frontmatter (sem as linhas em branco do começo).
    pub corpo: String,
    /// O arquivo começava com um bloco `---`?
    pub tem_frontmatter: bool,
}

impl Documento {
    /// Documento só com corpo, sem frontmatter.
    pub fn sem_campos(corpo: &str) -> Documento {
        Documento {
            campos: Mapping::new(),
            corpo: corpo.to_string(),
            tem_frontmatter: false,
        }
    }

    /// Valor de texto de um campo (números e booleanos viram texto também).
    pub fn texto(&self, chave: &str) -> Option<String> {
        texto_do_campo(&self.campos, chave)
    }
}

/// Separa o frontmatter do corpo.
///
/// - Sem `---` na primeira linha: o texto inteiro é corpo (não é erro).
/// - Com `---` mas sem a linha de fechamento: erro (arquivo mal formado).
/// - YAML inválido ou que não seja um mapa `chave: valor`: erro.
pub fn separar(texto: &str) -> anyhow::Result<Documento> {
    // Alguns editores colocam um BOM invisível no começo do arquivo.
    let texto = texto.strip_prefix('\u{feff}').unwrap_or(texto);
    let mut linhas = texto.split_inclusive('\n');
    let primeira = linhas.next().unwrap_or("");
    if primeira.trim_end() != "---" {
        return Ok(Documento::sem_campos(texto));
    }

    let mut yaml = String::new();
    // Quantos bytes do texto já foram lidos (para achar onde começa o corpo).
    let mut lidos = primeira.len();
    for linha in linhas {
        lidos += linha.len();
        let limpa = linha.trim_end();
        if limpa == "---" || limpa == "..." {
            let campos = interpretar_yaml(&yaml)?;
            let corpo = texto[lidos..].trim_start_matches(['\n', '\r']).to_string();
            return Ok(Documento {
                campos,
                corpo,
                tem_frontmatter: true,
            });
        }
        yaml.push_str(linha);
    }
    bail!("o frontmatter começa com '---' mas não tem a linha '---' de fechamento")
}

fn interpretar_yaml(yaml: &str) -> anyhow::Result<Mapping> {
    if yaml.trim().is_empty() {
        return Ok(Mapping::new());
    }
    let valor: Value =
        serde_yaml_ng::from_str(yaml).context("o frontmatter não é um YAML válido")?;
    match valor {
        Value::Mapping(mapa) => Ok(mapa),
        Value::Null => Ok(Mapping::new()),
        _ => bail!("o frontmatter precisa ser uma lista de 'chave: valor'"),
    }
}

/// Monta o texto do arquivo: frontmatter (se houver campos) + corpo.
pub fn montar(campos: &Mapping, corpo: &str) -> anyhow::Result<String> {
    let corpo = corpo.trim_start_matches(['\n', '\r']);
    let mut texto = String::new();
    if !campos.is_empty() {
        let yaml = serde_yaml_ng::to_string(campos).context("não consegui gerar o YAML")?;
        texto.push_str("---\n");
        texto.push_str(&yaml);
        if !yaml.ends_with('\n') {
            texto.push('\n');
        }
        texto.push_str("---\n\n");
    }
    texto.push_str(corpo);
    if !texto.ends_with('\n') {
        texto.push('\n');
    }
    Ok(texto)
}

/// Valor de texto de um campo. Números e booleanos são convertidos;
/// listas, mapas e campos vazios devolvem `None`.
pub fn texto_do_campo(campos: &Mapping, chave: &str) -> Option<String> {
    match campos.get(chave)? {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Lista de textos de um campo. Aceita lista (`[a, b]` ou `- a`) ou um
/// texto só (vira lista de um item).
pub fn lista_do_campo(campos: &Mapping, chave: &str) -> Vec<String> {
    match campos.get(chave) {
        Some(Value::Sequence(itens)) => itens
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.trim().to_string()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .filter(|s| !s.is_empty())
            .collect(),
        Some(_) => texto_do_campo(campos, chave).into_iter().collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn separa_campos_e_corpo() {
        let doc = separar(
            "---\nname: teste\ndescription: >-\n  duas\n  linhas\n---\n\n# Título\ncorpo\n",
        )
        .unwrap();
        assert!(doc.tem_frontmatter);
        assert_eq!(doc.texto("name").as_deref(), Some("teste"));
        // Bloco ">-" do YAML (texto dobrado) é lido certo.
        assert_eq!(doc.texto("description").as_deref(), Some("duas linhas"));
        assert_eq!(doc.corpo, "# Título\ncorpo\n");
    }

    #[test]
    fn sem_frontmatter_e_so_corpo() {
        let doc = separar("# Só texto\n---\nnão é frontmatter").unwrap();
        assert!(!doc.tem_frontmatter);
        assert!(doc.campos.is_empty());
        assert!(doc.corpo.starts_with("# Só texto"));
    }

    #[test]
    fn frontmatter_quebrado_e_erro() {
        assert!(separar("---\nname: x\nsem fechamento").is_err());
        assert!(separar("---\n- uma\n- lista\n---\n").is_err());
        assert!(separar("---\nchave: [aberta\n---\n").is_err());
        // BOM e CRLF (arquivos vindos do Windows) funcionam.
        let doc = separar("\u{feff}---\r\nname: x\r\n---\r\ncorpo").unwrap();
        assert_eq!(doc.texto("name").as_deref(), Some("x"));
        assert_eq!(doc.corpo, "corpo");
    }

    #[test]
    fn ida_e_volta_preserva_ordem_e_campos_desconhecidos() {
        let original = "---\nz_ultimo: 1\nname: x\nmetadata:\n  tags: [a, b]\n---\n\ncorpo\n";
        let doc = separar(original).unwrap();
        let texto = montar(&doc.campos, &doc.corpo).unwrap();
        let de_novo = separar(&texto).unwrap();
        assert_eq!(de_novo, doc);
        let chaves: Vec<String> = de_novo
            .campos
            .keys()
            .filter_map(|k| k.as_str().map(String::from))
            .collect();
        assert_eq!(chaves, vec!["z_ultimo", "name", "metadata"]);
    }

    #[test]
    fn listas_e_textos() {
        let doc = separar("---\nlinks:\n  - https://a\n  - https://b\nunico: so um\nn: 3\n---\n")
            .unwrap();
        assert_eq!(
            lista_do_campo(&doc.campos, "links"),
            vec!["https://a", "https://b"]
        );
        assert_eq!(lista_do_campo(&doc.campos, "unico"), vec!["so um"]);
        assert_eq!(doc.texto("n").as_deref(), Some("3"));
        assert!(lista_do_campo(&doc.campos, "nao_existe").is_empty());
    }
}
