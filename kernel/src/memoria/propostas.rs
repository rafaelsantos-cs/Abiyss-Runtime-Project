//! Fila de propostas de memória (tabela `propostas_memoria`).
//!
//! `memoria_propor` NUNCA grava no cofre: só coloca a proposta aqui, junto
//! com a origem calculada pelo kernel. Quem aplica (ou rejeita) é o
//! `abiyss sleep`.

use rusqlite::params;

use super::nota::{Fonte, Tipo};
use crate::db::Banco;
use crate::tempo::agora_ms;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstadoProposta {
    Pendente,
    Aplicada,
    Rejeitada,
}

impl EstadoProposta {
    pub fn como_texto(&self) -> &'static str {
        match self {
            EstadoProposta::Pendente => "pendente",
            EstadoProposta::Aplicada => "aplicada",
            EstadoProposta::Rejeitada => "rejeitada",
        }
    }

    pub fn de_texto(texto: &str) -> Option<EstadoProposta> {
        match texto {
            "pendente" => Some(EstadoProposta::Pendente),
            "aplicada" => Some(EstadoProposta::Aplicada),
            "rejeitada" => Some(EstadoProposta::Rejeitada),
            _ => None,
        }
    }
}

/// Dados para enfileirar uma proposta (já validados por `Memoria::propor`).
#[derive(Debug, Clone)]
pub struct NovaProposta {
    /// "interno" ou "externo".
    pub escopo: String,
    /// Caminho normalizado, relativo ao cofre.
    pub caminho: String,
    pub conteudo: String,
    pub tipo: Tipo,
    pub fonte: Fonte,
    /// Calculado pelo kernel: `Some(detalhe)` se o contexto que gerou a
    /// proposta tinha conteúdo externo.
    pub origem_externa: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Proposta {
    pub id: i64,
    pub criado_ms: i64,
    pub escopo: String,
    pub caminho: String,
    pub conteudo: String,
    pub tipo: String,
    pub fonte: String,
    pub origem_externa: Option<String>,
    pub estado: EstadoProposta,
    pub motivo: Option<String>,
    pub decidido_ms: Option<i64>,
}

const COLUNAS: &str = "id, criado_ms, escopo, caminho, conteudo, tipo, fonte, origem_externa, \
                       estado, motivo, decidido_ms";

fn linha_para_proposta(l: &rusqlite::Row<'_>) -> rusqlite::Result<Proposta> {
    let estado: String = l.get(8)?;
    Ok(Proposta {
        id: l.get(0)?,
        criado_ms: l.get(1)?,
        escopo: l.get(2)?,
        caminho: l.get(3)?,
        conteudo: l.get(4)?,
        tipo: l.get(5)?,
        fonte: l.get(6)?,
        origem_externa: l.get(7)?,
        estado: EstadoProposta::de_texto(&estado).unwrap_or(EstadoProposta::Pendente),
        motivo: l.get(9)?,
        decidido_ms: l.get(10)?,
    })
}

/// Enfileira e devolve o ID.
pub fn enfileirar(banco: &Banco, nova: &NovaProposta) -> anyhow::Result<i64> {
    let conexao = banco.conexao();
    conexao.execute(
        "INSERT INTO propostas_memoria
           (criado_ms, escopo, caminho, conteudo, tipo, fonte, origem_externa, estado)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pendente')",
        params![
            agora_ms(),
            nova.escopo,
            nova.caminho,
            nova.conteudo,
            nova.tipo.como_texto(),
            nova.fonte.como_texto(),
            nova.origem_externa
        ],
    )?;
    Ok(conexao.last_insert_rowid())
}

/// Pendentes, da mais antiga para a mais nova.
pub fn pendentes(banco: &Banco) -> anyhow::Result<Vec<Proposta>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(&format!(
        "SELECT {COLUNAS} FROM propostas_memoria WHERE estado = 'pendente' ORDER BY id ASC"
    ))?;
    let lista = consulta
        .query_map([], linha_para_proposta)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lista)
}

