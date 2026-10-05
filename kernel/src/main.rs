//! Ponto de entrada da CLI `abiyss`.
//!
//! Aqui ficam só a definição dos comandos (clap) e o despacho; a
//! implementação de cada comando está em `cli/`.

mod cli;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use abiyss::config::Config;
use cli::nim::QualModelo;

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
    /// Conversa com o Abiyss (histórico salvo no SQLite).
    Chat {
        /// Continua a conversa mais recente em vez de começar outra.
        #[arg(long)]
        continuar: bool,
        /// Continua uma conversa específica.
        #[arg(long)]
        conversa: Option<i64>,
        /// Envia só esta mensagem, mostra a resposta e sai.
        #[arg(short, long)]
        mensagem: Option<String>,
        /// Mostra o raciocínio do modelo (em cinza, no stderr).
        #[arg(long)]
        mostrar_raciocinio: bool,
    },
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    iniciar_logs();
    let cli = Cli::parse();
    let caminho_config = Config::caminho_padrao(cli.config.as_deref());

    match cli.comando {
        Comando::MockNim { porta } => cli::nim::rodar_mock(porta).await,
        Comando::TestarNim {
            modelo,
            sem_stream,
            mensagem,
        } => {
            let config = Config::carregar(&caminho_config)?;
            cli::nim::testar_nim(&config, modelo, !sem_stream, &mensagem).await
        }
        Comando::Chat {
            continuar,
            conversa,
            mensagem,
            mostrar_raciocinio,
        } => {
            let config = Config::carregar(&caminho_config)?;
            let opcoes = cli::chat::OpcoesChat {
                continuar,
                conversa,
                mensagem,
                mostrar_raciocinio,
            };
            cli::chat::executar(config, opcoes).await
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
