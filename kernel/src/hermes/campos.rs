//! Leitura tolerante de registros JSON de formato incerto.
//!
//! O formato exato dos arquivos do Hermes não pôde ser conferido, então
//! cada campo de destino aceita vários NOMES de origem (apelidos), em
//! português e em inglês. O `Leitor` anota:
//! - qual nome de origem foi usado para cada campo (vai para o relatório,
//!   para conferir as suposições com os dados reais);
//! - quais campos NÃO foram usados: eles são preservados (em `extras`) e
//!   listados no relatório. Nada é descartado em silêncio.

use std::collections::BTreeSet;

use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
use serde_json::{Map, Value};

pub struct Leitor<'a> {
    objeto: &'a Map<String, Value>,
    consumidos: BTreeSet<String>,
    /// (campo de destino, nome do campo de origem que foi usado)
    pub usados: Vec<(&'static str, String)>,
}

impl<'a> Leitor<'a> {
    pub fn novo(objeto: &'a Map<String, Value>) -> Leitor<'a> {
        Leitor {
            objeto,
            consumidos: BTreeSet::new(),
            usados: Vec::new(),
        }
    }

    /// Valor do primeiro apelido presente. `null` conta como ausente (mas
    /// o campo é dado como "lido", não como desconhecido).
    pub fn valor(&mut self, destino: &'static str, apelidos: &[&str]) -> Option<&'a Value> {
        for apelido in apelidos {
            if let Some(valor) = self.objeto.get(*apelido) {
                self.consumidos.insert(apelido.to_string());
                if valor.is_null() {
                    continue;
                }
                self.usados.push((destino, apelido.to_string()));
                return Some(valor);
            }
        }
        None
    }

    /// Valor como texto: textos, números e booleanos; listas viram itens
    /// separados por "; "; objetos viram JSON. Vazio = ausente.
    pub fn texto(&mut self, destino: &'static str, apelidos: &[&str]) -> Option<String> {
        self.valor(destino, apelidos)
            .map(valor_como_texto)
            .filter(|t| !t.trim().is_empty())
    }

    /// Campos que nenhuma leitura usou (preservados em `extras`).
    pub fn extras(&self) -> Map<String, Value> {
        self.objeto
            .iter()
            .filter(|(chave, _)| !self.consumidos.contains(*chave))
            .map(|(chave, valor)| (chave.clone(), valor.clone()))
            .collect()
    }
}

/// Qualquer valor JSON como texto legível.
pub fn valor_como_texto(valor: &Value) -> String {
    match valor {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        Value::Array(itens) => itens
            .iter()
            .map(valor_como_texto)
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("; "),
        Value::Object(_) => valor.to_string(),
    }
}

/// Momento (ms desde 1970) a partir de vários formatos de data:
/// RFC 3339, "AAAA-MM-DD HH:MM[:SS]", "AAAA-MM-DD", "DD/MM/AAAA" e
/// números (segundos ou milissegundos). Sem fuso = fuso local.
pub fn momento_de(valor: &Value) -> Option<i64> {
    match valor {
        Value::Number(n) => {
            let x = n.as_f64()?;
            // Até ~1973 em ms, ou até 5138 em segundos: dá para distinguir.
            if x.abs() < 1e11 {
                Some((x * 1000.0) as i64)
            } else {
                Some(x as i64)
            }
        }
        Value::String(s) => momento_de_texto(s.trim()),
        _ => None,
    }
}

fn momento_de_texto(texto: &str) -> Option<i64> {
    if let Ok(data) = chrono::DateTime::parse_from_rfc3339(texto) {
        return Some(data.timestamp_millis());
    }
    for formato in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(data) = NaiveDateTime::parse_from_str(texto, formato) {
            return Local
                .from_local_datetime(&data)
                .earliest()
                .map(|d| d.timestamp_millis());
        }
    }
    if let Some(dia) = data_de(texto) {
        return Local
            .from_local_datetime(&dia.and_hms_opt(0, 0, 0)?)
            .earliest()
            .map(|d| d.timestamp_millis());
    }
    texto
        .parse::<f64>()
        .ok()
        .and_then(|n| momento_de(&serde_json::json!(n)))
}

/// Uma data no texto todo: "AAAA-MM-DD" ou "DD/MM/AAAA".
pub fn data_de(texto: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(texto, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(texto, "%d/%m/%Y"))
        .ok()
}

/// Procura uma data em qualquer lugar do texto ("## 28/09/2026 (noite)").
pub fn achar_data_no_texto(texto: &str) -> Option<NaiveDate> {
    let caracteres: Vec<char> = texto.chars().collect();
    (0..caracteres.len().saturating_sub(9)).find_map(|i| {
        let pedaco = &caracteres[i..i + 10];
        // O chrono aceita anos curtos e espaços; aqui exigimos o formato exato.
        if tem_formato(pedaco, "dddd-dd-dd") || tem_formato(pedaco, "dd/dd/dddd") {
            data_de(&pedaco.iter().collect::<String>())
        } else {
            None
        }
    })
}

/// `modelo` usa 'd' para dígito; o resto precisa ser igual.
fn tem_formato(pedaco: &[char], modelo: &str) -> bool {
    pedaco.len() == modelo.chars().count()
        && pedaco.iter().zip(modelo.chars()).all(|(c, m)| {
            if m == 'd' {
                c.is_ascii_digit()
            } else {
                *c == m
            }
        })
}

/// Confiança como número de 0.0 a 1.0. Aceita 0.8, 80, "80%" e "0,8".
/// Texto como "alta" não vira número (fica preservado em `extras`).
pub fn confianca_de(valor: &Value) -> Option<f64> {
    let (numero, era_porcentagem) = match valor {
        Value::Number(n) => (n.as_f64()?, false),
        Value::String(s) => {
            let s = s.trim();
            let sem_porcento = s.strip_suffix('%');
            let numero = sem_porcento.unwrap_or(s).trim().replace(',', ".");
            (numero.parse::<f64>().ok()?, sem_porcento.is_some())
        }
        _ => return None,
    };
    if era_porcentagem || numero > 1.0 {
        (0.0..=100.0).contains(&numero).then_some(numero / 100.0)
    } else {
        (0.0..=1.0).contains(&numero).then_some(numero)
    }
}

/// Hash FNV-1a de 64 bits, em hexadecimal. Estável entre versões do Rust
/// (o `DefaultHasher` da biblioteca padrão não garante isso), então serve
/// de chave de importação para registros sem ID.
pub fn hash_estavel(texto: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in texto.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn leitor_anota_usados_e_preserva_o_resto() {
        let obj = json!({"action": "a", "acao": "b", "mood": "x", "outcome": null});
        let obj = obj.as_object().unwrap();
        let mut l = Leitor::novo(obj);
        assert_eq!(l.texto("acao", &["acao", "action"]).as_deref(), Some("b"));
        assert_eq!(l.texto("resultado", &["outcome"]), None);
        assert_eq!(l.usados, vec![("acao", "acao".to_string())]);
        // "action" (apelido não usado) e "mood" (desconhecido) ficam em extras.
        let extras = l.extras();
        assert_eq!(extras.len(), 2);
        assert!(extras.contains_key("action") && extras.contains_key("mood"));
    }

    #[test]
    fn datas_numeros_e_confianca() {
        assert_eq!(momento_de(&json!(1_759_150_800)), Some(1_759_150_800_000));
        assert_eq!(
            momento_de(&json!(1_759_150_800_000_i64)),
            Some(1_759_150_800_000)
        );
        assert_eq!(
            momento_de(&json!("2026-10-01T08:00:00Z")),
            Some(
                chrono::DateTime::parse_from_rfc3339("2026-10-01T08:00:00Z")
                    .unwrap()
                    .timestamp_millis()
            )
        );
        for texto in ["2026-09-30 22:05:00", "2026-10-02", "02/10/2026"] {
            assert!(momento_de(&json!(texto)).is_some(), "{texto}");
        }
        assert_eq!(momento_de(&json!("ontem")), None);
        assert_eq!(
            achar_data_no_texto("## 28/09/2026 (noite)"),
            NaiveDate::from_ymd_opt(2026, 9, 28)
        );
        assert_eq!(confianca_de(&json!(0.8)), Some(0.8));
        assert_eq!(confianca_de(&json!(65)), Some(0.65));
        assert_eq!(confianca_de(&json!("70%")), Some(0.7));
        assert_eq!(confianca_de(&json!("0,5")), Some(0.5));
        assert_eq!(confianca_de(&json!("alta")), None);
        assert_eq!(confianca_de(&json!(250)), None);
        assert_eq!(hash_estavel("abc"), "e71fa2190541574b");
    }
}
