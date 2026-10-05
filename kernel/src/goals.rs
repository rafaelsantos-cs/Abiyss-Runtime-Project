//! Goals (objetivos) e a máquina de estados deles.
//!
//! ```text
//! proposto ──► comprometido ──► executando ──► validando ──► concluído
//!    │              │               │  ▲            │
//!    │              │               │  └────────────┘ (validação falhou)
//!    │              ▼               ▼               ▼
//!    │          bloqueado ◄─────────┴───────────────┘
//!    │              │ (desbloqueado: volta a comprometido ou executando)
//!    ▼              ▼
//! abandonado ◄── (qualquer estado não terminal)
//! ```
//!
//! Toda transição é VALIDADA por código e registrada como evento com
//! motivo (tabela `eventos_goal`). "concluído" e "abandonado" são finais.

use rusqlite::{OptionalExtension, params};

use crate::db::Banco;
use crate::tempo::agora_ms;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EstadoGoal {
    Proposto,
    Comprometido,
    Executando,
    Validando,
    Concluido,
    Bloqueado,
    Abandonado,
}

impl EstadoGoal {
    pub const TODOS: [EstadoGoal; 7] = [
        EstadoGoal::Proposto,
        EstadoGoal::Comprometido,
        EstadoGoal::Executando,
        EstadoGoal::Validando,
        EstadoGoal::Concluido,
        EstadoGoal::Bloqueado,
        EstadoGoal::Abandonado,
    ];

    pub fn como_texto(&self) -> &'static str {
        match self {
            EstadoGoal::Proposto => "proposto",
            EstadoGoal::Comprometido => "comprometido",
            EstadoGoal::Executando => "executando",
            EstadoGoal::Validando => "validando",
            EstadoGoal::Concluido => "concluido",
            EstadoGoal::Bloqueado => "bloqueado",
            EstadoGoal::Abandonado => "abandonado",
        }
    }

    /// Aceita com ou sem acento ("concluído" ou "concluido").
    pub fn de_texto(texto: &str) -> Option<EstadoGoal> {
        match texto.trim().to_lowercase().as_str() {
            "proposto" => Some(EstadoGoal::Proposto),
            "comprometido" => Some(EstadoGoal::Comprometido),
            "executando" => Some(EstadoGoal::Executando),
            "validando" => Some(EstadoGoal::Validando),
            "concluido" | "concluído" => Some(EstadoGoal::Concluido),
            "bloqueado" => Some(EstadoGoal::Bloqueado),
            "abandonado" => Some(EstadoGoal::Abandonado),
            _ => None,
        }
    }

    /// Estados finais não saem mais do lugar.
    pub fn eh_terminal(&self) -> bool {
        matches!(self, EstadoGoal::Concluido | EstadoGoal::Abandonado)
    }

    /// Para quais estados é permitido ir a partir deste.
    pub fn proximos_permitidos(&self) -> &'static [EstadoGoal] {
        use EstadoGoal::*;
        match self {
            Proposto => &[Comprometido, Abandonado],
            Comprometido => &[Executando, Bloqueado, Abandonado],
            Executando => &[Validando, Bloqueado, Abandonado],
            // Validação pode aprovar (concluído) ou mandar de volta ao trabalho.
            Validando => &[Concluido, Executando, Bloqueado, Abandonado],
            Bloqueado => &[Comprometido, Executando, Abandonado],
            Concluido | Abandonado => &[],
        }
    }

    pub fn pode_ir_para(&self, destino: EstadoGoal) -> bool {
        self.proximos_permitidos().contains(&destino)
    }
}

impl std::fmt::Display for EstadoGoal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.como_texto())
    }
}

