//! Interocepção: o Abiyss "sentindo o próprio corpo".
//!
//! Tudo aqui é calculado por CÓDIGO (nunca pelo modelo): data e hora,
//! CPU, memória, disco, tokens gastos e uso dos pools de chamadas.
//! O texto resultante entra no contexto do heartbeat e da conversa.

use std::fmt::Write as _;
use std::path::Path;

use chrono::{Datelike, Local, TimeZone};
use rusqlite::params;
use sysinfo::{Disks, System};

use crate::config::Config;
use crate::db::Banco;
use crate::orquestrador::balde::{self, ConfigBalde};
use crate::orquestrador::{BALDE_CEREBRO, BALDE_SUBAGENTES};
use crate::tempo::agora_ms;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Uso de um pool de chamadas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsoPool {
    pub nome: String,
    pub requisicoes_ultimo_minuto: i64,
    pub limite_por_minuto: u32,
    pub erros_429_ultima_hora: i64,
    pub tokens_entrada_hoje: i64,
    pub tokens_saida_hoje: i64,
    pub tokens_24h: i64,
    /// Fichas disponíveis agora no balde principal.
    pub fichas: f64,
    pub bloqueado_segundos: u64,
}

/// Foto do "corpo" do Abiyss num instante.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Interocepcao {
    /// Data e hora local, já formatada (ex.: "segunda-feira, 2026-10-05 10:30:00 -03:00").
    pub agora: String,
    pub nucleos: usize,
    /// Carga média de 1, 5 e 15 minutos.
    pub carga: (f64, f64, f64),
    pub memoria_total_gib: f64,
    pub memoria_usada_gib: f64,
    pub disco_total_gib: f64,
    pub disco_livre_gib: f64,
    pub pools: Vec<UsoPool>,
    /// Fase do dia (vigília/descanso) e as horas ativas configuradas.
    pub fase: String,
    /// Trabalho autônomo de hoje contra o orçamento diário.
    pub orcamento: String,
    /// Situação do próprio kernel (disjuntor, continuidade...), uma linha
    /// por item; vazio quando não há nada a dizer.
    pub kernel: Vec<String>,
}

/// Nome do dia da semana em português.
fn dia_da_semana(numero_desde_domingo: u32) -> &'static str {
    match numero_desde_domingo {
        0 => "domingo",
        1 => "segunda-feira",
        2 => "terça-feira",
        3 => "quarta-feira",
        4 => "quinta-feira",
        5 => "sexta-feira",
        _ => "sábado",
    }
}

/// Data e hora local com dia da semana e fuso.
pub fn agora_formatado() -> String {
    let agora = Local::now();
    format!(
        "{}, {}",
        dia_da_semana(agora.weekday().num_days_from_sunday()),
        agora.format("%Y-%m-%d %H:%M:%S %:z")
    )
}

/// Meia-noite de hoje (fuso local), em ms.
fn inicio_do_dia_ms() -> i64 {
    let hoje = Local::now().date_naive();
    let meia_noite = hoje.and_hms_opt(0, 0, 0).expect("meia-noite sempre existe");
    Local
        .from_local_datetime(&meia_noite)
        .earliest()
        .map(|d| d.timestamp_millis())
        .unwrap_or_else(agora_ms)
}

/// Disco onde está `caminho` (ponto de montagem mais específico).
pub fn disco_de(caminho: &Path) -> (f64, f64) {
    let caminho = caminho
        .canonicalize()
        .unwrap_or_else(|_| caminho.to_path_buf());
    let discos = Disks::new_with_refreshed_list();
    let melhor = discos
        .list()
        .iter()
        .filter(|d| caminho.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len());
    match melhor {
        Some(d) => (
            d.total_space() as f64 / GIB,
            d.available_space() as f64 / GIB,
        ),
        None => (0.0, 0.0),
    }
}

