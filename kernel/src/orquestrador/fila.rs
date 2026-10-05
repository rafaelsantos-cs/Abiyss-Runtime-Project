//! Fila de prioridade para disputar as fichas de um pool.
//!
//! Quem tem prioridade MAIOR passa na frente; empate = ordem de chegada.
//! Só quem está na frente da fila pode tentar pegar ficha do balde.
//!
//! A fila tem tamanho máximo (`pools.<pool>.max_na_fila`): quem chega com
//! a fila cheia recebe erro na hora, em vez de acumular tarefas esperando
//! sem limite.
//!
//! Uso:
//! ```text
//! let lugar = fila.entrar(prioridade)?;  // entra na fila (ou erro: cheia)
//! fila.esperar_a_vez(&lugar).await;      // espera ser o primeiro
//! // ... pega a ficha ...
//! drop(lugar);                           // sai da fila e acorda os outros
//! ```

use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

/// Chave de ordenação: `Reverse` faz a prioridade MAIOR vir primeiro
/// no `BTreeSet`; o número de chegada desempata.
type Chave = (Reverse<u8>, u64);

#[derive(Default)]
struct EstadoFila {
    proximo_numero: u64,
    esperando: BTreeSet<Chave>,
}

pub struct FilaPrioridade {
    estado: Mutex<EstadoFila>,
    /// Máximo de lugares ocupados ao mesmo tempo.
    maximo: usize,
    /// Usado para acordar quem espera quando alguém sai da fila.
    aviso: Notify,
}

/// Um lugar na fila. Ao ser destruído (inclusive se a tarefa for
/// cancelada), sai da fila automaticamente.
pub struct Lugar {
    fila: Arc<FilaPrioridade>,
    chave: Chave,
}

impl Drop for Lugar {
    fn drop(&mut self) {
        self.fila.travar().esperando.remove(&self.chave);
        self.fila.aviso.notify_waiters();
    }
}

/// A fila já tinha `maximo` lugares ocupados.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("fila do pool cheia ({maximo} chamadas esperando)")]
pub struct FilaCheia {
    pub maximo: usize,
}

impl FilaPrioridade {
    /// Fila com no máximo `maximo` lugares (pelo menos 1).
    pub fn nova(maximo: usize) -> Arc<FilaPrioridade> {
        Arc::new(FilaPrioridade {
            estado: Mutex::new(EstadoFila::default()),
            maximo: maximo.max(1),
            aviso: Notify::new(),
        })
    }

    fn travar(&self) -> std::sync::MutexGuard<'_, EstadoFila> {
        self.estado.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Entra na fila com a prioridade dada (erro se estiver cheia).
    pub fn entrar(self: &Arc<Self>, prioridade: u8) -> Result<Lugar, FilaCheia> {
        let mut estado = self.travar();
        if estado.esperando.len() >= self.maximo {
            return Err(FilaCheia {
                maximo: self.maximo,
            });
        }
        let chave = (Reverse(prioridade), estado.proximo_numero);
        estado.proximo_numero += 1;
        estado.esperando.insert(chave);
        Ok(Lugar {
            fila: Arc::clone(self),
            chave,
        })
    }

    /// Este lugar é o primeiro da fila?
    pub fn eh_a_vez(&self, lugar: &Lugar) -> bool {
        self.travar().esperando.first() == Some(&lugar.chave)
    }

    /// Espera até `lugar` ser o primeiro da fila.
    pub async fn esperar_a_vez(&self, lugar: &Lugar) {
        loop {
            // Cria o "ouvido" ANTES de conferir, para não perder um aviso
            // que chegue entre a conferência e o `.await`.
            let aviso = self.aviso.notified();
            if self.eh_a_vez(lugar) {
                return;
            }
            aviso.await;
        }
    }

    /// Quantos estão esperando (para status/testes).
    pub fn tamanho(&self) -> usize {
        self.travar().esperando.len()
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn prioridade_maior_primeiro_e_empate_por_chegada() {
        let fila = FilaPrioridade::nova(10);
        let baixa = fila.entrar(1).unwrap();
        let media_1 = fila.entrar(2).unwrap();
        let media_2 = fila.entrar(2).unwrap();
        let alta = fila.entrar(3).unwrap();
        assert!(fila.eh_a_vez(&alta));
        drop(alta);
        assert!(fila.eh_a_vez(&media_1));
        drop(media_1);
        assert!(fila.eh_a_vez(&media_2));
        drop(media_2);
        assert!(fila.eh_a_vez(&baixa));
        drop(baixa);
        assert_eq!(fila.tamanho(), 0);
    }

    #[test]
    fn fila_cheia_recusa_e_libera_ao_sair() {
        let fila = FilaPrioridade::nova(2);
        let a = fila.entrar(1).unwrap();
        let _b = fila.entrar(1).unwrap();
        assert_eq!(fila.entrar(3).err(), Some(FilaCheia { maximo: 2 }));
        drop(a);
        assert!(fila.entrar(3).is_ok());
    }

    #[tokio::test]
    async fn quem_espera_e_acordado_quando_o_primeiro_sai() {
        let fila = FilaPrioridade::nova(10);
        let primeiro = fila.entrar(5).unwrap();
        let segundo = fila.entrar(1).unwrap();
        let fila2 = Arc::clone(&fila);
        let espera = tokio::spawn(async move {
            fila2.esperar_a_vez(&segundo).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!espera.is_finished());
        drop(primeiro);
        tokio::time::timeout(std::time::Duration::from_secs(1), espera)
            .await
            .expect("deveria ter acordado")
            .unwrap();
    }
}