/// As mais recentes primeiro (todas as situações).
pub fn recentes(banco: &Banco, limite: usize) -> anyhow::Result<Vec<Proposta>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(&format!(
        "SELECT {COLUNAS} FROM propostas_memoria ORDER BY id DESC LIMIT ?1"
    ))?;
    let lista = consulta
        .query_map(params![limite as i64], linha_para_proposta)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lista)
}

/// Registra a decisão do sleep. Só muda propostas ainda pendentes.
pub fn decidir(banco: &Banco, id: i64, estado: EstadoProposta, motivo: &str) -> anyhow::Result<()> {
    banco.conexao().execute(
        "UPDATE propostas_memoria SET estado = ?1, motivo = ?2, decidido_ms = ?3
         WHERE id = ?4 AND estado = 'pendente'",
        params![estado.como_texto(), motivo, agora_ms(), id],
    )?;
    Ok(())
}

/// Uma linha do registro de operações da memória.
#[derive(Debug, Clone, PartialEq)]
pub struct EntradaRegistro {
    pub momento_ms: i64,
    /// "criada", "atualizada", "esquecida", "rejeitada"...
    pub acao: String,
    pub caminho: String,
    /// Nunca guarda o CONTEÚDO de uma nota (só referências como "proposta #3").
    pub detalhe: String,
}

/// Acrescenta uma linha ao registro de operações.
pub fn registrar(banco: &Banco, acao: &str, caminho: &str, detalhe: &str) -> anyhow::Result<()> {
    banco.conexao().execute(
        "INSERT INTO registro_memoria (momento_ms, acao, caminho, detalhe) VALUES (?1, ?2, ?3, ?4)",
        params![agora_ms(), acao, caminho, detalhe],
    )?;
    Ok(())
}

/// Registro mais recente primeiro.
pub fn registro(banco: &Banco, limite: usize) -> anyhow::Result<Vec<EntradaRegistro>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT momento_ms, acao, caminho, detalhe FROM registro_memoria
         ORDER BY id DESC LIMIT ?1",
    )?;
    let lista = consulta
        .query_map(params![limite as i64], |l| {
            Ok(EntradaRegistro {
                momento_ms: l.get(0)?,
                acao: l.get(1)?,
                caminho: l.get(2)?,
                detalhe: l.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lista)
}

/// Quando a nota foi esquecida pela última vez (se foi).
pub fn esquecida_em(banco: &Banco, caminho: &str) -> anyhow::Result<Option<i64>> {
    let momento = banco.conexao().query_row(
        "SELECT MAX(momento_ms) FROM registro_memoria WHERE caminho = ?1 AND acao = 'esquecida'",
        params![caminho],
        |l| l.get(0),
    )?;
    Ok(momento)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn enfileira_decide_e_registra() {
        let banco = Banco::em_memoria().unwrap();
        let id = enfileirar(
            &banco,
            &NovaProposta {
                escopo: "interno".into(),
                caminho: "01_internal/a.md".into(),
                conteudo: "x".into(),
                tipo: Tipo::Dito,
                fonte: Fonte::Conversa,
                origem_externa: None,
            },
        )
        .unwrap();
        assert_eq!(pendentes(&banco).unwrap().len(), 1);
        decidir(&banco, id, EstadoProposta::Aplicada, "ok").unwrap();
        // Decidir de novo não muda nada (já não está pendente).
        decidir(&banco, id, EstadoProposta::Rejeitada, "tarde").unwrap();
        let p = &recentes(&banco, 5).unwrap()[0];
        assert_eq!(p.estado, EstadoProposta::Aplicada);
        assert!(pendentes(&banco).unwrap().is_empty());

        assert_eq!(esquecida_em(&banco, "01_internal/a.md").unwrap(), None);
        registrar(&banco, "esquecida", "01_internal/a.md", "").unwrap();
        assert!(esquecida_em(&banco, "01_internal/a.md").unwrap().is_some());
        assert_eq!(registro(&banco, 5).unwrap()[0].acao, "esquecida");
    }
}
