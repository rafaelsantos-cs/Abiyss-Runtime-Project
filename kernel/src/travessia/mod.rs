//! Travessia: o harness do benchmark do Abiyss (`abiyss travessia`).
//!
//! O jogo é o servidor MCP `recursos/mcp/jogo` (a única fonte da verdade); o
//! catálogo de fases sai do gerador de lá (`gerador.py`). Este módulo:
//!
//! 1. garante o catálogo da seed (gera se faltar) e lê o gabarito (o ótimo
//!    de cada fase), que NUNCA chega ao Abiyss: dele só sai o limite de
//!    jogadas (8 × ótimo + 20), escrito no arquivo da execução que só o
//!    servidor do jogo lê;
//! 2. monta uma instância isolada do Abiyss (banco, workspace e cofre
//!    próprios; `instancia`), com o servidor do jogo e as ferramentas de
//!    cada nível de sub-agente;
//! 3. deixa o cérebro jogar (`sessao`), com os sub-agentes rodando no mesmo
//!    processo, em paralelo, pelo pool deles;
//! 4. junta as métricas por fase e a linha do tempo (`metricas`), e o
//!    relatório de várias execuções (`relatorio`).
//!
//! Braços (sempre com as mesmas seeds): A completo, B sem delegação, C com
//! a memória apagada entre fases; `--relogio` (T) liga o orçamento de tempo
//! por fase em qualquer um deles.

pub mod instancia;
pub mod metricas;
pub mod mock;
pub mod relatorio;
pub mod sessao;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::config::Config;
use crate::db::Banco;
use crate::ferramentas::CaixaDeFerramentas;
use crate::mcp::PonteMcp;
use crate::orquestrador::Orquestrador;
use crate::subagentes::ExecutorSubagentes;
use crate::tempo::agora_ms;

/// Seção `[travessia]` do abiyss.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConfigTravessia {
    /// Pasta de tudo do benchmark: catálogo, gabarito e execuções. Fica
    /// FORA do workspace e de qualquer pasta montada na caixa do terminal
    /// (o kernel recusa um workspace que contenha a pasta de dados).
    pub pasta: String,
    /// Projeto do jogo (servidor MCP e gerador).
    pub jogo: String,
    /// Seeds padrão da campanha (`rodar` sem `--seed` usa todas).
    pub seeds: Vec<String>,
    /// Execuções por seed (`rodar` sem `--execucoes`).
    pub execucoes_por_seed: u32,
    /// Fases jogadas, em ordem.
    pub fases: Vec<u32>,
    /// Reinícios permitidos por fase (mais que isto = falha).
    pub max_reinicios: u32,
    /// Orçamento de tempo por fase no braço com relógio (T).
    pub relogio_minutos: u64,
    /// Ferramentas do jogo que os sub-agentes recebem (além das deles).
    /// Quem pode chamar cada ferramenta é decidido aqui, não pelo jogo.
    pub ferramentas_jogo_subagentes: Vec<String>,
    /// Mensagens do histórico no contexto de cada chamada do cérebro.
    pub historico_max_mensagens: usize,
    /// Trava contra laço: chamadas do cérebro numa fase antes de o harness
    /// encerrar a fase (desistência registrada como "trava do harness").
    pub max_chamadas_por_fase: u32,
    /// Trava contra laço: tempo máximo de uma execução inteira.
    pub max_minutos_por_execucao: u64,
    /// Respostas seguidas sem ferramenta (o cérebro "parou") antes de o
    /// harness desistir da execução.
    pub max_cutucadas: u32,
    /// Teto da ferramenta `aguardar` (segundos esperando sub-agentes).
    pub aguardar_max_segundos: u64,
    /// Orçamento do solucionador na geração (testes de aceite).
    pub gerador_max_estados: u64,
    pub gerador_max_segundos: u64,
}

