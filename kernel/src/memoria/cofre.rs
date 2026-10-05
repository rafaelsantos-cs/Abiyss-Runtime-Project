//! O cofre: uma pasta no formato do Obsidian com dois escopos,
//! `01_internal/` e `02_external/`.
//!
//! TODA gravação de nota passa por `Cofre::gravar`. É ali que o kernel
//! garante, por código (e não por instrução ao modelo):
//! 1. a REGRA DURA: conteúdo de origem externa nunca é gravado em 01_internal;
//! 2. toda nota tem procedência no frontmatter (`fonte`, `tipo`, `criado`,
//!    `atualizado`), preenchida pelo kernel;
//! 3. dito e deduzido nunca se misturam na mesma nota;
//! 4. gravar nunca apaga: conteúdo novo é ACRESCENTADO (com uma linha de
//!    procedência); só `abiyss memoria esquecer` remove uma nota;
//! 5. notas externas têm links canônicos, nível de navegador, data de
//!    revalidação e resumos sempre datados, e não apontam para notas internas.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use super::nota::{CAMPOS_DO_KERNEL, Escopo, Fonte, Nota, Procedencia, Tipo, extrair_wikilinks};
use crate::caminho_seguro::{ler_texto_limitado, resolver_dentro};
use crate::frontmatter::{self, Documento, Mapping, Value};
use crate::tempo::{agora_iso, hoje};

/// Mensagem da regra dura (aparece nos erros e nos relatórios do sleep).
pub const REGRA_DURA: &str = "conteúdo de origem externa (ferramentas, web, sub-agentes...) não pode ser gravado em 01_internal";

/// Níveis de navegador aceitos nas notas externas (o navegador em si é da
/// sessão C; aqui só se registra qual nível usar para cada fonte).
pub const NIVEIS_NAVEGADOR: &[&str] = &["rapido", "contemplativo", "agentico"];

/// Proteção contra cofres enormes na listagem.
const MAX_NOTAS_LISTADAS: usize = 50_000;

/// O que `gravar` fez.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gravacao {
    Criada,
    Atualizada,
}

