//! A instância do benchmark: um Abiyss isolado só para uma execução.
//!
//! `<travessia.pasta>/execucoes/<id>/` tem banco, workspace e cofre
//! PRÓPRIOS (o jogo não entra no diário nem na memória do Abiyss de
//! verdade), a memória central vazia e um `abiyss.toml` derivado do
//! principal: mesmos modelos, pools e núcleo de identidade; o servidor MCP
//! do jogo; o terminal (só para o sub-agente medium, como sempre); e as
//! ferramentas do jogo nos níveis de sub-agente.
//!
//! O catálogo, o gabarito e os logs ficam fora do workspace da instância
//! (a única pasta que a caixa do terminal monta). O arquivo da execução
//! (`execucao.json`, com o limite de jogadas de cada fase) também: só o
//! servidor do jogo o lê.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde_json::json;
use toml::{Table, Value as Toml};

use super::{Gabarito, OpcoesExecucao, Pastas, arquivo_fase, nome_seguro};
use crate::config::Config;
use crate::ferramentas::{ESCREVER_ARQUIVO, LER_ARQUIVO, LER_SKILL, LISTAR_ARQUIVOS};

/// Os arquivos de uma execução (todos fora do workspace).
#[derive(Debug, Clone)]
pub struct ArquivosExecucao {
    /// Lido pelo servidor do jogo (JOGO_EXECUCAO).
    pub execucao: PathBuf,
    /// Replay JSONL (gravado pelo servidor do jogo).
    pub replay: PathBuf,
    /// Métricas do jogo por fase (gravadas pelo servidor do jogo).
    pub jogo: PathBuf,
    /// Linha do tempo do harness (delegações, chamadas ao modelo, ferramentas).
    pub linha_do_tempo: PathBuf,
    /// Uma linha de métricas por fase (o que o relatório lê).
    pub metricas: PathBuf,
    /// Resumo em texto.
    pub resumo: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Instancia {
    pub id: String,
    pub pasta: PathBuf,
    pub config: Config,
    pub arquivos: ArquivosExecucao,
    pub opcoes: OpcoesExecucao,
    pub fases: Vec<u32>,
}

/// Ferramentas nativas que os sub-agentes mantêm na instância.
const NATIVAS: &[&str] = &[LER_ARQUIVO, LISTAR_ARQUIVOS, ESCREVER_ARQUIVO, LER_SKILL];

fn texto_caminho(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Entra (criando) na subtabela `nome`.
fn tabela<'a>(raiz: &'a mut Table, nome: &str) -> &'a mut Table {
    let item = raiz
        .entry(nome.to_string())
        .or_insert_with(|| Toml::Table(Table::new()));
    if !item.is_table() {
        *item = Toml::Table(Table::new());
    }
    item.as_table_mut().expect("acabou de virar tabela")
}

