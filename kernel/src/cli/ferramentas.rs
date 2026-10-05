//! `abiyss ferramentas`: lista as ferramentas que o modelo enxerga.
//! `abiyss skills`: lista as skills (nome + descrição) e os problemas.

use std::sync::Arc;

use abiyss::config::Config;
use abiyss::db::Banco;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::mcp::PonteMcp;
use abiyss::memoria::Memoria;
use abiyss::skills::Skills;

pub async fn listar(config: &Config) -> anyhow::Result<()> {
    let mcp = Arc::new(PonteMcp::iniciar(config).await);
    let banco = Banco::abrir(&config.caminho_banco())?;
    let memoria = Arc::new(Memoria::abrir(config, banco)?.com_mcp(mcp.clone()));
    let caixa = CaixaDeFerramentas::da_config(config)?
        .com_mcp(mcp.clone())
        .com_memoria(memoria);
    let servidores = mcp.descricao_servidores();
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

pub fn skills(config: &Config) -> anyhow::Result<()> {
    let skills = Skills::da_config(config);
    println!("Pasta de skills: {}", skills.raiz().display());
    if !skills.existe() {
        println!("(a pasta não existe: nenhuma skill disponível)");
        return Ok(());
    }
    let catalogo = skills.catalogo();
    if catalogo.skills.is_empty() {
        println!("Nenhuma skill válida.");
    }
    for skill in &catalogo.skills {
        let pasta = skill
            .pasta
            .strip_prefix(skills.raiz())
            .unwrap_or(&skill.pasta);
        println!(
            "- {} ({}): {}",
            skill.nome,
            pasta.display(),
            skill.descricao
        );
    }
    if !catalogo.problemas.is_empty() {
        println!("\nSkills ignoradas:");
        for p in &catalogo.problemas {
            println!("  {}: {}", p.caminho.display(), p.motivo);
        }
    }
    Ok(())
}
