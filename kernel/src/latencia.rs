//! Latência das chamadas ao modelo, lida do registro `chamadas_modelo`.
//!
//! O orquestrador grava, em toda tentativa, o tempo até o primeiro token
//! (`primeiro_token_ms`), a duração total (`duracao_ms`) e o nível da
//! tabela de esforço (`nivel_esforco`). Aqui só lemos e resumimos em
//! percentis (p50 e p95) por modelo e por pool, e por nível de esforço
//! dentro de cada um — para o `abiyss status` (calibrar o esforço contra a
//! latência na VM). Só tentativas com sucesso entram nos percentis; as
//! falhas são contadas à parte (a duração de um erro não é latência do modelo).

use std::collections::BTreeMap;

use rusqlite::params;

use crate::db::Banco;
use crate::esforco::NivelEsforco;

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

/// Resumo de um nível de esforço dentro de um par (modelo, pool).
#[derive(Debug, Clone, PartialEq)]
pub struct LatenciaEsforco {
    pub modelo: String,
    pub pool: String,
    /// `None` = chamada sem nível (ex.: `testar-nim`, ou anterior à migração 14).
    pub nivel: Option<NivelEsforco>,
    /// O nível era a confirmar: os campos dele não foram enviados (o pedido
    /// usou os parâmetros padrão do modelo). Fica num grupo separado.
    pub a_confirmar: bool,
    pub sucessos: usize,
    pub falhas: usize,
    pub primeiro_token: Percentis,
    pub total: Percentis,
}

/// Resume as chamadas feitas a partir de `desde_ms` por (modelo, pool,
/// nível de esforço), em ordem de modelo, pool e nível (do mais leve ao
/// mais pesado; sem nível por último).
pub fn resumo_por_esforco_desde(
    banco: &Banco,
    desde_ms: i64,
) -> anyhow::Result<Vec<LatenciaEsforco>> {
    // Chave de ordenação: o nível pela ordem da tabela, "sem nível" no fim.
    type Chave = (String, String, usize, bool);
    #[derive(Default)]
    struct Grupo {
        falhas: usize,
        primeiro_token: Vec<i64>,
        total: Vec<i64>,
    }
    let mut grupos: BTreeMap<Chave, Grupo> = BTreeMap::new();
    {
        let conexao = banco.conexao();
        let mut consulta = conexao.prepare(
            "SELECT modelo, pool, nivel_esforco, esforco_confirmado, status,
                    primeiro_token_ms, duracao_ms
               FROM chamadas_modelo
              WHERE momento_ms >= ?1",
        )?;
        let linhas = consulta.query_map(params![desde_ms], |l| {
            Ok((
                l.get::<_, String>(0)?,
                l.get::<_, String>(1)?,
                l.get::<_, Option<String>>(2)?,
                l.get::<_, Option<bool>>(3)?,
                l.get::<_, String>(4)?,
                l.get::<_, Option<i64>>(5)?,
                l.get::<_, i64>(6)?,
            ))
        })?;
        for linha in linhas {
            let (modelo, pool, nivel, confirmado, status, primeiro_token, duracao) = linha?;
            let ordem = nivel
                .as_deref()
                .and_then(NivelEsforco::de_texto)
                .map(|n| n as usize)
                .unwrap_or(NivelEsforco::TODOS.len());
            let a_confirmar = ordem < NivelEsforco::TODOS.len() && confirmado == Some(false);
            let grupo = grupos
                .entry((modelo, pool, ordem, a_confirmar))
                .or_default();
            if status == "ok" {
                grupo.total.push(duracao);
                if let Some(ms) = primeiro_token {
                    grupo.primeiro_token.push(ms);
                }
            } else {
                grupo.falhas += 1;
            }
        }
    }
    Ok(grupos
        .into_iter()
        .map(|((modelo, pool, ordem, a_confirmar), g)| LatenciaEsforco {
            modelo,
            pool,
            nivel: NivelEsforco::TODOS.get(ordem).copied(),
            a_confirmar,
            sucessos: g.total.len(),
            falhas: g.falhas,
            primeiro_token: Percentis::de_amostras(g.primeiro_token),
            total: Percentis::de_amostras(g.total),
        })
        .collect())
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

    #[test]
    fn resumo_por_esforco_separa_niveis_e_placeholders() {
        let banco = Banco::em_memoria().unwrap();
        let inserir = |nivel: Option<&str>, confirmado: Option<bool>, status: &str, total: i64| {
            banco
                .conexao()
                .execute(
                    "INSERT INTO chamadas_modelo (momento_ms, pool, origem, modelo, tentativa,
                        status, duracao_ms, primeiro_token_ms, stream, nivel_esforco,
                        esforco_confirmado)
                     VALUES (1000, 'cerebro', 'teste', 'glm', 1, ?1, ?2, ?3, 0, ?4, ?5)",
                    params![status, total, total / 2, nivel, confirmado],
                )
                .unwrap();
        };
        inserir(Some("high"), Some(true), "ok", 900);
        inserir(Some("medium"), Some(true), "ok", 100);
        inserir(Some("medium"), Some(true), "ok", 300);
        inserir(Some("medium"), Some(true), "erro", 5);
        inserir(Some("xhigh"), Some(false), "ok", 950);
        inserir(None, None, "ok", 50);

        let resumo = resumo_por_esforco_desde(&banco, 500).unwrap();
        // (nível, a confirmar, sucessos, falhas, p95 do total) de cada grupo.
        type Linha = (Option<NivelEsforco>, bool, usize, usize, Option<i64>);
        let linhas: Vec<Linha> = resumo
            .iter()
            .map(|l| (l.nivel, l.a_confirmar, l.sucessos, l.falhas, l.total.p95))
            .collect();
        assert_eq!(
            linhas,
            vec![
                (Some(NivelEsforco::Medium), false, 2, 1, Some(300)),
                (Some(NivelEsforco::High), false, 1, 0, Some(900)),
                (Some(NivelEsforco::Xhigh), true, 1, 0, Some(950)),
                (None, false, 1, 0, Some(50)),
            ]
        );
        assert_eq!(resumo[0].primeiro_token.p50, Some(50));
    }
}