/// Erros de regra de negócio dos goals.
#[derive(Debug, thiserror::Error)]
pub enum ErroGoal {
    #[error("goal {0} não existe")]
    NaoEncontrado(i64),
    #[error("transição inválida: {de} → {para} (permitidas a partir de {de}: {permitidas})")]
    TransicaoInvalida {
        de: EstadoGoal,
        para: EstadoGoal,
        permitidas: String,
    },
    #[error("toda transição precisa de um motivo")]
    MotivoVazio,
    #[error("{0}")]
    Invalido(String),
    #[error("erro no banco: {0}")]
    Banco(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Goal {
    pub id: i64,
    pub titulo: String,
    /// O "núcleo" do goal: a essência em uma ou duas frases, incluindo
    /// o critério de pronto. Vai no início E no fim do contexto do heartbeat.
    pub nucleo: String,
    pub descricao: String,
    /// Maior = mais importante.
    pub prioridade: i64,
    pub estado: EstadoGoal,
    pub criado_ms: i64,
    pub atualizado_ms: i64,
}

/// Um registro do histórico de estados de um goal.
#[derive(Debug, Clone, PartialEq)]
pub struct EventoGoal {
    pub momento_ms: i64,
    /// `None` na criação.
    pub de: Option<EstadoGoal>,
    pub para: EstadoGoal,
    pub motivo: String,
    /// Quem fez: "usuario", "abiyss" ou "kernel".
    pub autor: String,
}

/// Dados para criar um goal novo.
#[derive(Debug, Clone)]
pub struct NovoGoal {
    pub titulo: String,
    pub nucleo: String,
    pub descricao: String,
    pub prioridade: i64,
}

fn ler_estado(texto: &str) -> rusqlite::Result<EstadoGoal> {
    EstadoGoal::de_texto(texto).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("estado de goal desconhecido: {texto}").into(),
        )
    })
}

fn linha_para_goal(l: &rusqlite::Row<'_>) -> rusqlite::Result<Goal> {
    Ok(Goal {
        id: l.get(0)?,
        titulo: l.get(1)?,
        nucleo: l.get(2)?,
        descricao: l.get(3)?,
        prioridade: l.get(4)?,
        estado: ler_estado(&l.get::<_, String>(5)?)?,
        criado_ms: l.get(6)?,
        atualizado_ms: l.get(7)?,
    })
}

const COLUNAS: &str = "id, titulo, nucleo, descricao, prioridade, estado, criado_ms, atualizado_ms";

/// Cria um goal no estado `proposto` e registra o evento de criação.
pub fn criar(banco: &Banco, novo: &NovoGoal, autor: &str) -> Result<Goal, ErroGoal> {
    if novo.titulo.trim().is_empty() {
        return Err(ErroGoal::Invalido("o título não pode ser vazio".into()));
    }
    if novo.nucleo.trim().is_empty() {
        return Err(ErroGoal::Invalido(
            "o núcleo do goal não pode ser vazio".into(),
        ));
    }
    let agora = agora_ms();
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    transacao.execute(
        "INSERT INTO goals (titulo, nucleo, descricao, prioridade, estado, criado_ms, atualizado_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            novo.titulo.trim(),
            novo.nucleo.trim(),
            novo.descricao.trim(),
            novo.prioridade,
            EstadoGoal::Proposto.como_texto(),
            agora
        ],
    )?;
    let id = transacao.last_insert_rowid();
    transacao.execute(
        "INSERT INTO eventos_goal (goal_id, momento_ms, de, para, motivo, autor)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
        params![
            id,
            agora,
            EstadoGoal::Proposto.como_texto(),
            "goal criado",
            autor
        ],
    )?;
    transacao.commit()?;
    drop(conexao);
    obter(banco, id)
}

pub fn obter(banco: &Banco, id: i64) -> Result<Goal, ErroGoal> {
    banco
        .conexao()
        .query_row(
            &format!("SELECT {COLUNAS} FROM goals WHERE id = ?1"),
            params![id],
            linha_para_goal,
        )
        .optional()?
        .ok_or(ErroGoal::NaoEncontrado(id))
}

