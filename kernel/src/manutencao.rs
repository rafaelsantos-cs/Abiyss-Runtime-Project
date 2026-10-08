//! Manutenção do banco: retenção dos registros detalhados, checkpoint do
//! WAL e vacuum incremental. Roda dentro do daemon (e por `abiyss manutencao`).
//!
//! Retenção: linhas detalhadas mais velhas que N dias viram AGREGADOS
//! DIÁRIOS e são apagadas. Vale para:
//! - `chamadas_modelo` → `chamadas_modelo_diarias`;
//! - `fila_eventos` (só eventos JÁ CONSUMIDOS; pendente nunca é apagado)
//!   → `eventos_diarios`;
//! - `ciclos` → `ciclos_diarios`;
//! - `gateway_mensagens` (só as que já terminaram: nada na fila, nada
//!   ligado a um pedido ainda pendente) → `gateway_mensagens_diarias`.
//!
//! Os dias são dias UTC inteiros: um dia só é agregado quando ele inteiro
//! já passou do limite, e cada dia é agregado numa transação própria
//! (agregado gravado e detalhe apagado juntos, ou nada).
//!
//! Percentis não sobrevivem à agregação (guardamos soma, contagem e
//! máximo); o `abiyss status` só usa as últimas 24 h, que nunca são agregadas.

use rusqlite::params;
use serde::Deserialize;

use crate::db::Banco;

const DIA_MS: i64 = 24 * 60 * 60 * 1000;

/// Seção `[retencao]` do `abiyss.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigRetencao {
    /// Dias de detalhe guardados em `chamadas_modelo`.
    pub chamadas_modelo_dias: u32,
    /// Dias de detalhe dos eventos consumidos em `fila_eventos`.
    pub eventos_dias: u32,
    /// Dias de detalhe em `ciclos`.
    pub ciclos_dias: u32,
    /// Dias de detalhe das mensagens do gateway (Discord) já terminadas.
    pub gateway_dias: u32,
    /// Intervalo entre rodadas de retenção + vacuum incremental.
    pub manutencao_minutos: u64,
    /// Intervalo entre checkpoints do WAL.
    pub checkpoint_minutos: u64,
    /// Máximo de páginas devolvidas ao sistema por rodada de vacuum.
    pub vacuum_max_paginas: u32,
    /// Bancos criados antes do vacuum incremental precisam de UM `VACUUM`
    /// completo para mudar de modo. `true` = o daemon faz isso sozinho na
    /// primeira manutenção (trava o banco por alguns segundos).
    pub converter_auto_vacuum: bool,
}

impl Default for ConfigRetencao {
    fn default() -> Self {
        ConfigRetencao {
            chamadas_modelo_dias: 30,
            eventos_dias: 30,
            ciclos_dias: 30,
            gateway_dias: 30,
            manutencao_minutos: 360,
            checkpoint_minutos: 15,
            vacuum_max_paginas: 2_000,
            converter_auto_vacuum: true,
        }
    }
}

impl ConfigRetencao {
    pub fn validar(&self) -> anyhow::Result<()> {
        // Menos de 2 dias apagaria parte das últimas 24 h (status e
        // interocepção leem essa janela).
        for (nome, dias) in [
            ("chamadas_modelo_dias", self.chamadas_modelo_dias),
            ("eventos_dias", self.eventos_dias),
            ("ciclos_dias", self.ciclos_dias),
            ("gateway_dias", self.gateway_dias),
        ] {
            if dias < 2 {
                anyhow::bail!("retencao.{nome} precisa ser pelo menos 2");
            }
        }
        if self.manutencao_minutos == 0 || self.checkpoint_minutos == 0 {
            anyhow::bail!("retencao.manutencao_minutos e checkpoint_minutos precisam ser > 0");
        }
        Ok(())
    }
}

/// O que uma rodada de retenção fez.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelatorioRetencao {
    pub dias_agregados: usize,
    pub chamadas_apagadas: usize,
    pub eventos_apagados: usize,
    pub ciclos_apagados: usize,
    pub gateway_apagadas: usize,
}

impl RelatorioRetencao {
    pub fn como_texto(&self) -> String {
        format!(
            "{} dia(s) agregado(s); apagadas {} chamada(s), {} evento(s), {} ciclo(s), \
             {} mensagem(ns) do gateway",
            self.dias_agregados,
            self.chamadas_apagadas,
            self.eventos_apagados,
            self.ciclos_apagados,
            self.gateway_apagadas
        )
    }
}

