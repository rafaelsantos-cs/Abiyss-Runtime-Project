//! Crons persistidos no SQLite.
//!
//! Um cron é só um lembrete agendado: quando vence, o kernel publica um
//! evento na fila (código determinístico, sem chamar o modelo). O heartbeat
//! seguinte percebe o evento e decide o que fazer.
//!
//! Expressões no formato Unix de 5 campos: `minuto hora dia mês dia-da-semana`
//! (ex.: "0 9 * * 1-5" = 9h em dias úteis), no fuso horário LOCAL da máquina.

use std::str::FromStr;

use anyhow::{Context, bail};
use chrono::{DateTime, Local};
use croner::Cron;
use rusqlite::params;

use crate::db::Banco;
use crate::eventos;
use crate::tempo::agora_ms;

#[derive(Debug, Clone, PartialEq)]
pub struct ItemCron {
    pub id: i64,
    pub nome: String,
    pub expressao: String,
    pub mensagem: String,
    pub ativo: bool,
    pub proximo_ms: i64,
    pub ultimo_ms: Option<i64>,
}

/// Próximo disparo DEPOIS de `depois_de_ms`.
pub fn proximo_disparo(expressao: &str, depois_de_ms: i64) -> anyhow::Result<i64> {
    let cron = Cron::from_str(expressao)
        .with_context(|| format!("expressão cron inválida: '{expressao}'"))?;
    let base: DateTime<Local> = DateTime::from_timestamp_millis(depois_de_ms)
        .context("instante inválido")?
        .with_timezone(&Local);
    let proximo = cron
        .find_next_occurrence(&base, false)
        .with_context(|| format!("'{expressao}' não tem próximo disparo"))?;
    Ok(proximo.timestamp_millis())
}

/// Cadastra um cron novo.
pub fn adicionar(
    banco: &Banco,
    nome: &str,
    expressao: &str,
    mensagem: &str,
) -> anyhow::Result<ItemCron> {
    let nome = nome.trim();
    if nome.is_empty() {
        bail!("o nome do cron não pode ser vazio");
    }
    if mensagem.trim().is_empty() {
        bail!("a mensagem do cron não pode ser vazia");
    }
    if expressao.split_whitespace().count() != 5 {
        bail!("use o formato de 5 campos: minuto hora dia mês dia-da-semana");
    }
    let agora = agora_ms();
    let proximo = proximo_disparo(expressao, agora)?;
    banco
        .conexao()
        .execute(
            "INSERT INTO crons (nome, expressao, mensagem, ativo, proximo_ms, criado_ms)
             VALUES (?1, ?2, ?3, 1, ?4, ?5)",
            params![nome, expressao.trim(), mensagem.trim(), proximo, agora],
        )
        .with_context(|| format!("já existe um cron chamado '{nome}'?"))?;
    listar(banco)?
        .into_iter()
        .find(|c| c.nome == nome)
        .context("cron sumiu depois de criado")
}

pub fn remover(banco: &Banco, nome: &str) -> anyhow::Result<bool> {
    let n = banco
        .conexao()
        .execute("DELETE FROM crons WHERE nome = ?1", params![nome])?;
    Ok(n > 0)
}

pub fn listar(banco: &Banco) -> anyhow::Result<Vec<ItemCron>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT id, nome, expressao, mensagem, ativo, proximo_ms, ultimo_ms
         FROM crons ORDER BY proximo_ms ASC",
    )?;
    let lista = consulta
        .query_map([], |l| {
            Ok(ItemCron {
                id: l.get(0)?,
                nome: l.get(1)?,
                expressao: l.get(2)?,
                mensagem: l.get(3)?,
                ativo: l.get::<_, i64>(4)? != 0,
                proximo_ms: l.get(5)?,
                ultimo_ms: l.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lista)
}

/// Dispara os crons vencidos até `agora_ms`: publica um evento para cada
/// um e agenda o próximo disparo. Disparos perdidos (ex.: VM desligada)
/// viram UM evento só, não uma avalanche.
/// Devolve os nomes dos crons disparados.
pub fn disparar_vencidos(banco: &Banco, agora_ms: i64) -> anyhow::Result<Vec<String>> {
    let vencidos: Vec<ItemCron> = listar(banco)?
        .into_iter()
        .filter(|c| c.ativo && c.proximo_ms <= agora_ms)
        .collect();
    let mut disparados = Vec::new();
    for cron in vencidos {
        let conteudo = format!("Lembrete agendado '{}': {}", cron.nome, cron.mensagem);
        eventos::publicar(banco, eventos::TIPO_CRON, &cron.nome, &conteudo)?;
        let proximo = proximo_disparo(&cron.expressao, agora_ms)?;
        banco.conexao().execute(
            "UPDATE crons SET ultimo_ms = ?1, proximo_ms = ?2 WHERE id = ?3",
            params![agora_ms, proximo, cron.id],
        )?;
        disparados.push(cron.nome);
    }
    Ok(disparados)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn calcula_proximo_disparo() {
        let agora = agora_ms();
        let p = proximo_disparo("* * * * *", agora).unwrap();
        assert!(p > agora && p <= agora + 60_000);
        assert!(proximo_disparo("isso não é cron", agora).is_err());
    }

    #[test]
    fn dispara_vencidos_uma_vez_e_reagenda() {
        let banco = Banco::em_memoria().unwrap();
        let c = adicionar(&banco, "minuto", "* * * * *", "verifique a fila").unwrap();
        assert!(adicionar(&banco, "minuto", "* * * * *", "dup").is_err());
        assert!(adicionar(&banco, "ruim", "* * *", "x").is_err());

        // Antes da hora: nada.
        assert!(
            disparar_vencidos(&banco, c.proximo_ms - 1)
                .unwrap()
                .is_empty()
        );
        // Muito depois (vários minutos perdidos): um disparo só.
        let depois = c.proximo_ms + 10 * 60_000;
        assert_eq!(disparar_vencidos(&banco, depois).unwrap(), vec!["minuto"]);
        assert!(disparar_vencidos(&banco, depois).unwrap().is_empty());

        let pendentes = eventos::pendentes(&banco, 10).unwrap();
        assert_eq!(pendentes.len(), 1);
        assert_eq!(pendentes[0].tipo, eventos::TIPO_CRON);
        assert!(pendentes[0].conteudo.contains("verifique a fila"));

        let atualizado = &listar(&banco).unwrap()[0];
        assert_eq!(atualizado.ultimo_ms, Some(depois));
        assert!(atualizado.proximo_ms > depois);
        assert!(remover(&banco, "minuto").unwrap());
        assert!(!remover(&banco, "minuto").unwrap());
    }
}