/// Move o goal para `destino`, se a transição for permitida.
/// Tudo numa transação: ou muda o estado E registra o evento, ou nada.
pub fn transicionar(
    banco: &Banco,
    id: i64,
    destino: EstadoGoal,
    motivo: &str,
    autor: &str,
) -> Result<Goal, ErroGoal> {
    if motivo.trim().is_empty() {
        return Err(ErroGoal::MotivoVazio);
    }
    let agora = agora_ms();
    let mut conexao = banco.conexao();
    let transacao = conexao.transaction()?;
    let atual: Option<String> = transacao
        .query_row("SELECT estado FROM goals WHERE id = ?1", params![id], |l| {
            l.get(0)
        })
        .optional()?;
    let Some(atual) = atual else {
        return Err(ErroGoal::NaoEncontrado(id));
    };
    let atual = ler_estado(&atual)?;
    if !atual.pode_ir_para(destino) {
        let permitidas = atual
            .proximos_permitidos()
            .iter()
            .map(|e| e.como_texto())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ErroGoal::TransicaoInvalida {
            de: atual,
            para: destino,
            permitidas: if permitidas.is_empty() {
                "nenhuma (estado final)".into()
            } else {
                permitidas
            },
        });
    }
    transacao.execute(
        "UPDATE goals SET estado = ?1, atualizado_ms = ?2 WHERE id = ?3",
        params![destino.como_texto(), agora, id],
    )?;
    transacao.execute(
        "INSERT INTO eventos_goal (goal_id, momento_ms, de, para, motivo, autor)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            agora,
            atual.como_texto(),
            destino.como_texto(),
            motivo.trim(),
            autor
        ],
    )?;
    transacao.commit()?;
    drop(conexao);
    obter(banco, id)
}

/// Lista goals. `incluir_finais = false` esconde concluídos e abandonados.
pub fn listar(banco: &Banco, incluir_finais: bool) -> Result<Vec<Goal>, ErroGoal> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(&format!(
        "SELECT {COLUNAS} FROM goals ORDER BY prioridade DESC, id ASC"
    ))?;
    let goals = consulta
        .query_map([], linha_para_goal)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(goals
        .into_iter()
        .filter(|g| incluir_finais || !g.estado.eh_terminal())
        .collect())
}