/// Primeiro instante (ms UTC) que ainda é guardado em detalhe: o início do
/// dia UTC de `agora - dias`. Tudo antes disso é agregado.
pub fn limite_ms(agora_ms: i64, dias: u32) -> i64 {
    let limite = agora_ms - dias as i64 * DIA_MS;
    limite - limite.rem_euclid(DIA_MS)
}

/// "AAAA-MM-DD" do dia UTC que começa em `inicio_ms`.
fn nome_do_dia(inicio_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(inicio_ms)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| format!("dia-{}", inicio_ms / DIA_MS))
}

/// Inícios (ms) dos dias UTC que têm linhas antes de `limite` na tabela,
/// do mais antigo ao mais novo.
fn dias_antes_de(
    banco: &Banco,
    tabela: &str,
    coluna: &str,
    filtro_extra: &str,
    limite: i64,
) -> anyhow::Result<Vec<i64>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(&format!(
        "SELECT DISTINCT ({coluna} / {DIA_MS}) * {DIA_MS} FROM {tabela}
          WHERE {coluna} < ?1 {filtro_extra} ORDER BY 1"
    ))?;
    let dias = consulta
        .query_map(params![limite], |l| l.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(dias)
}

/// Agrega um dia numa transação e apaga o detalhe. `agregar` recebe
/// (?1 = nome do dia, ?2 = início, ?3 = fim); `apagar` recebe
/// (?1 = início, ?2 = fim). Devolve quantas linhas foram apagadas.
fn agregar_dia(banco: &Banco, inicio: i64, agregar: &str, apagar: &str) -> anyhow::Result<usize> {
    let fim = inicio + DIA_MS;
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    transacao.execute(agregar, params![nome_do_dia(inicio), inicio, fim])?;
    let apagadas = transacao.execute(apagar, params![inicio, fim])?;
    transacao.commit()?;
    Ok(apagadas)
}

const AGREGAR_CHAMADAS: &str = "
    INSERT INTO chamadas_modelo_diarias
        (dia, pool, modelo, status, chamadas, tokens_entrada, tokens_saida,
         duracao_soma_ms, duracao_max_ms, primeiro_token_soma_ms, primeiro_token_amostras)
    SELECT ?1, pool, modelo, status, COUNT(*), SUM(tokens_entrada), SUM(tokens_saida),
           SUM(duracao_ms), MAX(duracao_ms), COALESCE(SUM(primeiro_token_ms), 0),
           COUNT(primeiro_token_ms)
      FROM chamadas_modelo
     WHERE momento_ms >= ?2 AND momento_ms < ?3
     GROUP BY pool, modelo, status
    ON CONFLICT (dia, pool, modelo, status) DO UPDATE SET
        chamadas = chamadas + excluded.chamadas,
        tokens_entrada = tokens_entrada + excluded.tokens_entrada,
        tokens_saida = tokens_saida + excluded.tokens_saida,
        duracao_soma_ms = duracao_soma_ms + excluded.duracao_soma_ms,
        duracao_max_ms = MAX(duracao_max_ms, excluded.duracao_max_ms),
        primeiro_token_soma_ms = primeiro_token_soma_ms + excluded.primeiro_token_soma_ms,
        primeiro_token_amostras = primeiro_token_amostras + excluded.primeiro_token_amostras";

const APAGAR_CHAMADAS: &str = "
    DELETE FROM chamadas_modelo WHERE momento_ms >= ?1 AND momento_ms < ?2";

const AGREGAR_EVENTOS: &str = "
    INSERT INTO eventos_diarios (dia, tipo, origem, eventos)
    SELECT ?1, tipo, origem, COUNT(*)
      FROM fila_eventos
     WHERE momento_ms >= ?2 AND momento_ms < ?3 AND consumido_ms IS NOT NULL
     GROUP BY tipo, origem
    ON CONFLICT (dia, tipo, origem) DO UPDATE SET eventos = eventos + excluded.eventos";

const APAGAR_EVENTOS: &str = "
    DELETE FROM fila_eventos
     WHERE momento_ms >= ?1 AND momento_ms < ?2 AND consumido_ms IS NOT NULL";

