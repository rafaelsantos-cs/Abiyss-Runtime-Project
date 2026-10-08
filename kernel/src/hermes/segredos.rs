//! Proteção contra segredos na importação do Hermes.
//!
//! 1. Pelo NOME: arquivos e pastas cujo nome sugere segredo (`.env`,
//!    `auth.json`, `credentials.json`, `id_rsa`, `secrets/`...) nunca são
//!    abertos, lidos nem copiados; só aparecem no relatório como pulados.
//! 2. Pelo CONTEÚDO: um texto que vai ser importado e tem cara de chave de
//!    API (`nvapi-...`, `ghp_...`, `-----BEGIN ... PRIVATE KEY-----`...) não
//!    é importado; o relatório diz ONDE estava, nunca o valor.

/// Pedaços de nome que indicam segredo. O nome é quebrado em palavras
/// (letras e números), então "auth.json" bate em "auth", mas "author.md"
/// não bate em nada.
const PALAVRAS_DE_SEGREDO: &[&str] = &[
    "env",
    "auth",
    "oauth",
    "token",
    "tokens",
    "secret",
    "secrets",
    "segredo",
    "segredos",
    "credential",
    "credentials",
    "credencial",
    "credenciais",
    "password",
    "passwords",
    "passwd",
    "senha",
    "senhas",
    "apikey",
    "apikeys",
    "key",
    "keys",
    "cookie",
    "cookies",
    "private",
    "privkey",
    "rsa",
    "ed25519",
    "ecdsa",
    "ssh",
    "gnupg",
    "gpg",
    "pem",
    "p12",
    "pfx",
    "jks",
    "kdbx",
    "keystore",
    "keychain",
    "htpasswd",
    "netrc",
    "npmrc",
    "pypirc",
];

/// O nome deste arquivo ou pasta sugere segredo?
pub fn parece_segredo(nome: &str) -> bool {
    nome.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|palavra| PALAVRAS_DE_SEGREDO.contains(&palavra))
}

/// Algum pedaço do caminho (pasta ou arquivo) sugere segredo?
pub fn caminho_parece_segredo(caminho_relativo: &str) -> bool {
    caminho_relativo.split('/').any(parece_segredo)
}

/// Começos conhecidos de chaves de API (o resto precisa ser longo).
pub(crate) const PREFIXOS_DE_CHAVE: &[&str] = &[
    "nvapi-",
    "sk-",
    "sk_live_",
    "rk_live_",
    "ghp_",
    "gho_",
    "ghs_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "hf_",
    "AKIA",
    "AIza",
];
/// Quantos caracteres, no mínimo, depois do prefixo.
pub(crate) const MIN_CARACTERES_DEPOIS_DO_PREFIXO: usize = 16;

/// O texto parece conter um segredo? Devolve o TIPO (nunca o valor).
pub fn parece_conter_segredo(texto: &str) -> Option<&'static str> {
    if texto.contains("-----BEGIN") && texto.contains("PRIVATE KEY") {
        return Some("chave privada");
    }
    let separadores = |c: char| c.is_whitespace() || "\"'`,;:=()[]{}<>".contains(c);
    for pedaco in texto.split(separadores) {
        for prefixo in PREFIXOS_DE_CHAVE {
            if let Some(resto) = pedaco.strip_prefix(prefixo)
                && resto.len() >= MIN_CARACTERES_DEPOIS_DO_PREFIXO
                && resto
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Some(prefixo);
            }
        }
    }
    None
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn nomes_de_segredo() {
        for nome in [
            ".env",
            ".env.local",
            "auth.json",
            "AUTH.JSON",
            "credentials.json",
            "client_secret_123.json",
            "id_rsa",
            "id_ed25519.pub",
            "api_key.txt",
            "openai-apikey",
            "secrets",
            ".ssh",
            "cert.pem",
            "token-usage.json",
            ".netrc",
        ] {
            assert!(parece_segredo(nome), "{nome} deveria ser segredo");
        }
        for nome in [
            "SOUL.md",
            "MEMORY.md",
            "USER.md",
            "journal.jsonl",
            "auto-modelo.json",
            "diario-pessoal.md",
            "g-001-migrar-runtime.json",
            "config.yaml",
            "author.md",
            "keyboard.md",
            "monkey.txt",
            "environment-notes.md",
        ] {
            assert!(!parece_segredo(nome), "{nome} não deveria ser segredo");
        }
        assert!(caminho_parece_segredo("secrets/notas.md"));
        assert!(!caminho_parece_segredo("metacognition/goals/x.json"));
    }

    #[test]
    fn conteudo_com_cara_de_chave() {
        let falsa = format!("a chave é ghp_{}", "x".repeat(36));
        assert_eq!(parece_conter_segredo(&falsa), Some("ghp_"));
        let nim = format!("NIM_API_KEY=nvapi-{}", "a1".repeat(20));
        assert_eq!(parece_conter_segredo(&nim), Some("nvapi-"));
        assert_eq!(
            parece_conter_segredo("-----BEGIN OPENSSH PRIVATE KEY-----\nabc"),
            Some("chave privada")
        );
        // Texto normal (inclusive palavras parecidas) passa.
        assert_eq!(
            parece_conter_segredo("uso o sk-learn e o hf_ às vezes"),
            None
        );
        assert_eq!(parece_conter_segredo("O servidor roda numa VM ARM."), None);
    }
}
