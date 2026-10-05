//! `abiyss ferramentas`: lista as ferramentas que o modelo enxerga.

use std::sync::Arc;

use abiyss::config::Config;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::mcp::PonteMcp;

pub async fn listar(config: &Config) -> anyhow::Result<()> {
    let mcp = Arc::new(PonteMcp::iniciar(config).await);
    let caixa = CaixaDeFerramentas::da_config(config)?.com_mcp(mcp.clone());
    let servidores = mcp.servidores();
    println!(
        "Servidores MCP ativos: {}",
        if servidores.is_empty() {
            "(nenhum)".to_string()
        } else {
            servidores.join(", ")
        }
    );
    for ferramenta in caixa.definicoes() {
        println!(
            "- {}: {}",
            ferramenta.function.name, ferramenta.function.description
        );
    }
    drop(caixa);
    if let Ok(ponte) = Arc::try_unwrap(mcp) {
        ponte.encerrar().await;
    }
    Ok(())
}