/// Histórico de transições de um goal, do mais antigo ao mais novo.
pub fn eventos(banco: &Banco, id: i64) -> Result<Vec<EventoGoal>, ErroGoal> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT momento_ms, de, para, motivo, autor FROM eventos_goal
         WHERE goal_id = ?1 ORDER BY id ASC",
    )?;
    let lista = consulta
        .query_map(params![id], |l| {
            let de: Option<String> = l.get(1)?;
            Ok(EventoGoal {
                momento_ms: l.get(0)?,
                de: match de {
                    Some(t) => Some(ler_estado(&t)?),
                    None => None,
                },
                para: ler_estado(&l.get::<_, String>(2)?)?,
                motivo: l.get(3)?,
                autor: l.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lista)
}

/// Quantos goals em cada estado.
pub fn contar_por_estado(banco: &Banco) -> Result<Vec<(EstadoGoal, i64)>, ErroGoal> {
    let mut contagem = Vec::new();
    let conexao = banco.conexao();
    for estado in EstadoGoal::TODOS {
        let n: i64 = conexao.query_row(
            "SELECT COUNT(*) FROM goals WHERE estado = ?1",
            params![estado.como_texto()],
            |l| l.get(0),
        )?;
        contagem.push((estado, n));
    }
    Ok(contagem)
}

/// Escolhe, por código, o goal em que o heartbeat deve focar:
/// o mais adiantado no ciclo (executando > validando > comprometido >
/// proposto), depois o de maior prioridade, depois o mais antigo.
/// Goals bloqueados ou finais não entram.
pub fn em_foco(goals: &[Goal]) -> Option<Goal> {
    fn ordem(estado: EstadoGoal) -> Option<u8> {
        match estado {
            EstadoGoal::Executando => Some(0),
            EstadoGoal::Validando => Some(1),
            EstadoGoal::Comprometido => Some(2),
            EstadoGoal::Proposto => Some(3),
            _ => None,
        }
    }
    goals
        .iter()
        .filter_map(|g| ordem(g.estado).map(|o| (o, -g.prioridade, g.id, g)))
        .min_by_key(|(o, p, id, _)| (*o, *p, *id))
        .map(|(_, _, _, g)| g.clone())
}

#[cfg(test)]
mod testes {
    use super::*;

    fn novo(titulo: &str, prioridade: i64) -> NovoGoal {
        NovoGoal {
            titulo: titulo.into(),
            nucleo: format!("núcleo de {titulo}"),
            descricao: String::new(),
            prioridade,
        }
    }

    #[test]
    fn caminho_feliz_registra_cada_transicao() {
        let banco = Banco::em_memoria().unwrap();
        let g = criar(&banco, &novo("aprender Rust", 1), "usuario").unwrap();
        assert_eq!(g.estado, EstadoGoal::Proposto);
        for (destino, motivo) in [
            (EstadoGoal::Comprometido, "vale a pena"),
            (EstadoGoal::Executando, "começando"),
            (EstadoGoal::Validando, "terminei o rascunho"),
            (EstadoGoal::Executando, "faltou o capítulo 3"),
            (EstadoGoal::Validando, "capítulo 3 feito"),
            (EstadoGoal::Concluido, "critério atendido"),
        ] {
            transicionar(&banco, g.id, destino, motivo, "abiyss").unwrap();
        }
        let historico = eventos(&banco, g.id).unwrap();
        assert_eq!(historico.len(), 7);
        assert_eq!(historico[0].de, None);
        assert_eq!(historico[0].autor, "usuario");
        assert_eq!(historico[4].motivo, "faltou o capítulo 3");
        assert_eq!(historico[6].para, EstadoGoal::Concluido);
        assert_eq!(obter(&banco, g.id).unwrap().estado, EstadoGoal::Concluido);
    }

    #[test]
    fn transicoes_invalidas_sao_recusadas_sem_registrar() {
        let banco = Banco::em_memoria().unwrap();
        let g = criar(&banco, &novo("x", 0), "usuario").unwrap();
        let erro =
            transicionar(&banco, g.id, EstadoGoal::Concluido, "pulei", "abiyss").unwrap_err();
        assert!(matches!(erro, ErroGoal::TransicaoInvalida { .. }));
        assert!(erro.to_string().contains("comprometido, abandonado"));
        assert!(matches!(
            transicionar(&banco, g.id, EstadoGoal::Comprometido, "  ", "abiyss"),
            Err(ErroGoal::MotivoVazio)
        ));
        assert!(matches!(
            transicionar(&banco, 999, EstadoGoal::Comprometido, "m", "abiyss"),
            Err(ErroGoal::NaoEncontrado(999))
        ));
        assert_eq!(eventos(&banco, g.id).unwrap().len(), 1);

        transicionar(&banco, g.id, EstadoGoal::Abandonado, "desisti", "usuario").unwrap();
        // Estado final: não sai mais.
        assert!(transicionar(&banco, g.id, EstadoGoal::Proposto, "volta", "usuario").is_err());
    }

    #[test]
    fn todas_as_transicoes_da_tabela() {
        use EstadoGoal::*;
        // Conferência explícita da tabela inteira (7 x 7).
        let permitidas = [
            (Proposto, Comprometido),
            (Proposto, Abandonado),
            (Comprometido, Executando),
            (Comprometido, Bloqueado),
            (Comprometido, Abandonado),
            (Executando, Validando),
            (Executando, Bloqueado),
            (Executando, Abandonado),
            (Validando, Concluido),
            (Validando, Executando),
            (Validando, Bloqueado),
            (Validando, Abandonado),
            (Bloqueado, Comprometido),
            (Bloqueado, Executando),
            (Bloqueado, Abandonado),
        ];
        for de in EstadoGoal::TODOS {
            for para in EstadoGoal::TODOS {
                assert_eq!(
                    de.pode_ir_para(para),
                    permitidas.contains(&(de, para)),
                    "{de} → {para}"
                );
            }
        }
    }

    #[test]
    fn foco_escolhido_por_codigo() {
        let banco = Banco::em_memoria().unwrap();
        let a = criar(&banco, &novo("a", 5), "usuario").unwrap();
        let b = criar(&banco, &novo("b", 1), "usuario").unwrap();
        let c = criar(&banco, &novo("c", 9), "usuario").unwrap();
        // Só propostos: vence a maior prioridade.
        assert_eq!(em_foco(&listar(&banco, false).unwrap()).unwrap().id, c.id);
        // Um executando vence qualquer proposto.
        transicionar(&banco, b.id, EstadoGoal::Comprometido, "m", "u").unwrap();
        transicionar(&banco, b.id, EstadoGoal::Executando, "m", "u").unwrap();
        assert_eq!(em_foco(&listar(&banco, false).unwrap()).unwrap().id, b.id);
        // Bloqueado sai do foco.
        transicionar(&banco, b.id, EstadoGoal::Bloqueado, "m", "u").unwrap();
        assert_eq!(em_foco(&listar(&banco, false).unwrap()).unwrap().id, c.id);
        let _ = a;
        assert_eq!(
            EstadoGoal::de_texto("Concluído"),
            Some(EstadoGoal::Concluido)
        );
    }
}
