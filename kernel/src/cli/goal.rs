//! `abiyss goal ...` e `abiyss cron ...`: gerenciam goals e crons direto
//! no banco (não precisam das chaves do NIM).

use abiyss::config::Config;
use abiyss::cron;
use abiyss::db::Banco;
use abiyss::diario;
use abiyss::goals::{self, EstadoGoal, NovoGoal};
use abiyss::tempo::formatar_ms;

fn abrir_banco(config: &Config) -> anyhow::Result<Banco> {
    Banco::abrir(&config.caminho_banco())
}

pub fn adicionar(config: &Config, novo: NovoGoal) -> anyhow::Result<()> {
    let banco = abrir_banco(config)?;
    let g = goals::criar(&banco, &novo, "usuario")?;
    println!("Goal #{} criado (estado: {}).", g.id, g.estado);
    Ok(())
}

pub fn listar(config: &Config, todos: bool) -> anyhow::Result<()> {
    let banco = abrir_banco(config)?;
    let lista = goals::listar(&banco, todos)?;
    if lista.is_empty() {
        println!("Nenhum goal{}.", if todos { "" } else { " ativo" });
        return Ok(());
    }
    let foco = goals::em_foco(&lista).map(|g| g.id);
    for g in lista {
        let marca = if Some(g.id) == foco { "*" } else { " " };
        println!(
            "{marca} #{:<4} [{:<12}] p{:<3} {}",
            g.id,
            g.estado.como_texto(),
            g.prioridade,
            g.titulo
        );
    }
    println!("(* = em foco no próximo heartbeat)");
    Ok(())
}

pub fn mostrar(config: &Config, id: i64) -> anyhow::Result<()> {
    let banco = abrir_banco(config)?;
    let g = goals::obter(&banco, id)?;
    println!("Goal #{} — {}", g.id, g.titulo);
    println!("Estado: {}   Prioridade: {}", g.estado, g.prioridade);
    println!("Núcleo: {}", g.nucleo);
    if !g.descricao.is_empty() {
        println!("Descrição: {}", g.descricao);
    }
    println!(
        "Criado: {}   Atualizado: {}",
        formatar_ms(g.criado_ms),
        formatar_ms(g.atualizado_ms)
    );
    if let Some(extras) = goals::extras(&banco, id)? {
        println!("Campos extras (preservados da importação): {extras}");
    }
    let proximos: Vec<&str> = g
        .estado
        .proximos_permitidos()
        .iter()
        .map(|e| e.como_texto())
        .collect();
    println!(
        "Próximos estados possíveis: {}",
        if proximos.is_empty() {
            "nenhum (final)".to_string()
        } else {
            proximos.join(", ")
        }
    );
    println!("\nHistórico:");
    for e in goals::eventos(&banco, id)? {
        let de =
            e.de.map(|d| d.como_texto().to_string())
                .unwrap_or_else(|| "—".into());
        println!(
            "  {}  {} → {}  [{}] {}",
            formatar_ms(e.momento_ms),
            de,
            e.para,
            e.autor,
            e.motivo
        );
    }
    Ok(())
}

pub fn mover(config: &Config, id: i64, estado: &str, motivo: &str) -> anyhow::Result<()> {
    let banco = abrir_banco(config)?;
    let destino = EstadoGoal::de_texto(estado).ok_or_else(|| {
        let validos: Vec<&str> = EstadoGoal::TODOS.iter().map(|e| e.como_texto()).collect();
        anyhow::anyhow!(
            "estado desconhecido '{estado}'. Válidos: {}",
            validos.join(", ")
        )
    })?;
    let g = goals::transicionar(&banco, id, destino, motivo, "usuario")?;
    println!("Goal #{} agora está '{}'.", g.id, g.estado);
    Ok(())
}

pub fn cron_adicionar(
    config: &Config,
    nome: &str,
    expressao: &str,
    mensagem: &str,
) -> anyhow::Result<()> {
    let banco = abrir_banco(config)?;
    let c = cron::adicionar(&banco, nome, expressao, mensagem)?;
    println!(
        "Cron '{}' criado. Próximo disparo: {}",
        c.nome,
        formatar_ms(c.proximo_ms)
    );
    Ok(())
}

pub fn cron_listar(config: &Config) -> anyhow::Result<()> {
    let banco = abrir_banco(config)?;
    let lista = cron::listar(&banco)?;
    if lista.is_empty() {
        println!("Nenhum cron.");
    }
    for c in lista {
        println!(
            "{:<20} [{}] próximo: {}  último: {}  — {}",
            c.nome,
            c.expressao,
            formatar_ms(c.proximo_ms),
            c.ultimo_ms
                .map(formatar_ms)
                .unwrap_or_else(|| "nunca".into()),
            c.mensagem
        );
    }
    Ok(())
}

pub fn cron_remover(config: &Config, nome: &str) -> anyhow::Result<()> {
    let banco = abrir_banco(config)?;
    if cron::remover(&banco, nome)? {
        println!("Cron '{nome}' removido.");
    } else {
        anyhow::bail!("não existe cron chamado '{nome}'");
    }
    Ok(())
}

pub fn diario_listar(config: &Config, limite: usize) -> anyhow::Result<()> {
    let banco = abrir_banco(config)?;
    let entradas = diario::recentes(&banco, limite)?;
    if entradas.is_empty() {
        println!("Diário vazio.");
    }
    // Mostra do mais antigo para o mais novo.
    for e in entradas.into_iter().rev() {
        let goal = e.goal_id.map(|g| format!(" goal #{g}")).unwrap_or_default();
        println!(
            "#{} {} [{}{}]",
            e.id,
            formatar_ms(e.momento_ms),
            e.origem,
            goal
        );
        println!("  ação:        {}", e.acao);
        println!("  expectativa: {}", e.expectativa);
        if let Some(sinais) = &e.sinais {
            println!("  sinais:      {sinais}");
        }
        if let Some(risco) = &e.risco {
            println!("  risco:       {risco}");
        }
        if let Some(confianca) = e.confianca {
            println!("  confiança:   {:.0}%", confianca * 100.0);
        }
        match e.resultado {
            Some(r) => println!("  resultado:   {}", r.replace('\n', "\n               ")),
            None => println!("  resultado:   (ainda não observado)"),
        }
        if let Some(extras) = &e.extras {
            println!("  extras:      {extras}");
        }
    }
    Ok(())
}
