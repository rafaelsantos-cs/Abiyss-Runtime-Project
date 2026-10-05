//! Fila de prioridade para disputar as fichas de um pool.
//!
//! Quem tem prioridade MAIOR passa na frente; empate = ordem de chegada.
//! Só quem está na frente da fila pode tentar pegar ficha do balde.
//!
//! Uso:
//! ```text
//! let lugar = fila.entrar(prioridade);   // entra na fila
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

#[derive(Default)]
pub struct FilaPrioridade {
    estado: Mutex<EstadoFila>,
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

impl FilaPrioridade {
    pub fn nova() -> Arc<FilaPrioridade> {
        Arc::new(FilaPrioridade::default())
    }

    fn travar(&self) -> std::sync::MutexGuard<'_, EstadoFila> {
        self.estado.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Entra na fila com a prioridade dada.
    pub fn entrar(self: &Arc<Self>, prioridade: u8) -> Lugar {
        let mut estado = self.travar();
        let chave = (Reverse(prioridade), estado.proximo_numero);
        estado.proximo_numero += 1;
        estado.esperando.insert(chave);
        Lugar {
            fila: Arc::clone(self),
            chave,
        }
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
        let fila = FilaPrioridade::nova();
        let baixa = fila.entrar(1);
        let media_1 = fila.entrar(2);
        let media_2 = fila.entrar(2);
        let alta = fila.entrar(3);
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

    #[tokio::test]
    async fn quem_espera_e_acordado_quando_o_primeiro_sai() {
        let fila = FilaPrioridade::nova();
        let primeiro = fila.entrar(5);
        let segundo = fila.entrar(1);
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
