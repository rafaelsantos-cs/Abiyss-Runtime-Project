//! O que o Abiyss manda ao dono sem ele pedir: os pedidos (E9) por DM, com
//! lembretes limitados, e o resumo da manhã do sono (desligado por padrão).
//!
//! Tudo vira saída na fila do gateway (`registro`): adaptador fora do ar,
//! espera lá. As decisões são funções puras de contagens e instantes.

use rusqlite::{OptionalExtension, params};

use crate::daemon;
use crate::db::Banco;
use crate::pedidos::{self, Pedido};
use crate::tempo::{agora_ms, formatar_ms};

use super::ConfigGateway;
use super::registro::{self, NovaSaida};

/// Chave em `estado_daemon`: dia (local) do último resumo da manhã.
pub const CHAVE_RESUMO: &str = "gateway_resumo_dia";

const HORA_MS: i64 = 3_600_000;

/// Como andam as entregas de um pedido pelo Discord.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Envios {
    /// Quantas vezes já foi posto na fila (o primeiro + lembretes).
    pub vezes: u32,
    /// Alguma ainda esperando o adaptador?
    pub em_aberto: bool,
    /// Quando a última terminou (entregue ou desistida).
    pub ultimo_ms: Option<i64>,
}

/// Mandar (de novo) agora? O primeiro envio vai logo; depois, no máximo
/// `max_reenvios` lembretes, cada um `reenviar_apos_horas` depois do
/// anterior ter terminado (nunca dois na fila ao mesmo tempo).
pub fn deve_enviar(config: &ConfigGateway, envios: Envios, agora: i64) -> bool {
    if envios.vezes == 0 {
        return true;
    }
    if envios.em_aberto || envios.vezes > config.max_reenvios {
        return false;
    }
    envios
        .ultimo_ms
        .is_some_and(|fim| agora - fim >= config.reenviar_apos_horas as i64 * HORA_MS)
}

fn envios(banco: &Banco, pedido: i64) -> anyhow::Result<Envios> {
    let r = banco.conexao().query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(estado IN ('pendente', 'transmitindo')), 0),
                MAX(concluido_ms)
           FROM gateway_mensagens
          WHERE direcao = 'saida' AND tipo = 'pedido' AND pedido_id = ?1",
        params![pedido],
        |l| {
            Ok(Envios {
                vezes: l.get::<_, i64>(0)? as u32,
                em_aberto: l.get::<_, i64>(1)? > 0,
                ultimo_ms: l.get(2)?,
            })
        },
    )?;
    Ok(r)
}

/// O texto da DM de um pedido. `vez` 1 é o primeiro envio.
pub fn texto_do_pedido(p: &Pedido, vez: u32, total: u32) -> String {
    let mut t = String::new();
    if vez > 1 {
        t.push_str(&format!("🔁 Lembrete ({vez}/{total}) — "));
    }
    t.push_str(&format!(
        "❓ **Pedido #{}** (urgência {}, do {}, desde {})\n{}",
        p.id,
        p.urgencia,
        if p.origem == "sono" {
            "sono"
        } else {
            "ciclo autônomo"
        },
        formatar_ms(p.criado_ms),
        p.pergunta
    ));
    if !p.contexto.is_empty() {
        t.push_str(&format!("\n> {}", p.contexto.replace('\n', "\n> ")));
    }
    if let Some(origem) = &p.origem_externa {
        t.push_str(&format!(
            "\n⚠ Este pedido foi escrito depois de ler conteúdo externo ({origem}): \
             confira antes de concordar."
        ));
    }
    t.push_str("\nPara responder, use \"Responder\" nesta mensagem.");
    t
}

/// Põe na fila os pedidos pendentes que precisam ir (ou voltar) ao dono.
/// Devolve quantos.
pub fn agendar_pedidos(banco: &Banco, config: &ConfigGateway, agora: i64) -> anyhow::Result<usize> {
    if !config.pedidos_por_dm {
        return Ok(0);
    }
    let mut n = 0;
    for p in pedidos::pendentes(banco)? {
        let e = envios(banco, p.id)?;
        if !deve_enviar(config, e, agora) {
            continue;
        }
        let texto = texto_do_pedido(&p, e.vezes + 1, config.max_reenvios + 1);
        registro::nova_saida(
            banco,
            &NovaSaida {
                tipo: "pedido",
                estado: "pendente",
                canal_id: None,
                responde_a: None,
                pedido_id: Some(p.id),
                conteudo: &texto,
                anexo: None,
            },
            agora,
        )?;
        n += 1;
    }
    Ok(n)
}

/// O pedido de uma saída ainda está pendente? (Saída de pedido já
/// respondido por outro caminho não é entregue: seria pergunta velha.)
pub fn pedido_ainda_pendente(banco: &Banco, pedido: i64) -> anyhow::Result<bool> {
    Ok(pedidos::obter(banco, pedido)?.is_some_and(|p| p.estado == pedidos::EstadoPedido::Pendente))
}

