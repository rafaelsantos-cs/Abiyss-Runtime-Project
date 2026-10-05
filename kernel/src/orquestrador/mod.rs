//! Orquestrador de chamadas ao NIM, com DOIS pools separados.
//!
//! - **Pool do cérebro**: exclusivo do Abiyss, com a chave própria. Tem uma
//!   fatia por minuto reservada para conversa com o usuário: as chamadas
//!   autônomas (daemon) nunca passam de `limite - reserva` por minuto.
//! - **Pool dos sub-agentes**: outra chave, fila com prioridade
//!   (Ultra > Medium > Low) e limite de chamadas simultâneas por nível.
//!
//! Um pool NUNCA usa a capacidade do outro: cada um tem os seus baldes,
//! a sua fila e o seu cliente HTTP.
//!
//! Em toda chamada:
//! 1. espera a vez na fila de prioridade e uma ficha do token bucket;
//! 2. chama o NIM;
//! 3. em 429 respeita o Retry-After (bloqueando o pool inteiro); em erros
//!    temporários faz backoff exponencial; registra cada tentativa no banco.

pub mod balde;
pub mod fila;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use rusqlite::params;
use tokio::sync::Semaphore;

use crate::config::{self, Config, ConfigRetentativas};
use crate::db::Banco;
use crate::nim::{ClienteNim, ErroNim, EventoStream, PedidoChat, RespostaModelo};
use crate::tempo::agora_ms;
use balde::ConfigBalde;
use fila::FilaPrioridade;

/// Nomes dos baldes na tabela `baldes`.
pub const BALDE_CEREBRO: &str = "cerebro";
pub const BALDE_CEREBRO_AUTONOMO: &str = "cerebro_autonomo";
pub const BALDE_SUBAGENTES: &str = "subagentes";

/// Quem está usando o pool do cérebro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origem {
    /// Conversa com o usuário (`abiyss chat`): tem a fatia reservada.
    Conversa,
    /// Trabalho autônomo (heartbeat do daemon).
    Autonomo,
}

impl Origem {
    fn prioridade(&self) -> u8 {
        match self {
            Origem::Conversa => 2,
            Origem::Autonomo => 1,
        }
    }

    pub fn como_texto(&self) -> &'static str {
        match self {
            Origem::Conversa => "conversa",
            Origem::Autonomo => "autonomo",
        }
    }
}

/// Nível de um sub-agente. A ordem de prioridade é Ultra > Medium > Low.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Nivel {
    Ultra,
    Medium,
    Low,
}

impl Nivel {
    fn prioridade(&self) -> u8 {
        match self {
            Nivel::Ultra => 3,
            Nivel::Medium => 2,
            Nivel::Low => 1,
        }
    }

    pub fn como_texto(&self) -> &'static str {
        match self {
            Nivel::Ultra => "ultra",
            Nivel::Medium => "medium",
            Nivel::Low => "low",
        }
    }

    pub fn de_texto(texto: &str) -> Option<Nivel> {
        match texto.trim().to_lowercase().as_str() {
            "ultra" => Some(Nivel::Ultra),
            "medium" => Some(Nivel::Medium),
            "low" => Some(Nivel::Low),
            _ => None,
        }
    }
}

/// Tipo do callback de streaming (recebe cada pedaço de texto).
pub type AoReceber<'a> = Option<&'a mut (dyn FnMut(EventoStream) + Send)>;

/// O que os dois pools têm em comum: cliente, fila, banco e retentativas.
struct NucleoPool {
    nome: &'static str,
    cliente: ClienteNim,
    banco: Banco,
    fila: Arc<FilaPrioridade>,
    retentativas: ConfigRetentativas,
    /// Balde que é bloqueado quando chega um 429.
    balde_principal: ConfigBalde,
}

impl NucleoPool {
    /// Espera a vez na fila e pega 1 ficha de cada balde da lista.
    async fn esperar_ficha(&self, prioridade: u8, baldes: &[ConfigBalde]) -> anyhow::Result<()> {
        let lugar = self.fila.entrar(prioridade);
        loop {
            self.fila.esperar_a_vez(&lugar).await;
            match balde::tentar_pegar(&self.banco, baldes, agora_ms())? {
                // Pegou. Ao sair da função, `lugar` é destruído e o próximo da fila acorda.
                None => return Ok(()),
                // Sem ficha: dorme o necessário (+1 ms de folga contra arredondamento).
                Some(espera) => tokio::time::sleep(espera + Duration::from_millis(1)).await,
            }
        }
    }

    /// Espera antes da tentativa `tentativa + 1` (exponencial, com sorteio).
    fn backoff(&self, tentativa: u32) -> Duration {
        let r = &self.retentativas;
        let expoente = (tentativa.saturating_sub(1)).min(20);
        let base = r
            .backoff_inicial_ms
            .saturating_mul(1u64 << expoente)
            .min(r.backoff_maximo_ms);
        // "Jitter": entre 50% e 100% da espera, para vários clientes não
        // tentarem todos ao mesmo tempo.
        let fator = 0.5 + fastrand::f64() * 0.5;
        Duration::from_millis((base as f64 * fator) as u64)
    }

