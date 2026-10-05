//! Tipos básicos das notas do cofre: escopo, procedência e wikilinks.

use crate::frontmatter::Documento;

/// Pasta das notas internas (pessoas, preferências, auto-modelo, diário...).
pub const PASTA_INTERNA: &str = "01_internal";
/// Pasta do mapa de fontes externas (links canônicos, não enciclopédia).
pub const PASTA_EXTERNA: &str = "02_external";

/// Os dois escopos de memória.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escopo {
    /// `01_internal/`: quem o Abiyss é e quem ele conhece. Conteúdo vindo de
    /// ferramentas, web ou qualquer fonte externa NUNCA entra direto aqui.
    Interno,
    /// `02_external/`: mapa de fontes (site oficial, changelog, docs...).
    Externo,
}

impl Escopo {
    pub fn pasta(&self) -> &'static str {
        match self {
            Escopo::Interno => PASTA_INTERNA,
            Escopo::Externo => PASTA_EXTERNA,
        }
    }

    pub fn como_texto(&self) -> &'static str {
        match self {
            Escopo::Interno => "interno",
            Escopo::Externo => "externo",
        }
    }

    pub fn de_texto(texto: &str) -> Option<Escopo> {
        match texto.trim().to_lowercase().as_str() {
            "interno" | "internal" | PASTA_INTERNA => Some(Escopo::Interno),
            "externo" | "external" | PASTA_EXTERNA => Some(Escopo::Externo),
            _ => None,
        }
    }

    /// Escopo de um caminho relativo ao cofre ("01_internal/..." → Interno).
    pub fn do_caminho(caminho: &str) -> Option<Escopo> {
        let primeira = caminho.split('/').next().unwrap_or("");
        match primeira {
            PASTA_INTERNA => Some(Escopo::Interno),
            PASTA_EXTERNA => Some(Escopo::Externo),
            _ => None,
        }
    }
}

/// Onde procurar em `memoria_buscar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscopoBusca {
    Interno,
    Externo,
    Ambos,
}

impl EscopoBusca {
    pub fn de_texto(texto: &str) -> Option<EscopoBusca> {
        match texto.trim().to_lowercase().as_str() {
            "interno" => Some(EscopoBusca::Interno),
            "externo" => Some(EscopoBusca::Externo),
            "ambos" | "" => Some(EscopoBusca::Ambos),
            _ => None,
        }
    }

    pub fn como_texto(&self) -> &'static str {
        match self {
            EscopoBusca::Interno => "interno",
            EscopoBusca::Externo => "externo",
            EscopoBusca::Ambos => "ambos",
        }
    }

    pub fn inclui(&self, escopo: Escopo) -> bool {
        match self {
            EscopoBusca::Interno => escopo == Escopo::Interno,
            EscopoBusca::Externo => escopo == Escopo::Externo,
            EscopoBusca::Ambos => true,
        }
    }

    /// Os escopos cobertos, em ordem.
    pub fn escopos(&self) -> Vec<Escopo> {
        [Escopo::Interno, Escopo::Externo]
            .into_iter()
            .filter(|e| self.inclui(*e))
            .collect()
    }
}

/// Como o conhecimento chegou à memória (campo `fonte` do frontmatter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fonte {
    /// Aprendido numa conversa (proposto com `memoria_propor`).
    Conversa,
    /// Produzido durante o sono (consolidação).
    Sleep,
    /// Trazido de outro sistema (ex.: `abiyss importar-hermes`).
    Importacao,
}

impl Fonte {
    pub fn como_texto(&self) -> &'static str {
        match self {
            Fonte::Conversa => "conversa",
            Fonte::Sleep => "sleep",
            Fonte::Importacao => "importacao",
        }
    }

    pub fn de_texto(texto: &str) -> Option<Fonte> {
        match texto.trim() {
            "conversa" => Some(Fonte::Conversa),
            "sleep" => Some(Fonte::Sleep),
            "importacao" | "importação" => Some(Fonte::Importacao),
            _ => None,
        }
    }
}

/// Natureza do conteúdo (campo `tipo` do frontmatter). Uma nota tem UM
/// tipo só: o que foi dito e o que foi deduzido nunca se misturam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tipo {
    /// Afirmado diretamente (pelo usuário, ou pelo próprio Abiyss em
    /// primeira pessoa) e registrado como tal.
    Dito,
    /// Conclusão tirada a partir de sinais; pode estar errada.
    Deduzido,
}

