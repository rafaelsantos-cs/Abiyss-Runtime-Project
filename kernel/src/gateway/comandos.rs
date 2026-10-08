//! Comandos do dono no Discord. Respondidos pelo kernel, na hora (mesmo com
//! o Abiyss no meio de um turno) e sem chamar o modelo.
//!
//! Não existe pausa do heartbeat na CLI (só o disjuntor automático da
//! vigilância), então o gateway também não oferece uma.

use std::path::PathBuf;

use anyhow::bail;

use crate::caminho_seguro;
use crate::config::Config;
use crate::daemon;
use crate::db::Banco;
use crate::pedidos;
use crate::status;
use crate::tempo::formatar_ms;

/// Chave em `estado_daemon` da conversa do gateway (a mesma entre reinícios).
pub const CHAVE_CONVERSA: &str = "gateway_conversa";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comando {
    Status,
    Pedidos,
    Nova,
    Ajuda,
    /// Manda um arquivo do workspace (caminho relativo a ele).
    Arquivo(String),
}

/// O que um comando devolve: o texto e, às vezes, um arquivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resposta {
    pub texto: String,
    /// Caminho REAL, já conferido dentro do workspace.
    pub anexo: Option<PathBuf>,
}

impl From<String> for Resposta {
    fn from(texto: String) -> Resposta {
        Resposta { texto, anexo: None }
    }
}

pub const AJUDA: &str = "Comandos: `/status` (estado do Abiyss), `/pedidos` (perguntas dele \
                         esperando você), `/nova` (começa outra conversa), `/arquivo <caminho>` \
                         (um arquivo do workspace), `/ajuda`. Para responder a um pedido, use \
                         \"Responder\" na mensagem do pedido.";

/// Só a mensagem inteira (`/status`, `!status`, sem mais nada) é comando:
/// "/etc/hosts está errado" continua sendo conversa.
pub fn interpretar(texto: &str) -> Option<Comando> {
    let t = texto.trim();
    let nome = t.strip_prefix('/').or_else(|| t.strip_prefix('!'))?;
    if let Some((primeiro, resto)) = nome.split_once(char::is_whitespace)
        && primeiro.eq_ignore_ascii_case("arquivo")
    {
        return Some(Comando::Arquivo(resto.trim().to_string()));
    }
    match nome.to_lowercase().as_str() {
        "status" => Some(Comando::Status),
        "pedidos" => Some(Comando::Pedidos),
        "nova" => Some(Comando::Nova),
        "ajuda" | "help" => Some(Comando::Ajuda),
        _ => None,
    }
}

/// Executa e devolve a resposta.
pub fn executar(comando: Comando, config: &Config, banco: &Banco) -> anyhow::Result<Resposta> {
    match comando {
        Comando::Arquivo(pedido) => {
            let caminho = anexo_do_workspace(config, &pedido)?;
            let bytes = std::fs::metadata(&caminho)?.len();
            Ok(Resposta {
                texto: format!("📎 `{pedido}` ({bytes} bytes)"),
                anexo: Some(caminho),
            })
        }
        outro => executar_texto(outro, config, banco).map(Resposta::from),
    }
}

/// O arquivo pedido, se for um arquivo comum DENTRO do workspace (links
/// simbólicos resolvidos: um link para fora é recusado) e couber no teto.
pub fn anexo_do_workspace(config: &Config, pedido: &str) -> anyhow::Result<PathBuf> {
    if pedido.is_empty() {
        bail!("diga qual arquivo: `/arquivo relatorio.md` (caminho dentro do workspace)");
    }
    let raiz = config.caminho_workspace().canonicalize()?;
    let alvo = caminho_seguro::resolver_dentro(&raiz, pedido, "workspace")?;
    let real = alvo
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("não existe no workspace: {pedido}"))?;
    if !real.starts_with(&raiz) {
        bail!("o caminho sai do workspace (link simbólico?)");
    }
    let meta = std::fs::metadata(&real)?;
    if !meta.is_file() {
        bail!("não é um arquivo: {pedido}");
    }
    if meta.len() > config.gateway.max_bytes_anexo {
        bail!(
            "arquivo grande demais ({} bytes; limite {})",
            meta.len(),
            config.gateway.max_bytes_anexo
        );
    }
    Ok(real)
}

fn executar_texto(comando: Comando, config: &Config, banco: &Banco) -> anyhow::Result<String> {
    match comando {
        Comando::Arquivo(_) => unreachable!("tratado em executar"),
        Comando::Ajuda => Ok(AJUDA.to_string()),
        Comando::Status => {
            let saude = status::verificar(config, banco)?;
            let relatorio = status::relatorio(config, banco)?;
            Ok(format!(
                "**{}**\n```\n{}\n```",
                saude.linha,
                relatorio.trim_end()
            ))
        }
        Comando::Pedidos => {
            let lista = pedidos::pendentes(banco)?;
            if lista.is_empty() {
                return Ok("Nenhum pedido pendente.".to_string());
            }
            let mut t = format!("{} pedido(s) pendente(s):\n", lista.len());
            for p in &lista {
                t.push_str(&format!(
                    "- **#{}** [{}] {} (desde {})\n",
                    p.id,
                    p.urgencia,
                    p.pergunta,
                    formatar_ms(p.criado_ms)
                ));
            }
            t.push_str(
                "Responda com \"Responder\" na mensagem do pedido (na DM), ou com \
                 `abiyss pedidos responder <id> \"...\"` na VM.",
            );
            Ok(t)
        }
        Comando::Nova => {
            banco.conexao().execute(
                "DELETE FROM estado_daemon WHERE chave = ?1",
                [CHAVE_CONVERSA],
            )?;
            Ok("Ok: a próxima mensagem começa uma conversa nova.".to_string())
        }
    }
}

/// A conversa do gateway, se já existir.
pub fn conversa_atual(banco: &Banco) -> anyhow::Result<Option<i64>> {
    Ok(daemon::ler_estado(banco, CHAVE_CONVERSA)?.and_then(|v| v.parse().ok()))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn so_a_mensagem_inteira_e_comando() {
        assert_eq!(interpretar("/status"), Some(Comando::Status));
        assert_eq!(interpretar("  !Pedidos "), Some(Comando::Pedidos));
        assert_eq!(interpretar("/nova"), Some(Comando::Nova));
        assert_eq!(interpretar("/help"), Some(Comando::Ajuda));
        assert_eq!(interpretar("/status agora"), None);
        assert_eq!(interpretar("/etc/hosts está errado"), None);
        assert_eq!(interpretar("status"), None);
        assert_eq!(
            interpretar("/arquivo  relatorios/hoje.md "),
            Some(Comando::Arquivo("relatorios/hoje.md".into()))
        );
        assert_eq!(interpretar("/arquivo"), None);
    }
}
