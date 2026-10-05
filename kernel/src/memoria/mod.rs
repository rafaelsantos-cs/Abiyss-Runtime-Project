//! Memória de longo prazo do Abiyss, guardada num cofre do Obsidian.
//!
//! Dois escopos:
//! - `01_internal/`: pessoas, preferências, o auto-modelo, o diário
//!   pessoal, procedimentos. Toda nota tem procedência no frontmatter
//!   (`fonte`, `tipo`, `criado`, `atualizado`).
//! - `02_external/`: um MAPA de fontes, não uma enciclopédia: links
//!   canônicos, nível de navegador, `revalidar_apos` e resumos datados.
//!
//! Fluxo de escrita (o modelo nunca grava direto):
//!
//! ```text
//! memoria_propor ──► fila (SQLite) ──► abiyss sleep ──► Cofre::gravar
//!                    (origem calculada    (aplica ou       (único caminho
//!                     pelo kernel)         rejeita)         de escrita)
//! ```
//!
//! REGRA DURA (código, não instrução): conteúdo que veio de ferramentas,
//! web, sub-agentes ou qualquer fonte externa nunca é gravado em
//! 01_internal (nem na memória central). A origem é calculada pelo kernel
//! a partir do que estava no contexto quando o modelo pediu a gravação
//! (ver `ferramentas`).
//!
//! Além do cofre há a MEMÓRIA CENTRAL (`central`): um arquivo pequeno, com
//! orçamento em caracteres, injetado no system prompt em todo turno.

pub mod busca;
pub mod central;
pub mod cofre;
pub mod nota;
pub mod propostas;
pub mod sleep;

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::config::Config;
use crate::db::Banco;
use crate::frontmatter;
use busca::ResultadoBusca;
use central::MemoriaCentral;
use cofre::{Cofre, resolver_wikilink};
use nota::{Escopo, EscopoBusca, Fonte, Nota, Tipo, extrair_wikilinks};
use propostas::NovaProposta;
use sleep::RelatorioSleep;

/// Seção `[memoria]` do abiyss.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConfigMemoria {
    /// Pasta do cofre do Obsidian (relativa à raiz do projeto). Fica fora
    /// do git: tem dados pessoais.
    pub cofre: String,
    /// Máximo de resultados de `memoria_buscar`.
    pub max_resultados: usize,
    /// Tamanho máximo de uma nota lida por `memoria_ler`.
    pub max_bytes_nota: usize,
    /// Tamanho máximo do conteúdo de uma proposta.
    pub max_bytes_proposta: usize,
    /// Arquivo da memória central (relativo à raiz do projeto).
    pub central: String,
    /// Orçamento da memória central, em caracteres.
    pub limite_central_caracteres: usize,
}

impl Default for ConfigMemoria {
    fn default() -> Self {
        ConfigMemoria {
            cofre: "cofre".to_string(),
            max_resultados: 8,
            max_bytes_nota: 200_000,
            max_bytes_proposta: 20_000,
            central: "identity/memoria-central.md".to_string(),
            limite_central_caracteres: 4_000,
        }
    }
}

/// Escopo de `memoria_propor` que mira a memória central (não o cofre).
pub const ESCOPO_CENTRAL: &str = "central";
/// "Caminho" gravado nas propostas para a memória central.
pub const CAMINHO_CENTRAL: &str = "memoria-central";

/// Pedido de proposta (argumentos de `memoria_propor` + origem).
#[derive(Debug, Clone)]
pub struct PedidoProposta {
    pub escopo: String,
    pub caminho: String,
    pub conteudo: String,
    pub tipo: String,
    pub fonte: Fonte,
    /// Calculado pelo KERNEL (nunca pelo modelo).
    pub origem_externa: Option<String>,
}

/// Proposta aceita na fila.
#[derive(Debug, Clone, PartialEq)]
pub struct PropostaRegistrada {
    pub id: i64,
    pub caminho: String,
    /// Aviso para o modelo (ex.: "será rejeitada pela regra dura").
    pub aviso: Option<String>,
}

/// Resultado de uma busca.
#[derive(Debug, Clone, PartialEq)]
pub struct RespostaBusca {
    pub resultados: Vec<ResultadoBusca>,
    /// Qual motor respondeu ("texto").
    pub motor: String,
}

/// Uma nota lida, com os wikilinks já resolvidos.
#[derive(Debug, Clone, PartialEq)]
pub struct LeituraNota {
    pub nota: Nota,
    /// (alvo do link, nota encontrada)
    pub links: Vec<(String, Option<String>)>,
}

pub struct Memoria {
    config: ConfigMemoria,
    cofre: Cofre,
    central: MemoriaCentral,
    banco: Banco,
}

impl Memoria {
    /// Abre o cofre (criando as pastas dos escopos se preciso).
    pub fn abrir(config: &Config, banco: Banco) -> anyhow::Result<Memoria> {
        let cofre = Cofre::abrir(&config.caminho_cofre())?;
        Ok(Memoria {
            config: config.memoria.clone(),
            cofre,
            central: MemoriaCentral::da_config(config),
            banco,
        })
    }

