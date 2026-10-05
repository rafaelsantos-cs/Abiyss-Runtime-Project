//! Resolução segura de caminhos pedidos pelo modelo (ou por arquivos de
//! fora) dentro de uma pasta-raiz: workspace, skills, cofre de memória...
//!
//! Regras (iguais para todo mundo):
//! - só caminhos RELATIVOS à raiz; nada de `/absoluto` nem `..`;
//! - o trecho que já existe no disco, com links simbólicos resolvidos,
//!   tem de continuar dentro da raiz.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};

/// Transforma `pedido` num caminho dentro de `raiz` ou devolve erro
/// explicando por quê não. `raiz` precisa ser canônica (absoluta, sem
/// links). `nome_area` aparece nas mensagens ("workspace", "cofre"...).
pub fn resolver_dentro(raiz: &Path, pedido: &str, nome_area: &str) -> anyhow::Result<PathBuf> {
    let pedido = pedido.trim();
    if pedido.contains('\0') {
        bail!("caminho com caractere nulo");
    }
    let relativo = Path::new(if pedido.is_empty() { "." } else { pedido });

    // 1. Checagem "no papel": cada pedaço do caminho tem de ser um nome normal.
    for parte in relativo.components() {
        match parte {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                bail!("'..' não é permitido: use caminhos dentro do {nome_area}")
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("caminho absoluto não é permitido: use um caminho relativo ao {nome_area}")
            }
        }
    }
    let alvo = raiz.join(relativo);

    // 2. Checagem "no disco": o trecho que já existe, com os links
    //    simbólicos resolvidos, tem de continuar dentro da raiz.
    let existente = maior_ancestral_existente(&alvo);
    let real = existente
        .canonicalize()
        .with_context(|| format!("não consegui resolver {}", existente.display()))?;
    if !real.starts_with(raiz) {
        bail!("o caminho sai do {nome_area} (link simbólico?)");
    }
    Ok(alvo)
}

/// Sobe na árvore até achar um caminho que existe no disco.
pub fn maior_ancestral_existente(caminho: &Path) -> PathBuf {
    let mut atual = caminho.to_path_buf();
    // `symlink_metadata` conta links quebrados como "existentes", o que é
    // o certo aqui: queremos resolvê-los e ver para onde apontam.
    while atual.symlink_metadata().is_err() {
        if !atual.pop() {
            break;
        }
    }
    atual
}

/// Canonicaliza o que existir do caminho e junta o resto (para caminhos
/// que talvez ainda não existam, como `data/`).
pub fn canonico_aproximado(caminho: &Path) -> PathBuf {
    let existente = maior_ancestral_existente(caminho);
    let resto = caminho.strip_prefix(&existente).unwrap_or(Path::new(""));
    match existente.canonicalize() {
        Ok(base) => base.join(resto),
        Err(_) => caminho.to_path_buf(),
    }
}

/// Lê um arquivo de texto pequeno, recusando links simbólicos, arquivos
/// grandes demais e binários (com byte nulo).
pub fn ler_texto_limitado(caminho: &Path, max_bytes: usize) -> anyhow::Result<String> {
    let meta = caminho
        .symlink_metadata()
        .with_context(|| format!("não consegui abrir {}", caminho.display()))?;
    if meta.file_type().is_symlink() {
        bail!("não sigo links simbólicos: {}", caminho.display());
    }
    if !meta.is_file() {
        bail!("não é um arquivo: {}", caminho.display());
    }
    if meta.len() as usize > max_bytes {
        bail!(
            "arquivo grande demais ({} bytes; limite {max_bytes}): {}",
            meta.len(),
            caminho.display()
        );
    }
    let bytes = std::fs::read(caminho)?;
    if bytes.contains(&0) {
        bail!("parece ser binário; só leio texto: {}", caminho.display());
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn recusa_absoluto_ponto_ponto_e_nulo() {
        let pasta = tempfile::tempdir().unwrap();
        let raiz = pasta.path().canonicalize().unwrap();
        assert!(resolver_dentro(&raiz, "a/b.md", "cofre").is_ok());
        assert!(resolver_dentro(&raiz, "../fora", "cofre").is_err());
        assert!(resolver_dentro(&raiz, "/etc/passwd", "cofre").is_err());
        assert!(resolver_dentro(&raiz, "a\0b", "cofre").is_err());
        let erro = resolver_dentro(&raiz, "x/../../y", "cofre").unwrap_err();
        assert!(erro.to_string().contains("cofre"));
    }

    #[cfg(unix)]
    #[test]
    fn link_para_fora_e_recusado() {
        let pasta = tempfile::tempdir().unwrap();
        let raiz = pasta.path().join("raiz");
        std::fs::create_dir_all(&raiz).unwrap();
        let raiz = raiz.canonicalize().unwrap();
        std::os::unix::fs::symlink(pasta.path(), raiz.join("atalho")).unwrap();
        assert!(resolver_dentro(&raiz, "atalho/x", "cofre").is_err());
        std::fs::write(raiz.join("ok.md"), "oi").unwrap();
        std::os::unix::fs::symlink(raiz.join("ok.md"), raiz.join("link.md")).unwrap();
        assert!(ler_texto_limitado(&raiz.join("link.md"), 100).is_err());
        assert_eq!(ler_texto_limitado(&raiz.join("ok.md"), 100).unwrap(), "oi");
        assert!(ler_texto_limitado(&raiz.join("ok.md"), 1).is_err());
    }
}