/// O resumo da manhã: o que o sono da última noite contou ao acordar (o
/// evento `sono`, que por regra do sono não traz texto externo) ou, sem
/// sono concluído nas últimas 24 h, o que houve.
pub fn texto_do_resumo(banco: &Banco, agora: i64) -> anyhow::Result<String> {
    let desde = agora - 24 * HORA_MS;
    let ultimo = crate::sono::ultimo(banco)?.filter(|s| s.inicio_ms >= desde);
    let Some(sono) = ultimo else {
        return Ok("🌅 **Resumo da noite**: não houve sono nas últimas 24 h.".to_string());
    };
    let relato: Option<String> = banco
        .conexao()
        .query_row(
            "SELECT conteudo FROM fila_eventos
              WHERE tipo = 'sono' AND origem = ?1 AND momento_ms >= ?2
              ORDER BY id DESC LIMIT 1",
            params![sono.dia, desde],
            |l| l.get(0),
        )
        .optional()?;
    let mut t = format!(
        "🌅 **Resumo da noite** (revisão de {}, sono {})",
        sono.dia, sono.estado
    );
    match relato {
        Some(r) => t.push_str(&format!("\n{r}")),
        None => {
            if let Some(r) = &sono.resumo {
                t.push_str(&format!("\n{r}"));
            }
            if let Some(e) = &sono.erro {
                t.push_str(&format!("\nProblema: {e}"));
            }
        }
    }
    Ok(t)
}

/// Hora de mandar o resumo da manhã? (Ligado, passou da hora local e
/// ainda não foi hoje.)
pub fn hora_do_resumo(
    config: &ConfigGateway,
    agora_local: chrono::NaiveDateTime,
    ultimo_dia: Option<&str>,
) -> bool {
    if !config.resumo_manha {
        return false;
    }
    let Ok(minuto) = crate::ritmo::minuto_de_texto(&config.resumo_manha_hora) else {
        return false;
    };
    let hoje = agora_local.date().to_string();
    use chrono::Timelike;
    let agora_min = agora_local.hour() * 60 + agora_local.minute();
    agora_min >= minuto && ultimo_dia != Some(hoje.as_str())
}

/// Põe o resumo da manhã na fila, se for a hora. Devolve se pôs.
pub fn agendar_resumo(banco: &Banco, config: &ConfigGateway) -> anyhow::Result<bool> {
    let agora_local = chrono::Local::now().naive_local();
    let ultimo = daemon::ler_estado(banco, CHAVE_RESUMO)?;
    if !hora_do_resumo(config, agora_local, ultimo.as_deref()) {
        return Ok(false);
    }
    let texto = texto_do_resumo(banco, agora_ms())?;
    registro::nova_saida(
        banco,
        &NovaSaida {
            tipo: "resumo",
            estado: "pendente",
            canal_id: None,
            responde_a: None,
            pedido_id: None,
            conteudo: &texto,
            anexo: None,
        },
        agora_ms(),
    )?;
    daemon::gravar_estado(banco, CHAVE_RESUMO, &agora_local.date().to_string())?;
    Ok(true)
}

#[cfg(test)]
mod testes {
    use super::*;

    fn config() -> ConfigGateway {
        ConfigGateway {
            max_reenvios: 2,
            reenviar_apos_horas: 12,
            ..Default::default()
        }
    }

    #[test]
    fn primeiro_logo_lembretes_espacados_e_com_teto() {
        let c = config();
        let h = HORA_MS;
        assert!(deve_enviar(&c, Envios::default(), 0));
        let entregue = |vezes, fim| Envios {
            vezes,
            em_aberto: false,
            ultimo_ms: Some(fim),
        };
        // Esperando o adaptador: nada de outro na fila.
        assert!(!deve_enviar(
            &c,
            Envios {
                vezes: 1,
                em_aberto: true,
                ultimo_ms: None
            },
            100 * h
        ));
        assert!(!deve_enviar(&c, entregue(1, 0), 11 * h));
        assert!(deve_enviar(&c, entregue(1, 0), 12 * h));
        assert!(deve_enviar(&c, entregue(2, 0), 12 * h));
        // 1 + 2 lembretes = 3 vezes: acabou.
        assert!(!deve_enviar(&c, entregue(3, 0), 1000 * h));
        let sem_lembrete = ConfigGateway {
            max_reenvios: 0,
            ..config()
        };
        assert!(!deve_enviar(&sem_lembrete, entregue(1, 0), 1000 * h));
    }

    #[test]
    fn resumo_uma_vez_por_dia_depois_da_hora() {
        let mut c = config();
        let as_7 = chrono::NaiveDate::from_ymd_opt(2026, 10, 8)
            .unwrap()
            .and_hms_opt(7, 59, 0)
            .unwrap();
        let as_8 = as_7 + chrono::Duration::minutes(1);
        assert!(!hora_do_resumo(&c, as_8, None), "desligado por padrão");
        c.resumo_manha = true;
        assert!(!hora_do_resumo(&c, as_7, None));
        assert!(hora_do_resumo(&c, as_8, None));
        assert!(hora_do_resumo(&c, as_8, Some("2026-10-07")));
        assert!(!hora_do_resumo(&c, as_8, Some("2026-10-08")));
    }
}