fn uso_pool(
    banco: &Banco,
    nome: &str,
    balde_principal: &ConfigBalde,
    limite_por_minuto: u32,
) -> anyhow::Result<UsoPool> {
    let agora = agora_ms();
    let hoje = inicio_do_dia_ms();
    let (requisicoes, erros_429, entrada_hoje, saida_hoje, tokens_24h) = {
        let conexao = banco.conexao();
        let contar = |sql: &str, desde: i64| -> rusqlite::Result<i64> {
            conexao.query_row(sql, params![nome, desde], |l| l.get(0))
        };
        (
            contar(
                "SELECT COUNT(*) FROM chamadas_modelo WHERE pool = ?1 AND momento_ms >= ?2",
                agora - 60_000,
            )?,
            contar(
                "SELECT COUNT(*) FROM chamadas_modelo WHERE pool = ?1 AND momento_ms >= ?2 AND status = 'limite'",
                agora - 3_600_000,
            )?,
            contar(
                "SELECT COALESCE(SUM(tokens_entrada), 0) FROM chamadas_modelo WHERE pool = ?1 AND momento_ms >= ?2",
                hoje,
            )?,
            contar(
                "SELECT COALESCE(SUM(tokens_saida), 0) FROM chamadas_modelo WHERE pool = ?1 AND momento_ms >= ?2",
                hoje,
            )?,
            contar(
                "SELECT COALESCE(SUM(tokens_entrada + tokens_saida), 0) FROM chamadas_modelo WHERE pool = ?1 AND momento_ms >= ?2",
                agora - 86_400_000,
            )?,
        )
    };
    let foto = balde::fotografar(banco, balde_principal, agora)?;
    Ok(UsoPool {
        nome: nome.to_string(),
        requisicoes_ultimo_minuto: requisicoes,
        limite_por_minuto,
        erros_429_ultima_hora: erros_429,
        tokens_entrada_hoje: entrada_hoje,
        tokens_saida_hoje: saida_hoje,
        tokens_24h,
        fichas: foto.fichas,
        bloqueado_segundos: foto.bloqueado_por.as_secs(),
    })
}

/// Continuidade: desde quando o daemon está no ar e como foi o último sono.
fn continuidade(banco: &Banco, agora: i64) -> anyhow::Result<Vec<String>> {
    use crate::daemon::{CHAVE_INICIADO, CHAVE_PARADO, ler_estado};
    use crate::tempo::{formatar_duracao, formatar_ms};
    let numero = |chave: &str| -> anyhow::Result<Option<i64>> {
        Ok(ler_estado(banco, chave)?.and_then(|v| v.parse().ok()))
    };
    let mut linhas = Vec::new();
    // No ar = a última execução começou e ainda não parou.
    if let Some(inicio) = numero(CHAVE_INICIADO)?
        && numero(CHAVE_PARADO)?.is_none_or(|p| p < inicio)
    {
        linhas.push(format!(
            "No ar desde {} ({})",
            formatar_ms(inicio),
            formatar_duracao(agora - inicio)
        ));
    }
    if let Some(s) = crate::sono::ultimo(banco)? {
        linhas.push(format!(
            "Último sono: revisão de {}, {} (terminou há {})",
            s.dia,
            s.estado,
            formatar_duracao(agora - s.fim_ms.unwrap_or(s.inicio_ms))
        ));
    }
    if let Some(p) = crate::pedidos::resumo(banco, agora)? {
        linhas.push(p);
    }
    Ok(linhas)
}

impl Interocepcao {
    /// Mede tudo agora. Não chama o modelo nem a rede.
    pub fn medir(config: &Config, banco: &Banco) -> anyhow::Result<Interocepcao> {
        let mut sistema = System::new();
        sistema.refresh_memory();
        let carga = System::load_average();
        let nucleos = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let (disco_total, disco_livre) = disco_de(&config.resolver(&config.caminhos.dados));

        let c = &config.pools.cerebro;
        let s = &config.pools.subagentes;
        let pools = vec![
            uso_pool(
                banco,
                "cerebro",
                &ConfigBalde::por_minuto(BALDE_CEREBRO, c.requisicoes_por_minuto, c.rajada),
                c.requisicoes_por_minuto,
            )?,
            uso_pool(
                banco,
                "subagentes",
                &ConfigBalde::por_minuto(BALDE_SUBAGENTES, s.requisicoes_por_minuto, s.rajada),
                s.requisicoes_por_minuto,
            )?,
        ];

        let fase = format!(
            "{} (horas ativas {})",
            crate::ritmo::fase_agora(&config.ritmo).como_texto(),
            config.ritmo.horas_ativas
        );
        let uso = crate::orcamento::uso_desde(banco, crate::ritmo::inicio_do_dia_local_ms())?;
        let orcamento = crate::orcamento::descrever(&config.orcamento, uso);
        let mut kernel = continuidade(banco, agora_ms())?;
        let disjuntor = crate::vigilancia::ler_disjuntor(banco)?;
        if let Some(d) = crate::vigilancia::descrever_disjuntor(&disjuntor, agora_ms()) {
            kernel.push(format!("Disjuntor do heartbeat: {d}"));
        }

        Ok(Interocepcao {
            fase,
            orcamento,
            kernel,
            agora: agora_formatado(),
            nucleos,
            carga: (carga.one, carga.five, carga.fifteen),
            memoria_total_gib: sistema.total_memory() as f64 / GIB,
            memoria_usada_gib: sistema.used_memory() as f64 / GIB,
            disco_total_gib: disco_total,
            disco_livre_gib: disco_livre,
            pools,
        })
    }

