//! Backup noturno: banco, cofre, memória central e núcleo.
//!
//! O estado do Abiyss é insubstituível (memória, goals, diário). Antes de
//! cada sono — e com `abiyss backup` — o kernel guarda uma cópia em
//! `<dados>/backups/AAAA-MM-DD/`:
//!
//! - **banco:** `VACUUM INTO`, numa conexão própria e só de leitura. É um
//!   retrato consistente mesmo em modo WAL, com o daemon rodando. NUNCA se
//!   copia o `.db`/`-wal` a quente (a cópia pode nunca ter existido).
//!   Depois, `PRAGMA integrity_check` no arquivo gerado;
//! - **cofre, memória central e núcleo:** cópia recursiva que não segue
//!   links simbólicos, com teto de tamanho;
//! - **rotação:** os `manter_diarios` mais recentes + `manter_semanais`
//!   domingos anteriores; o resto é apagado;
//! - **disco:** abaixo de `disco_minimo_gb` livres, o backup é pulado (com
//!   aviso), para não encher o disco da VM.
//!
//! Restaurar é manual, de propósito (ver docs/SONO.md).

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use chrono::{Datelike, NaiveDate, Weekday};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;

use crate::config::Config;

/// `[backup]` no abiyss.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigBackup {
    pub ativo: bool,
    pub manter_diarios: usize,
    pub manter_semanais: usize,
    pub disco_minimo_gb: f64,
    pub max_megabytes_cofre: u64,
}

impl Default for ConfigBackup {
    fn default() -> Self {
        ConfigBackup {
            ativo: true,
            manter_diarios: 7,
            manter_semanais: 4,
            disco_minimo_gb: 2.0,
            max_megabytes_cofre: 2048,
        }
    }
}

/// O que um backup fez.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RelatorioBackup {
    /// Pasta do backup (vazia se foi pulado).
    pub pasta: PathBuf,
    /// `Some(motivo)` se o backup inteiro foi pulado.
    pub pulado: Option<String>,
    pub banco_bytes: u64,
    pub arquivos_copiados: usize,
    pub bytes_copiados: u64,
    pub links_ignorados: usize,
    /// Pastas de backups antigos apagadas pela rotação.
    pub removidos: Vec<String>,
    pub avisos: Vec<String>,
}

impl RelatorioBackup {
    pub fn resumo(&self) -> String {
        if let Some(motivo) = &self.pulado {
            return format!("backup pulado: {motivo}");
        }
        let mut texto = format!(
            "backup em {}: banco {} KiB, {} arquivo(s) ({} KiB) do cofre/identidade",
            self.pasta.display(),
            self.banco_bytes / 1024,
            self.arquivos_copiados,
            self.bytes_copiados / 1024
        );
        if self.links_ignorados > 0 {
            texto.push_str(&format!(
                ", {} link(s) simbólico(s) ignorado(s)",
                self.links_ignorados
            ));
        }
        if !self.removidos.is_empty() {
            texto.push_str(&format!("; rotação apagou: {}", self.removidos.join(", ")));
        }
        for aviso in &self.avisos {
            texto.push_str(&format!("; AVISO: {aviso}"));
        }
        texto
    }
}

/// Pasta onde ficam os backups.
pub fn pasta_backups(config: &Config) -> PathBuf {
    config.resolver(&config.caminhos.dados).join("backups")
}

