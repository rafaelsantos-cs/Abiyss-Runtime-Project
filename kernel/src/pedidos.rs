//! Pedidos ao usuário: a caixa de entrada assíncrona.
//!
//! Quando o Abiyss precisa do dono e ele não está conversando (heartbeat,
//! sono), não dá para perguntar na hora: o pedido fica guardado aqui.
//!
//! - o heartbeat pede com a ação `pedir_ao_usuario`; as perguntas do sono
//!   também viram pedidos;
//! - o dono responde pela CLI (`abiyss pedidos responder <id> "texto"`),
//!   na conversa (o modelo registra com a ferramenta `responder_pedido`) ou
//!   pelo Discord: com o gateway ligado, cada pedido vai por DM e um
//!   "Responder" naquela mensagem é a resposta (ver `gateway::agenda`);
//! - a resposta vira um evento `usuario`/`pedido:<id>` para o heartbeat;
//! - sem resposta em `[pedidos] expira_apos_horas`, o pedido expira (e
//!   também vira evento).
//!
//! A mesma pergunta pendente não duplica (chave normalizada) e há um teto
//! de pendentes.

use anyhow::{Context, bail};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;

use crate::db::Banco;
use crate::eventos;
use crate::tempo::{agora_ms, formatar_duracao, formatar_ms};

/// `[pedidos]` no abiyss.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigPedidos {
    /// Máximo de pedidos pendentes ao mesmo tempo.
    pub max_pendentes: usize,
    /// Sem resposta depois disto, o pedido expira.
    pub expira_apos_horas: u64,
}

impl Default for ConfigPedidos {
    fn default() -> Self {
        ConfigPedidos {
            max_pendentes: 10,
            expira_apos_horas: 72,
        }
    }
}

impl ConfigPedidos {
    pub fn validar(&self) -> anyhow::Result<()> {
        if self.max_pendentes == 0 || self.expira_apos_horas == 0 {
            bail!("pedidos.max_pendentes e pedidos.expira_apos_horas precisam ser > 0");
        }
        Ok(())
    }
}

/// Tamanho máximo da pergunta e do contexto (o resto é cortado).
const MAX_PERGUNTA: usize = 500;
const MAX_CONTEXTO: usize = 2_000;
const MAX_RESPOSTA: usize = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgencia {
    Baixa,
    Normal,
    Alta,
}

