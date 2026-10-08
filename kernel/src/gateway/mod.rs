//! Gateway: o dono conversa com o Abiyss pelo Discord, e o Abiyss alcança o
//! dono sem a CLI.
//!
//! O kernel não fala com o Discord. Quem fala é um processo separado, o
//! adaptador em `recursos/gateway/` (Python + discord.py), que só relata
//! FATOS (quem escreveu, onde, em resposta a quê) e entrega o que o kernel
//! manda. Todas as decisões de confiança ficam aqui, no kernel.
//!
//! Segurança da saída (ver `ocultar` e `servidor`): todo texto passa pelo
//! ocultador de segredos e pelos tetos de tamanho e de mensagens por minuto
//! no único caminho até o adaptador; o token do bot só existe no ambiente
//! do adaptador; arquivo, só de dentro do workspace (`/arquivo`).
//!
//! Confiança (ver `confianca`): só mensagens do ID de usuário do dono
//! (`dono_discord_id`) contam como o dono falando — por DM, ou no canal
//! permitido (`canal_id`, opcional). Todo o resto é conteúdo externo, e é
//! ignorado a não ser que venha do canal permitido.

pub mod agenda;
pub mod comandos;
pub mod confianca;
pub mod ocultar;
pub mod protocolo;
pub mod registro;
#[cfg(unix)]
pub mod servidor;

#[cfg(unix)]
pub use servidor::Gateway;

use anyhow::bail;
use serde::Deserialize;

use crate::db::Banco;
use crate::tempo::formatar_ms;

/// Chave em `estado_daemon`: o adaptador está conectado? (`conectado:<ms>`
/// ou `desconectado:<ms>`; gravada pelo gateway, lida pelo `abiyss status`.)
pub const CHAVE_ADAPTADOR: &str = "gateway_adaptador";

/// `[gateway]` no abiyss.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigGateway {
    /// Desligado (o padrão), o daemon nem abre o socket.
    pub ativo: bool,
    /// Socket Unix por onde o adaptador fala com o daemon (relativo à pasta
    /// do abiyss.toml). Só o usuário do daemon alcança (0600).
    pub socket: String,
    /// ID de usuário do Discord do dono (só dígitos). Obrigatório se ativo.
    pub dono_discord_id: String,
    /// ID de UM canal de servidor onde o Abiyss também escuta (vazio =
    /// só DMs). Lá, o dono fala com o Abiyss; os outros são conteúdo externo.
    pub canal_id: String,
    /// No canal permitido, a mensagem do dono só conta se mencionar o bot
    /// ou responder a uma mensagem dele (DM conta sempre).
    pub canal_exige_mencao: bool,
    /// Tempo máximo de UM turno de conversa pelo Discord (passou, o dono
    /// recebe o erro e a conversa segue).
    pub max_duracao_turno_segundos: u64,
    /// Pedidos pendentes (E9) vão ao dono por DM. Um "Responder" do Discord
    /// na mensagem do pedido é a resposta.
    pub pedidos_por_dm: bool,
    /// Lembretes de um pedido sem resposta (além do primeiro envio).
    pub max_reenvios: u32,
    /// Intervalo entre um envio de pedido e o lembrete seguinte.
    pub reenviar_apos_horas: u64,
    /// Resumo da noite (o que o sono contou ao acordar) por DM, uma vez por
    /// dia, a partir de `resumo_manha_hora` (fuso local).
    pub resumo_manha: bool,
    pub resumo_manha_hora: String,
    /// De quanto em quanto tempo o gateway olha pedidos e o resumo.
    pub verificacao_segundos: u64,
    /// Mensagem que chega maior que isto é cortada (com aviso no texto).
    pub max_caracteres_entrada: usize,
    /// Mensagens do dono por minuto; acima disso, registradas sem resposta
    /// (um aviso por minuto). Protege contra rajadas e loops.
    pub max_entrada_por_minuto: usize,
    /// Mensagens externas (canal permitido) por minuto que viram evento.
    pub max_externas_por_minuto: usize,
    /// Texto que sai maior que isto é cortado (o inteiro fica no histórico).
    pub max_caracteres_saida: usize,
    /// Mensagens entregues por minuto (as edições da resposta que chega aos
    /// poucos não contam: o adaptador já as espaça). O resto espera.
    pub max_saida_por_minuto: usize,
    /// Maior arquivo do workspace que `/arquivo` manda.
    pub max_bytes_anexo: u64,
}

