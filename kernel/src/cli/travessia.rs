//! `abiyss travessia`: o benchmark do jogo Travessia.

use std::path::Path;

use anyhow::Context;

use abiyss::config::Config;
use abiyss::travessia::{self, Braco, Modo, OpcoesExecucao, Pastas};

fn seeds(config: &Config, pedidas: Vec<String>) -> Vec<String> {
    if pedidas.is_empty() {
        config.travessia.seeds.clone()
    } else {
        pedidas
    }
}

pub async fn gerar(config: &Config, pedidas: Vec<String>) -> anyhow::Result<()> {
    for seed in seeds(config, pedidas) {
        eprintln!("gerando o catálogo da seed {seed}...");
        let saida = travessia::gerar_catalogo(config, &seed, &config.travessia.fases).await?;
        let pastas = Pastas::da_config(config);
        println!("seed {seed}: {}", pastas.catalogo(&seed).display());
        print!("{saida}");
    }
    Ok(())
}

pub async fn rodar(
    config: &Config,
    caminho_config: &Path,
    braco: &str,
    relogio: bool,
    modo: &str,
    pedidas: Vec<String>,
    execucoes: Option<u32>,
) -> anyhow::Result<()> {
    let braco =
        Braco::de_texto(braco).context("braço inválido: use A, B ou C (e --relogio para T)")?;
    let modo = Modo::de_texto(modo).context("modo inválido: use avaliacao ou campanha")?;
    let execucoes = execucoes
        .unwrap_or(config.travessia.execucoes_por_seed)
        .max(1);
    for seed in seeds(config, pedidas) {
        for numero in 1..=execucoes {
            let opcoes = OpcoesExecucao {
                braco,
                relogio,
                modo,
                seed: seed.clone(),
                numero,
                chaves: None,
            };
            eprintln!(
                "travessia: braço {}, seed {seed}, execução {numero}/{execucoes}",
                opcoes.rotulo_braco()
            );
            let resumo = travessia::rodar_execucao(config, caminho_config, &opcoes).await?;
            let concluidas = resumo.fases.iter().filter(|m| m.concluida).count();
            println!(
                "{}: {concluidas}/{} fase(s) concluída(s) ({}) → {}",
                resumo.id,
                resumo.fases.len(),
                resumo.motivo_fim,
                resumo.pasta.display()
            );
        }
    }
    Ok(())
}

pub fn relatorio(config: &Config) -> anyhow::Result<()> {
    let pastas = Pastas::da_config(config);
    let metricas = travessia::relatorio::ler_execucoes(&pastas.execucoes())?;
    let texto = travessia::relatorio::gerar(&metricas);
    std::fs::create_dir_all(&pastas.raiz)?;
    let arquivo = pastas.raiz.join("relatorio.md");
    std::fs::write(&arquivo, &texto)?;
    print!("{texto}");
    eprintln!("(salvo em {})", arquivo.display());
    Ok(())
}
