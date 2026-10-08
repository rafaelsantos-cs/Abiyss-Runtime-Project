//! Comandos do dono no Discord. Respondidos pelo kernel, na hora (mesmo com
//! o Abiyss no meio de um turno) e sem chamar o modelo.
//!
//! Não existe pausa do heartbeat na CLI (só o disjuntor automático da
//! vigilância), então o gateway também não oferece uma.

use crate::config::Config;
use crate::daemon;
use crate::db::Banco;
use crate::pedidos;
use crate::status;
use crate::tempo::formatar_ms;

/// Chave em `estado_daemon` da conversa do gateway (a mesma entre reinícios).
pub const CHAVE_CONVERSA: &str = "gateway_conversa";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comando {
    Status,
    Pedidos,
    Nova,
    Ajuda,
}

pub const AJUDA: &str = "Comandos: `/status` (estado do Abiyss), `/pedidos` (perguntas dele \
                         esperando você), `/nova` (começa outra conversa), `/ajuda`. Para \
                         responder a um pedido, use \"Responder\" na mensagem do pedido.";

/// Só a mensagem inteira (`/status`, `!status`, sem mais nada) é comando:
/// "/etc/hosts está errado" continua sendo conversa.
pub fn interpretar(texto: &str) -> Option<Comando> {
    let t = texto.trim();
    let nome = t.strip_prefix('/').or_else(|| t.strip_prefix('!'))?;
    match nome.to_lowercase().as_str() {
        "status" => Some(Comando::Status),
        "pedidos" => Some(Comando::Pedidos),
        "nova" => Some(Comando::Nova),
        "ajuda" | "help" => Some(Comando::Ajuda),
        _ => None,
    }
}

/// Executa e devolve o texto da resposta.
pub fn executar(comando: Comando, config: &Config, banco: &Banco) -> anyhow::Result<String> {
    match comando {
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
    }
}