    /// Bloco de texto compacto para o contexto do modelo.
    pub fn como_texto(&self) -> String {
        let mut t = String::new();
        let _ = writeln!(t, "Data e hora: {}", self.agora);
        if !self.fase.is_empty() {
            let _ = writeln!(t, "Fase do dia: {}", self.fase);
        }
        if !self.orcamento.is_empty() {
            let _ = writeln!(t, "Trabalho autônomo hoje: {}", self.orcamento);
        }
        for linha in &self.kernel {
            let _ = writeln!(t, "{linha}");
        }
        let _ = writeln!(
            t,
            "CPU: carga {:.2} / {:.2} / {:.2} (1/5/15 min) em {} núcleo(s)",
            self.carga.0, self.carga.1, self.carga.2, self.nucleos
        );
        let _ = writeln!(
            t,
            "Memória: {:.1} de {:.1} GiB em uso",
            self.memoria_usada_gib, self.memoria_total_gib
        );
        let _ = writeln!(
            t,
            "Disco (dados): {:.1} GiB livres de {:.1} GiB",
            self.disco_livre_gib, self.disco_total_gib
        );
        for p in &self.pools {
            let _ = write!(
                t,
                "Pool {}: {}/{} req no último minuto; tokens hoje {} entrada + {} saída (24h: {})",
                p.nome,
                p.requisicoes_ultimo_minuto,
                p.limite_por_minuto,
                p.tokens_entrada_hoje,
                p.tokens_saida_hoje,
                p.tokens_24h
            );
            if p.erros_429_ultima_hora > 0 {
                let _ = write!(
                    t,
                    "; {} limite(s) 429 na última hora",
                    p.erros_429_ultima_hora
                );
            }
            if p.bloqueado_segundos > 0 {
                let _ = write!(t, "; BLOQUEADO por mais {} s", p.bloqueado_segundos);
            }
            t.push('\n');
        }
        t.trim_end().to_string()
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::config::config_de_teste;

    #[test]
    fn mede_sem_rede_e_conta_tokens_por_pool() {
        let pasta = tempfile::tempdir().unwrap();
        let config = config_de_teste("http://127.0.0.1:9/v1", pasta.path());
        let banco = Banco::em_memoria().unwrap();
        let agora = agora_ms();
        for (pool, status, entrada, saida) in [
            ("cerebro", "ok", 100, 50),
            ("cerebro", "limite", 0, 0),
            ("subagentes", "ok", 10, 5),
        ] {
            banco
                .conexao()
                .execute(
                    "INSERT INTO chamadas_modelo (momento_ms, pool, origem, modelo, tentativa, status,
                       tokens_entrada, tokens_saida, duracao_ms) VALUES (?1, ?2, 'x', 'm', 1, ?3, ?4, ?5, 1)",
                    params![agora, pool, status, entrada, saida],
                )
                .unwrap();
        }
        let i = Interocepcao::medir(&config, &banco).unwrap();
        assert!(i.nucleos >= 1);
        assert!(i.memoria_total_gib > 0.0);
        let cerebro = &i.pools[0];
        assert_eq!(cerebro.requisicoes_ultimo_minuto, 2);
        assert_eq!(cerebro.erros_429_ultima_hora, 1);
        assert_eq!(cerebro.tokens_entrada_hoje, 100);
        assert_eq!(cerebro.tokens_24h, 150);
        assert_eq!(i.pools[1].tokens_saida_hoje, 5);

        let texto = i.como_texto();
        assert!(texto.starts_with("Data e hora: "));
        assert!(texto.contains("Pool cerebro: 2/40 req"));
        assert!(texto.contains("1 limite(s) 429"));
        assert!(texto.contains("Fase do dia: vigília"));
        assert!(texto.contains("Trabalho autônomo hoje:"));
    }

    #[test]
    fn dia_da_semana_em_portugues() {
        assert_eq!(dia_da_semana(0), "domingo");
        assert_eq!(dia_da_semana(3), "quarta-feira");
        assert!(agora_formatado().contains(':'));
    }
}