impl Default for ConfigTravessia {
    fn default() -> Self {
        ConfigTravessia {
            pasta: "data/travessia".into(),
            jogo: "recursos/mcp/jogo".into(),
            seeds: (1..=5).map(|n| format!("travessia-{n}")).collect(),
            execucoes_por_seed: 5,
            fases: (1..=12).collect(),
            max_reinicios: 5,
            relogio_minutos: 10,
            ferramentas_jogo_subagentes: vec![
                "jogo__regras".into(),
                "jogo__ver".into(),
                "jogo__simular".into(),
            ],
            historico_max_mensagens: 80,
            max_chamadas_por_fase: 300,
            max_minutos_por_execucao: 360,
            max_cutucadas: 3,
            aguardar_max_segundos: 300,
            gerador_max_estados: 5_000_000,
            gerador_max_segundos: 60,
        }
    }
}

impl ConfigTravessia {
    pub fn validar(&self) -> anyhow::Result<()> {
        if self.fases.is_empty() || self.fases.iter().any(|n| !(1..=12).contains(n)) {
            bail!("travessia.fases precisa ter fases de 1 a 12");
        }
        if self.execucoes_por_seed == 0 || self.max_chamadas_por_fase == 0 {
            bail!("travessia.execucoes_por_seed e max_chamadas_por_fase precisam ser > 0");
        }
        if self.historico_max_mensagens < 4 {
            bail!("travessia.historico_max_mensagens precisa ser pelo menos 4");
        }
        Ok(())
    }
}

/// Braço do teste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Braco {
    /// Completo (a linha de base).
    A,
    /// Sem delegação.
    B,
    /// Memória apagada entre fases (conversa nova e workspace limpo).
    C,
}

impl Braco {
    pub fn de_texto(texto: &str) -> Option<Braco> {
        match texto.trim().to_uppercase().as_str() {
            "A" => Some(Braco::A),
            "B" => Some(Braco::B),
            "C" => Some(Braco::C),
            _ => None,
        }
    }

    pub fn letra(&self) -> &'static str {
        match self {
            Braco::A => "A",
            Braco::B => "B",
            Braco::C => "C",
        }
    }

    pub fn delega(&self) -> bool {
        !matches!(self, Braco::B)
    }

    pub fn apaga_memoria(&self) -> bool {
        matches!(self, Braco::C)
    }
}

/// Modo de jogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modo {
    /// Uma falha leva à fase seguinte: toda execução mede todas as fases.
    Avaliacao,
    /// A primeira falha encerra a execução; a nota é até onde chegou.
    Campanha,
}

impl Modo {
    pub fn de_texto(texto: &str) -> Option<Modo> {
        match texto.trim().to_lowercase().as_str() {
            "avaliacao" | "avaliação" => Some(Modo::Avaliacao),
            "campanha" => Some(Modo::Campanha),
            _ => None,
        }
    }

    pub fn como_texto(&self) -> &'static str {
        match self {
            Modo::Avaliacao => "avaliacao",
            Modo::Campanha => "campanha",
        }
    }
}

/// O que define uma execução.
#[derive(Debug, Clone)]
pub struct OpcoesExecucao {
    pub braco: Braco,
    /// Braço com relógio (T): orçamento de tempo por fase.
    pub relogio: bool,
    pub modo: Modo,
    pub seed: String,
    /// Número da execução para esta seed e braço (1, 2, ...).
    pub numero: u32,
    /// Chaves explícitas (testes); `None` = as variáveis da config.
    pub chaves: Option<(String, String)>,
}

impl OpcoesExecucao {
    /// "A", "B+T"...
    pub fn rotulo_braco(&self) -> String {
        if self.relogio {
            format!("{}+T", self.braco.letra())
        } else {
            self.braco.letra().to_string()
        }
    }
}

/// Pastas do benchmark.
#[derive(Debug, Clone)]
pub struct Pastas {
    pub raiz: PathBuf,
    pub jogo: PathBuf,
}

impl Pastas {
    pub fn da_config(config: &Config) -> Pastas {
        Pastas {
            raiz: config.resolver(&config.travessia.pasta),
            jogo: config.resolver(&config.travessia.jogo),
        }
    }