const AGREGAR_CICLOS: &str = "
    INSERT INTO ciclos_diarios (dia, ciclos, com_modelo, com_erro, tokens, duracao_soma_ms)
    SELECT ?1, COUNT(*), SUM(chamou_modelo), COUNT(erro), SUM(tokens),
           COALESCE(SUM(fim_ms - inicio_ms), 0)
      FROM ciclos
     WHERE inicio_ms >= ?2 AND inicio_ms < ?3
    HAVING COUNT(*) > 0
    ON CONFLICT (dia) DO UPDATE SET
        ciclos = ciclos + excluded.ciclos,
        com_modelo = com_modelo + excluded.com_modelo,
        com_erro = com_erro + excluded.com_erro,
        tokens = tokens + excluded.tokens,
        duracao_soma_ms = duracao_soma_ms + excluded.duracao_soma_ms";

const APAGAR_CICLOS: &str = "
    DELETE FROM ciclos WHERE inicio_ms >= ?1 AND inicio_ms < ?2";

/// Mensagens do gateway que ainda servem: na fila (entrada esperando a
/// conversa, saída esperando entrega) ou ligadas a um pedido pendente (o
/// "responder" do dono no Discord precisa achar a mensagem do pedido).
const GATEWAY_TERMINADAS: &str = "
    estado NOT IN ('pendente', 'processando', 'transmitindo')
    AND (pedido_id IS NULL
         OR pedido_id NOT IN (SELECT id FROM pedidos_usuario WHERE estado = 'pendente'))";

const AGREGAR_GATEWAY: &str = "
    INSERT INTO gateway_mensagens_diarias (dia, direcao, tipo, estado, mensagens)
    SELECT ?1, direcao, tipo, estado, COUNT(*)
      FROM gateway_mensagens
     WHERE momento_ms >= ?2 AND momento_ms < ?3 AND GATEWAY_TERMINADAS
     GROUP BY direcao, tipo, estado
    ON CONFLICT (dia, direcao, tipo, estado) DO UPDATE SET
        mensagens = mensagens + excluded.mensagens";

const APAGAR_GATEWAY: &str = "
    DELETE FROM gateway_mensagens
     WHERE momento_ms >= ?1 AND momento_ms < ?2 AND GATEWAY_TERMINADAS";

/// Agrega e apaga tudo que passou da retenção. Idempotente: rodar de novo
/// sem linhas velhas não muda nada.
pub fn aplicar_retencao(
    banco: &Banco,
    config: &ConfigRetencao,
    agora_ms: i64,
) -> anyhow::Result<RelatorioRetencao> {
    let mut relatorio = RelatorioRetencao::default();
    let mut dias_tocados = std::collections::BTreeSet::new();

    let limite = limite_ms(agora_ms, config.chamadas_modelo_dias);
    for dia in dias_antes_de(banco, "chamadas_modelo", "momento_ms", "", limite)? {
        relatorio.chamadas_apagadas += agregar_dia(banco, dia, AGREGAR_CHAMADAS, APAGAR_CHAMADAS)?;
        dias_tocados.insert(dia);
    }

    let limite = limite_ms(agora_ms, config.eventos_dias);
    let so_consumidos = "AND consumido_ms IS NOT NULL";
    for dia in dias_antes_de(banco, "fila_eventos", "momento_ms", so_consumidos, limite)? {
        relatorio.eventos_apagados += agregar_dia(banco, dia, AGREGAR_EVENTOS, APAGAR_EVENTOS)?;
        dias_tocados.insert(dia);
    }

    let limite = limite_ms(agora_ms, config.ciclos_dias);
    for dia in dias_antes_de(banco, "ciclos", "inicio_ms", "", limite)? {
        relatorio.ciclos_apagados += agregar_dia(banco, dia, AGREGAR_CICLOS, APAGAR_CICLOS)?;
        dias_tocados.insert(dia);
    }

    // Os ids do Discord de cada mensagem saem junto (ON DELETE CASCADE).
    let limite = limite_ms(agora_ms, config.gateway_dias);
    let terminadas = format!("AND {GATEWAY_TERMINADAS}");
    let agregar = AGREGAR_GATEWAY.replace("GATEWAY_TERMINADAS", GATEWAY_TERMINADAS);
    let apagar = APAGAR_GATEWAY.replace("GATEWAY_TERMINADAS", GATEWAY_TERMINADAS);
    for dia in dias_antes_de(
        banco,
        "gateway_mensagens",
        "momento_ms",
        &terminadas,
        limite,
    )? {
        relatorio.gateway_apagadas += agregar_dia(banco, dia, &agregar, &apagar)?;
        dias_tocados.insert(dia);
    }

    relatorio.dias_agregados = dias_tocados.len();
    Ok(relatorio)
}