impl Default for ConfigGateway {
    fn default() -> Self {
        ConfigGateway {
            ativo: false,
            socket: "data/gateway/abiyss.sock".into(),
            dono_discord_id: String::new(),
            canal_id: String::new(),
            canal_exige_mencao: true,
            max_duracao_turno_segundos: 900,
            pedidos_por_dm: true,
            max_reenvios: 2,
            reenviar_apos_horas: 12,
            resumo_manha: false,
            resumo_manha_hora: "08:00".into(),
            verificacao_segundos: 30,
            max_caracteres_entrada: 4000,
            max_entrada_por_minuto: 20,
            max_externas_por_minuto: 10,
            max_caracteres_saida: 12_000,
            max_saida_por_minuto: 20,
            max_bytes_anexo: 8 * 1024 * 1024,
        }
    }
}

/// Uma linha para o `abiyss status`.
pub fn resumo_status(banco: &Banco) -> anyhow::Result<String> {
    let adaptador = match crate::daemon::ler_estado(banco, CHAVE_ADAPTADOR)?
        .as_deref()
        .and_then(|v| v.split_once(':'))
        .and_then(|(e, ms)| Some((e.to_string(), ms.parse::<i64>().ok()?)))
    {
        Some((estado, ms)) if estado == "conectado" => {
            format!("adaptador conectado desde {}", formatar_ms(ms))
        }
        Some((_, ms)) => format!("adaptador DESCONECTADO desde {}", formatar_ms(ms)),
        None => "o adaptador nunca conectou".to_string(),
    };
    let contar = |sql: &str| -> anyhow::Result<i64> {
        Ok(banco.conexao().query_row(sql, [], |l| l.get(0))?)
    };
    let saidas = contar(
        "SELECT COUNT(*) FROM gateway_mensagens
          WHERE direcao = 'saida' AND estado IN ('pendente', 'transmitindo')",
    )?;
    let entradas = contar(
        "SELECT COUNT(*) FROM gateway_mensagens
          WHERE direcao = 'entrada' AND estado IN ('pendente', 'processando')",
    )?;
    let falhas = contar(
        "SELECT COUNT(*) FROM gateway_mensagens
          WHERE direcao = 'saida' AND estado = 'falhou' AND momento_ms >= strftime('%s', 'now') * 1000 - 86400000",
    )?;
    Ok(format!(
        "Gateway (Discord): {adaptador}; {saidas} saída(s) na fila, {entradas} mensagem(ns) do \
         dono esperando, {falhas} entrega(s) desistida(s) nas últimas 24 h"
    ))
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
        if self.socket.trim().is_empty() || self.max_duracao_turno_segundos == 0 {
            bail!("gateway.socket não pode ser vazio e gateway.max_duracao_turno_segundos > 0");
        }
        if !id_discord_valido(&self.dono_discord_id) {
            bail!(
                "gateway.dono_discord_id precisa ser o ID de usuário do dono no Discord (só \
                 dígitos; Discord > Configurações > Avançado > Modo desenvolvedor, depois \
                 \"Copiar ID do usuário\")"
            );
        }
        if self.reenviar_apos_horas == 0 || self.verificacao_segundos == 0 {
            bail!("gateway.reenviar_apos_horas e gateway.verificacao_segundos precisam ser > 0");
        }
        if self.max_caracteres_entrada == 0
            || self.max_entrada_por_minuto == 0
            || self.max_externas_por_minuto == 0
            || self.max_saida_por_minuto == 0
            || self.max_bytes_anexo == 0
        {
            bail!("gateway: os limites (max_*) precisam ser > 0");
        }
        if self.max_caracteres_saida < 500 {
            bail!("gateway.max_caracteres_saida precisa ser pelo menos 500");
        }
        crate::ritmo::minuto_de_texto(&self.resumo_manha_hora)
            .map_err(|e| anyhow::anyhow!("gateway.resumo_manha_hora: {e:#}"))?;
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
