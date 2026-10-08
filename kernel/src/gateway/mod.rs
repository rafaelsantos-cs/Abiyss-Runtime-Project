//! Gateway: o dono conversa com o Abiyss pelo Discord, e o Abiyss alcança o
//! dono sem a CLI.
//!
//! O kernel não fala com o Discord. Quem fala é um processo separado, o
//! adaptador em `recursos/gateway/` (Python + discord.py), que só relata
//! FATOS (quem escreveu, onde, em resposta a quê) e entrega o que o kernel
//! manda. Todas as decisões de confiança ficam aqui, no kernel.
//!
//! Confiança (ver `confianca`): só mensagens do ID de usuário do dono
//! (`dono_discord_id`) contam como o dono falando — por DM, ou no canal
//! permitido (`canal_id`, opcional). Todo o resto é conteúdo externo, e é
//! ignorado a não ser que venha do canal permitido.

pub mod confianca;

use anyhow::bail;
use serde::Deserialize;

/// `[gateway]` no abiyss.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigGateway {
    /// Desligado (o padrão), o daemon nem abre o socket.
    pub ativo: bool,
    /// ID de usuário do Discord do dono (só dígitos). Obrigatório se ativo.
    pub dono_discord_id: String,
    /// ID de UM canal de servidor onde o Abiyss também escuta (vazio =
    /// só DMs). Lá, o dono fala com o Abiyss; os outros são conteúdo externo.
    pub canal_id: String,
    /// No canal permitido, a mensagem do dono só conta se mencionar o bot
    /// ou responder a uma mensagem dele (DM conta sempre).
    pub canal_exige_mencao: bool,
}

impl Default for ConfigGateway {
    fn default() -> Self {
        ConfigGateway {
            ativo: false,
            dono_discord_id: String::new(),
            canal_id: String::new(),
            canal_exige_mencao: true,
        }
    }
}

/// Um ID do Discord ("snowflake"): só dígitos, de 15 a 20.
pub fn id_discord_valido(id: &str) -> bool {
    (15..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

impl ConfigGateway {
    pub fn validar(&self) -> anyhow::Result<()> {
        if !self.ativo {
            return Ok(());
        }
        if !id_discord_valido(&self.dono_discord_id) {
            bail!(
                "gateway.dono_discord_id precisa ser o ID de usuário do dono no Discord (só \
                 dígitos; Discord > Configurações > Avançado > Modo desenvolvedor, depois \
                 \"Copiar ID do usuário\")"
            );
        }
        if !self.canal_id.is_empty() && !id_discord_valido(&self.canal_id) {
            bail!("gateway.canal_id precisa ser o ID de um canal do Discord (só dígitos) ou vazio");
        }
        Ok(())
    }

    /// O canal permitido, se houver.
    pub fn canal(&self) -> Option<&str> {
        (!self.canal_id.is_empty()).then_some(self.canal_id.as_str())
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn desligado_nao_exige_nada_e_ligado_exige_ids_validos() {
        let mut c = ConfigGateway::default();
        assert!(c.validar().is_ok());
        c.ativo = true;
        assert!(c.validar().is_err(), "sem dono");
        c.dono_discord_id = "123456789012345678".into();
        assert!(c.validar().is_ok());
        assert_eq!(c.canal(), None);
        c.canal_id = "#geral".into();
        assert!(c.validar().is_err());
        c.canal_id = "223456789012345678".into();
        assert!(c.validar().is_ok());
        assert_eq!(c.canal(), Some("223456789012345678"));
        c.dono_discord_id = "12345".into();
        assert!(c.validar().is_err(), "curto demais");
    }
}