pub fn criar(
    base: &Config,
    base_toml: &Path,
    pastas: &Pastas,
    opcoes: &OpcoesExecucao,
    gabaritos: &[Gabarito],
) -> anyhow::Result<Instancia> {
    let agora = chrono::Local::now();
    let id = format!(
        "{}-{}-{:02}-{}",
        opcoes.rotulo_braco().replace('+', "-"),
        nome_seguro(&opcoes.seed),
        opcoes.numero,
        agora.format("%Y%m%d-%H%M%S-%3f")
    );
    let pasta = pastas.execucoes().join(&id);
    for sub in ["dados", "workspace", "cofre", "identity"] {
        std::fs::create_dir_all(pasta.join(sub))
            .with_context(|| format!("não consegui criar {}", pasta.join(sub).display()))?;
    }
    let pasta = pasta
        .canonicalize()
        .with_context(|| format!("pasta inválida: {}", pasta.display()))?;
    std::fs::write(pasta.join("identity/memoria-central.md"), "")?;

    let arquivos = ArquivosExecucao {
        execucao: pasta.join("execucao.json"),
        replay: pasta.join("replay.jsonl"),
        jogo: pasta.join("jogo.jsonl"),
        linha_do_tempo: pasta.join("linha-do-tempo.jsonl"),
        metricas: pasta.join("metricas.jsonl"),
        resumo: pasta.join("resumo.txt"),
    };
    let fases: Vec<u32> = gabaritos.iter().map(|g| g.numero).collect();
    let catalogo = pastas.catalogo(&opcoes.seed);
    for n in &fases {
        if !catalogo.join(arquivo_fase(*n)).is_file() {
            bail!("a fase {n} não está no catálogo {}", catalogo.display());
        }
    }
    let t = &base.travessia;
    let execucao = json!({
        "formato": 1,
        "id": id,
        "braco": opcoes.rotulo_braco(),
        "modo": opcoes.modo.como_texto(),
        "catalogo": texto_caminho(&catalogo.canonicalize()?),
        "fases": fases,
        "max_jogadas": gabaritos
            .iter()
            .map(|g| (g.numero.to_string(), json!(g.max_jogadas())))
            .collect::<serde_json::Map<_, _>>(),
        "max_reinicios": t.max_reinicios,
        "relogio_segundos": if opcoes.relogio { Some(t.relogio_minutos * 60) } else { None },
        "replay": texto_caminho(&arquivos.replay),
        "metricas": texto_caminho(&arquivos.jogo),
    });
    std::fs::write(&arquivos.execucao, serde_json::to_string_pretty(&execucao)?)?;

    let caminho_toml = pasta.join("abiyss.toml");
    let texto = derivar_toml(base, base_toml, &pasta, pastas, &arquivos, &caminho_toml)?;
    std::fs::write(&caminho_toml, texto)?;
    let config = Config::carregar(&caminho_toml).with_context(|| {
        format!(
            "o abiyss.toml da instância é inválido ({})",
            caminho_toml.display()
        )
    })?;
    Ok(Instancia {
        id,
        pasta,
        config,
        arquivos,
        opcoes: opcoes.clone(),
        fases,
    })
}

