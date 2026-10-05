//! Memória central: um arquivo PEQUENO injetado no system prompt em todo
//! turno (conversa e heartbeat), junto com o núcleo de identidade.
//!
//! Formato (o mesmo separador `§` do Hermes), uma entrada por bloco, cada
//! uma marcada com o tipo para dito e deduzido nunca se misturarem:
//!
//! ```text
//! [dito] O usuário se chama Rafael.
//! §
//! [deduzido] Prefere respostas curtas.
//! ```
//!
//! Orçamento em CARACTERES (`[memoria] limite_central_caracteres`). Uma
//! escrita que passaria do limite é RECUSADA com o motivo; o kernel nunca
//! corta o texto em silêncio. O arquivo é relido a cada turno.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, bail};

use super::nota::Tipo;
use crate::config::Config;

/// Separador entre entradas (linha sozinha).
pub const SEPARADOR: &str = "§";

/// Aviso de "acima do orçamento" sai uma vez só por processo.
static JA_AVISOU: AtomicBool = AtomicBool::new(false);

/// Uma entrada da memória central.
#[derive(Debug, Clone, PartialEq)]
pub struct EntradaCentral {
    /// `None` se alguém editou o arquivo à mão e não marcou o tipo.
    pub tipo: Option<Tipo>,
    pub texto: String,
}

impl EntradaCentral {
    /// Texto como fica no arquivo: "[dito] ...".
    pub fn como_texto(&self) -> String {
        match self.tipo {
            Some(tipo) => format!("[{}] {}", tipo.como_texto(), self.texto),
            None => self.texto.clone(),
        }
    }
}

/// Erro de escrita na memória central.
#[derive(Debug, thiserror::Error)]
pub enum ErroCentral {
    #[error(
        "memória central sem espaço: {atual} + {acrescimo} = {total} caracteres, limite {limite}. \
         Nada foi gravado nem cortado; libere espaço ou guarde no cofre (01_internal)"
    )]
    Orcamento {
        atual: usize,
        acrescimo: usize,
        total: usize,
        limite: usize,
    },
    #[error("a entrada está vazia")]
    Vazia,
    #[error("a entrada não pode conter uma linha só com '§' (é o separador)")]
    Separador,
    #[error("erro de arquivo: {0:#}")]
    Arquivo(#[from] anyhow::Error),
}

#[derive(Debug, Clone)]
pub struct MemoriaCentral {
    caminho: PathBuf,
    limite: usize,
}

impl MemoriaCentral {
    pub fn nova(caminho: &Path, limite: usize) -> MemoriaCentral {
        MemoriaCentral {
            caminho: caminho.to_path_buf(),
            limite,
        }
    }

    pub fn da_config(config: &Config) -> MemoriaCentral {
        MemoriaCentral::nova(
            &config.caminho_memoria_central(),
            config.memoria.limite_central_caracteres,
        )
    }

    pub fn caminho(&self) -> &Path {
        &self.caminho
    }

    pub fn limite(&self) -> usize {
        self.limite
    }

    /// Texto do arquivo ("" se ainda não existe).
    pub fn texto(&self) -> String {
        std::fs::read_to_string(&self.caminho).unwrap_or_default()
    }

    /// Caracteres usados (é isto que conta no orçamento).
    pub fn uso(&self) -> usize {
        self.texto().trim().chars().count()
    }

    pub fn entradas(&self) -> Vec<EntradaCentral> {
        interpretar(&self.texto())
    }

    /// Já existe uma entrada com este texto (ignorando espaços nas pontas)?
    pub fn contem(&self, texto: &str) -> bool {
        self.entradas().iter().any(|e| e.texto == texto.trim())
    }

    /// Quantos caracteres o arquivo teria com mais esta entrada.
    pub fn uso_com(&self, tipo: Tipo, texto: &str) -> usize {
        let mut entradas = self.entradas();
        entradas.push(EntradaCentral {
            tipo: Some(tipo),
            texto: texto.trim().to_string(),
        });
        montar(&entradas).trim().chars().count()
    }

    /// Acrescenta uma entrada SE couber no orçamento. Devolve o novo uso.
    /// Não couber → erro com os números; o arquivo não é tocado.
    pub fn acrescentar(&self, tipo: Tipo, texto: &str) -> Result<usize, ErroCentral> {
        let texto = texto.trim();
        if texto.is_empty() {
            return Err(ErroCentral::Vazia);
        }
        if texto.lines().any(|l| l.trim() == SEPARADOR) {
            return Err(ErroCentral::Separador);
        }
        let atual = self.uso();
        let total = self.uso_com(tipo, texto);
        if total > self.limite {
            return Err(ErroCentral::Orcamento {
                atual,
                acrescimo: total - atual,
                total,
                limite: self.limite,
            });
        }
        let mut entradas = self.entradas();
        entradas.push(EntradaCentral {
            tipo: Some(tipo),
            texto: texto.to_string(),
        });
        self.gravar(&montar(&entradas))?;
        Ok(total)
    }

    fn gravar(&self, texto: &str) -> anyhow::Result<()> {
        if let Some(pasta) = self.caminho.parent() {
            std::fs::create_dir_all(pasta)?;
        }
        let temporario = self.caminho.with_extension("md.abiyss-tmp");
        std::fs::write(&temporario, texto)
            .with_context(|| format!("não consegui escrever {}", temporario.display()))?;
        std::fs::rename(&temporario, &self.caminho)?;
        Ok(())
    }