    pub fn catalogo(&self, seed: &str) -> PathBuf {
        self.raiz.join("catalogo").join(nome_seguro(seed))
    }

    pub fn gabarito(&self, seed: &str) -> PathBuf {
        self.raiz.join("gabarito").join(nome_seguro(seed))
    }

    pub fn execucoes(&self) -> PathBuf {
        self.raiz.join("execucoes")
    }
}

/// Seed como nome de pasta: letras, números, `-` e `_`.
pub fn nome_seguro(texto: &str) -> String {
    let nome: String = texto
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if nome.is_empty() { "_".into() } else { nome }
}

/// Nome do arquivo de uma fase no catálogo e no gabarito.
pub fn arquivo_fase(numero: u32) -> String {
    format!("fase-{numero:02}.json")
}

/// O que o harness lê do gabarito de uma fase.
#[derive(Debug, Clone, PartialEq)]
pub struct Gabarito {
    pub numero: u32,
    pub otimo: u32,
}

impl Gabarito {
    /// Limite de jogadas da fase: passou de 8 × ótimo + 20, falhou.
    pub fn max_jogadas(&self) -> u32 {
        8 * self.otimo + 20
    }
}

pub fn ler_gabarito(pastas: &Pastas, seed: &str, numero: u32) -> anyhow::Result<Gabarito> {
    let caminho = pastas.gabarito(seed).join(arquivo_fase(numero));
    let texto = std::fs::read_to_string(&caminho)
        .with_context(|| format!("não consegui ler o gabarito {}", caminho.display()))?;
    let dados: Value = serde_json::from_str(&texto)
        .with_context(|| format!("gabarito inválido: {}", caminho.display()))?;
    let otimo = dados["otimo"]
        .as_u64()
        .with_context(|| format!("gabarito sem 'otimo': {}", caminho.display()))?;
    Ok(Gabarito {
        numero,
        otimo: otimo as u32,
    })
}

/// O catálogo e o gabarito desta seed têm todas as fases?
pub fn catalogo_completo(pastas: &Pastas, seed: &str, fases: &[u32]) -> bool {
    fases.iter().all(|n| {
        pastas.catalogo(seed).join(arquivo_fase(*n)).is_file()
            && pastas.gabarito(seed).join(arquivo_fase(*n)).is_file()
    })
}

/// Gera o catálogo de uma seed com o gerador do jogo (`uv run gerador.py`).
/// Devolve o que o gerador imprimiu (o sha256 de cada fase).
pub async fn gerar_catalogo(config: &Config, seed: &str, fases: &[u32]) -> anyhow::Result<String> {
    let pastas = Pastas::da_config(config);
    let t = &config.travessia;
    let lista = fases
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let saida = tokio::process::Command::new("uv")
        .args([
            "run",
            "--frozen",
            "--quiet",
            "--no-dev",
            "gerador.py",
            "--seed",
            seed,
        ])
        .arg("--catalogo")
        .arg(pastas.catalogo(seed))
        .arg("--gabarito")
        .arg(pastas.gabarito(seed))
        .args(["--fases", &lista])
        .args(["--max-estados", &t.gerador_max_estados.to_string()])
        .args(["--max-segundos", &t.gerador_max_segundos.to_string()])
        .current_dir(&pastas.jogo)
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| format!("não consegui rodar o gerador em {}", pastas.jogo.display()))?;
    if !saida.status.success() {
        bail!(
            "o gerador falhou ({}): {}",
            saida.status,
            String::from_utf8_lossy(&saida.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&saida.stdout).into_owned())
}

/// Resumo de uma execução terminada.
#[derive(Debug, Clone)]
pub struct ResumoExecucao {
    pub id: String,
    pub pasta: PathBuf,
    pub fases: Vec<metricas::MetricaFase>,
    pub motivo_fim: String,
}