impl Tipo {
    pub fn como_texto(&self) -> &'static str {
        match self {
            Tipo::Dito => "dito",
            Tipo::Deduzido => "deduzido",
        }
    }

    pub fn de_texto(texto: &str) -> Option<Tipo> {
        match texto.trim().to_lowercase().as_str() {
            "dito" => Some(Tipo::Dito),
            "deduzido" => Some(Tipo::Deduzido),
            _ => None,
        }
    }
}

/// Campos do frontmatter que só o kernel preenche. Se vierem numa
/// proposta, são ignorados (o modelo não escolhe a própria procedência).
pub const CAMPOS_DO_KERNEL: &[&str] = &["fonte", "tipo", "criado", "atualizado"];

/// De onde veio um conteúdo que vai ser gravado no cofre.
#[derive(Debug, Clone, PartialEq)]
pub struct Procedencia {
    pub fonte: Fonte,
    pub tipo: Tipo,
    /// `Some(detalhe)` quando o conteúdo veio (ou pode ter vindo) de
    /// ferramentas, web, sub-agentes ou outra fonte externa. Calculado pelo
    /// KERNEL, nunca declarado pelo modelo.
    pub origem_externa: Option<String>,
    /// Data original do conteúdo (ex.: o dia de uma entrada importada).
    /// `None` = agora.
    pub criado: Option<String>,
    /// Texto curto para a linha de procedência de blocos acrescentados
    /// (ex.: "proposta #12").
    pub rotulo: String,
}

/// Uma nota lida do cofre.
#[derive(Debug, Clone, PartialEq)]
pub struct Nota {
    /// Caminho relativo ao cofre, com "/" (ex.: "01_internal/pessoas/ana.md").
    pub caminho: String,
    pub escopo: Escopo,
    /// Texto completo, como está no disco.
    pub texto: String,
    pub doc: Documento,
}

/// Alvos dos `[[wikilinks]]` de um texto, sem apelido (`|...`) nem
/// seção (`#...`), na ordem em que aparecem e sem repetição.
/// `![[...]]` (embutir) também conta como link.
pub fn extrair_wikilinks(texto: &str) -> Vec<String> {
    let mut alvos: Vec<String> = Vec::new();
    let mut resto = texto;
    while let Some(inicio) = resto.find("[[") {
        let depois = &resto[inicio + 2..];
        let Some(fim) = depois.find("]]") else {
            break;
        };
        let dentro = &depois[..fim];
        // Um "[[" no meio quer dizer que o primeiro não fechou: recomeça dali.
        if let Some(outro) = dentro.find("[[") {
            resto = &depois[outro..];
            continue;
        }
        let alvo = dentro.split(['|', '#', '^']).next().unwrap_or("").trim();
        if !alvo.is_empty() && !dentro.contains('\n') && !alvos.iter().any(|a| a == alvo) {
            alvos.push(alvo.to_string());
        }
        resto = &depois[fim + 2..];
    }
    alvos
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn extrai_wikilinks_como_o_obsidian() {
        let texto = "Ver [[rust]], [[02_external/docs/tokio|Tokio]] e [[pessoas/ana#Gostos]].\n\
                     Repetido [[rust]]. Embutido ![[diagrama]]. Quebrado [[sem fim\n[[ok]]";
        assert_eq!(
            extrair_wikilinks(texto),
            vec![
                "rust",
                "02_external/docs/tokio",
                "pessoas/ana",
                "diagrama",
                "ok"
            ]
        );
        assert!(extrair_wikilinks("nada aqui [ [x] ]").is_empty());
    }

    #[test]
    fn escopos_e_tipos() {
        assert_eq!(
            Escopo::do_caminho("01_internal/a.md"),
            Some(Escopo::Interno)
        );
        assert_eq!(Escopo::do_caminho("outra/a.md"), None);
        assert_eq!(EscopoBusca::de_texto(""), Some(EscopoBusca::Ambos));
        assert!(!EscopoBusca::Interno.inclui(Escopo::Externo));
        assert_eq!(Tipo::de_texto("Deduzido"), Some(Tipo::Deduzido));
        assert_eq!(Tipo::de_texto("talvez"), None);
        assert_eq!(Fonte::de_texto("importacao"), Some(Fonte::Importacao));
    }
}
