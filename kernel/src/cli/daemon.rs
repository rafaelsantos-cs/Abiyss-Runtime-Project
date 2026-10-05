//! `abiyss daemon` e `abiyss status`.

use std::sync::Arc;

use abiyss::config::Config;
use abiyss::daemon::{Daemon, OpcoesDaemon, TravaDaemon};
use abiyss::db::Banco;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::mcp::PonteMcp;
use abiyss::memoria::Memoria;
use abiyss::orquestrador::Orquestrador;
use abiyss::status;

pub async fn executar(config: Config, opcoes: OpcoesDaemon) -> anyhow::Result<()> {
    // Primeiro a trava: se já houver um daemon, nem abrimos nada.
    let _trava = TravaDaemon::adquirir(&config)?;
    let banco = Banco::abrir(&config.caminho_banco())?;
    let orquestrador = Orquestrador::da_config(&config, banco.clone())?;
    // Ferramentas dos sub-agentes: nativas + memória + MCP (cada nível recebe
    // um recorte). Para sub-agentes, tudo conta como conteúdo externo.
    let mcp = Arc::new(PonteMcp::iniciar(&config).await);
    // O qmd (se configurado e ativo) é o motor de busca da memória.
    let memoria = Arc::new(Memoria::abrir(&config, banco.clone())?.com_mcp(mcp.clone()));
    let ferramentas = Arc::new(
        CaixaDeFerramentas::da_config(&config)?
            .com_mcp(mcp.clone())
            .com_memoria(memoria),
    );
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
