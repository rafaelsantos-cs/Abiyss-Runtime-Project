//! `abiyss sleep` (versão mínima): aplica as propostas pendentes.
//!
//! Para cada proposta, na ordem em que chegaram:
//! - escopo interno com origem externa → REJEITADA (a regra dura);
//! - nota esquecida pelo usuário depois da proposta → rejeitada (não volta);
//! - o resto passa por `Cofre::gravar`, que confere procedência, tipos e as
//!   regras das notas externas. Erro ali também vira rejeição com motivo;
//! - escopo "central" (memória central): mesma regra dura; repetida não
//!   entra de novo; acima do orçamento é REJEITADA (nunca cortada).
//!
//! Cada decisão fica na própria proposta e no registro de operações.

use anyhow::{Context, bail};

use super::central::MemoriaCentral;
use super::cofre::{Cofre, REGRA_DURA};
use super::nota::{CAMPOS_DO_KERNEL, Escopo, Fonte, Procedencia, Tipo};
use super::propostas::{self, EstadoProposta, Proposta};
use crate::db::Banco;
use crate::frontmatter;

/// O que aconteceu com uma proposta.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisaoSleep {
    pub id: i64,
    pub caminho: String,
    pub aplicada: bool,
    /// "criada (dito, conversa)" ou o motivo da rejeição.
    pub detalhe: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RelatorioSleep {
    pub decisoes: Vec<DecisaoSleep>,
}

impl RelatorioSleep {
    pub fn aplicadas(&self) -> usize {
        self.decisoes.iter().filter(|d| d.aplicada).count()
    }

    pub fn rejeitadas(&self) -> usize {
        self.decisoes.iter().filter(|d| !d.aplicada).count()
    }
}

/// Aplica (ou rejeita) todas as propostas pendentes.
pub fn aplicar_pendentes(
    cofre: &Cofre,
    central: &MemoriaCentral,
    banco: &Banco,
) -> anyhow::Result<RelatorioSleep> {
    let mut relatorio = RelatorioSleep::default();
    for proposta in propostas::pendentes(banco)? {
        let rotulo = format!("proposta #{}", proposta.id);
        let resultado = if proposta.escopo == super::ESCOPO_CENTRAL {
            aplicar_central(central, &proposta)
        } else {
            aplicar(cofre, banco, &proposta)
        };
        let decisao = match resultado {
            Ok((acao, detalhe)) => {
                propostas::decidir(banco, proposta.id, EstadoProposta::Aplicada, &detalhe)?;
                propostas::registrar(banco, acao, &proposta.caminho, &rotulo)?;
                DecisaoSleep {
                    id: proposta.id,
                    caminho: proposta.caminho.clone(),
                    aplicada: true,
                    detalhe,
                }
            }
            Err(e) => {
                let motivo = format!("{e:#}");
                propostas::decidir(banco, proposta.id, EstadoProposta::Rejeitada, &motivo)?;
                propostas::registrar(banco, "rejeitada", &proposta.caminho, &rotulo)?;
                DecisaoSleep {
                    id: proposta.id,
                    caminho: proposta.caminho.clone(),
                    aplicada: false,
                    detalhe: motivo,
                }
            }
        };
        relatorio.decisoes.push(decisao);
    }
    Ok(relatorio)
}

/// Aplica uma proposta para a memória central.
fn aplicar_central(
    central: &MemoriaCentral,
    proposta: &Proposta,
) -> anyhow::Result<(&'static str, String)> {
    let tipo = Tipo::de_texto(&proposta.tipo)
        .with_context(|| format!("tipo desconhecido '{}'", proposta.tipo))?;
    // A memória central é interna: vale a mesma regra dura.
    if let Some(origem) = &proposta.origem_externa {
        bail!("{REGRA_DURA} nem na memória central (origem: {origem})");
    }
    if central.contem(&proposta.conteudo) {
        return Ok((
            "sem_mudanca",
            "já estava na memória central (nada repetido)".to_string(),
        ));
    }
    let uso = central.acrescentar(tipo, &proposta.conteudo)?;
    Ok((
        "acrescentada",
        format!(
            "acrescentada à memória central ({uso}/{} caracteres)",
            central.limite()
        ),
    ))
}

/// Aplica uma proposta. Devolve a ação ("criada"/"atualizada") e um resumo.
fn aplicar(
    cofre: &Cofre,
    banco: &Banco,
    proposta: &Proposta,
) -> anyhow::Result<(&'static str, String)> {
    let escopo = Escopo::de_texto(&proposta.escopo)
        .with_context(|| format!("escopo desconhecido '{}'", proposta.escopo))?;
    let tipo = Tipo::de_texto(&proposta.tipo)
        .with_context(|| format!("tipo desconhecido '{}'", proposta.tipo))?;
    let fonte = Fonte::de_texto(&proposta.fonte)
        .with_context(|| format!("fonte desconhecida '{}'", proposta.fonte))?;

    // A regra dura, com motivo claro no relatório (o `Cofre::gravar`
    // confere de novo: são duas barreiras independentes).
    if escopo == Escopo::Interno
        && let Some(origem) = &proposta.origem_externa
    {
        bail!("{REGRA_DURA} (origem: {origem})");
    }

    // O que o usuário mandou esquecer não volta por uma proposta antiga.
    if let Some(quando) = propostas::esquecida_em(banco, &proposta.caminho)?
        && quando >= proposta.criado_ms
    {
        bail!("a nota foi esquecida pelo usuário depois desta proposta");
    }

    let doc = frontmatter::separar(&proposta.conteudo)?;
    let ignorados: Vec<&str> = CAMPOS_DO_KERNEL
        .iter()
        .copied()
        .filter(|c| doc.campos.contains_key(*c))
        .collect();
    let procedencia = Procedencia {
        fonte,
        tipo,
        origem_externa: proposta.origem_externa.clone(),
        criado: None,
        rotulo: format!("proposta #{}", proposta.id),
    };
    let gravacao = cofre.gravar(&proposta.caminho, &doc, &procedencia)?;
    let mut detalhe = format!(
        "{} ({}, {})",
        gravacao.como_texto(),
        tipo.como_texto(),
        fonte.como_texto()
    );
    if !ignorados.is_empty() {
        detalhe.push_str(&format!(
            "; campos do kernel ignorados: {}",
            ignorados.join(", ")
        ));
    }
    Ok((gravacao.como_texto(), detalhe))
}
