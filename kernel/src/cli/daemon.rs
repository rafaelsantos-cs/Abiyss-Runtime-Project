//! `abiyss daemon` e `abiyss status`.

use abiyss::config::Config;
use abiyss::daemon::{Daemon, OpcoesDaemon, TravaDaemon};
use abiyss::db::Banco;
use abiyss::orquestrador::Orquestrador;
use abiyss::status;

pub async fn executar(config: Config, opcoes: OpcoesDaemon) -> anyhow::Result<()> {
    // Primeiro a trava: se já houver um daemon, nem abrimos nada.
    let _trava = TravaDaemon::adquirir(&config)?;
    let banco = Banco::abrir(&config.caminho_banco())?;
    let orquestrador = Orquestrador::da_config(&config, banco.clone())?;
    let daemon = Daemon::novo(config, banco, orquestrador);
    daemon.rodar(&opcoes).await
}

pub fn status(config: &Config) -> anyhow::Result<()> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    print!("{}", status::relatorio(config, &banco)?);
    Ok(())
}
