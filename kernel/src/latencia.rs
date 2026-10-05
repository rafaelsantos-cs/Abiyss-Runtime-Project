//! Latência das chamadas ao modelo, lida do registro `chamadas_modelo`.
//!
//! O orquestrador grava, em toda tentativa, o tempo até o primeiro token
//! (`primeiro_token_ms`) e a duração total (`duracao_ms`). Aqui só lemos e
//! resumimos em percentis (p50 e p95) por modelo e por pool — para o
//! `abiyss status`. Só tentativas com sucesso entram nos percentis; as
//! falhas são contadas à parte (a duração de um erro não é latência do modelo).

use rusqlite::params;

use crate::db::Banco;

/// Janela padrão do `abiyss status`.
pub const JANELA_24H_MS: i64 = 24 * 60 * 60 * 1000;

/// Resumo de um par (modelo, pool) dentro da janela.
#[derive(Debug, Clone, PartialEq)]
pub struct LatenciaModelo {
    pub modelo: String,
    pub pool: String,
    /// Tentativas com sucesso (as que entram nos percentis).
    pub sucessos: usize,
    /// Tentativas que falharam (429, HTTP, rede...).
    pub falhas: usize,
    pub primeiro_token: Percentis,
    pub total: Percentis,
}

/// p50 e p95 em milissegundos (`None` se não houver amostras).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Percentis {
    pub p50: Option<i64>,
    pub p95: Option<i64>,
}

impl Percentis {
    /// Calcula a partir de amostras em qualquer ordem.
    pub fn de_amostras(mut amostras: Vec<i64>) -> Percentis {
        amostras.sort_unstable();
        Percentis {
            p50: percentil(&amostras, 50.0),
            p95: percentil(&amostras, 95.0),
        }
    }
}

/// Percentil pelo método do "posto mais próximo" (nearest rank): sempre
/// devolve um valor que realmente aconteceu. `ordenados` precisa estar em
/// ordem crescente.
pub fn percentil(ordenados: &[i64], p: f64) -> Option<i64> {
    if ordenados.is_empty() {
        return None;
    }
    let posto = ((p / 100.0) * ordenados.len() as f64).ceil() as usize;
    let indice = posto.clamp(1, ordenados.len()) - 1;
    Some(ordenados[indice])
}

/// Resume as chamadas feitas a partir de `desde_ms`, por (modelo, pool),
/// ordenado por modelo e depois pool.
pub fn resumo_desde(banco: &Banco, desde_ms: i64) -> anyhow::Result<Vec<LatenciaModelo>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT modelo, pool, status, primeiro_token_ms, duracao_ms
           FROM chamadas_modelo
          WHERE momento_ms >= ?1
          ORDER BY modelo, pool",
    )?;
    let linhas = consulta.query_map(params![desde_ms], |linha| {
        Ok((
            linha.get::<_, String>(0)?,
            linha.get::<_, String>(1)?,
            linha.get::<_, String>(2)?,
            linha.get::<_, Option<i64>>(3)?,
            linha.get::<_, i64>(4)?,
        ))
    })?;

    // Agrupa as amostras (as linhas já vêm ordenadas por modelo e pool).
    struct Grupo {
        modelo: String,
        pool: String,
        falhas: usize,
        primeiro_token: Vec<i64>,
        total: Vec<i64>,
    }
    let mut grupos: Vec<Grupo> = Vec::new();
    for linha in linhas {
        let (modelo, pool, status, primeiro_token, duracao) = linha?;
        let mesmo_grupo = grupos
            .last()
            .is_some_and(|g| g.modelo == modelo && g.pool == pool);
        if !mesmo_grupo {
            grupos.push(Grupo {
                modelo,
                pool,
                falhas: 0,
                primeiro_token: Vec::new(),
                total: Vec::new(),
            });
        }
        let grupo = grupos.last_mut().expect("acabamos de garantir um grupo");
        if status == "ok" {
            grupo.total.push(duracao);
            if let Some(ms) = primeiro_token {
                grupo.primeiro_token.push(ms);
            }
        } else {
            grupo.falhas += 1;
        }
    }

    Ok(grupos
        .into_iter()
        .map(|g| LatenciaModelo {
            modelo: g.modelo,
            pool: g.pool,
            sucessos: g.total.len(),
            falhas: g.falhas,
            primeiro_token: Percentis::de_amostras(g.primeiro_token),
            total: Percentis::de_amostras(g.total),
        })
        .collect())
}

/// Texto curto de um valor em ms ("1234 ms", ou "—" sem amostras).
pub fn formatar(ms: Option<i64>) -> String {
    match ms {
        Some(ms) => format!("{ms} ms"),
        None => "—".to_string(),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn percentil_posto_mais_proximo() {
        let v: Vec<i64> = (1..=100).collect();
        assert_eq!(percentil(&v, 50.0), Some(50));
        assert_eq!(percentil(&v, 95.0), Some(95));
        assert_eq!(percentil(&[7], 95.0), Some(7));
        assert_eq!(percentil(&[1, 2], 50.0), Some(1));
        assert_eq!(percentil(&[1, 2], 95.0), Some(2));
        assert_eq!(percentil(&[], 50.0), None);
    }

    #[test]
    fn resumo_agrupa_por_modelo_e_pool_e_ignora_falhas_nos_percentis() {
        let banco = Banco::em_memoria().unwrap();
        let inserir = |momento: i64,
                       modelo: &str,
                       pool: &str,
                       status: &str,
                       ttft: Option<i64>,
                       total: i64| {
            banco
                .conexao()
                .execute(
                    "INSERT INTO chamadas_modelo (momento_ms, pool, origem, modelo, tentativa,
                        status, duracao_ms, primeiro_token_ms, stream)
                     VALUES (?1, ?2, 'teste', ?3, 1, ?4, ?5, ?6, 1)",
                    params![momento, pool, modelo, status, total, ttft],
                )
                .unwrap();
        };
        for i in 1..=20 {
            inserir(1_000, "glm", "cerebro", "ok", Some(i * 10), i * 100);
        }
        inserir(1_000, "glm", "cerebro", "erro", None, 99_999);
        inserir(1_000, "glm", "subagentes", "ok", Some(5), 50);
        // Fora da janela: não conta.
        inserir(10, "glm", "cerebro", "ok", Some(1), 1);

        let resumo = resumo_desde(&banco, 500).unwrap();
        assert_eq!(resumo.len(), 2);
        let cerebro = &resumo[0];
        assert_eq!(
            (cerebro.modelo.as_str(), cerebro.pool.as_str()),
            ("glm", "cerebro")
        );
        assert_eq!(cerebro.sucessos, 20);
        assert_eq!(cerebro.falhas, 1);
        assert_eq!(cerebro.primeiro_token.p50, Some(100));
        assert_eq!(cerebro.primeiro_token.p95, Some(190));
        assert_eq!(cerebro.total.p50, Some(1_000));
        assert_eq!(cerebro.total.p95, Some(1_900));
        assert_eq!(resumo[1].pool, "subagentes");
        assert_eq!(resumo[1].total.p95, Some(50));
    }
}
