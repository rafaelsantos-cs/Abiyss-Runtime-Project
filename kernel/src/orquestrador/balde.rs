//! Token bucket ("balde de fichas") guardado no SQLite.
//!
//! Ideia: o balde tem no máximo `capacidade` fichas e ganha
//! `recarga_por_segundo` fichas por segundo. Cada requisição gasta 1 ficha.
//! Sem ficha, espera. Com `capacidade = 1` e 40 req/min, isso dá no máximo
//! uma requisição a cada 1,5 s, sem rajadas.
//!
//! Por que no SQLite e não na memória? Porque `abiyss chat` e
//! `abiyss daemon` são processos diferentes usando a MESMA chave. Se cada
//! um tivesse seu balde, juntos passariam do limite.
//!
//! O balde também guarda `bloqueado_ate_ms`: quando o NIM responde 429 com
//! Retry-After, o pool inteiro fica parado até aquele instante.

use std::time::Duration;

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use crate::db::Banco;

/// Parâmetros de um balde.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigBalde {
    /// Nome (chave primária na tabela `baldes`).
    pub nome: String,
    pub capacidade: f64,
    pub recarga_por_segundo: f64,
}

impl ConfigBalde {
    /// Balde para `limite` requisições por minuto, aceitando rajadas de até `rajada`.
    pub fn por_minuto(nome: &str, limite: u32, rajada: u32) -> ConfigBalde {
        let limite = limite.max(1);
        ConfigBalde {
            nome: nome.to_string(),
            capacidade: rajada.clamp(1, limite) as f64,
            recarga_por_segundo: limite as f64 / 60.0,
        }
    }
}

/// Estado de um balde como está no banco.
#[derive(Debug, Clone, Copy, PartialEq)]
struct EstadoBalde {
    tokens: f64,
    atualizado_ms: i64,
    bloqueado_ate_ms: i64,
}

/// Quantas fichas o balde tem `agora`, somando a recarga desde a última vez.
fn fichas_agora(estado: &EstadoBalde, config: &ConfigBalde, agora_ms: i64) -> f64 {
    // `max(0)`: se o relógio voltar no tempo, não "desrecarrega".
    let segundos = (agora_ms - estado.atualizado_ms).max(0) as f64 / 1000.0;
    (estado.tokens + segundos * config.recarga_por_segundo).min(config.capacidade)
}

/// Quanto falta para o balde ter 1 ficha (zero se já tem).
fn espera_por_uma_ficha(fichas: f64, config: &ConfigBalde) -> Duration {
    if fichas >= 1.0 {
        Duration::ZERO
    } else {
        Duration::from_secs_f64((1.0 - fichas) / config.recarga_por_segundo)
    }
}

fn ler_ou_criar(
    transacao: &Transaction<'_>,
    config: &ConfigBalde,
    agora_ms: i64,
) -> rusqlite::Result<EstadoBalde> {
    let existente = transacao
        .query_row(
            "SELECT tokens, atualizado_ms, bloqueado_ate_ms FROM baldes WHERE nome = ?1",
            params![config.nome],
            |linha| {
                Ok(EstadoBalde {
                    tokens: linha.get(0)?,
                    atualizado_ms: linha.get(1)?,
                    bloqueado_ate_ms: linha.get(2)?,
                })
            },
        )
        .optional()?;
    match existente {
        Some(estado) => Ok(estado),
        None => {
            // Balde novo começa cheio.
            let estado = EstadoBalde {
                tokens: config.capacidade,
                atualizado_ms: agora_ms,
                bloqueado_ate_ms: 0,
            };
            transacao.execute(
                "INSERT INTO baldes (nome, tokens, atualizado_ms, bloqueado_ate_ms)
                 VALUES (?1, ?2, ?3, 0)",
                params![config.nome, estado.tokens, estado.atualizado_ms],
            )?;
            Ok(estado)
        }
    }
}

/// Tenta tirar 1 ficha de CADA balde da lista, tudo ou nada.
///
/// - `Ok(None)`: conseguiu; pode fazer a requisição.
/// - `Ok(Some(espera))`: não conseguiu; tente de novo depois de `espera`.
///
/// "Tudo ou nada" importa no pool do cérebro: chamadas autônomas precisam
/// de uma ficha do balde autônomo E uma do balde total.
pub fn tentar_pegar(
    banco: &Banco,
    baldes: &[ConfigBalde],
    agora_ms: i64,
) -> anyhow::Result<Option<Duration>> {
    let mut conexao = banco.conexao();
    // IMMEDIATE: trava para escrita já no início, assim dois processos
    // não leem o mesmo valor e gastam a mesma ficha.
    let transacao = conexao.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let mut fichas_por_balde = Vec::new();
    let mut maior_espera = Duration::ZERO;
    for config in baldes {
        let estado = ler_ou_criar(&transacao, config, agora_ms)?;
        let fichas = fichas_agora(&estado, config, agora_ms);
        if estado.bloqueado_ate_ms > agora_ms {
            let bloqueio = Duration::from_millis((estado.bloqueado_ate_ms - agora_ms) as u64);
            maior_espera = maior_espera.max(bloqueio);
        }
        maior_espera = maior_espera.max(espera_por_uma_ficha(fichas, config));
        fichas_por_balde.push((config, fichas));
    }

    if !maior_espera.is_zero() {
        // Não gastamos nada. (O commit só grava baldes recém-criados.)
        transacao.commit()?;
        return Ok(Some(maior_espera));
    }

    for (config, fichas) in fichas_por_balde {
        transacao.execute(
            "UPDATE baldes SET tokens = ?1, atualizado_ms = ?2 WHERE nome = ?3",
            params![fichas - 1.0, agora_ms, config.nome],
        )?;
    }
    transacao.commit()?;
    Ok(None)
}