/// Faz o backup de `hoje`. `disco_livre_gb` é medido por quem chama
/// (separado para os testes).
pub fn fazer_backup(
    config: &Config,
    hoje: NaiveDate,
    disco_livre_gb: f64,
) -> anyhow::Result<RelatorioBackup> {
    let cfg = &config.backup;
    if disco_livre_gb < cfg.disco_minimo_gb {
        return Ok(RelatorioBackup {
            pulado: Some(format!(
                "só {disco_livre_gb:.1} GiB livres (mínimo {:.1} GiB)",
                cfg.disco_minimo_gb
            )),
            ..Default::default()
        });
    }
    let raiz = pasta_backups(config);
    std::fs::create_dir_all(&raiz)
        .with_context(|| format!("não consegui criar {}", raiz.display()))?;
    let nome = hoje.format("%Y-%m-%d").to_string();
    let destino = raiz.join(&nome);
    // Monta numa pasta temporária e só troca no fim: um segundo backup no
    // mesmo dia substitui o primeiro sem deixar uma cópia pela metade.
    let temporaria = raiz.join(format!(".{nome}.montando"));
    if temporaria.exists() {
        std::fs::remove_dir_all(&temporaria)?;
    }
    std::fs::create_dir_all(&temporaria)?;

    let mut relatorio = RelatorioBackup::default();
    let montagem = (|| -> anyhow::Result<()> {
        // 1. Banco.
        let banco = config.caminho_banco();
        if banco.is_file() {
            let copia = temporaria.join("abiyss.db");
            copiar_banco(&banco, &copia)?;
            relatorio.banco_bytes = std::fs::metadata(&copia)?.len();
        } else {
            relatorio
                .avisos
                .push("banco ainda não existe: nada a copiar".into());
        }

        // 2. Cofre (com teto), memória central e núcleo.
        let cofre = config.caminho_cofre();
        if cofre.is_dir() {
            let limite = cfg.max_megabytes_cofre * 1024 * 1024;
            let tamanho = tamanho_sem_links(&cofre)?;
            if tamanho > limite {
                relatorio.avisos.push(format!(
                    "cofre com {} MiB passa do teto de {} MiB: cofre NÃO copiado",
                    tamanho / 1024 / 1024,
                    cfg.max_megabytes_cofre
                ));
            } else {
                copiar_arvore(&cofre, &temporaria.join("cofre"), &mut relatorio)?;
            }
        }
        let identidade = temporaria.join("identity");
        for arquivo in [
            config.caminho_memoria_central(),
            config.caminho_identidade(),
        ] {
            copiar_arquivo_se_existe(&arquivo, &identidade, &mut relatorio)?;
        }
        Ok(())
    })();
    if let Err(e) = montagem {
        let _ = std::fs::remove_dir_all(&temporaria);
        return Err(e);
    }

    if destino.exists() {
        std::fs::remove_dir_all(&destino)
            .with_context(|| format!("não consegui substituir {}", destino.display()))?;
    }
    std::fs::rename(&temporaria, &destino)?;
    relatorio.pasta = destino;
    relatorio.removidos = rodar_rotacao(&raiz, hoje, cfg.manter_diarios, cfg.manter_semanais)?;
    Ok(relatorio)
}

