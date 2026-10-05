//! Ferramentas nativas de arquivo, confinadas à pasta `workspace/`.
//!
//! Regras de segurança (verificadas por código, não pelo modelo):
//! - só caminhos RELATIVOS ao workspace; nada de `/absoluto` nem `..`;
//! - links simbólicos não podem levar para fora do workspace;
//! - nunca escreve através de um link simbólico;
//! - o próprio workspace não pode conter o kernel, `identity/`, o banco,
//!   o `.env` nem a config (checado ao abrir, em `Workspace::abrir`).

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};

/// Limites de tamanho das operações (seção `[ferramentas]` do abiyss.toml).
pub use crate::config::ConfigFerramentas as LimitesWorkspace;

#[derive(Debug, Clone)]
pub struct Workspace {
    /// Caminho canônico (absoluto, sem links) da pasta do workspace.
    raiz: PathBuf,
    limites: LimitesWorkspace,
}

impl Workspace {
    /// Abre (criando se preciso) o workspace e confere que ele não
    /// engloba nenhum dos caminhos `protegidos` (kernel, identity...)
    /// nem fica dentro deles.
    pub fn abrir(
        pasta: &Path,
        protegidos: &[PathBuf],
        limites: LimitesWorkspace,
    ) -> anyhow::Result<Workspace> {
        std::fs::create_dir_all(pasta)
            .with_context(|| format!("não consegui criar o workspace {}", pasta.display()))?;
        let raiz = pasta.canonicalize()?;

        for protegido in protegidos {
            let protegido = canonico_aproximado(protegido);
            if protegido.starts_with(&raiz) {
                bail!(
                    "workspace inválido: {} contém a área protegida {}",
                    raiz.display(),
                    protegido.display()
                );
            }
            if raiz.starts_with(&protegido) {
                bail!(
                    "workspace inválido: {} fica dentro da área protegida {}",
                    raiz.display(),
                    protegido.display()
                );
            }
        }
        Ok(Workspace { raiz, limites })
    }

    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    /// Transforma o caminho pedido pelo modelo num caminho seguro dentro
    /// do workspace (ou devolve erro explicando por quê não).
    pub fn resolver(&self, pedido: &str) -> anyhow::Result<PathBuf> {
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
                    bail!("'..' não é permitido: use caminhos dentro do workspace")
                }
                Component::RootDir | Component::Prefix(_) => {
                    bail!("caminho absoluto não é permitido: use um caminho relativo ao workspace")
                }
            }
        }
        let alvo = self.raiz.join(relativo);

        // 2. Checagem "no disco": o trecho que já existe, com os links
        //    simbólicos resolvidos, tem de continuar dentro do workspace.
        let existente = maior_ancestral_existente(&alvo);
        let real = existente
            .canonicalize()
            .with_context(|| format!("não consegui resolver {}", existente.display()))?;
        if !real.starts_with(&self.raiz) {
            bail!("o caminho sai do workspace (link simbólico?)");
        }
        Ok(alvo)
    }

    /// Caminho relativo ao workspace, para mostrar nas mensagens.
    fn relativo(&self, caminho: &Path) -> String {
        let r = caminho.strip_prefix(&self.raiz).unwrap_or(caminho);
        if r.as_os_str().is_empty() {
            ".".to_string()
        } else {
            r.display().to_string()
        }
    }

    /// Lê um arquivo de texto (cortado em `max_bytes_leitura`).
    pub fn ler(&self, pedido: &str) -> anyhow::Result<String> {
        let caminho = self.resolver(pedido)?;
        if !caminho.is_file() {
            bail!("'{}' não é um arquivo existente", self.relativo(&caminho));
        }
        let bytes = std::fs::read(&caminho)?;
        let total = bytes.len();
        let limite = self.limites.max_bytes_leitura;
        let cortado = total > limite;
        let pedaco = &bytes[..total.min(limite)];
        // `from_utf8_lossy` aceita um caractere cortado no fim; mas um arquivo
        // binário de verdade vira lixo, então recusamos se tiver byte nulo.
        if pedaco.contains(&0) {
            bail!(
                "'{}' parece ser binário; só leio texto",
                self.relativo(&caminho)
            );
        }
        let mut texto = String::from_utf8_lossy(pedaco).to_string();
        if cortado {
            texto.push_str(&format!(
                "\n[... cortado: mostrando {limite} de {total} bytes]"
            ));
        }
        Ok(texto)
    }

    /// Lista uma pasta (opcionalmente com subpastas).
    pub fn listar(&self, pedido: &str, recursivo: bool) -> anyhow::Result<String> {
        let caminho = self.resolver(pedido)?;
        if !caminho.is_dir() {
            bail!("'{}' não é uma pasta existente", self.relativo(&caminho));
        }
        let mut linhas = Vec::new();
        let mut pendentes = vec![caminho];
        while let Some(pasta) = pendentes.pop() {
            let mut entradas: Vec<_> = std::fs::read_dir(&pasta)?.filter_map(Result::ok).collect();
            entradas.sort_by_key(|e| e.file_name());
            for entrada in entradas {
                if linhas.len() >= self.limites.max_itens_listagem {
                    linhas.push(format!(
                        "[... parei em {} itens]",
                        self.limites.max_itens_listagem
                    ));
                    return Ok(linhas.join("\n"));
                }
                // `symlink_metadata` NÃO segue links: um link aparece como link.
                let meta = entrada.path().symlink_metadata()?;
                let nome = self.relativo(&entrada.path());
                if meta.file_type().is_symlink() {
                    linhas.push(format!("{nome} (link simbólico)"));
                } else if meta.is_dir() {
                    linhas.push(format!("{nome}/"));
                    if recursivo {
                        pendentes.push(entrada.path());
                    }
                } else {
                    linhas.push(format!("{nome} ({} bytes)", meta.len()));
                }
            }
        }
        if linhas.is_empty() {
            Ok("(pasta vazia)".to_string())
        } else {
            Ok(linhas.join("\n"))
        }
    }

    /// Escreve (ou acrescenta ao fim de) um arquivo de texto.
    pub fn escrever(
        &self,
        pedido: &str,
        conteudo: &str,
        acrescentar: bool,
    ) -> anyhow::Result<String> {
        if conteudo.len() > self.limites.max_bytes_escrita {
            bail!(
                "conteúdo grande demais ({} bytes; limite {})",
                conteudo.len(),
                self.limites.max_bytes_escrita
            );
        }
        let caminho = self.resolver(pedido)?;
        if caminho == self.raiz {
            bail!("informe o nome de um arquivo");
        }
        if let Ok(meta) = caminho.symlink_metadata() {
            if meta.file_type().is_symlink() {
                bail!("não escrevo através de links simbólicos");
            }
            if meta.is_dir() {
                bail!("'{}' é uma pasta", self.relativo(&caminho));
            }
        }
        if let Some(pasta) = caminho.parent() {
            std::fs::create_dir_all(pasta)?;
            // Confere de novo depois de criar as pastas (defesa extra).
            if !pasta.canonicalize()?.starts_with(&self.raiz) {
                bail!("o caminho sai do workspace");
            }
        }
        let mut arquivo = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(acrescentar)
            .truncate(!acrescentar)
            .open(&caminho)?;
        arquivo.write_all(conteudo.as_bytes())?;
        let acao = if acrescentar {
            "Acrescentei"
        } else {
            "Escrevi"
        };
        Ok(format!(
            "{acao} {} bytes em {}",
            conteudo.len(),
            self.relativo(&caminho)
        ))
    }
}