/// Resultado de `PRAGMA wal_checkpoint(TRUNCATE)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultadoCheckpoint {
    /// Outro processo estava lendo/escrevendo: o checkpoint ficou parcial
    /// (tenta de novo no próximo intervalo).
    pub ocupado: bool,
    /// Páginas no WAL antes do checkpoint (-1 se o banco não está em WAL).
    pub paginas_wal: i64,
    /// Páginas copiadas para o banco.
    pub paginas_copiadas: i64,
}

/// Copia o WAL para o banco e trunca o arquivo WAL.
pub fn checkpoint_wal(banco: &Banco) -> anyhow::Result<ResultadoCheckpoint> {
    let resultado = banco
        .conexao()
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |l| {
            Ok(ResultadoCheckpoint {
                ocupado: l.get::<_, i64>(0)? != 0,
                paginas_wal: l.get(1)?,
                paginas_copiadas: l.get(2)?,
            })
        })?;
    Ok(resultado)
}

/// O banco está em modo de vacuum incremental?
pub fn vacuum_incremental_ativo(banco: &Banco) -> anyhow::Result<bool> {
    let modo: i64 = banco
        .conexao()
        .query_row("PRAGMA auto_vacuum", [], |l| l.get(0))?;
    // 0 = nenhum, 1 = completo, 2 = incremental.
    Ok(modo == 2)
}

/// Muda um banco antigo para vacuum incremental (exige um VACUUM completo,
/// que reescreve o arquivo inteiro e trava o banco enquanto roda).
pub fn converter_para_vacuum_incremental(banco: &Banco) -> anyhow::Result<()> {
    let conexao = banco.conexao();
    conexao.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    conexao.execute_batch("VACUUM")?;
    Ok(())
}

/// Devolve ao sistema até `max_paginas` páginas livres. Devolve quantas
/// foram liberadas (0 se o banco não está em modo incremental).
pub fn vacuum_incremental(banco: &Banco, max_paginas: u32) -> anyhow::Result<i64> {
    if !vacuum_incremental_ativo(banco)? {
        return Ok(0);
    }
    let conexao = banco.conexao();
    let livres = |c: &rusqlite::Connection| -> rusqlite::Result<i64> {
        c.query_row("PRAGMA freelist_count", [], |l| l.get(0))
    };
    let antes = livres(&conexao)?;
    // O pragma libera UMA página a cada passo: é preciso percorrer todas as
    // linhas do resultado (um `execute` só daria um passo).
    let mut vacuum = conexao.prepare(&format!("PRAGMA incremental_vacuum({max_paginas})"))?;
    let mut passos = vacuum.query([])?;
    while passos.next()?.is_some() {}
    drop(passos);
    drop(vacuum);
    Ok(antes - livres(&conexao)?)
}

/// Uma rodada completa (retenção + vacuum), como o daemon faz. Devolve um
/// resumo em texto para o log.
pub fn rodada_completa(
    banco: &Banco,
    config: &ConfigRetencao,
    agora_ms: i64,
) -> anyhow::Result<String> {
    let retencao = aplicar_retencao(banco, config, agora_ms)?;
    let mut resumo = retencao.como_texto();
    if !vacuum_incremental_ativo(banco)? && config.converter_auto_vacuum {
        converter_para_vacuum_incremental(banco)?;
        resumo.push_str("; banco convertido para vacuum incremental");
    }
    let liberadas = vacuum_incremental(banco, config.vacuum_max_paginas)?;
    resumo.push_str(&format!("; vacuum: {liberadas} página(s) liberada(s)"));
    Ok(resumo)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn limite_cai_no_inicio_do_dia_utc() {
        // 2026-10-05T15:00:00Z
        let agora = 1_791_212_400_000;
        let limite = limite_ms(agora, 2);
        assert_eq!(limite % DIA_MS, 0);
        assert_eq!(nome_do_dia(limite), "2026-10-03");
        assert!(agora - limite >= 2 * DIA_MS);
        assert!(agora - limite < 3 * DIA_MS);
    }

    #[test]
    fn retencao_minima_e_validada() {
        let mut c = ConfigRetencao::default();
        assert!(c.validar().is_ok());
        c.ciclos_dias = 1;
        assert!(c.validar().is_err());
    }
}
