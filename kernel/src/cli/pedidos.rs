//! `abiyss pedidos`: as perguntas que o Abiyss guardou para você.
//! Não precisa das chaves do NIM.

use abiyss::config::Config;
use abiyss::db::Banco;
use abiyss::pedidos::{self, EstadoPedido, Pedido};
use abiyss::tempo::formatar_ms;

fn imprimir(p: &Pedido) {
    let goal = p.goal_id.map(|g| format!(" goal #{g}")).unwrap_or_default();
    println!(
        "#{:<4} [{}] {} ({}{goal}, {})",
        p.id,
        p.urgencia,
        p.estado.como_texto(),
        p.origem,
        formatar_ms(p.criado_ms)
    );
    println!("      {}", p.pergunta);
    if !p.contexto.is_empty() {
        println!("      contexto: {}", p.contexto);
    }
    if let Some(r) = &p.resposta {
        println!("      resposta: {r}");
    }
}

pub fn listar(config: &Config, todos: bool) -> anyhow::Result<()> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    let lista = if todos {
        pedidos::recentes(&banco, 100)?
    } else {
        pedidos::pendentes(&banco)?
    };
    if lista.is_empty() {
        println!("Nenhum pedido{}.", if todos { "" } else { " pendente" });
        return Ok(());
    }
    for p in &lista {
        imprimir(p);
    }
    if lista.iter().any(|p| p.estado == EstadoPedido::Pendente) {
        println!("\nResponda com: abiyss pedidos responder <id> \"sua resposta\"");
    }
    Ok(())
}

pub fn responder(config: &Config, id: i64, resposta: &str) -> anyhow::Result<()> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    // Pela CLI, a resposta é do dono, sem conteúdo externo no meio.
    let p = pedidos::responder(&banco, id, resposta, None)?;
    println!(
        "Pedido #{} respondido. O Abiyss vê a resposta no próximo ciclo do heartbeat.",
        p.id
    );
    Ok(())
}

pub fn cancelar(config: &Config, id: i64) -> anyhow::Result<()> {
    let banco = Banco::abrir(&config.caminho_banco())?;
    pedidos::cancelar(&banco, id)?;
    println!("Pedido #{id} cancelado.");
    Ok(())
}
