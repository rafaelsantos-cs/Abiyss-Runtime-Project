//! Ponto de entrada da CLI `abiyss`.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};

use abiyss::config::{self, Config};
use abiyss::nim::mock::MockNim;
use abiyss::nim::{self, ClienteNim, EventoStream, Mensagem};

#[derive(Parser)]
#[command(
    name = "abiyss",
    version,
    about = "Kernel do Abiyss, agente autônomo 24/7"
)]
struct Cli {
    /// Caminho do abiyss.toml (padrão: ./abiyss.toml).
    #[arg(long, global = true, env = "ABIYSS_CONFIG")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    comando: Comando,
}

#[derive(Subcommand)]
enum Comando {
    /// Faz UMA chamada simples ao NIM para conferir chave, URL e ID do modelo.
    TestarNim {
        /// Qual modelo da config usar.
        #[arg(long, value_enum, default_value_t = QualModelo::Cerebro)]
        modelo: QualModelo,
        /// Desliga o streaming (resposta chega inteira no fim).
        #[arg(long)]
        sem_stream: bool,
        /// Mensagem a enviar.
        #[arg(default_value = "Responda apenas: ok")]
        mensagem: String,
    },
    /// Sobe um NIM de mentira em 127.0.0.1 (para testar sem gastar cota).
    MockNim {
        #[arg(long, default_value_t = 8089)]
        porta: u16,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum QualModelo {
    Cerebro,
    Ultra,
    Medium,
    Low,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    iniciar_logs();
    let cli = Cli::parse();

    match cli.comando {
        Comando::MockNim { porta } => rodar_mock(porta).await,
        Comando::TestarNim {
            modelo,
            sem_stream,
            mensagem,
        } => {
            let config = carregar_config(cli.config.as_deref())?;
            testar_nim(&config, modelo, !sem_stream, &mensagem).await
        }
    }
}

/// Logs vão para o stderr. Controle o nível com RUST_LOG (ex.: RUST_LOG=abiyss=debug).
fn iniciar_logs() {
    let filtro = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("abiyss=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filtro)
        .with_writer(std::io::stderr)
        .init();
}

fn carregar_config(explicito: Option<&std::path::Path>) -> anyhow::Result<Config> {
    let caminho = Config::caminho_padrao(explicito);
    Config::carregar(&caminho)
}

async fn rodar_mock(porta: u16) -> anyhow::Result<()> {
    let mock = MockNim::iniciar_em(&format!("127.0.0.1:{porta}"))
        .await
        .with_context(|| format!("não consegui abrir a porta {porta}"))?;
    println!("Mock do NIM ouvindo em {}", mock.base_url());
    println!("Use esse valor em nim.base_url (num abiyss.toml separado) e qualquer chave.");
    println!("Ctrl+C para parar.");
    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn testar_nim(
    config: &Config,
    qual: QualModelo,
    stream: bool,
    mensagem: &str,
) -> anyhow::Result<()> {
    // O cérebro usa a chave do pool do cérebro; os sub-agentes, a do pool deles.
    let (modelo, nome_chave) = match qual {
        QualModelo::Cerebro => (&config.modelos.cerebro, &config.pools.cerebro.api_key_env),
        QualModelo::Ultra => (
            &config.modelos.sub_ultra,
            &config.pools.subagentes.api_key_env,
        ),
        QualModelo::Medium => (
            &config.modelos.sub_medium,
            &config.pools.subagentes.api_key_env,
        ),
        QualModelo::Low => (
            &config.modelos.sub_low,
            &config.pools.subagentes.api_key_env,
        ),
    };
    let chave = config::ler_chave(nome_chave)?;
    let cliente = ClienteNim::novo(
        &config.nim.base_url,
        &chave,
        Duration::from_secs(config.nim.timeout_conexao_segundos),
        Duration::from_secs(config.nim.timeout_leitura_segundos),
    )?;
    let pedido = nim::montar_pedido(modelo, vec![Mensagem::usuario(mensagem)], vec![]);

    eprintln!("→ {} em {}", modelo.id, config.nim.base_url);
    let resposta = if stream {
        let mut mostrar = |evento: EventoStream| match evento {
            EventoStream::Texto(t) => {
                print!("{t}");
                let _ = std::io::stdout().flush();
            }
            EventoStream::Raciocinio(r) => eprint!("\x1b[2m{r}\x1b[0m"),
            EventoStream::InicioFerramenta(_) => {}
        };
        let r = cliente.completar_stream(&pedido, &mut mostrar).await?;
        println!();
        r
    } else {
        let r = cliente.completar(&pedido).await?;
        println!("{}", r.mensagem.texto());
        r
    };
    eprintln!(
        "← fim: {:?} | tokens: {} entrada + {} saída",
        resposta.motivo_fim, resposta.uso.prompt_tokens, resposta.uso.completion_tokens
    );
    Ok(())
}
