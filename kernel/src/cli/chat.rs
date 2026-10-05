//! `abiyss chat`: conversa interativa no terminal.

use std::io::Write;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};

use abiyss::chat::SessaoChat;
use abiyss::config::Config;
use abiyss::db::Banco;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::historico;
use abiyss::mcp::PonteMcp;
use abiyss::memoria::Memoria;
use abiyss::nim::EventoStream;
use abiyss::orquestrador::Orquestrador;
use abiyss::subagentes::ControleSubagentes;

pub struct OpcoesChat {
    pub continuar: bool,
    pub conversa: Option<i64>,
    pub mensagem: Option<String>,
    pub mostrar_raciocinio: bool,
}

const AJUDA: &str = "Comandos: /nova (nova conversa), /sair (encerra), /ajuda";

pub async fn executar(config: Config, opcoes: OpcoesChat) -> anyhow::Result<()> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    let orquestrador = Orquestrador::da_config(&config, banco.clone())?;
    // Sobe os servidores MCP (os que falharem são ignorados, com aviso no log).
    let mcp = Arc::new(PonteMcp::iniciar(&config).await);
    // `delegar` só grava o pedido: quem executa é o daemon.
    let controle = ControleSubagentes::novo(config.clone(), banco.clone(), None);
    // Memória: o modelo só propõe; quem grava é o `abiyss sleep`.
    // O qmd (se configurado e ativo) é o motor de busca da memória.
    let memoria = Arc::new(Memoria::abrir(&config, banco.clone())?.com_mcp(mcp.clone()));
    let ferramentas = Arc::new(
        CaixaDeFerramentas::da_config(&config)?
            .com_mcp(mcp.clone())
            .com_memoria(memoria)
            .com_subagentes(controle)
            // Respostas do dono aos pedidos do ciclo autônomo.
            .com_pedidos(banco.clone()),
    );

    // Qual conversa usar: a pedida, a última, ou uma nova.
    let existente = match opcoes.conversa {
        Some(id) => Some(id),
        None if opcoes.continuar => historico::ultima_conversa(&banco)?,
        None => None,
    };
    let mut sessao = match existente {
        Some(id) => SessaoChat::retomar(
            config.clone(),
            orquestrador.clone(),
            banco.clone(),
            ferramentas.clone(),
            id,
        )?,
        None => SessaoChat::nova(
            config.clone(),
            orquestrador.clone(),
            banco.clone(),
            ferramentas.clone(),
        )?,
    };

    // Modo "uma mensagem só" (bom para scripts).
    if let Some(texto) = opcoes.mensagem {
        let resultado = turno(&mut sessao, &texto, opcoes.mostrar_raciocinio).await;
        encerrar(sessao, ferramentas, mcp).await;
        return resultado;
    }

    println!("Conversa {} com o Abiyss. {AJUDA}", sessao.conversa);
    let mut linhas = BufReader::new(tokio::io::stdin()).lines();
    loop {
        print!("\nvocê> ");
        std::io::stdout().flush()?;
        // `None` = fim da entrada (Ctrl+D).
        let Some(linha) = linhas.next_line().await? else {
            println!();
            break;
        };
        let linha = linha.trim();
        match linha {
            "" => continue,
            "/sair" | "/exit" => break,
            "/ajuda" | "/help" => println!("{AJUDA}"),
            "/nova" => {
                sessao = SessaoChat::nova(
                    config.clone(),
                    orquestrador.clone(),
                    banco.clone(),
                    ferramentas.clone(),
                )?;
                println!("Nova conversa: {}", sessao.conversa);
            }
            texto => {
                // Um erro num turno não encerra o chat.
                if let Err(e) = turno(&mut sessao, texto, opcoes.mostrar_raciocinio).await {
                    eprintln!("\n[erro] {e:#}");
                }
            }
        }
    }
    encerrar(sessao, ferramentas, mcp).await;
    Ok(())
}

/// Solta a sessão e fecha os servidores MCP com educação.
async fn encerrar(sessao: SessaoChat, ferramentas: Arc<CaixaDeFerramentas>, mcp: Arc<PonteMcp>) {
    drop(sessao);
    drop(ferramentas);
    if let Ok(ponte) = Arc::try_unwrap(mcp) {
        ponte.encerrar().await;
    }
}

/// Envia uma mensagem e mostra a resposta em streaming.
async fn turno(
    sessao: &mut SessaoChat,
    texto: &str,
    mostrar_raciocinio: bool,
) -> anyhow::Result<()> {
    print!("abiyss> ");
    std::io::stdout().flush()?;
    let mut mostrar = |evento: EventoStream| match evento {
        EventoStream::Texto(t) => {
            print!("{t}");
            let _ = std::io::stdout().flush();
        }
        EventoStream::Raciocinio(r) => {
            if mostrar_raciocinio {
                eprint!("\x1b[2m{r}\x1b[0m");
            }
        }
        EventoStream::InicioFerramenta(nome) => eprint!("\n[ferramenta: {nome}] "),
    };
    sessao.enviar(texto, Some(&mut mostrar)).await?;
    println!();
    Ok(())
}
