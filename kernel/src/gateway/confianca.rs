//! Quem está falando? A decisão é do kernel, a partir dos FATOS que o
//! adaptador relata (IDs, nunca nomes: um nome de exibição qualquer um
//! copia, o ID de usuário não).
//!
//! | De onde                  | Quem                       | Vira                    |
//! |--------------------------|----------------------------|-------------------------|
//! | DM                       | o dono                     | o dono falando          |
//! | DM                       | qualquer outro, bot        | ignorado                |
//! | canal permitido          | o dono (com menção/reply)  | o dono falando          |
//! | canal permitido          | o dono sem chamar o bot    | ignorado (falava com outros) |
//! | canal permitido          | qualquer outro, bot        | conteúdo EXTERNO        |
//! | qualquer outro canal     | qualquer um                | ignorado                |
//!
//! Conteúdo externo nunca é tratado como pedido do dono: entra na fila de
//! eventos marcado com a origem, como um relatório de sub-agente.

use serde::{Deserialize, Serialize};

use super::ConfigGateway;

/// O que o adaptador conta sobre uma mensagem do Discord. Nada aqui é
/// decisão: são fatos que o discord.py entrega.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MensagemDiscord {
    /// ID da mensagem.
    pub id: String,
    pub canal_id: String,
    /// Veio por mensagem direta (DM)?
    pub dm: bool,
    pub autor_id: String,
    /// Nome de exibição (conteúdo externo: só para rotular, nunca para decidir).
    #[serde(default)]
    pub autor_nome: String,
    /// Bot ou webhook.
    #[serde(default)]
    pub autor_bot: bool,
    /// A mensagem menciona o bot do Abiyss?
    #[serde(default)]
    pub menciona_bot: bool,
    /// ID da mensagem citada (o "responder" do Discord), se houver.
    #[serde(default)]
    pub responde_a: Option<String>,
    /// A mensagem citada é do próprio bot?
    #[serde(default)]
    pub responde_ao_bot: bool,
    pub texto: String,
}

/// Por que uma mensagem foi ignorada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotivoIgnorada {
    /// Canal de servidor fora da lista.
    CanalNaoPermitido,
    /// DM de alguém que não é o dono (ou de um bot).
    DmDeEstranho,
    /// O dono, no canal permitido, sem mencionar o bot nem responder a ele.
    DonoSemChamar,
}

