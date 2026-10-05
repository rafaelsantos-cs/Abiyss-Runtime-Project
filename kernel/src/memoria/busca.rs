//! Busca no cofre por texto simples (sem índice, sem modelo).
//!
//! Lê as notas do escopo pedido, conta quantas vezes cada termo da
//! consulta aparece (sem diferenciar maiúsculas nem acentos) e devolve as
//! melhores com um trecho em volta da primeira ocorrência.

use super::cofre::{Cofre, sem_acento};
use super::nota::{Escopo, EscopoBusca};

/// Tamanho máximo de nota considerada na busca.
const MAX_BYTES_NOTA_BUSCA: usize = 512 * 1024;
/// Caracteres de cada lado da ocorrência no trecho.
const MARGEM_TRECHO: usize = 90;

/// Um resultado de `memoria_buscar`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultadoBusca {
    /// Caminho relativo ao cofre.
    pub caminho: String,
    pub escopo: Escopo,
    pub trecho: String,
    pub pontuacao: f64,
}

/// Busca simples por termos. Termos com menos de 2 letras são ignorados.
pub fn buscar_texto(
    cofre: &Cofre,
    consulta: &str,
    escopo: EscopoBusca,
    max_resultados: usize,
) -> Vec<ResultadoBusca> {
    let termos = termos_da_consulta(consulta);
    if termos.is_empty() {
        return Vec::new();
    }
    let mut resultados = Vec::new();
    for e in escopo.escopos() {
        for caminho in cofre.listar(Some(e)) {
            let Ok(nota) = cofre.ler(&caminho, MAX_BYTES_NOTA_BUSCA) else {
                continue;
            };
            let texto: Vec<char> = nota.texto.chars().collect();
            let normal: Vec<char> = texto.iter().map(|c| normalizar(*c)).collect();
            let caminho_normal: String = caminho.chars().map(normalizar).collect();

            let mut pontuacao = 0.0;
            let mut primeira: Option<usize> = None;
            for termo in &termos {
                let posicoes = ocorrencias(&normal, termo);
                // Cada termo conta no máximo 10 vezes (nota repetitiva não domina).
                pontuacao += posicoes.len().min(10) as f64;
                if caminho_normal.contains(termo.iter().collect::<String>().as_str()) {
                    pontuacao += 3.0;
                }
                if let Some(p) = posicoes.first() {
                    primeira = Some(primeira.map_or(*p, |atual: usize| atual.min(*p)));
                }
            }
            if pontuacao > 0.0 {
                resultados.push(ResultadoBusca {
                    trecho: trecho(&texto, primeira.unwrap_or(0)),
                    caminho,
                    escopo: e,
                    pontuacao,
                });
            }
        }
    }
    resultados.sort_by(|a, b| {
        b.pontuacao
            .total_cmp(&a.pontuacao)
            .then_with(|| a.caminho.cmp(&b.caminho))
    });
    resultados.truncate(max_resultados);
    resultados
}

/// Minúscula e sem acento, sempre um caractere por caractere.
fn normalizar(c: char) -> char {
    sem_acento(c.to_lowercase().next().unwrap_or(c))
}

fn termos_da_consulta(consulta: &str) -> Vec<Vec<char>> {
    let mut termos: Vec<Vec<char>> = Vec::new();
    for palavra in consulta.split(|c: char| !c.is_alphanumeric()) {
        let termo: Vec<char> = palavra.chars().map(normalizar).collect();
        if termo.len() >= 2 && !termos.contains(&termo) {
            termos.push(termo);
        }
    }
    termos
}

/// Posições (em caracteres) onde `termo` aparece em `texto`.
fn ocorrencias(texto: &[char], termo: &[char]) -> Vec<usize> {
    if termo.is_empty() || texto.len() < termo.len() {
        return Vec::new();
    }
    (0..=texto.len() - termo.len())
        .filter(|&i| texto[i..i + termo.len()] == *termo)
        .collect()
}

/// Trecho em volta da posição, numa linha só.
fn trecho(texto: &[char], posicao: usize) -> String {
    let inicio = posicao.saturating_sub(MARGEM_TRECHO);
    let fim = (posicao + MARGEM_TRECHO).min(texto.len());
    let pedaco: String = texto[inicio..fim].iter().collect();
    let mut linha = pedaco.split_whitespace().collect::<Vec<_>>().join(" ");
    if inicio > 0 {
        linha.insert(0, '…');
    }
    if fim < texto.len() {
        linha.push('…');
    }
    linha
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::frontmatter::Documento;
    use crate::memoria::nota::{Fonte, Procedencia, Tipo};

    #[test]
    fn acha_por_termo_sem_acento_e_respeita_escopo() {
        let pasta = tempfile::tempdir().unwrap();
        let cofre = Cofre::abrir(pasta.path()).unwrap();
        let interna = Procedencia {
            fonte: Fonte::Conversa,
            tipo: Tipo::Dito,
            origem_externa: None,
            criado: None,
            rotulo: "t".into(),
        };
        cofre
            .gravar(
                "01_internal/preferencias/cafe.md",
                &Documento::sem_campos("Prefere café sem açúcar, de manhã."),
                &interna,
            )
            .unwrap();
        cofre
            .gravar(
                "01_internal/pessoas/ana.md",
                &Documento::sem_campos("Ana não toma café."),
                &interna,
            )
            .unwrap();

        let r = buscar_texto(&cofre, "CAFE acucar", EscopoBusca::Interno, 10);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].caminho, "01_internal/preferencias/cafe.md");
        assert!(r[0].trecho.contains("café sem açúcar"));
        assert!(buscar_texto(&cofre, "café", EscopoBusca::Externo, 10).is_empty());
        assert!(buscar_texto(&cofre, "a", EscopoBusca::Ambos, 10).is_empty());
        assert_eq!(buscar_texto(&cofre, "café", EscopoBusca::Ambos, 1).len(), 1);
    }
}
