//! `abiyss sleep` e `abiyss memoria ...`: memória de longo prazo (cofre).
//! Não precisam das chaves do NIM.

use std::sync::Arc;

use abiyss::config::Config;
use abiyss::daemon;
use abiyss::db::Banco;
use abiyss::mcp::PonteMcp;
use abiyss::memoria::Memoria;
use abiyss::memoria::central::MemoriaCentral;
use abiyss::memoria::nota::EscopoBusca;
use abiyss::memoria::propostas::{self, EstadoProposta};
use abiyss::orquestrador::Orquestrador;
use abiyss::sono::{self, Gatilho, Sono};
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

/// `abiyss sleep --completo`: pede ao daemon ou dorme aqui.
pub async fn sono_completo(config: &Config, dia: Option<&str>) -> anyhow::Result<()> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    if daemon::esta_rodando(config) {
        if dia.is_some() {
            println!("(com o daemon rodando, o dia revisado é o da janela mais recente)");
        }
        sono::pedir(&banco)?;
        println!(
            "Pedido registrado: o daemon dorme no próximo tique (até {} s), depois do ciclo em andamento.
             Acompanhe com `abiyss status` e veja o resultado com `abiyss sleep --relatorio`.",
            config.daemon.cron_verificacao_segundos
        );
        return Ok(());
    }
    let dia = match dia {
        Some(d) => chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!("--dia precisa ser AAAA-MM-DD"))?,
        None => sono::dia_da_janela_atual(&config.sono, chrono::Local::now().naive_local()),
    };
    let orquestrador = Orquestrador::da_config(config, banco.clone())?;
    let sono = Sono::novo(config.clone(), banco, orquestrador);
    println!("Dormindo (revisão de {dia})...");
    let relatorio = sono.dormir(dia, Gatilho::Pedido).await?;
    println!(
        "Estado: {}\n{}",
        relatorio.estado,
        relatorio.resumo_para_evento()
    );
    Ok(())
}

/// `abiyss sleep --relatorio`: o relatório do último sono (ou de um dia).
pub fn relatorio_sono(config: &Config, dia: Option<&str>) -> anyhow::Result<()> {
    match sono::ler_relatorio(config, dia)? {
        Some((caminho, texto)) => {
            println!("{}\n", caminho.display());
            print!("{texto}");
        }
        None => println!(
            "Nenhum relatório de sono{} em {}.",
            dia.map(|d| format!(" de {d}")).unwrap_or_default(),
            sono::pasta_relatorios(config).display()
        ),
    }
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

/// Busca como a ferramenta `memoria_buscar` (sobe os servidores MCP para
/// usar o qmd, se estiver configurado). Bom para testar o qmd na VM.
pub async fn buscar(config: &Config, consulta: &str, escopo: &str) -> anyhow::Result<()> {
    let escopo = EscopoBusca::de_texto(escopo)
        .ok_or_else(|| anyhow::anyhow!("escopo inválido: use interno, externo ou ambos"))?;
    let mcp = Arc::new(PonteMcp::iniciar(config).await);
    let memoria = abrir(config)?.com_mcp(mcp.clone());
    let resposta = memoria.buscar(consulta, escopo).await;
    drop(memoria);
    if let Ok(ponte) = Arc::try_unwrap(mcp) {
        ponte.encerrar().await;
    }
    let resposta = resposta?;
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

/// Mostra a memória central e quanto do orçamento ela usa.
pub fn central(config: &Config) -> anyhow::Result<()> {
    let central = MemoriaCentral::da_config(config);
    let uso = central.uso();
    println!(
        "Memória central: {} ({uso}/{} caracteres{})",
        central.caminho().display(),
        central.limite(),
        if uso > central.limite() {
            ", ACIMA DO ORÇAMENTO"
        } else {
            ""
        }
    );
    let entradas = central.entradas();
    if entradas.is_empty() {
        println!("(vazia)");
    }
    for (i, e) in entradas.iter().enumerate() {
        let tipo = e.tipo.map(|t| t.como_texto()).unwrap_or("sem tipo");
        println!(
            "{:>3}. [{tipo}] {}",
            i + 1,
            e.texto.replace('\n', "\n     ")
        );
    }
    Ok(())
}