    /// Faz a chamada completa: fila → ficha → NIM → retentativas.
    async fn executar(
        &self,
        prioridade: u8,
        baldes: &[ConfigBalde],
        origem: &str,
        pedido: &PedidoChat,
        mut ao_receber: AoReceber<'_>,
    ) -> anyhow::Result<RespostaModelo> {
        let maximo = self.retentativas.max_tentativas.max(1);
        let mut tentativa = 0;
        loop {
            tentativa += 1;
            self.esperar_ficha(prioridade, baldes).await?;

            let inicio = Instant::now();
            let resultado = match ao_receber.as_mut() {
                Some(callback) => self.cliente.completar_stream(pedido, *callback).await,
                None => self.cliente.completar(pedido).await,
            };
            self.registrar(
                origem,
                &pedido.model,
                tentativa,
                &resultado,
                inicio.elapsed(),
            );

            let erro = match resultado {
                Ok(resposta) => return Ok(resposta),
                Err(erro) => erro,
            };
            if !erro.eh_retentavel() || tentativa >= maximo {
                return Err(anyhow::Error::new(erro).context(format!(
                    "chamada ao NIM falhou (pool {}, tentativa {tentativa}/{maximo})",
                    self.nome
                )));
            }

            let espera = erro
                .retry_after()
                .unwrap_or_else(|| self.backoff(tentativa));
            if erro.eh_limite_de_taxa() {
                // 429: o limite é do pool todo, então todo mundo espera.
                let ate = agora_ms() + espera.as_millis() as i64;
                balde::bloquear(&self.banco, &self.balde_principal, ate)?;
            }
            tracing::warn!(
                pool = self.nome,
                tentativa,
                espera_ms = espera.as_millis() as u64,
                "erro temporário do NIM, tentando de novo: {erro}"
            );
            tokio::time::sleep(espera).await;
        }
    }

    /// Guarda a tentativa em `chamadas_modelo`. Falha aqui não derruba a chamada.
    fn registrar(
        &self,
        origem: &str,
        modelo: &str,
        tentativa: u32,
        resultado: &Result<RespostaModelo, ErroNim>,
        duracao: Duration,
    ) {
        let (status, http, entrada, saida) = match resultado {
            Ok(r) => ("ok", None, r.uso.prompt_tokens, r.uso.completion_tokens),
            Err(ErroNim::LimiteDeTaxa { .. }) => ("limite", Some(429), 0, 0),
            Err(ErroNim::Http { status, .. }) => ("erro", Some(*status as i64), 0, 0),
            Err(_) => ("erro", None, 0, 0),
        };
        let gravou = self.banco.conexao().execute(
            "INSERT INTO chamadas_modelo
               (momento_ms, pool, origem, modelo, tentativa, status, http_status,
                tokens_entrada, tokens_saida, duracao_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                agora_ms(),
                self.nome,
                origem,
                modelo,
                tentativa,
                status,
                http,
                entrada as i64,
                saida as i64,
                duracao.as_millis() as i64
            ],
        );
        if let Err(e) = gravou {
            tracing::warn!("não consegui registrar a chamada no banco: {e}");
        }
    }
}

/// Pool exclusivo do Abiyss.
pub struct PoolCerebro {
    nucleo: NucleoPool,
    balde_total: ConfigBalde,
    balde_autonomo: ConfigBalde,
}

impl PoolCerebro {
    /// Chama o modelo pelo pool do cérebro.
    /// `ao_receber = Some(...)` liga o streaming.
    pub async fn chamar(
        &self,
        origem: Origem,
        pedido: &PedidoChat,
        ao_receber: AoReceber<'_>,
    ) -> anyhow::Result<RespostaModelo> {
        // Conversa só precisa do balde total. Autônomo precisa também do
        // balde autônomo, que é menor: é isso que garante a reserva.
        let baldes = match origem {
            Origem::Conversa => vec![self.balde_total.clone()],
            Origem::Autonomo => vec![self.balde_autonomo.clone(), self.balde_total.clone()],
        };
        self.nucleo
            .executar(
                origem.prioridade(),
                &baldes,
                origem.como_texto(),
                pedido,
                ao_receber,
            )
            .await
    }

    pub fn baldes(&self) -> Vec<ConfigBalde> {
        vec![self.balde_total.clone(), self.balde_autonomo.clone()]
    }
}

/// Pool compartilhado pelos sub-agentes.
pub struct PoolSubagentes {
    nucleo: NucleoPool,
    balde: ConfigBalde,
    vagas_ultra: Arc<Semaphore>,
    vagas_medium: Arc<Semaphore>,
    vagas_low: Arc<Semaphore>,
}

impl PoolSubagentes {
    /// Chama o modelo pelo pool dos sub-agentes, no nível indicado.
    pub async fn chamar(
        &self,
        nivel: Nivel,
        pedido: &PedidoChat,
        ao_receber: AoReceber<'_>,
    ) -> anyhow::Result<RespostaModelo> {
        let vagas = match nivel {
            Nivel::Ultra => &self.vagas_ultra,
            Nivel::Medium => &self.vagas_medium,
            Nivel::Low => &self.vagas_low,
        };
        // A vaga fica presa durante a chamada inteira (inclusive retentativas).
        let _vaga = vagas.acquire().await.context("semáforo do pool fechado")?;
        self.nucleo
            .executar(
                nivel.prioridade(),
                std::slice::from_ref(&self.balde),
                nivel.como_texto(),
                pedido,
                ao_receber,
            )
            .await
    }

