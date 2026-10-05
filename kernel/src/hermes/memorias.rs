//! `memories/MEMORY.md` e `memories/USER.md` → memória central.
//!
//! As entradas são separadas por `§` (formato do Hermes). Cada uma entra
//! na memória central enquanto couber no orçamento; as que não couberem vão
//! para uma nota em `01_internal/` (com `fonte: importacao`), nunca cortadas.
//! O Hermes não diz se uma entrada foi afirmada ou deduzida: por cautela,
//! todas entram como `deduzido` (o usuário pode promover a `dito` depois).

use super::campos::hash_estavel;
use super::segredos::parece_conter_segredo;
use super::{Contexto, Secao};
use crate::frontmatter::{Documento, Value};
use crate::importacoes;
use crate::memoria::central::{self, EntradaCentral, SEPARADOR};
use crate::memoria::nota::{Fonte, Procedencia, Tipo};

/// (arquivo de origem, nota para o excedente)
const ORIGENS: &[(&str, &str)] = &[
    ("memories/MEMORY.md", "01_internal/memoria/hermes-memory.md"),
    ("memories/USER.md", "01_internal/pessoas/usuario-hermes.md"),
];

/// Divide o texto nas entradas: linhas só com "§" (formato do Hermes) ou,
/// se não houver nenhuma, qualquer "§".
pub fn dividir_entradas(texto: &str) -> Vec<String> {
    let tem_linha_separadora = texto.lines().any(|l| l.trim() == SEPARADOR);
    let blocos: Vec<String> = if tem_linha_separadora {
        let mut blocos = vec![String::new()];
        for linha in texto.lines() {
            if linha.trim() == SEPARADOR {
                blocos.push(String::new());
            } else if let Some(atual) = blocos.last_mut() {
                atual.push_str(linha);
                atual.push('\n');
            }
        }
        blocos
    } else {
        texto.split(SEPARADOR).map(String::from).collect()
    };
    blocos
        .into_iter()
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty())
        .collect()
}

pub(super) fn importar(ctx: &mut Contexto<'_>) -> anyhow::Result<Secao> {
    let limite = ctx.central.limite();
    let mut secao = Secao::nova(&format!(
        "Memória central: memories/MEMORY.md e USER.md → {} (limite {limite} caracteres)",
        ctx.central.caminho().display()
    ));
    // Simulação do orçamento (vale também com --aplicar, para decidir o destino).
    let mut simulada: Vec<EntradaCentral> = ctx.central.entradas();
    let mut no_central = 0;

    for (arquivo, nota_excedente) in ORIGENS {
        if !ctx.tem(arquivo) {
            secao.linhas.push(format!("({arquivo} não encontrado)"));
            continue;
        }
        let texto = match ctx.ler(arquivo) {
            Ok(t) => t,
            Err(e) => {
                secao.erros.push(format!("{e:#}"));
                continue;
            }
        };
        let texto_excedente = ctx
            .cofre
            .ler(nota_excedente, usize::MAX)
            .map(|n| n.texto)
            .unwrap_or_default();
        let mut excedentes: Vec<(String, String)> = Vec::new();
        for (i, entrada) in dividir_entradas(&texto).into_iter().enumerate() {
            let chave = format!("hermes:memoria:{arquivo}:{}", hash_estavel(&entrada));
            let ja_esta = importacoes::ja_importado(ctx.banco, &chave)?
                || simulada.iter().any(|e| e.texto == entrada)
                || texto_excedente.contains(&entrada);
            if ja_esta {
                secao.ja_importados += 1;
                continue;
            }
            if let Some(tipo) = parece_conter_segredo(&entrada) {
                secao.erros.push(format!(
                    "{arquivo}, entrada {}: parece conter um segredo ({tipo}); NÃO importada",
                    i + 1
                ));
                continue;
            }
            simulada.push(EntradaCentral {
                tipo: Some(Tipo::Deduzido),
                texto: entrada.clone(),
            });
            let cabe = central::montar(&simulada).trim().chars().count() <= limite;
            if !cabe {
                simulada.pop();
                excedentes.push((chave, entrada));
                continue;
            }
            if ctx.aplicar {
                ctx.central.acrescentar(Tipo::Deduzido, &entrada)?;
                importacoes::registrar(
                    ctx.banco,
                    &chave,
                    "memoria_central",
                    "memoria-central",
                    "",
                )?;
            }
            secao.novos += 1;
            no_central += 1;
        }

        if !excedentes.is_empty() {
            let corpo = excedentes
                .iter()
                .map(|(_, e)| e.as_str())
                .collect::<Vec<_>>()
                .join(&format!("\n\n{SEPARADOR}\n\n"));
            if let Some(tipo) = parece_conter_segredo(&corpo) {
                secao.erros.push(format!(
                    "excedente de {arquivo} parece conter segredo ({tipo})"
                ));
                continue;
            }
            let mut doc = Documento::sem_campos(&corpo);
            doc.campos.insert(
                Value::String("origem".into()),
                Value::String(format!("hermes:{arquivo}")),
            );
            let procedencia = Procedencia {
                fonte: Fonte::Importacao,
                tipo: Tipo::Deduzido,
                origem_externa: None,
                criado: None,
                rotulo: format!("excedente da memória central, de {arquivo}"),
            };
            if ctx.aplicar {
                ctx.cofre.gravar(nota_excedente, &doc, &procedencia)?;
                for (chave, _) in &excedentes {
                    importacoes::registrar(ctx.banco, chave, "nota", nota_excedente, "")?;
                }
            }
            secao.novos += excedentes.len();
            secao.linhas.push(format!(
                "+ {} entrada(s) de {arquivo} não couberam no orçamento → {nota_excedente}",
                excedentes.len()
            ));
        }
    }
    let uso = central::montar(&simulada).trim().chars().count();
    secao.linhas.push(format!(
        "+ {no_central} entrada(s) na memória central; uso depois da importação: {uso}/{limite} caracteres"
    ));
    Ok(secao)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn divide_entradas_do_hermes() {
        let texto = "Primeira.\n§\nSegunda,\nem duas linhas.\n  §  \n\n§\nTerceira.\n";
        assert_eq!(
            dividir_entradas(texto),
            vec!["Primeira.", "Segunda,\nem duas linhas.", "Terceira."]
        );
        assert_eq!(dividir_entradas("a § b §c"), vec!["a", "b", "c"]);
        assert!(dividir_entradas("  \n").is_empty());
    }
}
