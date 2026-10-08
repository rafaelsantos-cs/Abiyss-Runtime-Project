//! Quem está falando? A decisão é do kernel, a partir dos FATOS que o
//! adaptador relata (IDs, nunca nomes: um nome de exibição qualquer um
//! copia, o ID de usuário não).
//!
//! | De onde | Quem | Vira |
//! |---|---|---|
//! | qualquer lugar | bot ou webhook | ignorado (evita conversa entre bots) |
//! | DM | o dono | **dono** (nível 1) |
//! | DM | pessoa da lista `[[gateway.pessoas]]` | **terceiro conhecido** (nível 2) |
//! | DM | qualquer outro | **desconhecido** (nível 3: ignorado, resposta fixa opcional) |
//! | canal fora de `[[gateway.canais]]` | qualquer um | ignorado |
//! | canal permitido, sem mencionar o bot nem responder a ele | qualquer um | ignorado |
//! | canal permitido, chamando o bot | o dono | **dono** |
//! | canal permitido, chamando o bot | pessoa da lista | **terceiro conhecido** |
//! | canal permitido, chamando o bot | qualquer outro | **terceiro** ("alguém no canal") |
//!
//! Terceiro só conversa: nunca comando, nunca resposta a pedido, contexto
//! mínimo, ferramentas restritas (ver `chat::Perfil::Terceiro`).

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
    /// Bot ou webhook (em qualquer lugar).
    Bot,
    /// Canal de servidor fora da lista.
    CanalNaoPermitido,
    /// Canal permitido, mas a mensagem não chama o bot.
    SemChamar,
}

impl MotivoIgnorada {
    pub fn como_texto(&self) -> &'static str {
        match self {
            MotivoIgnorada::Bot => "bot ou webhook",
            MotivoIgnorada::CanalNaoPermitido => "canal não permitido",
            MotivoIgnorada::SemChamar => "no canal sem mencionar o bot nem responder a ele",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remetente {
    /// Nível 1: o dono.
    Dono,
    /// Nível 2: pessoa conhecida, ou alguém num canal permitido. Só conversa.
    Terceiro {
        /// Rótulo do kernel (da config, ou "alguém no canal X").
        rotulo: String,
        /// Está na lista de pessoas?
        conhecido: bool,
    },
    /// Nível 3: DM de quem não está em lista nenhuma.
    Desconhecido,
    Ignorado(MotivoIgnorada),
}

/// Decide quem está falando.
pub fn classificar(config: &ConfigGateway, m: &MensagemDiscord) -> Remetente {
    if m.autor_bot {
        return Remetente::Ignorado(MotivoIgnorada::Bot);
    }
    let do_dono = m.autor_id == config.dono_discord_id;
    let pessoa = config.pessoa(&m.autor_id);
    if m.dm {
        return match (do_dono, pessoa) {
            (true, _) => Remetente::Dono,
            (false, Some(p)) => Remetente::Terceiro {
                rotulo: p.rotulo.clone(),
                conhecido: true,
            },
            (false, None) => Remetente::Desconhecido,
        };
    }
    let Some(canal) = config.canal(&m.canal_id) else {
        return Remetente::Ignorado(MotivoIgnorada::CanalNaoPermitido);
    };
    if !m.menciona_bot && !m.responde_ao_bot {
        return Remetente::Ignorado(MotivoIgnorada::SemChamar);
    }
    match (do_dono, pessoa) {
        (true, _) => Remetente::Dono,
        (false, Some(p)) => Remetente::Terceiro {
            rotulo: p.rotulo.clone(),
            conhecido: true,
        },
        (false, None) => Remetente::Terceiro {
            rotulo: format!("alguém no canal {}", canal.rotulo),
            conhecido: false,
        },
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::gateway::Conhecido;

    const DONO: &str = "111111111111111111";
    const CANAL: &str = "222222222222222222";
    const ANA: &str = "333333333333333333";
    const ESTRANHO: &str = "444444444444444444";

    fn config() -> ConfigGateway {
        ConfigGateway {
            ativo: true,
            dono_discord_id: DONO.into(),
            pessoas: vec![Conhecido {
                id: ANA.into(),
                rotulo: "Ana".into(),
            }],
            canais: vec![Conhecido {
                id: CANAL.into(),
                rotulo: "#geral".into(),
            }],
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

    fn chamando(mut m: MensagemDiscord) -> MensagemDiscord {
        m.menciona_bot = true;
        m
    }

    #[test]
    fn dms_nos_tres_niveis() {
        let c = config();
        assert_eq!(classificar(&c, &msg(DONO, true, "9")), Remetente::Dono);
        assert_eq!(
            classificar(&c, &msg(ANA, true, "9")),
            Remetente::Terceiro {
                rotulo: "Ana".into(),
                conhecido: true
            }
        );
        assert_eq!(
            classificar(&c, &msg(ESTRANHO, true, "9")),
            Remetente::Desconhecido
        );
        // Nome de exibição copiado não muda nada.
        let mut impostor = msg(ESTRANHO, true, "9");
        impostor.autor_nome = "dono".into();
        assert_eq!(classificar(&c, &impostor), Remetente::Desconhecido);
        // Bot: nunca, nem com o ID do dono.
        let mut bot = msg(DONO, true, "9");
        bot.autor_bot = true;
        assert_eq!(
            classificar(&c, &bot),
            Remetente::Ignorado(MotivoIgnorada::Bot)
        );
    }

    #[test]
    fn canais_so_os_permitidos_e_so_chamando_o_bot() {
        let c = config();
        assert_eq!(
            classificar(&c, &chamando(msg(DONO, false, "999999999999999999"))),
            Remetente::Ignorado(MotivoIgnorada::CanalNaoPermitido)
        );
        for autor in [DONO, ANA, ESTRANHO] {
            assert_eq!(
                classificar(&c, &msg(autor, false, CANAL)),
                Remetente::Ignorado(MotivoIgnorada::SemChamar),
                "{autor}"
            );
        }
        assert_eq!(
            classificar(&c, &chamando(msg(DONO, false, CANAL))),
            Remetente::Dono
        );
        let mut respondendo = msg(ANA, false, CANAL);
        respondendo.responde_ao_bot = true;
        assert_eq!(
            classificar(&c, &respondendo),
            Remetente::Terceiro {
                rotulo: "Ana".into(),
                conhecido: true
            }
        );
        assert_eq!(
            classificar(&c, &chamando(msg(ESTRANHO, false, CANAL))),
            Remetente::Terceiro {
                rotulo: "alguém no canal #geral".into(),
                conhecido: false
            }
        );
    }
}
