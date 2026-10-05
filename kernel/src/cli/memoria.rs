//! `abiyss sleep` e `abiyss memoria ...`: memória de longo prazo (cofre).
//! Não precisam das chaves do NIM.

use abiyss::config::Config;
use abiyss::db::Banco;
use abiyss::memoria::Memoria;
use abiyss::memoria::nota::EscopoBusca;
use abiyss::memoria::propostas::{self, EstadoProposta};
use abiyss::tempo::formatar_ms;

fn abrir(config: &Config) -> anyhow::Result<Memoria> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    Memoria::abrir(config, banco)
}

/// Aplica as propostas pendentes e mostra o que aconteceu com cada uma.
pub fn sleep(config: &Config) -> anyhow::Result<()> {
    let memoria = abrir(config)?;
    let relatorio = memoria.sleep()?;
    if relatorio.decisoes.is_empty() {
        println!("Nenhuma proposta pendente.");
        return Ok(());
    }
    for d in &relatorio.decisoes {
        let marca = if d.aplicada { "aplicada " } else { "REJEITADA" };
        println!("#{:<4} {marca} {}: {}", d.id, d.caminho, d.detalhe);
    }
    println!(
        "\n{} aplicada(s), {} rejeitada(s). Cofre: {}",
        relatorio.aplicadas(),
        relatorio.rejeitadas(),
        memoria.cofre().raiz().display()
    );
    Ok(())
}

/// Remove uma nota e registra que ela foi removida (não o conteúdo).
pub fn esquecer(config: &Config, caminho: &str) -> anyhow::Result<()> {
    let memoria = abrir(config)?;
    let removida = memoria.esquecer(caminho)?;
    println!("Nota esquecida: {removida} (o registro guarda só o caminho, nunca o conteúdo).");
    Ok(())
}

pub fn propostas(config: &Config, todas: bool) -> anyhow::Result<()> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    let lista = if todas {
        propostas::recentes(&banco, 100)?
    } else {
        propostas::pendentes(&banco)?
    };
    if lista.is_empty() {
        println!("Nenhuma proposta{}.", if todas { "" } else { " pendente" });
    }
    for p in lista {
        let origem = match &p.origem_externa {
            Some(o) => format!(" — origem externa: {o}"),
            None => String::new(),
        };
        println!(
            "#{} {} [{}] {} ({}, {}){origem}",
            p.id,
            formatar_ms(p.criado_ms),
            p.estado.como_texto(),
            p.caminho,
            p.tipo,
            p.fonte
        );
        if p.estado != EstadoProposta::Pendente
            && let Some(motivo) = &p.motivo
        {
            println!("    {motivo}");
        }
    }
    Ok(())
}

pub async fn buscar(config: &Config, consulta: &str, escopo: &str) -> anyhow::Result<()> {
    let escopo = EscopoBusca::de_texto(escopo)
        .ok_or_else(|| anyhow::anyhow!("escopo inválido: use interno, externo ou ambos"))?;
    let memoria = abrir(config)?;
    let resposta = memoria.buscar(consulta, escopo).await?;
    println!(
        "{} resultado(s) (motor: {})",
        resposta.resultados.len(),
        resposta.motor
    );
    for r in resposta.resultados {
        println!(
            "- {} [{}] ({:.1}): {}",
            r.caminho,
            r.escopo.como_texto(),
            r.pontuacao,
            r.trecho
        );
    }
    Ok(())
}

pub fn registro(config: &Config, limite: usize) -> anyhow::Result<()> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    let entradas = propostas::registro(&banco, limite)?;
    if entradas.is_empty() {
        println!("Registro vazio.");
    }
    for e in entradas.into_iter().rev() {
        let detalhe = if e.detalhe.is_empty() {
            String::new()
        } else {
            format!(" ({})", e.detalhe)
        };
        println!(
            "{} {:<10} {}{detalhe}",
            formatar_ms(e.momento_ms),
            e.acao,
            e.caminho
        );
    }
    Ok(())
}
