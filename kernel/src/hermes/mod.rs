//! `abiyss importar-hermes`: traz a memória do Abiyss do framework antigo
//! (Hermes) para este runtime.
//!
//! - **Simulação por padrão**: mostra o relatório do que SERIA importado.
//!   Só grava com `--aplicar`.
//! - **Idempotente**: cada item tem uma chave estável (tabela
//!   `importacoes`); rodar duas vezes não duplica nada.
//! - **Segredos**: arquivos cujo nome sugere segredo (`.env`, `auth.json`...)
//!   nunca são abertos; só aparecem no relatório. Conteúdo com cara de
//!   chave de API também não é importado.
//! - **Tolerante**: formatos incertos; campos desconhecidos são
//!   preservados (`extras`) e listados no relatório, nunca descartados.
//! - Só os arquivos MAPEADOS são lidos (lista abaixo); o resto é só
//!   listado pelo nome.
//!
//! | Origem (relativa à pasta do Hermes) | Destino |
//! |---|---|
//! | `metacognition/goals/*.json` | tabela `goals` |
//! | `metacognition/journal.jsonl` | tabela `diario` |
//! | `metacognition/diario-pessoal.md` | `01_internal/diario/` |
//! | `metacognition/identity/auto-modelo.json`, `SOUL.md` | `01_internal/identidade/` + `identity/nucleo.proposto.md` |
//! | `memories/MEMORY.md`, `memories/USER.md` | memória central (+ excedente em `01_internal/`) |
//!
//! As suposições sobre cada formato estão em `docs/IMPORTAR_HERMES.md`.

pub mod campos;
mod diario_pessoal;
mod goals;
mod identidade;
mod journal;
mod memorias;
pub mod segredos;

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use crate::config::Config;
use crate::db::Banco;
use crate::frontmatter::{Documento, Value};
use crate::importacoes;
use crate::memoria::central::MemoriaCentral;
use crate::memoria::cofre::Cofre;
use crate::memoria::nota::{Escopo, Procedencia};
use crate::memoria::propostas;
use campos::Leitor;
use segredos::{caminho_parece_segredo, parece_conter_segredo, parece_segredo};

/// Tamanho máximo de um arquivo do Hermes lido pelo importador.
const MAX_BYTES_ARQUIVO: u64 = 64 * 1024 * 1024;
/// Proteção contra pastas enormes no inventário.
const MAX_ENTRADAS_INVENTARIO: usize = 100_000;
/// Pastas grandes e sem memória: nem são percorridas (aparecem no relatório).
const PASTAS_NAO_PERCORRIDAS: &[&str] = &[
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
];

/// O que existe na pasta do Hermes (só nomes; nenhum arquivo é aberto aqui).
#[derive(Debug, Default)]
struct Inventario {
    /// Arquivos comuns, relativos à origem, com "/".
    arquivos: Vec<String>,
    /// Arquivos e pastas cujo nome sugere segredo.
    segredos: Vec<String>,
    links: Vec<String>,
    pastas_puladas: Vec<String>,
}

/// Uma parte do relatório (uma origem do Hermes).
#[derive(Debug, Default)]
pub struct Secao {
    pub titulo: String,
    pub novos: usize,
    pub ja_importados: usize,
    /// Linhas de detalhe ("+ novo", "= já importado", avisos...).
    pub linhas: Vec<String>,
    pub erros: Vec<String>,
    /// campo de destino → (campo de origem → vezes)
    pub mapeamento: BTreeMap<&'static str, BTreeMap<String, usize>>,
    /// Campos desconhecidos (preservados) → vezes.
    pub desconhecidos: BTreeMap<String, usize>,
}

impl Secao {
    fn nova(titulo: &str) -> Secao {
        Secao {
            titulo: titulo.to_string(),
            ..Default::default()
        }
    }

    /// Junta ao relatório o que o leitor de campos anotou.
    fn anotar(&mut self, leitor: &Leitor<'_>) {
        for (destino, origem) in &leitor.usados {
            *self
                .mapeamento
                .entry(destino)
                .or_default()
                .entry(origem.clone())
                .or_default() += 1;
        }
        for chave in leitor.extras().keys() {
            *self.desconhecidos.entry(chave.clone()).or_default() += 1;
        }
    }
}

/// Relatório completo de uma importação (simulada ou aplicada).
#[derive(Debug, Default)]
pub struct Relatorio {
    pub origem: PathBuf,
    pub aplicado: bool,
    pub segredos_pulados: Vec<String>,
    pub links_ignorados: Vec<String>,
    pub pastas_puladas: Vec<String>,
    pub nao_mapeados: Vec<String>,
    pub secoes: Vec<Secao>,
}