/// Bloqueia o balde até `ate_ms` (usado quando o NIM manda 429).
/// Se já houver um bloqueio mais longo, ele é mantido.
pub fn bloquear(banco: &Banco, config: &ConfigBalde, ate_ms: i64) -> anyhow::Result<()> {
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ler_ou_criar(&transacao, config, ate_ms)?;
    transacao.execute(
        "UPDATE baldes SET bloqueado_ate_ms = MAX(bloqueado_ate_ms, ?1) WHERE nome = ?2",
        params![ate_ms, config.nome],
    )?;
    transacao.commit()?;
    Ok(())
}

/// Foto de um balde, para o `abiyss status` e a interocepção.
#[derive(Debug, Clone)]
pub struct FotoBalde {
    pub nome: String,
    pub fichas: f64,
    pub capacidade: f64,
    pub bloqueado_por: Duration,
}

pub fn fotografar(banco: &Banco, config: &ConfigBalde, agora_ms: i64) -> anyhow::Result<FotoBalde> {
    let conexao = banco.conexao();
    let estado = conexao
        .query_row(
            "SELECT tokens, atualizado_ms, bloqueado_ate_ms FROM baldes WHERE nome = ?1",
            params![config.nome],
            |linha| {
                Ok(EstadoBalde {
                    tokens: linha.get(0)?,
                    atualizado_ms: linha.get(1)?,
                    bloqueado_ate_ms: linha.get(2)?,
                })
            },
        )
        .optional()?;
    let (fichas, bloqueado_por) = match estado {
        Some(e) => (
            fichas_agora(&e, config, agora_ms),
            Duration::from_millis((e.bloqueado_ate_ms - agora_ms).max(0) as u64),
        ),
        None => (config.capacidade, Duration::ZERO),
    };
    Ok(FotoBalde {
        nome: config.nome.clone(),
        fichas,
        capacidade: config.capacidade,
        bloqueado_por,
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    fn balde(limite: u32, rajada: u32) -> ConfigBalde {
        ConfigBalde::por_minuto("teste", limite, rajada)
    }

    /// Atalho: tenta pegar ficha de um único balde no instante `t` (ms).
    fn pegar(banco: &Banco, b: &ConfigBalde, t: i64) -> Option<Duration> {
        tentar_pegar(banco, std::slice::from_ref(b), t).unwrap()
    }

    #[test]
    fn balde_novo_comeca_cheio_e_esvazia() {
        let banco = Banco::em_memoria().unwrap();
        let b = balde(60, 2); // 1 ficha/s, até 2 acumuladas
        assert_eq!(pegar(&banco, &b, 0), None);
        assert_eq!(pegar(&banco, &b, 0), None);
        // Terceira no mesmo instante: precisa esperar 1 s.
        assert_eq!(pegar(&banco, &b, 0), Some(Duration::from_secs(1)));
        // Meio segundo depois: falta meio segundo.
        assert_eq!(pegar(&banco, &b, 500), Some(Duration::from_millis(500)));
        assert_eq!(pegar(&banco, &b, 1000), None);
    }

    #[test]
    fn recarga_nao_passa_da_capacidade() {
        let banco = Banco::em_memoria().unwrap();
        let b = balde(60, 1);
        assert_eq!(pegar(&banco, &b, 0), None);
        // Uma hora parado continua dando só 1 ficha (capacidade 1).
        let uma_hora = 3_600_000;
        assert_eq!(pegar(&banco, &b, uma_hora), None);
        assert!(pegar(&banco, &b, uma_hora).is_some());
    }

    #[test]
    fn quarenta_por_minuto_no_maximo() {
        let banco = Banco::em_memoria().unwrap();
        let b = balde(40, 1);
        // Simula 60 s pedindo ficha a cada 10 ms: só ~40 passam.
        let mut aceitas = 0;
        for t in (0..60_000).step_by(10) {
            if pegar(&banco, &b, t).is_none() {
                aceitas += 1;
            }
        }
        assert!((40..=41).contains(&aceitas), "aceitas = {aceitas}");
    }

    #[test]
    fn tudo_ou_nada_entre_dois_baldes() {
        let banco = Banco::em_memoria().unwrap();
        let total = balde(60, 1);
        let autonomo = ConfigBalde::por_minuto("autonomo", 30, 1);
        // Gasta a ficha do total sozinho.
        assert_eq!(pegar(&banco, &total, 0), None);
        // Pedido duplo falha e NÃO gasta a ficha do autônomo.
        assert!(
            tentar_pegar(&banco, &[autonomo.clone(), total.clone()], 0)
                .unwrap()
                .is_some()
        );
        assert_eq!(pegar(&banco, &autonomo, 0), None);
    }

    #[test]
    fn bloqueio_de_429_segura_o_balde() {
        let banco = Banco::em_memoria().unwrap();
        let b = balde(6000, 10);
        bloquear(&banco, &b, 5_000).unwrap();
        assert_eq!(pegar(&banco, &b, 1_000), Some(Duration::from_secs(4)));
        // Bloqueio menor não encurta o maior.
        bloquear(&banco, &b, 2_000).unwrap();
        assert!(pegar(&banco, &b, 4_000).is_some());
        assert_eq!(pegar(&banco, &b, 5_000), None);
    }
}
