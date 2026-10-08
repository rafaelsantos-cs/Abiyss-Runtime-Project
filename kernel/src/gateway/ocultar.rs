//! Segredos nunca saem pelo Discord: todo texto que o gateway entrega
//! passa por aqui (é o único caminho até o adaptador).
//!
//! O que vira `***`:
//! - os mesmos padrões do servidor MCP `ambiente` (`ocultar_segredos` em
//!   `recursos/mcp/ambiente/sistema.py`): `--password=x`, `--token x`,
//!   `API_KEY=x`, `usuario:senha@host`. Um teste confere que as expressões
//!   são idênticas às de lá;
//! - chaves com cara de chave (os prefixos da importação do Hermes:
//!   `nvapi-`, `sk-`, `ghp_`...), mesmo soltas no texto;
//! - blocos de chave privada (`-----BEGIN ... PRIVATE KEY-----`);
//! - os VALORES das variáveis de ambiente do processo com nome de segredo
//!   (`*KEY*`, `*TOKEN*`, `*SECRET*`, `*PASSWORD*`, `*SENHA*`): as chaves
//!   do NIM e o token do Discord, se estiverem no `.env`, nunca saem nem
//!   escritos por extenso.
//!
//! Na resposta que chega aos poucos, a última palavra (ainda incompleta)
//! fica guardada até a próxima: `https://usuario:se` não aparece no
//! Discord antes de o `@` chegar e o padrão casar.

use std::sync::LazyLock;

use regex::Regex;

/// Padrões copiados de `recursos/mcp/ambiente/sistema.py` (sem mudar nada:
/// o teste `padroes_iguais_aos_do_ambiente` lê o arquivo e compara).
pub const SEGREDO_OPCAO: &str = r"(?i)((?:^|\s)--?[\w.-]*(?:pass(?:word|wd)?|senha|token|secret|segredo|api[-_]?key|apikey|auth|credential)[\w.-]*)(=|\s+)(\S+)";
pub const SEGREDO_ATRIBUICAO: &str = r"(?i)\b([\w.-]*(?:pass(?:word|wd)?|senha|token|secret|segredo|api[-_]?key|apikey)[\w.-]*=)(\S+)";
pub const SEGREDO_URL: &str = r"(?i)\b([a-z][a-z0-9+.-]*://)[^/\s:@]+:[^/\s@]+@";

const OCULTO: &str = "***";
/// Valor de variável de ambiente mais curto que isto não é procurado no
/// texto (sumiria com palavras comuns).
const MIN_VALOR_SECRETO: usize = 8;
const NOMES_SECRETOS: &[&str] = &["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "SENHA"];

struct Padroes {
    url: Regex,
    opcao: Regex,
    atribuicao: Regex,
    chave: Regex,
    privada: Regex,
}

static PADROES: LazyLock<Padroes> = LazyLock::new(|| {
    let prefixos = crate::hermes::segredos::PREFIXOS_DE_CHAVE
        .iter()
        .map(|p| regex::escape(p))
        .collect::<Vec<_>>()
        .join("|");
    Padroes {
        url: Regex::new(SEGREDO_URL).expect("padrão fixo"),
        opcao: Regex::new(SEGREDO_OPCAO).expect("padrão fixo"),
        atribuicao: Regex::new(SEGREDO_ATRIBUICAO).expect("padrão fixo"),
        chave: Regex::new(&format!(
            r"(^|[^A-Za-z0-9_-])(?:{prefixos})[A-Za-z0-9_-]{{{},}}",
            crate::hermes::segredos::MIN_CARACTERES_DEPOIS_DO_PREFIXO
        ))
        .expect("padrão fixo"),
        // Bloco inteiro, ou do BEGIN até o fim (o END ainda não chegou).
        privada: Regex::new(
            r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|\z)",
        )
        .expect("padrão fixo"),
    }
});

/// O ocultador do gateway. Guarda os valores secretos do ambiente lidos ao
/// subir (o processo não muda de chave enquanto roda).
#[derive(Clone, Default)]
pub struct Ocultador {
    valores: Vec<String>,
}

impl Ocultador {
    /// Com os valores das variáveis de ambiente com nome de segredo.
    pub fn do_ambiente() -> Ocultador {
        Ocultador::com_valores(
            std::env::vars()
                .filter(|(nome, _)| {
                    let nome = nome.to_uppercase();
                    NOMES_SECRETOS.iter().any(|s| nome.contains(s))
                })
                .map(|(_, valor)| valor),
        )
    }

    pub fn com_valores(valores: impl IntoIterator<Item = String>) -> Ocultador {
        let mut valores: Vec<String> = valores
            .into_iter()
            .map(|v| v.trim().to_string())
            .filter(|v| v.chars().count() >= MIN_VALOR_SECRETO)
            .collect();
        // Os maiores primeiro (um valor que contém outro some inteiro).
        valores.sort_by_key(|v| std::cmp::Reverse(v.len()));
        valores.dedup();
        Ocultador { valores }
    }