impl Urgencia {
    pub fn como_texto(&self) -> &'static str {
        match self {
            Urgencia::Baixa => "baixa",
            Urgencia::Normal => "normal",
            Urgencia::Alta => "alta",
        }
    }

    /// Vazio = normal.
    pub fn de_texto(texto: &str) -> Option<Urgencia> {
        match texto.trim().to_lowercase().as_str() {
            "baixa" => Some(Urgencia::Baixa),
            "" | "normal" | "media" | "média" => Some(Urgencia::Normal),
            "alta" => Some(Urgencia::Alta),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstadoPedido {
    Pendente,
    Respondido,
    Expirado,
    Cancelado,
}

impl EstadoPedido {
    pub fn como_texto(&self) -> &'static str {
        match self {
            EstadoPedido::Pendente => "pendente",
            EstadoPedido::Respondido => "respondido",
            EstadoPedido::Expirado => "expirado",
            EstadoPedido::Cancelado => "cancelado",
        }
    }

    pub fn de_texto(texto: &str) -> Option<EstadoPedido> {
        match texto {
            "pendente" => Some(EstadoPedido::Pendente),
            "respondido" => Some(EstadoPedido::Respondido),
            "expirado" => Some(EstadoPedido::Expirado),
            "cancelado" => Some(EstadoPedido::Cancelado),
            _ => None,
        }
    }
}

/// Um pedido novo.
#[derive(Debug, Clone)]
pub struct NovoPedido {
    /// "heartbeat" ou "sono".
    pub origem: String,
    pub goal_id: Option<i64>,
    pub pergunta: String,
    pub contexto: String,
    pub urgencia: Urgencia,
    /// O texto do pedido derivou de conteúdo externo? (calculado pelo kernel)
    pub origem_externa: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pedido {
    pub id: i64,
    pub criado_ms: i64,
    pub origem: String,
    pub goal_id: Option<i64>,
    pub pergunta: String,
    pub contexto: String,
    pub urgencia: String,
    pub estado: EstadoPedido,
    pub resposta: Option<String>,
    pub respondido_ms: Option<i64>,
    /// Origem externa do texto do pedido (se houver).
    pub origem_externa: Option<String>,
    /// A resposta foi dada numa janela com conteúdo externo?
    pub resposta_externa: Option<String>,
}

/// O que `criar` fez.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Criacao {
    Novo(i64),
    /// A mesma pergunta já estava pendente: nada criado.
    Duplicado(i64),
}

impl Criacao {
    pub fn id(&self) -> i64 {
        match self {
            Criacao::Novo(id) | Criacao::Duplicado(id) => *id,
        }
    }
}

/// Chave para não duplicar: minúsculas, só letras e números.
pub fn chave(pergunta: &str) -> String {
    pergunta
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn cortar(texto: &str, max: usize) -> String {
    texto.trim().chars().take(max).collect()
}

const COLUNAS: &str = "id, criado_ms, origem, goal_id, pergunta, contexto, urgencia, estado, \
                       resposta, respondido_ms, origem_externa, resposta_externa";

fn da_linha(l: &rusqlite::Row<'_>) -> rusqlite::Result<Pedido> {
    let estado: String = l.get(7)?;
    Ok(Pedido {
        id: l.get(0)?,
        criado_ms: l.get(1)?,
        origem: l.get(2)?,
        goal_id: l.get(3)?,
        pergunta: l.get(4)?,
        contexto: l.get(5)?,
        urgencia: l.get(6)?,
        estado: EstadoPedido::de_texto(&estado).unwrap_or(EstadoPedido::Pendente),
        resposta: l.get(8)?,
        respondido_ms: l.get(9)?,
        origem_externa: l.get(10)?,
        resposta_externa: l.get(11)?,
    })
}

/// Cria um pedido (ou devolve o pendente com a mesma pergunta).
pub fn criar(banco: &Banco, config: &ConfigPedidos, novo: &NovoPedido) -> anyhow::Result<Criacao> {
    let pergunta = cortar(&novo.pergunta, MAX_PERGUNTA);
    if pergunta.is_empty() {
        bail!("a pergunta está vazia");
    }
    let chave = chave(&pergunta);
    let conexao = banco.conexao();
    let existente: Option<i64> = conexao
        .query_row(
            "SELECT id FROM pedidos_usuario WHERE chave = ?1 AND estado = 'pendente'",
            params![chave],
            |l| l.get(0),
        )
        .optional()?;
    if let Some(id) = existente {
        return Ok(Criacao::Duplicado(id));
    }
    let pendentes: i64 = conexao.query_row(
        "SELECT COUNT(*) FROM pedidos_usuario WHERE estado = 'pendente'",
        [],
        |l| l.get(0),
    )?;
    if pendentes as usize >= config.max_pendentes {
        bail!(
            "já há {pendentes} pedido(s) pendente(s) (limite {}): espere o usuário responder \
             ou junte as perguntas",
            config.max_pendentes
        );
    }
    conexao.execute(
        "INSERT INTO pedidos_usuario
           (criado_ms, origem, goal_id, pergunta, contexto, urgencia, estado, origem_externa, chave)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pendente', ?7, ?8)",
        params![
            agora_ms(),
            novo.origem,
            novo.goal_id,
            pergunta,
            cortar(&novo.contexto, MAX_CONTEXTO),
            novo.urgencia.como_texto(),
            novo.origem_externa,
            chave
        ],
    )?;
    Ok(Criacao::Novo(conexao.last_insert_rowid()))
}

pub fn obter(banco: &Banco, id: i64) -> anyhow::Result<Option<Pedido>> {
    let p = banco
        .conexao()
        .query_row(
            &format!("SELECT {COLUNAS} FROM pedidos_usuario WHERE id = ?1"),
            params![id],
            da_linha,
        )
        .optional()?;
    Ok(p)
}

/// Pendentes, os mais urgentes e mais antigos primeiro.
pub fn pendentes(banco: &Banco) -> anyhow::Result<Vec<Pedido>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(&format!(
        "SELECT {COLUNAS} FROM pedidos_usuario WHERE estado = 'pendente'
         ORDER BY CASE urgencia WHEN 'alta' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END, id"
    ))?;
    let lista = consulta
        .query_map([], da_linha)?
        .collect::<Result<_, _>>()?;
    Ok(lista)
}

/// Os mais recentes, de qualquer estado.
pub fn recentes(banco: &Banco, limite: usize) -> anyhow::Result<Vec<Pedido>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(&format!(
        "SELECT {COLUNAS} FROM pedidos_usuario ORDER BY id DESC LIMIT ?1"
    ))?;
    let lista = consulta
        .query_map(params![limite as i64], da_linha)?
        .collect::<Result<_, _>>()?;
    Ok(lista)
}