    /// Bloco para o system prompt. NUNCA corta: se o arquivo foi editado à
    /// mão e passou do limite, vai inteiro e o log avisa (uma vez).
    pub fn bloco_para_prompt(&self) -> Option<String> {
        let texto = self.texto();
        let texto = texto.trim();
        if texto.is_empty() {
            return None;
        }
        let uso = texto.chars().count();
        if uso > self.limite && !JA_AVISOU.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                "memória central acima do orçamento ({uso}/{} caracteres) em {}; vai inteira para o prompt, mas corrija o arquivo",
                self.limite,
                self.caminho.display()
            );
        }
        Some(format!(
            "Fatos essenciais que você guardou ({uso}/{} caracteres). [dito] = afirmado; \
[deduzido] = inferência sua.\n\n{texto}",
            self.limite
        ))
    }
}

/// Lê as entradas do texto do arquivo.
pub fn interpretar(texto: &str) -> Vec<EntradaCentral> {
    let mut entradas = Vec::new();
    let mut atual: Vec<&str> = Vec::new();
    for linha in texto.lines().chain(std::iter::once(SEPARADOR)) {
        if linha.trim() == SEPARADOR {
            let bloco = atual.join("\n");
            let bloco = bloco.trim();
            if !bloco.is_empty() {
                entradas.push(interpretar_entrada(bloco));
            }
            atual.clear();
        } else {
            atual.push(linha);
        }
    }
    entradas
}

fn interpretar_entrada(bloco: &str) -> EntradaCentral {
    for tipo in [Tipo::Dito, Tipo::Deduzido] {
        let marca = format!("[{}]", tipo.como_texto());
        if let Some(resto) = bloco.strip_prefix(&marca) {
            return EntradaCentral {
                tipo: Some(tipo),
                texto: resto.trim().to_string(),
            };
        }
    }
    EntradaCentral {
        tipo: None,
        texto: bloco.to_string(),
    }
}

/// Monta o texto do arquivo a partir das entradas.
pub fn montar(entradas: &[EntradaCentral]) -> String {
    let blocos: Vec<String> = entradas.iter().map(|e| e.como_texto()).collect();
    let mut texto = blocos.join(&format!("\n{SEPARADOR}\n"));
    if !texto.is_empty() {
        texto.push('\n');
    }
    texto
}

/// Confere se um texto de proposta serve para a memória central.
pub fn validar_texto(texto: &str) -> anyhow::Result<()> {
    if texto.trim().is_empty() {
        bail!("a entrada está vazia");
    }
    if texto.lines().any(|l| l.trim() == SEPARADOR) {
        bail!("a entrada não pode conter uma linha só com '§' (é o separador)");
    }
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn interpreta_e_monta_ida_e_volta() {
        let texto =
            "[dito] Nome: Rafael.\n§\n[deduzido] Gosta de\nrespostas curtas.\n§\nsem tipo\n§\n\n";
        let entradas = interpretar(texto);
        assert_eq!(entradas.len(), 3);
        assert_eq!(entradas[0].tipo, Some(Tipo::Dito));
        assert_eq!(entradas[1].texto, "Gosta de\nrespostas curtas.");
        assert_eq!(entradas[2].tipo, None);
        assert_eq!(interpretar(&montar(&entradas)), entradas);
        assert!(interpretar("").is_empty());
    }

    #[test]
    fn recusa_acima_do_orcamento_sem_cortar() {
        let pasta = tempfile::tempdir().unwrap();
        let central = MemoriaCentral::nova(&pasta.path().join("central.md"), 40);
        assert_eq!(
            central.acrescentar(Tipo::Dito, "Nome: Rafael.").unwrap(),
            20
        );
        let antes = central.texto();
        let erro = central
            .acrescentar(Tipo::Deduzido, "Uma frase longa demais para caber.")
            .unwrap_err();
        match erro {
            ErroCentral::Orcamento {
                atual,
                total,
                limite,
                ..
            } => {
                assert_eq!(atual, 20);
                assert!(total > limite);
            }
            outro => panic!("erro inesperado: {outro}"),
        }
        // Nada mudou no arquivo.
        assert_eq!(central.texto(), antes);
        assert!(central.contem("Nome: Rafael."));
        assert!(matches!(
            central.acrescentar(Tipo::Dito, "a\n§\nb"),
            Err(ErroCentral::Separador)
        ));
        assert!(matches!(
            central.acrescentar(Tipo::Dito, "  "),
            Err(ErroCentral::Vazia)
        ));
    }

    #[test]
    fn bloco_do_prompt_nunca_corta() {
        let pasta = tempfile::tempdir().unwrap();
        let caminho = pasta.path().join("central.md");
        let central = MemoriaCentral::nova(&caminho, 10);
        assert!(central.bloco_para_prompt().is_none());
        // Editado à mão, acima do limite: vai inteiro.
        let longo = "[dito] ".to_string() + &"x".repeat(50);
        std::fs::write(&caminho, &longo).unwrap();
        let bloco = central.bloco_para_prompt().unwrap();
        assert!(bloco.contains(&longo));
        assert!(bloco.contains("57/10"));
    }
}