impl Gravacao {
    pub fn como_texto(&self) -> &'static str {
        match self {
            Gravacao::Criada => "criada",
            Gravacao::Atualizada => "atualizada",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Cofre {
    /// Caminho canônico da pasta do cofre.
    raiz: PathBuf,
}

impl Cofre {
    /// Abre o cofre, criando a pasta e as duas pastas de escopo se preciso.
    pub fn abrir(pasta: &Path) -> anyhow::Result<Cofre> {
        for escopo in [Escopo::Interno, Escopo::Externo] {
            let p = pasta.join(escopo.pasta());
            std::fs::create_dir_all(&p)
                .with_context(|| format!("não consegui criar {}", p.display()))?;
        }
        Ok(Cofre {
            raiz: pasta.canonicalize()?,
        })
    }

    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    /// Valida e normaliza um caminho relativo ao cofre
    /// (ex.: "01_internal/pessoas/ana" → "01_internal/pessoas/ana.md").
    pub fn normalizar(&self, caminho: &str) -> anyhow::Result<(Escopo, String)> {
        let mut c = caminho.trim().replace('\\', "/");
        while let Some(resto) = c.strip_prefix("./") {
            c = resto.to_string();
        }
        if c.is_empty() {
            bail!("caminho vazio");
        }
        if !c.ends_with(".md") {
            if Path::new(&c).extension().is_some() {
                bail!("só notas .md ficam no cofre: '{caminho}'");
            }
            c.push_str(".md");
        }
        let Some(escopo) = Escopo::do_caminho(&c) else {
            bail!("o caminho precisa começar com 01_internal/ ou 02_external/: '{caminho}'");
        };
        let partes: Vec<&str> = c.split('/').collect();
        if partes.len() < 2 {
            bail!("informe uma nota dentro de {}", escopo.pasta());
        }
        for parte in &partes {
            if parte.is_empty() || parte.starts_with('.') {
                bail!("partes vazias, ocultas ou '..' não são permitidas: '{caminho}'");
            }
        }
        // Links simbólicos não podem levar para fora do cofre.
        resolver_dentro(&self.raiz, &c, "cofre")?;
        Ok((escopo, c))
    }

    /// Caminho de uma proposta: relativo ao escopo ("pessoas/ana.md") ou já
    /// com o prefixo do escopo ("01_internal/pessoas/ana.md").
    pub fn caminho_no_escopo(&self, escopo: Escopo, caminho: &str) -> anyhow::Result<String> {
        let c = caminho.trim().replace('\\', "/");
        let completo = if Escopo::do_caminho(&c).is_some() {
            c
        } else {
            format!("{}/{}", escopo.pasta(), c.trim_start_matches("./"))
        };
        let (achado, normal) = self.normalizar(&completo)?;
        if achado != escopo {
            bail!(
                "o caminho '{caminho}' é do escopo {} mas a proposta é para o escopo {}",
                achado.como_texto(),
                escopo.como_texto()
            );
        }
        Ok(normal)
    }

    fn absoluto(&self, relativo: &str) -> PathBuf {
        self.raiz.join(relativo)
    }

    /// A nota existe? (caminho já normalizado)
    pub fn existe(&self, relativo: &str) -> bool {
        self.absoluto(relativo).is_file()
    }

    /// Lê uma nota. Frontmatter quebrado não impede a leitura (vira corpo).
    pub fn ler(&self, caminho: &str, max_bytes: usize) -> anyhow::Result<Nota> {
        let (escopo, caminho) = self.normalizar(caminho)?;
        let arquivo = self.absoluto(&caminho);
        if !arquivo.exists() {
            bail!("a nota '{caminho}' não existe");
        }
        let texto = ler_texto_limitado(&arquivo, max_bytes)?;
        let doc = frontmatter::separar(&texto).unwrap_or_else(|_| Documento::sem_campos(&texto));
        Ok(Nota {
            caminho,
            escopo,
            texto,
            doc,
        })
    }

    /// Todas as notas (.md) de um escopo, ou das duas se `None`, em ordem.
    /// Pastas ocultas (`.obsidian`...) e links simbólicos ficam de fora.
    pub fn listar(&self, escopo: Option<Escopo>) -> Vec<String> {
        let escopos = match escopo {
            Some(e) => vec![e],
            None => vec![Escopo::Interno, Escopo::Externo],
        };
        let mut notas = Vec::new();
        for e in escopos {
            let mut pendentes = vec![self.absoluto(e.pasta())];
            while let Some(pasta) = pendentes.pop() {
                let Ok(entradas) = std::fs::read_dir(&pasta) else {
                    continue;
                };
                for entrada in entradas.filter_map(Result::ok) {
                    if notas.len() >= MAX_NOTAS_LISTADAS {
                        break;
                    }
                    let nome = entrada.file_name().to_string_lossy().to_string();
                    let Ok(tipo) = entrada.file_type() else {
                        continue;
                    };
                    if nome.starts_with('.') || tipo.is_symlink() {
                        continue;
                    }
                    if tipo.is_dir() {
                        pendentes.push(entrada.path());
                    } else if tipo.is_file()
                        && nome.ends_with(".md")
                        && let Ok(relativo) = entrada.path().strip_prefix(&self.raiz)
                    {
                        notas.push(relativo.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
        }
        notas.sort();
        notas
    }

    /// Grava conteúdo numa nota: cria (com procedência no frontmatter) ou
    /// acrescenta ao fim. ÚNICO caminho de escrita de notas do kernel.
    pub fn gravar(
        &self,
        caminho: &str,
        novo: &Documento,
        procedencia: &Procedencia,
    ) -> anyhow::Result<Gravacao> {
        let (escopo, caminho) = self.normalizar(caminho)?;

        // 1. A REGRA DURA, checada aqui, no único caminho de escrita.
        if escopo == Escopo::Interno
            && let Some(origem) = &procedencia.origem_externa
        {
            bail!("{REGRA_DURA} (origem: {origem})");
        }

        let destino = self.absoluto(&caminho);
        let agora = agora_iso();
        let (campos, corpo, gravacao) = if destino.exists() {
            let texto = ler_texto_limitado(&destino, usize::MAX)?;
            let atual = frontmatter::separar(&texto).with_context(|| {
                format!("a nota '{caminho}' tem frontmatter inválido; corrija-a antes")
            })?;
            // 3. Dito e deduzido nunca se misturam.
            match atual.texto("tipo").as_deref().and_then(Tipo::de_texto) {
                Some(tipo) if tipo == procedencia.tipo => {}
                Some(tipo) => bail!(
                    "a nota '{caminho}' é '{}' e o conteúdo novo é '{}': dito e deduzido nunca se misturam na mesma nota (use outra nota)",
                    tipo.como_texto(),
                    procedencia.tipo.como_texto()
                ),
                None => bail!(
                    "a nota '{caminho}' não tem 'tipo' (dito|deduzido) no frontmatter; sem isso não dá para garantir que dito e deduzido não se misturem"
                ),
            }
            let mut campos = atual.campos.clone();
            acrescentar_fonte(&mut campos, procedencia.fonte);
            campos.insert(texto_yaml("atualizado"), texto_yaml(&agora));
            mesclar_campos(&mut campos, &novo.campos);
            // 4. Acrescenta, com uma linha dizendo de onde veio o bloco.
            let corpo = format!(
                "{}\n\n*(acrescentado em {} — fonte: {}, {})*\n\n{}",
                atual.corpo.trim_end(),
                hoje(),
                procedencia.fonte.como_texto(),
                procedencia.rotulo,
                novo.corpo.trim()
            );
            (campos, corpo, Gravacao::Atualizada)
        } else {
            // 2. Procedência preenchida pelo kernel, nesta ordem.
            let mut campos = Mapping::new();
            campos.insert(
                texto_yaml("fonte"),
                texto_yaml(procedencia.fonte.como_texto()),
            );
            campos.insert(
                texto_yaml("tipo"),
                texto_yaml(procedencia.tipo.como_texto()),
            );
            let criado = procedencia.criado.clone().unwrap_or_else(|| agora.clone());
            campos.insert(texto_yaml("criado"), texto_yaml(&criado));
            campos.insert(texto_yaml("atualizado"), texto_yaml(&agora));
            mesclar_campos(&mut campos, &novo.campos);
            (campos, novo.corpo.trim().to_string(), Gravacao::Criada)
        };

        // 5. Regras das notas externas (sobre o resultado final).
        if escopo == Escopo::Externo {
            validar_externa(&campos, &corpo, &self.listar(None))
                .with_context(|| format!("nota externa '{caminho}' recusada"))?;
        }

        let texto = frontmatter::montar(&campos, &corpo)?;
        self.escrever_atomico(&destino, &texto)?;
        Ok(gravacao)
    }

    /// Escreve num arquivo temporário e renomeia: uma queda no meio nunca
    /// deixa uma nota pela metade.
    fn escrever_atomico(&self, destino: &Path, texto: &str) -> anyhow::Result<()> {
        if let Ok(meta) = destino.symlink_metadata()
            && meta.file_type().is_symlink()
        {
            bail!("não escrevo através de links simbólicos");
        }
        let pasta = destino
            .parent()
            .context("caminho de nota sem pasta")?
            .to_path_buf();
        std::fs::create_dir_all(&pasta)?;
        // Confere de novo depois de criar as pastas (defesa extra).
        if !pasta.canonicalize()?.starts_with(&self.raiz) {
            bail!("o caminho sai do cofre");
        }
        let nome = destino
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let temporario = pasta.join(format!(".{nome}.abiyss-tmp"));
        {
            let mut arquivo = std::fs::File::create(&temporario)?;
            arquivo.write_all(texto.as_bytes())?;
            arquivo.sync_all()?;
        }
        std::fs::rename(&temporario, destino)?;
        Ok(())
    }

    /// Remove uma nota (usado só por `abiyss memoria esquecer`).
    pub fn apagar(&self, caminho: &str) -> anyhow::Result<String> {
        let (_, caminho) = self.normalizar(caminho)?;
        let arquivo = self.absoluto(&caminho);
        let meta = arquivo
            .symlink_metadata()
            .with_context(|| format!("a nota '{caminho}' não existe"))?;
        if !meta.is_file() {
            bail!("'{caminho}' não é uma nota");
        }
        std::fs::remove_file(&arquivo)?;
        Ok(caminho)
    }
}

/// Resolve um `[[wikilink]]` como o Obsidian: pelo nome do arquivo em
/// qualquer pasta, ou pelo fim do caminho se tiver "/". Havendo mais de
/// uma nota, vence o caminho mais curto.
pub fn resolver_wikilink(alvo: &str, notas: &[String]) -> Option<String> {
    let alvo = alvo.trim().trim_end_matches(".md").to_lowercase();
    notas
        .iter()
        .filter(|nota| {
            let sem_extensao = nota.trim_end_matches(".md").to_lowercase();
            if alvo.contains('/') {
                sem_extensao == alvo || sem_extensao.ends_with(&format!("/{alvo}"))
            } else {
                sem_extensao.rsplit('/').next() == Some(alvo.as_str())
            }
        })
        .min_by_key(|nota| (nota.len(), (*nota).clone()))
        .cloned()
}

fn texto_yaml(texto: &str) -> Value {
    Value::String(texto.to_string())
}

/// Junta os campos de uma proposta aos da nota. Campos do kernel são
/// ignorados; listas são unidas (sem repetir); o resto é substituído.
fn mesclar_campos(campos: &mut Mapping, novos: &Mapping) {
    for (chave, valor) in novos {
        let Some(nome) = chave.as_str() else {
            continue;
        };
        if CAMPOS_DO_KERNEL.contains(&nome) {
            continue;
        }
        match (campos.get_mut(chave), valor) {
            (Some(Value::Sequence(atuais)), Value::Sequence(itens)) => {
                for item in itens {
                    if !atuais.contains(item) {
                        atuais.push(item.clone());
                    }
                }
            }
            _ => {
                campos.insert(chave.clone(), valor.clone());
            }
        }
    }
}

/// `fonte` vira lista quando a nota recebe conteúdo de mais de uma fonte
/// (ex.: importada do Hermes e depois completada numa conversa).
fn acrescentar_fonte(campos: &mut Mapping, fonte: Fonte) {
    let nova = texto_yaml(fonte.como_texto());
    let chave = texto_yaml("fonte");
    match campos.get_mut(&chave) {
        Some(Value::Sequence(lista)) => {
            if !lista.contains(&nova) {
                lista.push(nova);
            }
        }
        Some(atual) if *atual != nova => {
            let antiga = atual.clone();
            *atual = Value::Sequence(vec![antiga, nova]);
        }
        Some(_) => {}
        None => {
            campos.insert(chave, nova);
        }
    }
}

/// Regras do mapa de fontes externas.
fn validar_externa(campos: &Mapping, corpo: &str, notas: &[String]) -> anyhow::Result<()> {
    // Links canônicos: lista de URLs ou mapa nome → URL (site, changelog, docs...).
    let links: Vec<String> = match campos.get("links") {
        Some(Value::Mapping(mapa)) => mapa
            .values()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .collect(),
        _ => frontmatter::lista_do_campo(campos, "links"),
    };
    if links.is_empty() {
        bail!("falta 'links' no frontmatter (links canônicos: site oficial, changelog, docs)");
    }
    if let Some(ruim) = links
        .iter()
        .find(|l| !l.starts_with("https://") && !l.starts_with("http://"))
    {
        bail!("link que não é URL http(s): '{ruim}'");
    }

    let navegador = frontmatter::texto_do_campo(campos, "navegador")
        .map(|n| sem_acentos(&n.to_lowercase()))
        .unwrap_or_default();
    if !NIVEIS_NAVEGADOR.contains(&navegador.as_str()) {
        bail!(
            "'navegador' precisa ser um destes: {} (veio '{navegador}')",
            NIVEIS_NAVEGADOR.join(", ")
        );
    }

    let revalidar = frontmatter::texto_do_campo(campos, "revalidar_apos").unwrap_or_default();
    if achar_data(&revalidar).is_none() {
        bail!("'revalidar_apos' precisa ser uma data AAAA-MM-DD (veio '{revalidar}')");
    }

    // Resumos em cache sempre carregam a data.
    if campos.contains_key("resumo")
        && frontmatter::texto_do_campo(campos, "resumo_em")
            .and_then(|d| achar_data(&d))
            .is_none()
    {
        bail!("o campo 'resumo' precisa vir com 'resumo_em: AAAA-MM-DD'");
    }
    for linha in corpo.lines() {
        let titulo = linha.trim_start();
        if titulo.starts_with('#')
            && sem_acentos(&titulo.to_lowercase()).contains("resumo")
            && achar_data(titulo).is_none()
        {
            bail!(
                "resumo em cache sem data: '{}' (use, por exemplo, '## Resumo em cache ({})')",
                titulo.trim(),
                hoje()
            );
        }
    }

    // O mapa externo não aponta para notas internas.
    for alvo in extrair_wikilinks(corpo) {
        let interna = alvo.to_lowercase().starts_with("01_internal")
            || resolver_wikilink(&alvo, notas)
                .is_some_and(|n| Escopo::do_caminho(&n) == Some(Escopo::Interno));
        if interna {
            bail!("notas externas não podem apontar para notas internas ([[{alvo}]])");
        }
    }
    Ok(())
}

/// Procura uma data AAAA-MM-DD válida dentro do texto.
pub fn achar_data(texto: &str) -> Option<chrono::NaiveDate> {
    let bytes = texto.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    (0..=bytes.len() - 10).find_map(|i| {
        let pedaco = texto.get(i..i + 10)?;
        // Formato exato AAAA-MM-DD (o chrono sozinho aceita anos curtos).
        let formato_ok = pedaco.char_indices().all(|(j, c)| {
            if j == 4 || j == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        });
        if !formato_ok {
            return None;
        }
        chrono::NaiveDate::parse_from_str(pedaco, "%Y-%m-%d").ok()
    })
}

/// Tira os acentos mais comuns do português (para comparar palavras).
pub fn sem_acentos(texto: &str) -> String {
    texto.chars().map(sem_acento).collect()
}

/// Versão de um caractere sem acento (sempre UM caractere: as posições
/// no texto continuam valendo).
pub fn sem_acento(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'A',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'É' | 'È' | 'Ê' | 'Ë' => 'E',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
        'ç' => 'c',
        'Ç' => 'C',
        outro => outro,
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn procedencia(tipo: Tipo, externa: bool) -> Procedencia {
        Procedencia {
            fonte: Fonte::Conversa,
            tipo,
            origem_externa: externa.then(|| "mcp:x".to_string()),
            criado: None,
            rotulo: "teste".into(),
        }
    }

    fn cofre() -> (tempfile::TempDir, Cofre) {
        let pasta = tempfile::tempdir().unwrap();
        let cofre = Cofre::abrir(&pasta.path().join("cofre")).unwrap();
        (pasta, cofre)
    }

    #[test]
    fn normaliza_e_recusa_caminhos_ruins() {
        let (_p, cofre) = cofre();
        assert_eq!(
            cofre.normalizar("./01_internal/pessoas/ana").unwrap().1,
            "01_internal/pessoas/ana.md"
        );
        for ruim in [
            "",
            "pessoas/ana.md",
            "01_internal",
            "01_internal/../x.md",
            "01_internal/.obsidian/x.md",
            "/01_internal/x.md",
            "01_internal//x.md",
            "01_internal/x.txt",
        ] {
            assert!(cofre.normalizar(ruim).is_err(), "deveria recusar '{ruim}'");
        }
        assert_eq!(
            cofre
                .caminho_no_escopo(Escopo::Externo, "ferramentas/rust")
                .unwrap(),
            "02_external/ferramentas/rust.md"
        );
        assert!(
            cofre
                .caminho_no_escopo(Escopo::Interno, "02_external/x.md")
                .is_err()
        );
    }

    #[test]
    fn regra_dura_no_unico_caminho_de_escrita() {
        let (_p, cofre) = cofre();
        let doc = Documento::sem_campos("o site diz X");
        let erro = cofre
            .gravar("01_internal/x.md", &doc, &procedencia(Tipo::Dito, true))
            .unwrap_err();
        assert!(erro.to_string().contains(REGRA_DURA));
        assert!(!cofre.existe("01_internal/x.md"));
    }

    #[test]
    fn cria_com_procedencia_e_acrescenta_sem_misturar_tipos() {
        let (_p, cofre) = cofre();
        let mut campos = Mapping::new();
        campos.insert(texto_yaml("fonte"), texto_yaml("web")); // ignorado
        campos.insert(
            texto_yaml("aliases"),
            Value::Sequence(vec![texto_yaml("Ana")]),
        );
        let doc = Documento {
            campos,
            corpo: "Gosta de chá.".into(),
            tem_frontmatter: true,
        };
        let p = procedencia(Tipo::Dito, false);
        assert_eq!(
            cofre
                .gravar("01_internal/pessoas/ana.md", &doc, &p)
                .unwrap(),
            Gravacao::Criada
        );
        let nota = cofre.ler("01_internal/pessoas/ana.md", 10_000).unwrap();
        assert_eq!(nota.doc.texto("fonte").as_deref(), Some("conversa"));
        assert_eq!(nota.doc.texto("tipo").as_deref(), Some("dito"));
        assert!(nota.doc.texto("criado").is_some());
        assert!(nota.doc.texto("atualizado").is_some());

        let mais = Documento::sem_campos("Mora em Recife.");
        let mut importada = p.clone();
        importada.fonte = Fonte::Importacao;
        assert_eq!(
            cofre
                .gravar("01_internal/pessoas/ana.md", &mais, &importada)
                .unwrap(),
            Gravacao::Atualizada
        );
        let nota = cofre.ler("01_internal/pessoas/ana.md", 10_000).unwrap();
        assert!(nota.doc.corpo.contains("Gosta de chá."));
        assert!(nota.doc.corpo.contains("Mora em Recife."));
        assert_eq!(
            frontmatter::lista_do_campo(&nota.doc.campos, "fonte"),
            vec!["conversa", "importacao"]
        );

        let deduzido = procedencia(Tipo::Deduzido, false);
        let erro = cofre
            .gravar("01_internal/pessoas/ana.md", &mais, &deduzido)
            .unwrap_err();
        assert!(erro.to_string().contains("nunca se misturam"));
    }

    #[test]
    fn regras_das_notas_externas() {
        let (_p, cofre) = cofre();
        let p = procedencia(Tipo::Dito, true); // externo aceita origem externa
        let com = |yaml: &str, corpo: &str| Documento {
            campos: frontmatter::separar(&format!("---\n{yaml}\n---\n"))
                .unwrap()
                .campos,
            corpo: corpo.into(),
            tem_frontmatter: true,
        };
        let valido =
            "links: [https://www.rust-lang.org]\nnavegador: rapido\nrevalidar_apos: 2026-11-05";
        assert!(
            cofre
                .gravar("02_external/a.md", &com("navegador: rapido", ""), &p)
                .is_err()
        );
        assert!(
            cofre
                .gravar(
                    "02_external/a.md",
                    &com(&valido.replace("rapido", "turbo"), ""),
                    &p
                )
                .is_err()
        );
        assert!(
            cofre
                .gravar(
                    "02_external/a.md",
                    &com(valido, "## Resumo em cache\nx"),
                    &p
                )
                .is_err()
        );
        cofre
            .gravar(
                "01_internal/eu.md",
                &Documento::sem_campos("x"),
                &procedencia(Tipo::Dito, false),
            )
            .unwrap();
        assert!(
            cofre
                .gravar("02_external/a.md", &com(valido, "ver [[eu]]"), &p)
                .is_err()
        );
        cofre
            .gravar(
                "02_external/a.md",
                &com(valido, "## Resumo em cache (2026-10-05)\nRust 1.9x."),
                &p,
            )
            .unwrap();
        assert!(cofre.existe("02_external/a.md"));
    }

    #[test]
    fn wikilinks_resolvem_como_no_obsidian() {
        let notas = vec![
            "01_internal/pessoas/ana.md".to_string(),
            "02_external/ferramentas/rust.md".to_string(),
            "02_external/arquivo/velho/rust.md".to_string(),
        ];
        assert_eq!(
            resolver_wikilink("rust", &notas).as_deref(),
            Some("02_external/ferramentas/rust.md")
        );
        assert_eq!(
            resolver_wikilink("velho/rust", &notas).as_deref(),
            Some("02_external/arquivo/velho/rust.md")
        );
        assert_eq!(
            resolver_wikilink("Ana", &notas).as_deref(),
            Some("01_internal/pessoas/ana.md")
        );
        assert_eq!(resolver_wikilink("nada", &notas), None);
        assert_eq!(
            achar_data("## Resumo (2026-10-05)").unwrap().to_string(),
            "2026-10-05"
        );
        assert!(achar_data("2026-13-40").is_none());
    }
}