/// Registra a resposta do dono e avisa o heartbeat (evento
/// `usuario`/`pedido:<id>`). `origem_externa`: a janela da conversa em que
/// a resposta foi dada tinha conteúdo externo (regra do chat).
pub fn responder(
    banco: &Banco,
    id: i64,
    resposta: &str,
    origem_externa: Option<&str>,
) -> anyhow::Result<Pedido> {
    let resposta = cortar(resposta, MAX_RESPOSTA);
    if resposta.is_empty() {
        bail!("a resposta está vazia");
    }
    let pedido = obter(banco, id)?.with_context(|| format!("pedido #{id} não existe"))?;
    if pedido.estado != EstadoPedido::Pendente {
        bail!(
            "o pedido #{id} não está pendente (está {})",
            pedido.estado.como_texto()
        );
    }
    let agora = agora_ms();
    banco.conexao().execute(
        "UPDATE pedidos_usuario SET estado = 'respondido', resposta = ?1, respondido_ms = ?2,
                                    resposta_externa = ?3
         WHERE id = ?4 AND estado = 'pendente'",
        params![resposta, agora, origem_externa, id],
    )?;
    // O evento cita a pergunta: se ela (ou a resposta) veio de contexto
    // externo, o evento também é externo.
    let externo = match (origem_externa, pedido.origem_externa.as_deref()) {
        (Some(a), Some(b)) => Some(format!("{a}, {b}")),
        (Some(a), None) | (None, Some(a)) => Some(a.to_string()),
        (None, None) => None,
    };
    eventos::publicar_com_origem(
        banco,
        eventos::TIPO_USUARIO,
        &format!("pedido:{id}"),
        &format!(
            "O usuário respondeu ao seu pedido #{id} (\"{}\"): {resposta}",
            pedido.pergunta
        ),
        externo.as_deref(),
    )?;
    obter(banco, id)?.context("pedido sumiu depois de respondido")
}

/// Cancela um pedido pendente (o dono não vai responder).
pub fn cancelar(banco: &Banco, id: i64) -> anyhow::Result<()> {
    let n = banco.conexao().execute(
        "UPDATE pedidos_usuario SET estado = 'cancelado' WHERE id = ?1 AND estado = 'pendente'",
        params![id],
    )?;
    if n == 0 {
        bail!("o pedido #{id} não existe ou não está pendente");
    }
    Ok(())
}

/// Pendentes criados antes de `agora - expira_apos_horas` expiram; cada um
/// vira um evento `kernel`/`pedido:<id>`. Devolve quantos expiraram.
pub fn expirar_vencidos(
    banco: &Banco,
    config: &ConfigPedidos,
    agora: i64,
) -> anyhow::Result<usize> {
    let limite = agora - config.expira_apos_horas as i64 * 3_600_000;
    let vencidos: Vec<Pedido> = {
        let conexao = banco.conexao();
        let mut consulta = conexao.prepare(&format!(
            "SELECT {COLUNAS} FROM pedidos_usuario WHERE estado = 'pendente' AND criado_ms < ?1"
        ))?;
        consulta
            .query_map(params![limite], da_linha)?
            .collect::<Result<_, _>>()?
    };
    for p in &vencidos {
        banco.conexao().execute(
            "UPDATE pedidos_usuario SET estado = 'expirado' WHERE id = ?1 AND estado = 'pendente'",
            params![p.id],
        )?;
        eventos::publicar_com_origem(
            banco,
            eventos::TIPO_KERNEL,
            &format!("pedido:{}", p.id),
            &format!(
                "Seu pedido #{} (\"{}\") expirou sem resposta depois de {}. Decida sem a \
                 resposta, pergunte de outro jeito ou bloqueie o que depende dela.",
                p.id,
                p.pergunta,
                formatar_duracao(agora - p.criado_ms)
            ),
            p.origem_externa.as_deref(),
        )?;
    }
    Ok(vencidos.len())
}

/// Bloco com os pendentes, para o contexto da conversa (ou `None`).
pub fn bloco_para_chat(banco: &Banco) -> anyhow::Result<Option<String>> {
    let lista = pendentes(banco)?;
    if lista.is_empty() {
        return Ok(None);
    }
    let mut t = String::from(
        "Pedidos seus ao dono, ainda sem resposta. Se ele responder algum nesta conversa, \
         registre com a ferramenta responder_pedido (id e a resposta, nas palavras dele).\n",
    );
    for p in &lista {
        t.push_str(&format!(
            "- #{} [{}] {}{} (desde {})\n",
            p.id,
            p.urgencia,
            p.pergunta,
            if p.contexto.is_empty() {
                String::new()
            } else {
                format!(" — contexto: {}", p.contexto)
            },
            formatar_ms(p.criado_ms)
        ));
    }
    Ok(Some(t))
}

