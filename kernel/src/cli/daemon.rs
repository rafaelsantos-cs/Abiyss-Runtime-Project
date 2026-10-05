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
use abiyss::{manutencao, tempo};

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
    // Supervisão dos servidores MCP: reinicia quem cai, trava ou passa dos limites.
    let supervisao = tokio::spawn(mcp.clone().supervisionar());
    let daemon = Daemon::novo(config, banco, orquestrador, ferramentas);
    let resultado = daemon.rodar(&opcoes).await;
    drop(daemon);
    supervisao.abort();
    let _ = supervisao.await;
    if let Ok(ponte) = Arc::try_unwrap(mcp) {
        ponte.encerrar().await;
    }
    resultado
}

pub fn status(config: &Config, verificar: bool) -> anyhow::Result<()> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    if verificar {
        let v = status::verificar(config, &banco)?;
        println!("{}", v.linha);
        drop(banco);
        std::process::exit(v.codigo);
    }
    print!("{}", status::relatorio(config, &banco)?);
    Ok(())
}

/// `abiyss manutencao`: uma rodada de manutenção do banco, na hora.
pub fn manutencao(config: &Config) -> anyhow::Result<()> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    let resumo = manutencao::rodada_completa(&banco, &config.retencao, tempo::agora_ms())?;
    println!("Retenção e vacuum: {resumo}");
    let checkpoint = manutencao::checkpoint_wal(&banco)?;
    println!(
        "Checkpoint do WAL: {} página(s) copiada(s){}",
        checkpoint.paginas_copiadas,
        if checkpoint.ocupado {
            " (parcial: outro processo usando o banco)"
        } else {
            ""
        }
    );
    Ok(())
}
