//! `abiyss daemon` e `abiyss status`.

use std::sync::Arc;

use abiyss::config::Config;
use abiyss::daemon::{Daemon, OpcoesDaemon, TravaDaemon};
use abiyss::db::Banco;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::mcp::PonteMcp;
use abiyss::orquestrador::Orquestrador;
use abiyss::status;

pub async fn executar(config: Config, opcoes: OpcoesDaemon) -> anyhow::Result<()> {
    // Primeiro a trava: se já houver um daemon, nem abrimos nada.
    let _trava = TravaDaemon::adquirir(&config)?;
    let banco = Banco::abrir(&config.caminho_banco())?;
    let orquestrador = Orquestrador::da_config(&config, banco.clone())?;
    // Ferramentas dos sub-agentes: nativas + MCP (cada nível recebe um recorte).
    let mcp = Arc::new(PonteMcp::iniciar(&config).await);
    let ferramentas = Arc::new(CaixaDeFerramentas::da_config(&config)?.com_mcp(mcp.clone()));
    let daemon = Daemon::novo(config, banco, orquestrador, ferramentas);
    let resultado = daemon.rodar(&opcoes).await;
    drop(daemon);
    if let Ok(ponte) = Arc::try_unwrap(mcp) {
        ponte.encerrar().await;
    }
    resultado
}

pub fn status(config: &Config) -> anyhow::Result<()> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    print!("{}", status::relatorio(config, &banco)?);
    Ok(())
}