/// O `abiyss.toml` da instância: o principal com caminhos próprios, sem
/// gateway, sono, backup nem qmd, com o servidor do jogo.
fn derivar_toml(
    base: &Config,
    base_toml: &Path,
    pasta: &Path,
    pastas: &Pastas,
    arquivos: &ArquivosExecucao,
    caminho_toml: &Path,
) -> anyhow::Result<String> {
    let texto = std::fs::read_to_string(base_toml)
        .with_context(|| format!("não consegui ler {}", base_toml.display()))?;
    let mut raiz: Table = toml::from_str(&texto)
        .with_context(|| format!("TOML inválido: {}", base_toml.display()))?;

    let caminhos = tabela(&mut raiz, "caminhos");
    caminhos.insert(
        "dados".into(),
        Toml::String(texto_caminho(&pasta.join("dados"))),
    );
    caminhos.insert(
        "workspace".into(),
        Toml::String(texto_caminho(&pasta.join("workspace"))),
    );
    caminhos.insert(
        "identidade".into(),
        Toml::String(texto_caminho(&base.caminho_identidade())),
    );
    caminhos.insert(
        "skills".into(),
        Toml::String(texto_caminho(&base.caminho_skills())),
    );

    // As mesmas pastas de skills (só leitura), com a confiança já decidida.
    let raizes: Vec<Toml> = base
        .raizes_skills()
        .into_iter()
        .map(|r| {
            let mut t = Table::new();
            t.insert("caminho".into(), Toml::String(texto_caminho(&r.caminho)));
            t.insert("confiavel".into(), Toml::Boolean(r.confiavel));
            Toml::Table(t)
        })
        .collect();
    tabela(&mut raiz, "skills").insert("raizes".into(), Toml::Array(raizes));

    let memoria = tabela(&mut raiz, "memoria");
    memoria.insert(
        "cofre".into(),
        Toml::String(texto_caminho(&pasta.join("cofre"))),
    );
    memoria.insert(
        "central".into(),
        Toml::String(texto_caminho(&pasta.join("identity/memoria-central.md"))),
    );
    tabela(memoria, "qmd").insert("servidor".into(), Toml::String(String::new()));

    for secao in ["gateway", "sono", "backup"] {
        tabela(&mut raiz, secao).insert("ativo".into(), Toml::Boolean(false));
    }
    let orcamento = tabela(&mut raiz, "orcamento");
    orcamento.insert("chamadas_autonomas_por_dia".into(), Toml::Integer(0));
    orcamento.insert("tokens_autonomos_por_dia".into(), Toml::Integer(0));

    let travessia = tabela(&mut raiz, "travessia");
    travessia.insert("pasta".into(), Toml::String(texto_caminho(&pastas.raiz)));
    travessia.insert("jogo".into(), Toml::String(texto_caminho(&pastas.jogo)));

    // Servidores MCP: o jogo e, se o principal tem, o terminal.
    let mut servidores = Vec::new();
    let mut jogo = Table::new();
    jogo.insert("nome".into(), Toml::String("jogo".into()));
    jogo.insert("comando".into(), Toml::String("uv".into()));
    jogo.insert(
        "args".into(),
        Toml::Array(
            ["run", "--frozen", "--quiet", "--no-dev", "servidor.py"]
                .iter()
                .map(|a| Toml::String(a.to_string()))
                .collect(),
        ),
    );
    jogo.insert(
        "diretorio".into(),
        Toml::String(texto_caminho(&pastas.jogo)),
    );
    jogo.insert("timeout_segundos".into(), Toml::Integer(60));
    let mut env = Table::new();
    env.insert(
        "JOGO_EXECUCAO".into(),
        Toml::String(texto_caminho(&arquivos.execucao)),
    );
    jogo.insert("env".into(), Toml::Table(env));
    servidores.push(Toml::Table(jogo));
    if let Some(item) = base
        .mcp
        .servidores
        .iter()
        .find(|s| s.nome == "terminal" && s.ativo && s.url.is_none())
    {
        let mut terminal = Table::new();
        terminal.insert("nome".into(), Toml::String("terminal".into()));
        terminal.insert("comando".into(), Toml::String(item.comando.clone()));
        terminal.insert(
            "args".into(),
            Toml::Array(item.args.iter().cloned().map(Toml::String).collect()),
        );
        if let Some(d) = &item.diretorio {
            terminal.insert(
                "diretorio".into(),
                Toml::String(texto_caminho(&base.resolver(d))),
            );
        }
        terminal.insert(
            "timeout_segundos".into(),
            Toml::Integer(item.timeout_segundos as i64),
        );
        let mut env: Table = item
            .env
            .iter()
            .map(|(k, v)| (k.clone(), Toml::String(v.clone())))
            .collect();
        // O terminal lê os limites e o workspace do abiyss.toml DA INSTÂNCIA.
        env.insert(
            "ABIYSS_CONFIG".into(),
            Toml::String(texto_caminho(caminho_toml)),
        );
        env.insert("ABIYSS_MCP_NOME".into(), Toml::String("terminal".into()));
        terminal.insert("env".into(), Toml::Table(env));
        servidores.push(Toml::Table(terminal));
    }
    tabela(&mut raiz, "mcp").insert("servidores".into(), Toml::Array(servidores));

    // Ferramentas dos sub-agentes: as nativas e o terminal que o nível já
    // tinha (web e o resto não sobem na instância), mais as do jogo.
    let subagentes = tabela(&mut raiz, "subagentes");
    for (nivel, cfg) in [
        ("ultra", &base.subagentes.ultra),
        ("medium", &base.subagentes.medium),
        ("low", &base.subagentes.low),
    ] {
        let mut lista: Vec<Toml> = cfg
            .ferramentas
            .iter()
            .filter(|f| NATIVAS.contains(&f.as_str()) || f.starts_with("terminal__"))
            .cloned()
            .map(Toml::String)
            .collect();
        for f in &base.travessia.ferramentas_jogo_subagentes {
            lista.push(Toml::String(f.clone()));
        }
        tabela(subagentes, nivel).insert("ferramentas".into(), Toml::Array(lista));
    }
    Ok(toml::to_string(&raiz)?)
}