/// Uma linha para a interocepção (ou `None` sem pendentes).
pub fn resumo(banco: &Banco, agora: i64) -> anyhow::Result<Option<String>> {
    let lista = pendentes(banco)?;
    let Some(mais_antigo) = lista.iter().map(|p| p.criado_ms).min() else {
        return Ok(None);
    };
    Ok(Some(format!(
        "Pedidos ao usuário pendentes: {} (o mais antigo há {})",
        lista.len(),
        formatar_duracao(agora - mais_antigo)
    )))
}

#[cfg(test)]
mod testes {
    use super::*;

    fn novo(pergunta: &str) -> NovoPedido {
        NovoPedido {
            origem: "heartbeat".into(),
            goal_id: Some(1),
            pergunta: pergunta.into(),
            contexto: "c".into(),
            urgencia: Urgencia::Normal,
            origem_externa: None,
        }
    }

    #[test]
    fn chave_ignora_caixa_e_pontuacao() {
        assert_eq!(
            chave("Posso apagar a pasta X?"),
            chave("  posso APAGAR a pasta x ")
        );
        assert_ne!(chave("Posso apagar X?"), chave("Posso apagar Y?"));
    }

    #[test]
    fn cria_deduplica_limita_responde_e_expira() {
        let banco = Banco::em_memoria().unwrap();
        let config = ConfigPedidos {
            max_pendentes: 2,
            expira_apos_horas: 1,
        };
        let a = criar(&banco, &config, &novo("Posso apagar a pasta X?")).unwrap();
        assert!(matches!(a, Criacao::Novo(_)));
        assert_eq!(
            criar(&banco, &config, &novo("posso apagar a pasta x")).unwrap(),
            Criacao::Duplicado(a.id())
        );
        criar(&banco, &config, &novo("Qual é o prazo?")).unwrap();
        let erro = criar(&banco, &config, &novo("Terceira?")).unwrap_err();
        assert!(erro.to_string().contains("limite 2"));

        // Responder: estado, evento e não responde duas vezes.
        let p = responder(&banco, a.id(), "Pode.", None).unwrap();
        assert_eq!(p.estado, EstadoPedido::Respondido);
        assert!(responder(&banco, a.id(), "De novo", None).is_err());
        let evento = eventos::pendentes(&banco, 10).unwrap().pop().unwrap();
        assert_eq!(evento.tipo, eventos::TIPO_USUARIO);
        assert_eq!(evento.origem, format!("pedido:{}", a.id()));
        assert!(evento.conteudo.contains("Pode."));
        assert!(!evento.eh_externo());
        // Respondida, a mesma pergunta pode ser feita de novo.
        assert!(matches!(
            criar(&banco, &config, &novo("Posso apagar a pasta X?")).unwrap(),
            Criacao::Novo(_)
        ));

        // Expiração: só os mais velhos que o limite.
        let daqui_a_2h = agora_ms() + 2 * 3_600_000;
        assert_eq!(expirar_vencidos(&banco, &config, agora_ms()).unwrap(), 0);
        assert_eq!(expirar_vencidos(&banco, &config, daqui_a_2h).unwrap(), 2);
        assert!(pendentes(&banco).unwrap().is_empty());
        let ultimo = eventos::pendentes(&banco, 10).unwrap().pop().unwrap();
        assert_eq!(ultimo.tipo, eventos::TIPO_KERNEL);
        assert!(ultimo.conteudo.contains("expirou sem resposta"));
    }

    #[test]
    fn resposta_em_contexto_externo_marca_o_evento() {
        let banco = Banco::em_memoria().unwrap();
        let config = ConfigPedidos::default();
        let id = criar(&banco, &config, &novo("Qual site usar?"))
            .unwrap()
            .id();
        responder(&banco, id, "o oficial", Some("mcp:web")).unwrap();
        let p = obter(&banco, id).unwrap().unwrap();
        assert_eq!(p.resposta_externa.as_deref(), Some("mcp:web"));
        let evento = eventos::pendentes(&banco, 10).unwrap().pop().unwrap();
        assert!(evento.eh_externo());
    }
}