/// Sobe na árvore até achar um caminho que existe no disco.
fn maior_ancestral_existente(caminho: &Path) -> PathBuf {
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
/// protegidos que talvez ainda não existam, como `data/`).
fn canonico_aproximado(caminho: &Path) -> PathBuf {
    let existente = maior_ancestral_existente(caminho);
    let resto = caminho.strip_prefix(&existente).unwrap_or(Path::new(""));
    match existente.canonicalize() {
        Ok(base) => base.join(resto),
        Err(_) => caminho.to_path_buf(),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let pasta = tempfile::tempdir().unwrap();
        let ws = Workspace::abrir(
            &pasta.path().join("workspace"),
            &[pasta.path().join("kernel"), pasta.path().join("identity")],
            LimitesWorkspace::default(),
        )
        .unwrap();
        (pasta, ws)
    }

    #[test]
    fn escreve_le_e_lista_dentro_do_workspace() {
        let (_p, ws) = workspace();
        ws.escrever("notas/dia1.md", "olá", false).unwrap();
        ws.escrever("notas/dia1.md", " mundo", true).unwrap();
        assert_eq!(ws.ler("notas/dia1.md").unwrap(), "olá mundo");
        assert!(ws.ler("./notas/../notas/dia1.md").is_err());
        let lista = ws.listar("", true).unwrap();
        assert!(lista.contains("notas/"));
        assert!(lista.contains("notas/dia1.md (10 bytes)"));
    }

    #[test]
    fn recusa_absoluto_e_ponto_ponto() {
        let (_p, ws) = workspace();
        assert!(ws.ler("/etc/passwd").is_err());
        assert!(ws.ler("../identity/nucleo.md").is_err());
        assert!(ws.escrever("../kernel/src/main.rs", "x", false).is_err());
        assert!(ws.escrever("a/../../fora.txt", "x", false).is_err());
        assert!(ws.listar("..", false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn link_simbolico_nao_escapa() {
        let (pasta, ws) = workspace();
        let fora = pasta.path().join("identity");
        std::fs::create_dir_all(&fora).unwrap();
        std::fs::write(fora.join("nucleo.md"), "segredo").unwrap();
        std::os::unix::fs::symlink(&fora, ws.raiz().join("atalho")).unwrap();
        std::os::unix::fs::symlink(fora.join("nucleo.md"), ws.raiz().join("arquivo")).unwrap();

        assert!(ws.ler("atalho/nucleo.md").is_err());
        assert!(ws.ler("arquivo").is_err());
        assert!(ws.escrever("atalho/novo.md", "x", false).is_err());
        assert!(ws.escrever("arquivo", "x", false).is_err());
        assert_eq!(
            std::fs::read_to_string(fora.join("nucleo.md")).unwrap(),
            "segredo"
        );
        assert!(!fora.join("novo.md").exists());
    }

    #[test]
    fn workspace_nao_pode_engolir_area_protegida() {
        let pasta = tempfile::tempdir().unwrap();
        let protegidos = [pasta.path().join("kernel"), pasta.path().join("identity")];
        // Workspace = raiz do projeto: conteria o kernel.
        assert!(Workspace::abrir(pasta.path(), &protegidos, LimitesWorkspace::default()).is_err());
        // Workspace dentro de identity/.
        assert!(
            Workspace::abrir(
                &pasta.path().join("identity/ws"),
                &protegidos,
                LimitesWorkspace::default()
            )
            .is_err()
        );
    }

    #[test]
    fn limites_de_tamanho_e_binario() {
        let pasta = tempfile::tempdir().unwrap();
        let limites = LimitesWorkspace {
            max_bytes_leitura: 5,
            max_bytes_escrita: 8,
            max_itens_listagem: 2,
        };
        let ws = Workspace::abrir(&pasta.path().join("ws"), &[], limites).unwrap();
        assert!(ws.escrever("a.txt", "123456789", false).is_err());
        ws.escrever("a.txt", "12345678", false).unwrap();
        assert!(ws.ler("a.txt").unwrap().contains("cortado"));
        std::fs::write(ws.raiz().join("b.bin"), [0u8, 1, 2]).unwrap();
        assert!(ws.ler("b.bin").is_err());
        ws.escrever("c.txt", "x", false).unwrap();
        assert!(ws.listar(".", false).unwrap().contains("parei em 2"));
    }
}
