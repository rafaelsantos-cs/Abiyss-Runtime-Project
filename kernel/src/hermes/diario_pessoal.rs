//! `metacognition/diario-pessoal.md` → `01_internal/diario/`.
//!
//! O arquivo é dividido nos títulos (`#`, `##`...) que têm data
//! ("2026-09-28" ou "28/09/2026"): uma nota por dia
//! (`01_internal/diario/2026-09-28.md`); dois títulos do mesmo dia vão
//! para a mesma nota, na ordem. O que vem antes do primeiro título com data
//! (ou o arquivo todo, se não houver datas) vai para
//! `01_internal/diario/diario-pessoal-hermes.md`.

use std::collections::BTreeMap;

use chrono::NaiveDate;

use super::campos::achar_data_no_texto;
use super::{Contexto, Secao};
use crate::frontmatter::Documento;
use crate::memoria::nota::{Fonte, Procedencia, Tipo};

const ARQUIVO: &str = "metacognition/diario-pessoal.md";
const PASTA_DESTINO: &str = "01_internal/diario";

/// Texto do diário dividido: (dia, trechos daquele dia) + o resto sem data.
#[derive(Debug, Default, PartialEq)]
pub struct DiarioDividido {
    pub por_dia: BTreeMap<NaiveDate, Vec<String>>,
    pub sem_data: String,
}

/// Guarda o bloco que terminou no dia certo (ou no "sem data").
fn fechar(dia: Option<NaiveDate>, bloco: &mut String, dividido: &mut DiarioDividido) {
    let texto = bloco.trim().to_string();
    if !texto.is_empty() {
        match dia {
            Some(dia) => dividido.por_dia.entry(dia).or_default().push(texto),
            None => dividido.sem_data = texto,
        }
    }
    bloco.clear();
}

pub fn dividir(texto: &str) -> DiarioDividido {
    let mut dividido = DiarioDividido::default();
    let mut dia_atual: Option<NaiveDate> = None;
    let mut bloco = String::new();
    for linha in texto.lines() {
        let titulo_com_data = linha
            .trim_start()
            .starts_with('#')
            .then(|| achar_data_no_texto(linha))
            .flatten();
        if let Some(dia) = titulo_com_data {
            fechar(dia_atual, &mut bloco, &mut dividido);
            dia_atual = Some(dia);
        }
        bloco.push_str(linha);
        bloco.push('\n');
    }
    fechar(dia_atual, &mut bloco, &mut dividido);
    dividido
}

pub(super) fn importar(ctx: &mut Contexto<'_>) -> anyhow::Result<Secao> {
    let mut secao =
        Secao::nova("Diário pessoal: metacognition/diario-pessoal.md → 01_internal/diario/");
    if !ctx.tem(ARQUIVO) {
        secao.linhas.push("(arquivo não encontrado)".into());
        return Ok(secao);
    }
    let texto = match ctx.ler(ARQUIVO) {
        Ok(t) => t,
        Err(e) => {
            secao.erros.push(format!("{e:#}"));
            return Ok(secao);
        }
    };
    let dividido = dividir(&texto);
    // O diário é escrita do próprio Abiyss, em primeira pessoa: "dito".
    let procedencia = |criado: Option<String>| Procedencia {
        fonte: Fonte::Importacao,
        tipo: Tipo::Dito,
        origem_externa: None,
        criado,
        rotulo: format!("importado de {ARQUIVO}"),
    };
    if !dividido.sem_data.is_empty() {
        ctx.importar_nota(
            &mut secao,
            "hermes:diario-pessoal:sem-data",
            &format!("{PASTA_DESTINO}/diario-pessoal-hermes.md"),
            Documento::sem_campos(&dividido.sem_data),
            &procedencia(None),
            ARQUIVO,
        )?;
    }
    for (dia, trechos) in &dividido.por_dia {
        ctx.importar_nota(
            &mut secao,
            &format!("hermes:diario-pessoal:{dia}"),
            &format!("{PASTA_DESTINO}/{dia}.md"),
            Documento::sem_campos(&trechos.join("\n\n")),
            &procedencia(Some(dia.to_string())),
            ARQUIVO,
        )?;
    }
    Ok(secao)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn divide_por_dia_e_junta_o_mesmo_dia() {
        let texto = "# Diário\n\nIntro.\n\n## 2026-09-27\nPrimeiro.\n### detalhe sem data\nmais\n\n## 28/09/2026\nSegundo.\n\n## 2026-09-28 (noite)\nTerceiro.\n";
        let d = dividir(texto);
        assert_eq!(d.sem_data, "# Diário\n\nIntro.");
        assert_eq!(d.por_dia.len(), 2);
        let dia27 = &d.por_dia[&NaiveDate::from_ymd_opt(2026, 9, 27).unwrap()];
        assert!(dia27[0].contains("### detalhe sem data"));
        let dia28 = &d.por_dia[&NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()];
        assert_eq!(dia28.len(), 2);
        assert!(dia28[1].starts_with("## 2026-09-28 (noite)"));

        let sem_datas = dividir("só um texto\nsem títulos");
        assert!(sem_datas.por_dia.is_empty());
        assert_eq!(sem_datas.sem_data, "só um texto\nsem títulos");
    }
}