    /// O texto sem nada com cara de segredo.
    pub fn ocultar(&self, texto: &str) -> String {
        let mut t = texto.to_string();
        for valor in &self.valores {
            if t.contains(valor.as_str()) {
                t = t.replace(valor.as_str(), OCULTO);
            }
        }
        let p = &*PADROES;
        let t = p.privada.replace_all(&t, "[chave privada oculta]");
        let t = p.chave.replace_all(&t, "${1}***");
        // Mesma ordem do `ocultar_segredos` do ambiente.
        let t = p.url.replace_all(&t, "${1}***@");
        let t = p.opcao.replace_all(&t, "${1}${2}***");
        let t = p.atribuicao.replace_all(&t, "${1}***");
        t.into_owned()
    }

    /// Para o texto que ainda está chegando: a última palavra (sem espaço
    /// depois) fica de fora até se completar.
    pub fn ocultar_parcial(&self, texto: &str) -> String {
        let completo = match texto.char_indices().rev().find(|(_, c)| c.is_whitespace()) {
            Some((i, c)) => &texto[..i + c.len_utf8()],
            None => "",
        };
        self.ocultar(completo)
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn mesmos_casos_do_ambiente() {
        // Os casos de recursos/mcp/ambiente/tests/test_sistema.py.
        let o = Ocultador::default();
        for (entrada, saida) in [
            (
                "mysql -u root --password=hunter2 db",
                "mysql -u root --password=*** db",
            ),
            ("app --token abc123 --verbose", "app --token *** --verbose"),
            ("app --api-key=XYZ", "app --api-key=***"),
            (
                "curl https://user:senha@exemplo.com/x",
                "curl https://***@exemplo.com/x",
            ),
            (
                "env API_KEY=xyz OPENAI_API_KEY=abc python",
                "env API_KEY=*** OPENAI_API_KEY=*** python",
            ),
            (
                "postgres: password=s3gredo user=x",
                "postgres: password=*** user=x",
            ),
            ("ls -la /home", "ls -la /home"),
        ] {
            assert_eq!(o.ocultar(entrada), saida, "{entrada}");
        }
    }

    #[test]
    fn padroes_iguais_aos_do_ambiente() {
        let caminho = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../recursos/mcp/ambiente/sistema.py"
        );
        let fonte = std::fs::read_to_string(caminho).unwrap();
        let padrao = |nome: &str| -> String {
            let depois = &fonte[fonte.find(&format!("{nome} = re.compile(")).unwrap()..];
            let inicio = depois.find("r\"").unwrap() + 2;
            let fim = inicio + depois[inicio..].find('"').unwrap();
            depois[inicio..fim].to_string()
        };
        assert_eq!(padrao("_SEGREDO_OPCAO"), SEGREDO_OPCAO);
        assert_eq!(padrao("_SEGREDO_ATRIBUICAO"), SEGREDO_ATRIBUICAO);
        assert_eq!(padrao("_SEGREDO_URL"), SEGREDO_URL);
    }

    #[test]
    fn chaves_soltas_blocos_privados_e_valores_do_ambiente() {
        let nim = format!("nvapi-{}", "a1B2".repeat(10));
        // Um valor qualquer de variável secreta do ambiente (ex.: o token do bot).
        let do_ambiente = "valor-secreto-do-ambiente-0123456789";
        let o = Ocultador::com_valores([do_ambiente.to_string(), "curta".into()]);
        let texto = format!(
            "a chave é {nim}, o token do bot é {do_ambiente} e o sk-learn é uma biblioteca.\n\
             -----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----\nfim"
        );
        let t = o.ocultar(&texto);
        assert!(!t.contains(&nim) && !t.contains(do_ambiente), "{t}");
        assert!(t.contains("a chave é ***, o token do bot é ***"));
        assert!(t.contains("sk-learn"), "palavra comum fica");
        assert!(t.contains("[chave privada oculta]\nfim"));
        assert!(!t.contains("AAAA"));
        // Valor curto não é procurado (sumiria com palavras comuns).
        assert_eq!(o.ocultar("curta"), "curta");
        // Bloco privado sem o END ainda: some até o fim.
        assert_eq!(
            o.ocultar("veja:\n-----BEGIN RSA PRIVATE KEY-----\nMIIE"),
            "veja:\n[chave privada oculta]"
        );
    }

    #[test]
    fn parcial_segura_a_palavra_incompleta() {
        let o = Ocultador::default();
        // O "@" ainda não chegou: nada da URL aparece.
        assert_eq!(o.ocultar_parcial("acesse https://admin:hunt"), "acesse ");
        assert_eq!(
            o.ocultar_parcial("acesse https://admin:hunter2@host/x e"),
            "acesse https://***@host/x "
        );
        assert_eq!(o.ocultar_parcial("--password hun"), "--password ");
        assert_eq!(
            o.ocultar_parcial("--password hunter2 ok"),
            "--password *** "
        );
        // Ocultar de novo não estraga.
        assert_eq!(o.ocultar("token=***"), "token=***");
    }
}
