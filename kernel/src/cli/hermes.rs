//! `abiyss importar-hermes`: traz a memória do Hermes (simulação por padrão).

use std::path::Path;

use abiyss::config::Config;

pub fn importar(config: &Config, origem: &Path, aplicar: bool) -> anyhow::Result<()> {
    let relatorio = abiyss::hermes::importar(config, origem, aplicar)?;
    print!("{relatorio}");
    Ok(())
}
