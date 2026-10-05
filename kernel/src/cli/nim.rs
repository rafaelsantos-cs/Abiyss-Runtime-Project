//! Comandos de diagnóstico do NIM: `testar-nim` e `mock-nim`.

use std::io::Write;
use std::time::Duration;

use anyhow::Context;

use abiyss::config::Config;
use abiyss::db::Banco;
use abiyss::nim::mock::{MockNim, roteiro_resistencia};
use abiyss::nim::{self, EventoStream, Mensagem};
use abiyss::orquestrador::{AoReceber, Nivel, Origem, Orquestrador};

/// Qual modelo da config usar no `testar-nim`.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum QualModelo {
    Cerebro,
    Ultra,
    Medium,
    Low,
}

/// `abiyss esforco`: a tabela de cada modelo, nível por nível.
pub fn esforco(config: &Config) -> anyhow::Result<()> {
    println!("Modos: raso = minimal, low, medium; profundo = high, xhigh, ultra.");
    for papel in abiyss::config::ConfigModelos::PAPEIS {
        println!("\n[modelos.{papel}]");
        for linha in abiyss::esforco::descrever(&config.modelos, papel)? {
            println!("  {linha}");
        }
    }
    Ok(())
}

/// O que o `mock-nim` responde.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum RoteiroMock {
    Eco,
    Resistencia,
}

pub async fn rodar_mock(porta: u16, roteiro: RoteiroMock, atraso_ms: u64) -> anyhow::Result<()> {
    let mock = MockNim::iniciar_em(&format!("127.0.0.1:{porta}"))
        .await
        .with_context(|| format!("não consegui abrir a porta {porta}"))?;
    if let RoteiroMock::Resistencia = roteiro {
        mock.definir_roteiro(roteiro_resistencia(Duration::from_millis(atraso_ms)));
    }
    println!("Mock do NIM ouvindo em {}", mock.base_url());
    println!("Use esse valor em nim.base_url (num abiyss.toml separado) e qualquer chave.");
    println!("Ctrl+C para parar.");
    tokio::signal::ctrl_c().await?;
    Ok(())
}

pub async fn testar_nim(
    config: &Config,
    qual: QualModelo,
    stream: bool,
    mensagem: &str,
) -> anyhow::Result<()> {
    // Passa pelo orquestrador: respeita o rate limit e fica registrado no banco.
    let banco = Banco::abrir(&config.caminho_banco())?;
    let orquestrador = Orquestrador::da_config(config, banco)?;
    let modelo = match qual {
        QualModelo::Cerebro => &config.modelos.cerebro,
        QualModelo::Ultra => &config.modelos.sub_ultra,
        QualModelo::Medium => &config.modelos.sub_medium,
        QualModelo::Low => &config.modelos.sub_low,
    };
    let pedido = nim::montar_pedido(modelo, vec![Mensagem::usuario(mensagem)], vec![]);

    let mut mostrar = |evento: EventoStream| match evento {
        EventoStream::Texto(t) => {
            print!("{t}");
            let _ = std::io::stdout().flush();
        }
        EventoStream::Raciocinio(r) => eprint!("\x1b[2m{r}\x1b[0m"),
        EventoStream::InicioFerramenta(_) => {}
    };
    let ao_receber: AoReceber<'_> = if stream { Some(&mut mostrar) } else { None };

    eprintln!("→ {} em {}", modelo.id, config.nim.base_url);
    let resposta = match qual {
        QualModelo::Cerebro => {
            orquestrador
                .cerebro
                .chamar(Origem::Conversa, &pedido, ao_receber)
                .await?
        }
        QualModelo::Ultra => {
            orquestrador
                .subagentes
                .chamar(Nivel::Ultra, &pedido, ao_receber)
                .await?
        }
        QualModelo::Medium => {
            orquestrador
                .subagentes
                .chamar(Nivel::Medium, &pedido, ao_receber)
                .await?
        }
        QualModelo::Low => {
            orquestrador
                .subagentes
                .chamar(Nivel::Low, &pedido, ao_receber)
                .await?
        }
    };
    if stream {
        println!();
    } else {
        println!("{}", resposta.mensagem.texto());
    }
    eprintln!(
        "← fim: {:?} | tokens: {} entrada + {} saída",
        resposta.motivo_fim, resposta.uso.prompt_tokens, resposta.uso.completion_tokens
    );
    Ok(())
}