impl Relatorio {
    pub fn total_novos(&self) -> usize {
        self.secoes.iter().map(|s| s.novos).sum()
    }

    pub fn total_ja_importados(&self) -> usize {
        self.secoes.iter().map(|s| s.ja_importados).sum()
    }

    pub fn total_erros(&self) -> usize {
        self.secoes.iter().map(|s| s.erros.len()).sum()
    }
}

/// Tudo de que os importadores de cada origem precisam.
struct Contexto<'a> {
    origem: PathBuf,
    inventario: Inventario,
    banco: &'a Banco,
    cofre: &'a Cofre,
    central: &'a MemoriaCentral,
    /// `identity/nucleo.proposto.md` (rascunho; nunca o `nucleo.md`).
    caminho_proposto: PathBuf,
    aplicar: bool,
    /// Arquivos que o importador leu (o resto vira "não mapeado").
    lidos: HashSet<String>,
    /// IDs de goals do Hermes importados (ou que seriam) nesta execução.
    goals_planejados: HashSet<String>,
}

impl Contexto<'_> {
    /// O arquivo existe no inventário (já sem segredos e sem links)?
    fn tem(&self, relativo: &str) -> bool {
        self.inventario.arquivos.iter().any(|a| a == relativo)
    }

    /// ÚNICO jeito de ler um arquivo do Hermes. Recusa nomes com cara de
    /// segredo (em qualquer parte do caminho), links simbólicos e arquivos
    /// grandes demais.
    fn ler(&mut self, relativo: &str) -> anyhow::Result<String> {
        if caminho_parece_segredo(relativo) {
            bail!("'{relativo}' tem nome de segredo: não abro");
        }
        let caminho = self.origem.join(relativo);
        let meta = caminho
            .symlink_metadata()
            .with_context(|| format!("não achei '{relativo}'"))?;
        if meta.file_type().is_symlink() {
            bail!("'{relativo}' é link simbólico: não sigo");
        }
        if !meta.is_file() {
            bail!("'{relativo}' não é um arquivo");
        }
        if meta.len() > MAX_BYTES_ARQUIVO {
            bail!("'{relativo}' é grande demais ({} bytes)", meta.len());
        }
        let bytes =
            std::fs::read(&caminho).with_context(|| format!("não consegui ler '{relativo}'"))?;
        self.lidos.insert(relativo.to_string());
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    /// Grava (ou só simula) uma nota interna importada. Cuida de: chave já
    /// importada, conteúdo com cara de segredo, nota esquecida pelo usuário
    /// e conflito de nome com uma nota que não veio do Hermes.
    fn importar_nota(
        &self,
        secao: &mut Secao,
        chave: &str,
        destino: &str,
        mut doc: Documento,
        procedencia: &Procedencia,
        origem: &str,
    ) -> anyhow::Result<()> {
        if importacoes::ja_importado(self.banco, chave)? {
            secao.ja_importados += 1;
            secao.linhas.push(format!("= {destino} (já importada)"));
            return Ok(());
        }
        if let Some(tipo) = parece_conter_segredo(&doc.corpo) {
            secao.erros.push(format!(
                "{origem}: parece conter um segredo ({tipo}); NÃO importado — revise o arquivo à mão"
            ));
            return Ok(());
        }
        if propostas::esquecida_em(self.banco, destino)?.is_some() {
            secao.linhas.push(format!(
                "- {destino}: esquecida pelo usuário antes; não reimportada"
            ));
            return Ok(());
        }
        // A nota já existe e veio desta mesma origem (ex.: o banco foi
        // recriado e a chave se perdeu): é a mesma importação, não duplica.
        let marca_origem = format!("hermes:{origem}");
        let mesma_origem = |caminho: &str| {
            self.cofre
                .ler(caminho, usize::MAX)
                .is_ok_and(|n| n.doc.texto("origem").as_deref() == Some(marca_origem.as_str()))
        };
        for caminho in [
            destino.to_string(),
            format!("{}-hermes.md", destino.trim_end_matches(".md")),
        ] {
            if self.cofre.existe(&caminho) && mesma_origem(&caminho) {
                if self.aplicar {
                    importacoes::registrar(self.banco, chave, "nota", &caminho, "")?;
                }
                secao.ja_importados += 1;
                secao
                    .linhas
                    .push(format!("= {caminho} (já existia, mesma origem)"));
                return Ok(());
            }
        }
        // Nome já usado por outra nota: grava ao lado, com sufixo.
        let mut final_ = destino.to_string();
        if self.cofre.existe(destino) {
            final_ = format!("{}-hermes.md", destino.trim_end_matches(".md"));
            if self.cofre.existe(&final_) {
                secao.erros.push(format!(
                    "{destino} e {final_} já existem e não vieram desta importação; nada gravado"
                ));
                return Ok(());
            }
        }
        doc.campos.insert(
            Value::String("origem".into()),
            Value::String(marca_origem.clone()),
        );
        if self.aplicar {
            self.cofre.gravar(&final_, &doc, procedencia)?;
            importacoes::registrar(self.banco, chave, "nota", &final_, "")?;
        }
        secao.novos += 1;
        secao.linhas.push(format!("+ {final_}"));
        Ok(())
    }
}