    pub fn cofre(&self) -> &Cofre {
        &self.cofre
    }

    pub fn central(&self) -> &MemoriaCentral {
        &self.central
    }

    pub fn config(&self) -> &ConfigMemoria {
        &self.config
    }

    /// Busca por texto no(s) escopo(s) pedido(s).
    pub async fn buscar(
        &self,
        consulta: &str,
        escopo: EscopoBusca,
    ) -> anyhow::Result<RespostaBusca> {
        if consulta.trim().is_empty() {
            bail!("a consulta está vazia");
        }
        Ok(RespostaBusca {
            resultados: busca::buscar_texto(
                &self.cofre,
                consulta,
                escopo,
                self.config.max_resultados,
            ),
            motor: "texto".to_string(),
        })
    }

    /// Lê uma nota e resolve os wikilinks dela.
    pub fn ler(&self, caminho: &str) -> anyhow::Result<LeituraNota> {
        let nota = self.cofre.ler(caminho, self.config.max_bytes_nota)?;
        let todas = self.cofre.listar(None);
        let links = extrair_wikilinks(&nota.doc.corpo)
            .into_iter()
            .map(|alvo| {
                let achada = resolver_wikilink(&alvo, &todas);
                (alvo, achada)
            })
            .collect();
        Ok(LeituraNota { nota, links })
    }

    /// Valida e coloca uma proposta na fila. NÃO grava no cofre.
    /// Escopos: "interno", "externo" ou "central" (memória central).
    pub fn propor(&self, pedido: &PedidoProposta) -> anyhow::Result<PropostaRegistrada> {
        let central = pedido.escopo.trim().eq_ignore_ascii_case(ESCOPO_CENTRAL);
        let escopo = if central {
            None
        } else {
            Some(Escopo::de_texto(&pedido.escopo).with_context(|| {
                format!(
                    "escopo '{}' inválido: use 'interno', 'externo' ou 'central'",
                    pedido.escopo
                )
            })?)
        };
        let tipo = Tipo::de_texto(&pedido.tipo).with_context(|| {
            format!(
                "tipo '{}' inválido: use 'dito' (afirmado) ou 'deduzido' (inferido)",
                pedido.tipo
            )
        })?;
        let conteudo = pedido.conteudo.trim();
        if conteudo.is_empty() {
            bail!("o conteúdo está vazio");
        }
        if conteudo.len() > self.config.max_bytes_proposta {
            bail!(
                "conteúdo grande demais ({} bytes; limite {})",
                conteudo.len(),
                self.config.max_bytes_proposta
            );
        }
        let (texto_escopo, caminho) = match escopo {
            Some(escopo) => {
                // Frontmatter quebrado é recusado já, não só no sleep.
                frontmatter::separar(conteudo)?;
                let caminho = self.cofre.caminho_no_escopo(escopo, &pedido.caminho)?;
                (escopo.como_texto(), caminho)
            }
            None => {
                central::validar_texto(conteudo)?;
                (ESCOPO_CENTRAL, CAMINHO_CENTRAL.to_string())
            }
        };

        let id = propostas::enfileirar(
            &self.banco,
            &NovaProposta {
                escopo: texto_escopo.to_string(),
                caminho: caminho.clone(),
                conteudo: conteudo.to_string(),
                tipo,
                fonte: pedido.fonte,
                origem_externa: pedido.origem_externa.clone(),
            },
        )?;
        let interna = central || escopo == Some(Escopo::Interno);
        let mut aviso = match &pedido.origem_externa {
            Some(origem) if interna => Some(format!(
                "o contexto desta conversa tem conteúdo externo ({origem}); propostas para 01_internal \
                 ou para a memória central com essa origem são REJEITADAS no sleep. Se for um dado \
                 externo útil, proponha no escopo externo."
            )),
            _ => None,
        };
        // Memória central: avisa já se não vai caber (o sleep recusa).
        if central && aviso.is_none() {
            let uso = self.central.uso_com(tipo, conteudo);
            if uso > self.central.limite() {
                aviso = Some(format!(
                    "a memória central ficaria com {uso} caracteres (limite {}); do jeito que está, \
                     a proposta será recusada no sleep. Prefira uma nota em 01_internal.",
                    self.central.limite()
                ));
            }
        }
        Ok(PropostaRegistrada { id, caminho, aviso })
    }

    /// Aplica as propostas pendentes (`abiyss sleep`).
    pub fn sleep(&self) -> anyhow::Result<RelatorioSleep> {
        sleep::aplicar_pendentes(&self.cofre, &self.central, &self.banco)
    }

    /// Remove uma nota e registra QUE foi removida (nunca o conteúdo).
    pub fn esquecer(&self, caminho: &str) -> anyhow::Result<String> {
        let removida = self.cofre.apagar(caminho)?;
        propostas::registrar(&self.banco, "esquecida", &removida, "")?;
        Ok(removida)
    }
}