    pub fn baldes(&self) -> Vec<ConfigBalde> {
        vec![self.balde.clone()]
    }
}

/// Os dois pools juntos. `Clone` é barato (só copia os `Arc`).
#[derive(Clone)]
pub struct Orquestrador {
    pub cerebro: Arc<PoolCerebro>,
    pub subagentes: Arc<PoolSubagentes>,
}

impl Orquestrador {
    /// Monta o orquestrador lendo as chaves das variáveis de ambiente
    /// indicadas no `abiyss.toml`.
    pub fn da_config(config: &Config, banco: Banco) -> anyhow::Result<Orquestrador> {
        let chave_cerebro = config::ler_chave(&config.pools.cerebro.api_key_env)?;
        let chave_sub = config::ler_chave(&config.pools.subagentes.api_key_env)?;
        Orquestrador::novo(config, banco, &chave_cerebro, &chave_sub)
    }

    /// Monta o orquestrador com chaves explícitas (usado nos testes).
    pub fn novo(
        config: &Config,
        banco: Banco,
        chave_cerebro: &str,
        chave_subagentes: &str,
    ) -> anyhow::Result<Orquestrador> {
        let conexao = Duration::from_secs(config.nim.timeout_conexao_segundos);
        let leitura = Duration::from_secs(config.nim.timeout_leitura_segundos);
        let c = &config.pools.cerebro;
        let s = &config.pools.subagentes;

        let balde_total =
            ConfigBalde::por_minuto(BALDE_CEREBRO, c.requisicoes_por_minuto, c.rajada);
        let balde_autonomo = ConfigBalde::por_minuto(
            BALDE_CEREBRO_AUTONOMO,
            c.requisicoes_por_minuto - c.reserva_conversa_por_minuto,
            c.rajada,
        );
        let cerebro = PoolCerebro {
            nucleo: NucleoPool {
                nome: "cerebro",
                cliente: ClienteNim::novo(&config.nim.base_url, chave_cerebro, conexao, leitura)?,
                banco: banco.clone(),
                fila: FilaPrioridade::nova(),
                retentativas: c.retentativas.clone(),
                balde_principal: balde_total.clone(),
            },
            balde_total,
            balde_autonomo,
        };

        let balde_sub =
            ConfigBalde::por_minuto(BALDE_SUBAGENTES, s.requisicoes_por_minuto, s.rajada);
        let subagentes = PoolSubagentes {
            nucleo: NucleoPool {
                nome: "subagentes",
                cliente: ClienteNim::novo(
                    &config.nim.base_url,
                    chave_subagentes,
                    conexao,
                    leitura,
                )?,
                banco,
                fila: FilaPrioridade::nova(),
                retentativas: s.retentativas.clone(),
                balde_principal: balde_sub.clone(),
            },
            balde: balde_sub,
            vagas_ultra: Arc::new(Semaphore::new(s.concorrencia.ultra as usize)),
            vagas_medium: Arc::new(Semaphore::new(s.concorrencia.medium as usize)),
            vagas_low: Arc::new(Semaphore::new(s.concorrencia.low as usize)),
        };

        Ok(Orquestrador {
            cerebro: Arc::new(cerebro),
            subagentes: Arc::new(subagentes),
        })
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn backoff_dobra_e_respeita_o_teto() {
        let nucleo = NucleoPool {
            nome: "t",
            cliente: ClienteNim::novo(
                "http://x/v1",
                "k",
                Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .unwrap(),
            banco: Banco::em_memoria().unwrap(),
            fila: FilaPrioridade::nova(),
            retentativas: ConfigRetentativas {
                max_tentativas: 10,
                backoff_inicial_ms: 100,
                backoff_maximo_ms: 1_000,
            },
            balde_principal: ConfigBalde::por_minuto("t", 60, 1),
        };
        for _ in 0..50 {
            let b1 = nucleo.backoff(1).as_millis();
            let b2 = nucleo.backoff(2).as_millis();
            let b9 = nucleo.backoff(9).as_millis();
            assert!((50..=100).contains(&b1), "b1 = {b1}");
            assert!((100..=200).contains(&b2), "b2 = {b2}");
            assert!((500..=1000).contains(&b9), "b9 = {b9}");
        }
    }

    #[test]
    fn nivel_de_texto() {
        assert_eq!(Nivel::de_texto(" Ultra "), Some(Nivel::Ultra));
        assert_eq!(Nivel::de_texto("low"), Some(Nivel::Low));
        assert_eq!(Nivel::de_texto("max"), None);
        assert!(Nivel::Ultra.prioridade() > Nivel::Medium.prioridade());
        assert!(Origem::Conversa.prioridade() > Origem::Autonomo.prioridade());
    }
}