/// Linha do tempo compartilhada entre a sessão e o observador dos
/// sub-agentes (eventos de ferramenta; o resto vem do banco no fim).
pub type LinhaDoTempo = Arc<Mutex<Vec<Value>>>;

pub fn registrar(linha: &LinhaDoTempo, evento: Value) {
    if let Ok(mut lista) = linha.lock() {
        lista.push(evento);
    }
}

/// Uma execução inteira: catálogo → instância → jogo → métricas.
pub async fn rodar_execucao(
    base: &Config,
    base_toml: &Path,
    opcoes: &OpcoesExecucao,
) -> anyhow::Result<ResumoExecucao> {
    let pastas = Pastas::da_config(base);
    let t = &base.travessia;
    if !catalogo_completo(&pastas, &opcoes.seed, &t.fases) {
        tracing::info!("gerando o catálogo da seed {}", opcoes.seed);
        gerar_catalogo(base, &opcoes.seed, &t.fases).await?;
    }
    let mut gabaritos = Vec::new();
    for n in &t.fases {
        gabaritos.push(ler_gabarito(&pastas, &opcoes.seed, *n)?);
    }

    let instancia = instancia::criar(base, base_toml, &pastas, opcoes, &gabaritos)?;
    let config = instancia.config.clone();
    let banco = Banco::abrir(&config.caminho_banco())?;
    let orquestrador = match &opcoes.chaves {
        Some((cerebro, sub)) => Orquestrador::novo(&config, banco.clone(), cerebro, sub)?,
        None => Orquestrador::da_config(&config, banco.clone())?,
    };
    let mcp = Arc::new(PonteMcp::iniciar(&config).await);
    if !mcp.servidores().iter().any(|s| s == "jogo") {
        bail!("o servidor do jogo não subiu (veja o log acima)");
    }

    let linha: LinhaDoTempo = Arc::new(Mutex::new(Vec::new()));
    let observador: crate::subagentes::ObservadorFerramentas = {
        let linha = linha.clone();
        Arc::new(move |id: i64, nome: &str, erro: bool| {
            registrar(
                &linha,
                json!({"tipo": "ferramenta", "t": agora_ms(), "quem": format!("subagente:{id}"),
                       "nome": nome, "erro": erro}),
            );
        })
    };
    let base_subagentes = Arc::new(CaixaDeFerramentas::da_config(&config)?.com_mcp(mcp.clone()));
    let executor = ExecutorSubagentes::com_observador(
        config.clone(),
        banco.clone(),
        orquestrador.clone(),
        base_subagentes,
        Some(observador),
    );
    let tarefa_executor = tokio::spawn(Arc::clone(&executor).rodar());

    let mut caixa = CaixaDeFerramentas::da_config(&config)?.com_mcp(mcp.clone());
    if opcoes.braco.delega() {
        caixa = caixa.com_subagentes(executor.controle());
    }
    let mut sessao = sessao::SessaoJogo::nova(
        config.clone(),
        banco.clone(),
        orquestrador,
        Arc::new(caixa),
        mcp.clone(),
        opcoes.braco,
        instancia.arquivos.clone(),
        linha.clone(),
    )?;
    let fim = sessao.jogar().await;
    tarefa_executor.abort();
    let _ = tarefa_executor.await;
    drop(sessao);
    drop(executor);
    match Arc::try_unwrap(mcp) {
        Ok(ponte) => ponte.encerrar().await,
        Err(_) => tracing::warn!("ponte MCP ainda em uso; os servidores morrem com o processo"),
    }
    let motivo_fim = match fim {
        Ok(motivo) => motivo,
        Err(e) => format!("erro: {e:#}"),
    };

    let eventos = linha.lock().map(|l| l.clone()).unwrap_or_default();
    let fases = metricas::fechar(&instancia, &banco, &gabaritos, &eventos, &motivo_fim)?;
    Ok(ResumoExecucao {
        id: instancia.id.clone(),
        pasta: instancia.pasta.clone(),
        fases,
        motivo_fim,
    })
}