/// Importa (ou simula, com `aplicar = false`) a pasta do Hermes.
pub fn importar(config: &Config, origem: &Path, aplicar: bool) -> anyhow::Result<Relatorio> {
    let origem = origem
        .canonicalize()
        .with_context(|| format!("pasta de origem inválida: {}", origem.display()))?;
    if !origem.is_dir() {
        bail!("{} não é uma pasta", origem.display());
    }
    let inventario = percorrer(&origem);

    // Na simulação, nada que ainda não existe é criado: banco em memória
    // e cofre numa pasta temporária (apagada no fim).
    let banco = if aplicar || config.caminho_banco().exists() {
        Banco::abrir(&config.caminho_banco())?
    } else {
        Banco::em_memoria()?
    };
    let caminho_cofre = config.caminho_cofre();
    let cofre_pronto = [Escopo::Interno, Escopo::Externo]
        .iter()
        .all(|e| caminho_cofre.join(e.pasta()).is_dir());
    let temporario = if aplicar || cofre_pronto {
        None
    } else {
        Some(std::env::temp_dir().join(format!("abiyss-simulacao-cofre-{}", std::process::id())))
    };
    let cofre = Cofre::abrir(temporario.as_deref().unwrap_or(&caminho_cofre))?;
    let central = MemoriaCentral::da_config(config);
    let caminho_proposto = config
        .caminho_identidade()
        .with_file_name("nucleo.proposto.md");
    if caminho_proposto == config.caminho_identidade() {
        bail!("o rascunho não pode ter o mesmo caminho do núcleo de identidade");
    }

    let mut ctx = Contexto {
        origem: origem.clone(),
        inventario,
        banco: &banco,
        cofre: &cofre,
        central: &central,
        caminho_proposto,
        aplicar,
        lidos: HashSet::new(),
        goals_planejados: HashSet::new(),
    };
    // Goals antes do journal: as entradas do journal apontam para goals.
    let secoes = vec![
        goals::importar(&mut ctx)?,
        journal::importar(&mut ctx)?,
        diario_pessoal::importar(&mut ctx)?,
        identidade::importar(&mut ctx)?,
        memorias::importar(&mut ctx)?,
    ];

    let nao_mapeados = ctx
        .inventario
        .arquivos
        .iter()
        .filter(|a| !ctx.lidos.contains(*a))
        .cloned()
        .collect();
    let relatorio = Relatorio {
        origem,
        aplicado: aplicar,
        segredos_pulados: std::mem::take(&mut ctx.inventario.segredos),
        links_ignorados: std::mem::take(&mut ctx.inventario.links),
        pastas_puladas: std::mem::take(&mut ctx.inventario.pastas_puladas),
        nao_mapeados,
        secoes,
    };
    if let Some(pasta) = temporario {
        let _ = std::fs::remove_dir_all(pasta);
    }
    Ok(relatorio)
}