/// `VACUUM INTO` numa conexão própria, só de leitura, e confere o resultado.
fn copiar_banco(origem: &Path, destino: &Path) -> anyhow::Result<()> {
    let conexao = Connection::open_with_flags(
        origem,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("não consegui abrir {} para o backup", origem.display()))?;
    conexao.busy_timeout(std::time::Duration::from_secs(30))?;
    let caminho = destino.to_str().context("caminho do backup não é UTF-8")?;
    conexao
        .execute("VACUUM INTO ?1", [caminho])
        .context("VACUUM INTO falhou")?;
    drop(conexao);

    let copia = Connection::open_with_flags(destino, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let resultado: String = copia.query_row("PRAGMA integrity_check", [], |l| l.get(0))?;
    if resultado != "ok" {
        bail!("integrity_check do backup falhou: {resultado}");
    }
    Ok(())
}

/// Soma o tamanho dos arquivos, sem seguir links simbólicos.
fn tamanho_sem_links(pasta: &Path) -> anyhow::Result<u64> {
    let mut total = 0;
    for entrada in std::fs::read_dir(pasta)? {
        let entrada = entrada?;
        let tipo = entrada.file_type()?;
        if tipo.is_dir() {
            total += tamanho_sem_links(&entrada.path())?;
        } else if tipo.is_file() {
            total += entrada.metadata()?.len();
        }
    }
    Ok(total)
}

/// Copia uma árvore sem seguir links simbólicos (contados no relatório).
fn copiar_arvore(
    origem: &Path,
    destino: &Path,
    relatorio: &mut RelatorioBackup,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(destino)?;
    for entrada in std::fs::read_dir(origem)? {
        let entrada = entrada?;
        // `file_type` da entrada NÃO segue links.
        let tipo = entrada.file_type()?;
        let alvo = destino.join(entrada.file_name());
        if tipo.is_symlink() {
            relatorio.links_ignorados += 1;
        } else if tipo.is_dir() {
            copiar_arvore(&entrada.path(), &alvo, relatorio)?;
        } else if tipo.is_file() {
            relatorio.bytes_copiados += std::fs::copy(entrada.path(), &alvo)?;
            relatorio.arquivos_copiados += 1;
        }
    }
    Ok(())
}

fn copiar_arquivo_se_existe(
    arquivo: &Path,
    pasta: &Path,
    relatorio: &mut RelatorioBackup,
) -> anyhow::Result<()> {
    let Ok(meta) = std::fs::symlink_metadata(arquivo) else {
        return Ok(());
    };
    if meta.file_type().is_symlink() {
        relatorio.links_ignorados += 1;
        return Ok(());
    }
    if meta.is_file() {
        std::fs::create_dir_all(pasta)?;
        let nome = arquivo.file_name().context("arquivo sem nome")?;
        relatorio.bytes_copiados += std::fs::copy(arquivo, pasta.join(nome))?;
        relatorio.arquivos_copiados += 1;
    }
    Ok(())
}

/// Quais datas a rotação mantém (função pura): as `diarios` mais
/// recentes e, antes delas, até `semanais` domingos.
pub fn datas_mantidas(datas: &[NaiveDate], diarios: usize, semanais: usize) -> Vec<NaiveDate> {
    let mut ordenadas = datas.to_vec();
    ordenadas.sort_unstable_by(|a, b| b.cmp(a));
    ordenadas.dedup();
    let mut manter: Vec<NaiveDate> = ordenadas.iter().take(diarios).copied().collect();
    manter.extend(
        ordenadas
            .iter()
            .skip(diarios)
            .filter(|d| d.weekday() == Weekday::Sun)
            .take(semanais)
            .copied(),
    );
    manter
}

/// Apaga backups antigos (só pastas com nome de data). Devolve os nomes.
fn rodar_rotacao(
    raiz: &Path,
    hoje: NaiveDate,
    diarios: usize,
    semanais: usize,
) -> anyhow::Result<Vec<String>> {
    let mut datas = Vec::new();
    for entrada in std::fs::read_dir(raiz)? {
        let entrada = entrada?;
        if !entrada.file_type()?.is_dir() {
            continue;
        }
        let nome = entrada.file_name().to_string_lossy().to_string();
        if let Ok(data) = NaiveDate::parse_from_str(&nome, "%Y-%m-%d")
            && data <= hoje
        {
            datas.push(data);
        }
    }
    let manter = datas_mantidas(&datas, diarios.max(1), semanais);
    let mut removidos = Vec::new();
    for data in datas {
        if !manter.contains(&data) {
            let nome = data.format("%Y-%m-%d").to_string();
            std::fs::remove_dir_all(raiz.join(&nome))?;
            removidos.push(nome);
        }
    }
    removidos.sort();
    Ok(removidos)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn rotacao_mantem_diarios_e_domingos() {
        let hoje = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap(); // segunda-feira
        let datas: Vec<NaiveDate> = (0..60).map(|i| hoje - chrono::Days::new(i)).collect();
        let manter = datas_mantidas(&datas, 7, 4);
        assert_eq!(manter.len(), 11);
        // Os 7 mais recentes: 29/09 a 05/10.
        assert!(manter.contains(&NaiveDate::from_ymd_opt(2026, 9, 29).unwrap()));
        assert!(!manter.contains(&NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()));
        // Depois, só domingos: 27/09, 20/09, 13/09, 06/09.
        for dia in [27, 20, 13, 6] {
            assert!(manter.contains(&NaiveDate::from_ymd_opt(2026, 9, dia).unwrap()));
        }
        assert!(!manter.contains(&NaiveDate::from_ymd_opt(2026, 8, 30).unwrap()));
    }
}