impl MotivoIgnorada {
    pub fn como_texto(&self) -> &'static str {
        match self {
            MotivoIgnorada::CanalNaoPermitido => "canal não permitido",
            MotivoIgnorada::DmDeEstranho => "DM de quem não é o dono",
            MotivoIgnorada::DonoSemChamar => "dono no canal sem chamar o bot",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remetente {
    /// O dono falando com o Abiyss.
    Dono,
    /// Conteúdo externo (outra pessoa ou bot no canal permitido). `origem`
    /// é o rótulo gravado junto (`discord:canal:<id>:autor:<id>`).
    Externo {
        origem: String,
    },
    Ignorado(MotivoIgnorada),
}

/// Decide quem está falando.
pub fn classificar(config: &ConfigGateway, m: &MensagemDiscord) -> Remetente {
    let do_dono = !m.autor_bot && m.autor_id == config.dono_discord_id;
    if m.dm {
        return if do_dono {
            Remetente::Dono
        } else {
            Remetente::Ignorado(MotivoIgnorada::DmDeEstranho)
        };
    }
    if config.canal() != Some(m.canal_id.as_str()) {
        return Remetente::Ignorado(MotivoIgnorada::CanalNaoPermitido);
    }
    if do_dono {
        if config.canal_exige_mencao && !m.menciona_bot && !m.responde_ao_bot {
            return Remetente::Ignorado(MotivoIgnorada::DonoSemChamar);
        }
        return Remetente::Dono;
    }
    Remetente::Externo {
        origem: origem_externa(&m.canal_id, &m.autor_id),
    }
}

/// Rótulo da origem de uma mensagem externa.
pub fn origem_externa(canal_id: &str, autor_id: &str) -> String {
    format!("discord:canal:{canal_id}:autor:{autor_id}")
}

#[cfg(test)]
mod testes {
    use super::*;

    const DONO: &str = "111111111111111111";
    const CANAL: &str = "222222222222222222";

    fn config() -> ConfigGateway {
        ConfigGateway {
            ativo: true,
            dono_discord_id: DONO.into(),
            canal_id: CANAL.into(),
            ..Default::default()
        }
    }

    fn msg(autor: &str, dm: bool, canal: &str) -> MensagemDiscord {
        MensagemDiscord {
            id: "1".into(),
            canal_id: canal.into(),
            dm,
            autor_id: autor.into(),
            autor_nome: "dono".into(),
            autor_bot: false,
            menciona_bot: false,
            responde_a: None,
            responde_ao_bot: false,
            texto: "oi".into(),
        }
    }

    #[test]
    fn dono_por_dm_e_estranho_por_dm() {
        let c = config();
        assert_eq!(classificar(&c, &msg(DONO, true, "9")), Remetente::Dono);
        assert_eq!(
            classificar(&c, &msg("333333333333333333", true, "9")),
            Remetente::Ignorado(MotivoIgnorada::DmDeEstranho)
        );
        // Um bot com o ID do dono não existe, mas o fato "bot" vence.
        let mut bot = msg(DONO, true, "9");
        bot.autor_bot = true;
        assert_eq!(
            classificar(&c, &bot),
            Remetente::Ignorado(MotivoIgnorada::DmDeEstranho)
        );
    }

    #[test]
    fn nome_de_exibicao_nao_conta() {
        let mut impostor = msg("333333333333333333", true, "9");
        impostor.autor_nome = "dono".into();
        assert_ne!(classificar(&config(), &impostor), Remetente::Dono);
    }

    #[test]
    fn canal_permitido_outros_canais_e_mencao() {
        let c = config();
        // Outro canal: ignorado, seja quem for.
        assert_eq!(
            classificar(&c, &msg(DONO, false, "444444444444444444")),
            Remetente::Ignorado(MotivoIgnorada::CanalNaoPermitido)
        );
        // Sem canal configurado, nenhum canal de servidor vale.
        let so_dm = ConfigGateway {
            canal_id: String::new(),
            ..config()
        };
        assert_eq!(
            classificar(&so_dm, &msg(DONO, false, CANAL)),
            Remetente::Ignorado(MotivoIgnorada::CanalNaoPermitido)
        );
        // Dono no canal: só chamando o bot.
        assert_eq!(
            classificar(&c, &msg(DONO, false, CANAL)),
            Remetente::Ignorado(MotivoIgnorada::DonoSemChamar)
        );
        let mut chamando = msg(DONO, false, CANAL);
        chamando.menciona_bot = true;
        assert_eq!(classificar(&c, &chamando), Remetente::Dono);
        let mut respondendo = msg(DONO, false, CANAL);
        respondendo.responde_ao_bot = true;
        assert_eq!(classificar(&c, &respondendo), Remetente::Dono);
        let sem_exigir = ConfigGateway {
            canal_exige_mencao: false,
            ..config()
        };
        assert_eq!(
            classificar(&sem_exigir, &msg(DONO, false, CANAL)),
            Remetente::Dono
        );
        // Outra pessoa (ou bot) no canal: externo, com a origem marcada.
        let externo = Remetente::Externo {
            origem: format!("discord:canal:{CANAL}:autor:555555555555555555"),
        };
        let mut outro = msg("555555555555555555", false, CANAL);
        outro.menciona_bot = true;
        assert_eq!(classificar(&c, &outro), externo);
        outro.autor_bot = true;
        assert_eq!(classificar(&c, &outro), externo);
    }
}
