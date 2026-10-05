//! Sessão de conversa com o usuário (`abiyss chat`).
//!
//! A cada mensagem do usuário:
//! 1. grava a mensagem no histórico;
//! 2. monta o contexto: system prompt (regras + núcleo) + histórico recente;
//! 3. chama o cérebro pelo pool, com origem `Conversa` (fatia reservada);
//! 4. grava a resposta.

use crate::config::Config;
use crate::db::Banco;
use crate::historico;
use crate::identidade::Identidade;
use crate::nim::{self, Mensagem, Uso};
use crate::orquestrador::{AoReceber, Origem, Orquestrador};

/// O que um turno de conversa devolve.
#[derive(Debug, Clone)]
pub struct RespostaTurno {
    pub texto: String,
    /// Tokens somados de todas as chamadas do turno.
    pub uso: Uso,
}

pub struct SessaoChat {
    config: Config,
    orquestrador: Orquestrador,
    banco: Banco,
    /// ID da conversa no banco.
    pub conversa: i64,
}

impl SessaoChat {
    /// Começa uma conversa nova.
    pub fn nova(config: Config, orquestrador: Orquestrador, banco: Banco) -> anyhow::Result<Self> {
        let conversa = historico::criar_conversa(&banco)?;
        Ok(SessaoChat {
            config,
            orquestrador,
            banco,
            conversa,
        })
    }

    /// Retoma uma conversa existente.
    pub fn retomar(
        config: Config,
        orquestrador: Orquestrador,
        banco: Banco,
        conversa: i64,
    ) -> anyhow::Result<Self> {
        if !historico::conversa_existe(&banco, conversa)? {
            anyhow::bail!("conversa {conversa} não existe");
        }
        Ok(SessaoChat {
            config,
            orquestrador,
            banco,
            conversa,
        })
    }

    /// Monta a lista de mensagens enviada ao modelo.
    fn montar_contexto(&self) -> anyhow::Result<Vec<Mensagem>> {
        // O núcleo é relido a cada turno: editar o arquivo vale na hora.
        let identidade = Identidade::carregar(&self.config.caminho_identidade());
        let mut mensagens = vec![Mensagem::sistema(identidade.prompt_sistema(None))];
        mensagens.extend(historico::carregar(
            &self.banco,
            self.conversa,
            self.config.chat.historico_max_mensagens,
        )?);
        Ok(mensagens)
    }

    /// Envia uma mensagem do usuário e devolve a resposta do Abiyss.
    /// Com `ao_receber = Some(...)`, o texto chega em streaming.
    pub async fn enviar(
        &mut self,
        texto: &str,
        ao_receber: AoReceber<'_>,
    ) -> anyhow::Result<RespostaTurno> {
        historico::adicionar(&self.banco, self.conversa, &Mensagem::usuario(texto))?;

        let mensagens = self.montar_contexto()?;
        let pedido = nim::montar_pedido(&self.config.modelos.cerebro, mensagens, vec![]);
        let resposta = self
            .orquestrador
            .cerebro
            .chamar(Origem::Conversa, &pedido, ao_receber)
            .await?;

        historico::adicionar(&self.banco, self.conversa, &resposta.mensagem)?;
        Ok(RespostaTurno {
            texto: resposta.mensagem.texto().to_string(),
            uso: resposta.uso,
        })
    }
}