/// Lista o que há na origem SÓ PELOS NOMES (nenhum arquivo é aberto).
fn percorrer(origem: &Path) -> Inventario {
    let mut inventario = Inventario::default();
    let mut pendentes = vec![PathBuf::new()];
    let mut vistos = 0;
    while let Some(relativa) = pendentes.pop() {
        let Ok(entradas) = std::fs::read_dir(origem.join(&relativa)) else {
            continue;
        };
        let mut entradas: Vec<_> = entradas.filter_map(Result::ok).collect();
        entradas.sort_by_key(|e| e.file_name());
        for entrada in entradas {
            vistos += 1;
            if vistos > MAX_ENTRADAS_INVENTARIO {
                return inventario;
            }
            let nome = entrada.file_name().to_string_lossy().to_string();
            let caminho = relativa.join(&nome).to_string_lossy().replace('\\', "/");
            // `file_type` da entrada não segue links simbólicos.
            let Ok(tipo) = entrada.file_type() else {
                continue;
            };
            if tipo.is_symlink() {
                inventario.links.push(caminho);
            } else if parece_segredo(&nome) {
                let marca = if tipo.is_dir() { "/" } else { "" };
                inventario.segredos.push(format!("{caminho}{marca}"));
            } else if tipo.is_dir() {
                if PASTAS_NAO_PERCORRIDAS.contains(&nome.as_str()) {
                    inventario.pastas_puladas.push(format!("{caminho}/"));
                } else {
                    pendentes.push(relativa.join(&nome));
                }
            } else if tipo.is_file() {
                inventario.arquivos.push(caminho);
            }
        }
    }
    inventario.arquivos.sort();
    inventario.segredos.sort();
    inventario.links.sort();
    inventario
}

impl fmt::Display for Relatorio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.aplicado {
            writeln!(f, "Importação do Hermes — APLICADA")?;
        } else {
            writeln!(
                f,
                "Importação do Hermes — SIMULAÇÃO: nada foi gravado. Rode de novo com --aplicar para importar."
            )?;
        }
        writeln!(f, "Origem: {}", self.origem.display())?;

        lista(
            f,
            "Pulados por parecerem segredos (NÃO foram abertos)",
            &self.segredos_pulados,
        )?;
        lista(f, "Links simbólicos (não seguidos)", &self.links_ignorados)?;
        lista(f, "Pastas não percorridas", &self.pastas_puladas)?;
        lista(
            f,
            "Não mapeados (só listados; não foram lidos)",
            &agrupar(&self.nao_mapeados),
        )?;

        for secao in &self.secoes {
            writeln!(f, "\n== {} ==", secao.titulo)?;
            writeln!(
                f,
                "  novos: {}   já importados: {}   erros: {}",
                secao.novos,
                secao.ja_importados,
                secao.erros.len()
            )?;
            for linha in &secao.linhas {
                writeln!(f, "  {linha}")?;
            }
            if !secao.mapeamento.is_empty() {
                writeln!(f, "  mapeamento de campos (destino ← origem):")?;
                for (destino, origens) in &secao.mapeamento {
                    let partes: Vec<String> =
                        origens.iter().map(|(o, n)| format!("{o} ({n}x)")).collect();
                    writeln!(f, "    {destino} ← {}", partes.join(", "))?;
                }
            }
            if !secao.desconhecidos.is_empty() {
                let partes: Vec<String> = secao
                    .desconhecidos
                    .iter()
                    .map(|(c, n)| format!("{c} ({n}x)"))
                    .collect();
                writeln!(
                    f,
                    "  campos desconhecidos (preservados, não descartados): {}",
                    partes.join(", ")
                )?;
            }
            for erro in &secao.erros {
                writeln!(f, "  ERRO: {erro}")?;
            }
        }
        writeln!(
            f,
            "\nTotal: {} novo(s), {} já importado(s), {} erro(s).",
            self.total_novos(),
            self.total_ja_importados(),
            self.total_erros()
        )?;
        writeln!(
            f,
            "Suposições sobre os formatos do Hermes: docs/IMPORTAR_HERMES.md"
        )
    }
}

fn lista(f: &mut fmt::Formatter<'_>, titulo: &str, itens: &[String]) -> fmt::Result {
    if itens.is_empty() {
        return Ok(());
    }
    writeln!(f, "\n{titulo}:")?;
    for item in itens {
        writeln!(f, "  {item}")?;
    }
    Ok(())
}

/// Pastas com muitos arquivos não mapeados viram uma linha só.
fn agrupar(caminhos: &[String]) -> Vec<String> {
    let mut por_pasta: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for caminho in caminhos {
        let pasta = match caminho.split_once('/') {
            Some((p, _)) => format!("{p}/"),
            None => String::new(),
        };
        por_pasta.entry(pasta).or_default().push(caminho);
    }
    let mut linhas = Vec::new();
    for (pasta, arquivos) in por_pasta {
        if !pasta.is_empty() && arquivos.len() > 5 {
            linhas.push(format!("{pasta} ({} arquivos)", arquivos.len()));
        } else {
            linhas.extend(arquivos.into_iter().cloned());
        }
    }
    linhas
}
