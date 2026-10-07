//! Apoio comum aos testes de integração: monta um "projeto" temporário
//! (pasta com identity/ e data/) apontando para o mock do NIM.

#![allow(dead_code)] // Cada arquivo de teste usa só parte destas funções.

use std::path::PathBuf;
use std::sync::Arc;

use abiyss::config::{Config, config_de_teste};
use abiyss::db::Banco;
use abiyss::ferramentas::CaixaDeFerramentas;
use abiyss::nim::mock::MockNim;
use abiyss::orquestrador::Orquestrador;
use tempfile::TempDir;

pub const NUCLEO_DE_TESTE: &str = "Sou o Abiyss de teste. Gosto de respostas curtas.";

pub struct Ambiente {
    /// Mantém a pasta viva enquanto o teste roda (apagada no fim).
    pub pasta: TempDir,
    pub mock: MockNim,
    pub config: Config,
    pub banco: Banco,
    pub orquestrador: Orquestrador,
    pub ferramentas: Arc<CaixaDeFerramentas>,
}

impl Ambiente {
    pub async fn novo() -> Ambiente {
        let pasta = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(pasta.path().join("identity")).unwrap();
        std::fs::write(pasta.path().join("identity/nucleo.md"), NUCLEO_DE_TESTE).unwrap();

        let mock = MockNim::iniciar().await;
        let mut config = config_de_teste(&mock.base_url(), pasta.path());
        // Limites altos: estes testes não são sobre rate limit.
        config.pools.cerebro.requisicoes_por_minuto = 60_000;
        config.pools.cerebro.rajada = 100;
        config.pools.subagentes.requisicoes_por_minuto = 60_000;
        config.pools.subagentes.rajada = 100;
        for r in [
            &mut config.pools.cerebro.retentativas,
            &mut config.pools.subagentes.retentativas,
        ] {
            r.backoff_inicial_ms = 10;
            r.backoff_maximo_ms = 50;
        }

        let banco = Banco::abrir(&config.caminho_banco()).unwrap();
        let orquestrador =
            Orquestrador::novo(&config, banco.clone(), "nvapi-cerebro", "nvapi-sub").unwrap();
        let ferramentas = Arc::new(CaixaDeFerramentas::da_config(&config).unwrap());
        Ambiente {
            pasta,
            mock,
            config,
            banco,
            orquestrador,
            ferramentas,
        }
    }

    pub fn caminho(&self, relativo: &str) -> PathBuf {
        self.pasta.path().join(relativo)
    }
}

/// Espera o relógio de parede passar do milissegundo `ms`. O kernel compara
/// instantes em milissegundos (`agora_ms`): o que o teste fizer depois desta
/// espera fica com um instante maior que `ms`, nunca igual, seja qual for a
/// velocidade da máquina.
pub async fn esperar_o_relogio_passar(ms: i64) {
    while abiyss::tempo::agora_ms() <= ms {
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
}
