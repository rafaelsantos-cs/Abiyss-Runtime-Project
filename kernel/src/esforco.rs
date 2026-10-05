//! Tabela de esforço por modelo.
//!
//! O resto do kernel pensa em NÍVEIS abstratos de esforço — `minimal`, `low`,
//! `medium`, `high`, `xhigh`, `ultra` — e não no que cada modelo aceita. O
//! `abiyss.toml` traduz cada nível, modelo por modelo, para o que aquele
//! modelo realmente entende:
//!
//! - raciocínio ligado/desligado (`pensar`), gravado no campo do corpo da
//!   requisição indicado em `campo_pensar` (ex.: `chat_template_kwargs.enable_thinking`);
//! - orçamento de raciocínio (`orcamento`), no campo `campo_orcamento`;
//! - `max_tokens` e campos `extra` livres;
//! - ou OUTRO modelo como substituto (`usar_modelo`), quando este não tem
//!   o nível pedido.
//!
//! Capacidades que ainda não foram confirmadas no NIM ficam marcadas com
//! `a_confirmar = "..."`: o nível funciona (com o melhor palpite), mas a
//! resolução avisa que é um placeholder. Nível ausente da tabela também é
//! placeholder: usa os parâmetros padrão do modelo.
//!
//! Por cima dos níveis há dois MODOS:
//! - `raso` (shallow): `medium` para baixo;
//! - `profundo` (depth): `high` para cima.
//!
//! Este módulo só RESOLVE (config → `ConfigModelo` pronto para
//! `nim::montar_pedido`). Quem decide qual nível usar em cada situação
//! (chat, heartbeat, sub-agentes) ainda não usa esta tabela.

use std::collections::BTreeMap;

use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::config::{ConfigModelo, ConfigModelos};

/// Níveis abstratos, do mais leve ao mais pesado (a ordem importa: `Ord`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NivelEsforco {
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Ultra,
}

impl NivelEsforco {
    pub const TODOS: [NivelEsforco; 6] = [
        NivelEsforco::Minimal,
        NivelEsforco::Low,
        NivelEsforco::Medium,
        NivelEsforco::High,
        NivelEsforco::Xhigh,
        NivelEsforco::Ultra,
    ];

    pub fn como_texto(&self) -> &'static str {
        match self {
            NivelEsforco::Minimal => "minimal",
            NivelEsforco::Low => "low",
            NivelEsforco::Medium => "medium",
            NivelEsforco::High => "high",
            NivelEsforco::Xhigh => "xhigh",
            NivelEsforco::Ultra => "ultra",
        }
    }

    pub fn de_texto(texto: &str) -> Option<NivelEsforco> {
        let texto = texto.trim().to_lowercase();
        NivelEsforco::TODOS
            .into_iter()
            .find(|n| n.como_texto() == texto)
    }
}

/// Os dois modos expostos por cima da tabela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModoEsforco {
    /// Shallow: `minimal`, `low` e `medium`.
    Raso,
    /// Depth: `high`, `xhigh` e `ultra`.
    Profundo,
}

impl ModoEsforco {
    pub fn como_texto(&self) -> &'static str {
        match self {
            ModoEsforco::Raso => "raso",
            ModoEsforco::Profundo => "profundo",
        }
    }

    /// Aceita o nome em português e em inglês.
    pub fn de_texto(texto: &str) -> Option<ModoEsforco> {
        match texto.trim().to_lowercase().as_str() {
            "raso" | "shallow" => Some(ModoEsforco::Raso),
            "profundo" | "depth" => Some(ModoEsforco::Profundo),
            _ => None,
        }
    }

    /// O modo a que um nível pertence.
    pub fn do_nivel(nivel: NivelEsforco) -> ModoEsforco {
        if nivel <= NivelEsforco::Medium {
            ModoEsforco::Raso
        } else {
            ModoEsforco::Profundo
        }
    }

    /// Níveis do modo, do mais leve ao mais pesado.
    pub fn niveis(&self) -> &'static [NivelEsforco] {
        match self {
            ModoEsforco::Raso => &NivelEsforco::TODOS[..3],
            ModoEsforco::Profundo => &NivelEsforco::TODOS[3..],
        }
    }

    /// Nível usado quando ninguém pede um específico: a fronteira entre os
    /// modos (`medium` no raso, `high` no profundo).
    pub fn nivel_padrao(&self) -> NivelEsforco {
        match self {
            ModoEsforco::Raso => NivelEsforco::Medium,
            ModoEsforco::Profundo => NivelEsforco::High,
        }
    }

    /// Traz um nível pedido para dentro do modo (ex.: `ultra` no modo raso
    /// vira `medium`; `low` no modo profundo vira `high`).
    pub fn limitar(&self, nivel: NivelEsforco) -> NivelEsforco {
        let niveis = self.niveis();
        nivel.clamp(niveis[0], niveis[niveis.len() - 1])
    }
}

/// `[modelos.<papel>.esforco]` no `abiyss.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigEsforcoModelo {
    /// Campo (com pontos para objetos aninhados) que liga/desliga o
    /// raciocínio. Vazio = o modelo não tem esse botão.
    pub campo_pensar: String,
    /// Campo do orçamento de raciocínio (em tokens). Vazio = não aceita.
    pub campo_orcamento: String,
    /// Tradução de cada nível.
    pub niveis: BTreeMap<NivelEsforco, ConfigNivelEsforco>,
}

/// Um nível da tabela de um modelo.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigNivelEsforco {
    /// Liga (true) ou desliga (false) o raciocínio.
    pub pensar: Option<bool>,
    /// Orçamento de raciocínio em tokens.
    pub orcamento: Option<u64>,
    /// Substitui o `max_tokens` do modelo.
    pub max_tokens: Option<u32>,
    /// Campos extras mesclados por cima do `extra` do modelo.
    pub extra: Map<String, Value>,
    /// Usa OUTRO modelo de `[modelos]` neste nível (ex.: "cerebro"). Os
    /// campos acima são aplicados no modelo substituto, com os
    /// `campo_pensar`/`campo_orcamento` DELE.
    pub usar_modelo: Option<String>,
    /// Marca de placeholder: o que falta confirmar (no NIM real) sobre
    /// este nível. Presente = o nível ainda não é confiável.
    pub a_confirmar: Option<String>,
}

/// Resultado da resolução de (papel, nível).
#[derive(Debug, Clone)]
pub struct ModeloComEsforco {
    pub nivel: NivelEsforco,
    /// Papel pedido (ex.: "sub_low").
    pub papel_pedido: String,
    /// Papel cujo modelo será usado (difere do pedido quando há substituto).
    pub papel_usado: String,
    /// Pronto para `nim::montar_pedido`.
    pub modelo: ConfigModelo,
    /// `false` quando o nível é placeholder (`a_confirmar` ou ausente).
    pub confirmado: bool,
    /// Explicação do placeholder, se houver.
    pub nota: Option<String>,
}

/// Resolve um nível para um papel de `[modelos]`.
pub fn resolver(
    modelos: &ConfigModelos,
    papel: &str,
    nivel: NivelEsforco,
) -> anyhow::Result<ModeloComEsforco> {
    let base = modelos
        .por_papel(papel)
        .with_context(|| format!("modelo desconhecido: {papel}"))?;

    let Some(item) = base.esforco.niveis.get(&nivel) else {
        return Ok(ModeloComEsforco {
            nivel,
            papel_pedido: papel.to_string(),
            papel_usado: papel.to_string(),
            modelo: base.clone(),
            confirmado: false,
            nota: Some(format!(
                "nível {} não configurado para modelos.{papel}: usa os parâmetros padrão do modelo",
                nivel.como_texto()
            )),
        });
    };

    let papel_usado = item.usar_modelo.as_deref().unwrap_or(papel);
    let alvo = modelos
        .por_papel(papel_usado)
        .with_context(|| format!("modelos.{papel}: usar_modelo desconhecido: {papel_usado}"))?;

    let mut modelo = alvo.clone();
    if let Some(max_tokens) = item.max_tokens {
        modelo.max_tokens = Some(max_tokens);
    }
    mesclar(&mut modelo.extra, &item.extra);
    if let Some(pensar) = item.pensar {
        definir_campo(
            &mut modelo.extra,
            &alvo.esforco.campo_pensar,
            Value::Bool(pensar),
        )
        .with_context(|| format!("modelos.{papel_usado}.esforco.campo_pensar"))?;
    }
    if let Some(orcamento) = item.orcamento {
        definir_campo(
            &mut modelo.extra,
            &alvo.esforco.campo_orcamento,
            Value::from(orcamento),
        )
        .with_context(|| format!("modelos.{papel_usado}.esforco.campo_orcamento"))?;
    }

    Ok(ModeloComEsforco {
        nivel,
        papel_pedido: papel.to_string(),
        papel_usado: papel_usado.to_string(),
        modelo,
        confirmado: item.a_confirmar.is_none(),
        nota: item.a_confirmar.clone(),
    })
}

/// Resolve dentro de um modo: o nível pedido é trazido para dentro do
/// modo; sem pedido, usa o nível padrão do modo.
pub fn resolver_modo(
    modelos: &ConfigModelos,
    papel: &str,
    modo: ModoEsforco,
    pedido: Option<NivelEsforco>,
) -> anyhow::Result<ModeloComEsforco> {
    let nivel = match pedido {
        Some(nivel) => modo.limitar(nivel),
        None => modo.nivel_padrao(),
    };
    resolver(modelos, papel, nivel)
}

/// Checagens feitas ao carregar a config: um nível não pode pedir um botão
/// que o modelo (ou o substituto) não tem, nem apontar para um modelo que
/// não existe.
pub fn validar(modelos: &ConfigModelos) -> anyhow::Result<()> {
    for papel in ConfigModelos::PAPEIS {
        let base = modelos.por_papel(papel).expect("papel da lista fixa");
        for campo in [&base.esforco.campo_pensar, &base.esforco.campo_orcamento] {
            if !campo.is_empty() && campo.split('.').any(|parte| parte.trim().is_empty()) {
                bail!("modelos.{papel}.esforco: campo inválido \"{campo}\"");
            }
        }
        for (nivel, item) in &base.esforco.niveis {
            let onde = format!("modelos.{papel}.esforco.niveis.{}", nivel.como_texto());
            let alvo = match &item.usar_modelo {
                None => base,
                Some(outro) if outro == papel => {
                    bail!("{onde}: usar_modelo aponta para o próprio modelo")
                }
                Some(outro) => modelos.por_papel(outro).with_context(|| {
                    format!(
                        "{onde}: usar_modelo = \"{outro}\" não existe (use um de: {})",
                        ConfigModelos::PAPEIS.join(", ")
                    )
                })?,
            };
            if item.pensar.is_some() && alvo.esforco.campo_pensar.is_empty() {
                bail!("{onde}: `pensar` definido, mas o modelo não tem campo_pensar");
            }
            if item.orcamento.is_some() && alvo.esforco.campo_orcamento.is_empty() {
                bail!("{onde}: `orcamento` definido, mas o modelo não tem campo_orcamento");
            }
        }
    }
    Ok(())
}

/// Grava `valor` no caminho com pontos ("a.b.c"), criando os objetos que
/// faltarem. Caminho vazio = o modelo não aceita esse botão (erro).
fn definir_campo(
    destino: &mut Map<String, Value>,
    caminho: &str,
    valor: Value,
) -> anyhow::Result<()> {
    if caminho.is_empty() {
        bail!("o modelo não tem esse campo configurado");
    }
    let partes: Vec<&str> = caminho.split('.').collect();
    let (ultima, caminho_pais) = partes.split_last().expect("split nunca é vazio");
    let mut atual = destino;
    for parte in caminho_pais {
        let filho = atual
            .entry(parte.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !filho.is_object() {
            bail!("\"{parte}\" já existe e não é um objeto");
        }
        atual = filho.as_object_mut().expect("conferido acima");
    }
    atual.insert(ultima.to_string(), valor);
    Ok(())
}

/// Mescla `origem` por cima de `destino`: objetos são mesclados campo a
/// campo; qualquer outro valor é substituído.
fn mesclar(destino: &mut Map<String, Value>, origem: &Map<String, Value>) {
    for (chave, valor) in origem {
        match (destino.get_mut(chave), valor) {
            (Some(Value::Object(dentro)), Value::Object(novo)) => mesclar(dentro, novo),
            _ => {
                destino.insert(chave.clone(), valor.clone());
            }
        }
    }
}

/// Uma linha de texto por nível (para `abiyss esforco`).
pub fn descrever(modelos: &ConfigModelos, papel: &str) -> anyhow::Result<Vec<String>> {
    let mut linhas = Vec::new();
    for nivel in NivelEsforco::TODOS {
        let r = resolver(modelos, papel, nivel)?;
        let substituto = if r.papel_usado != r.papel_pedido {
            format!(" → usa {}", r.papel_usado)
        } else {
            String::new()
        };
        let extra = if r.modelo.extra.is_empty() {
            "{}".to_string()
        } else {
            Value::Object(r.modelo.extra.clone()).to_string()
        };
        let max_tokens = r
            .modelo
            .max_tokens
            .map(|n| n.to_string())
            .unwrap_or_else(|| "padrão".to_string());
        let marca = match &r.nota {
            Some(nota) => format!("  [A CONFIRMAR: {nota}]"),
            None => String::new(),
        };
        linhas.push(format!(
            "{:<8} {:<9} {}{substituto}  max_tokens={max_tokens}  extra={extra}{marca}",
            nivel.como_texto(),
            ModoEsforco::do_nivel(nivel).como_texto(),
            r.modelo.id,
        ));
    }
    Ok(linhas)
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::config::Config;
    use serde_json::json;

    const CONFIG: &str = r#"
        [nim]
        base_url = "http://127.0.0.1:9/v1"

        [modelos.cerebro]
        id = "teste/cerebro"
        max_tokens = 1000
        extra = { chat_template_kwargs = { enable_thinking = true, clear_thinking = false } }
        [modelos.cerebro.esforco]
        campo_pensar = "chat_template_kwargs.enable_thinking"
        campo_orcamento = "chat_template_kwargs.thinking_budget"
        [modelos.cerebro.esforco.niveis]
        minimal = { pensar = false, max_tokens = 200 }
        medium = { pensar = true, orcamento = 4096 }
        ultra = { pensar = true, extra = { reasoning_effort = "max" }, a_confirmar = "formato do esforço máximo" }

        [modelos.sub_ultra]
        id = "teste/ultra"
        [modelos.sub_medium]
        id = "teste/medium"
        [modelos.sub_low]
        id = "teste/low"
        [modelos.sub_low.esforco.niveis]
        xhigh = { usar_modelo = "cerebro", pensar = true }

        [pools.cerebro]
        api_key_env = "A"
        [pools.subagentes]
        api_key_env = "B"
    "#;

    fn modelos() -> ConfigModelos {
        Config::de_texto(CONFIG).unwrap().modelos
    }

    #[test]
    fn modos_cobrem_os_niveis_sem_sobreposicao() {
        assert_eq!(
            ModoEsforco::Raso.niveis(),
            &[
                NivelEsforco::Minimal,
                NivelEsforco::Low,
                NivelEsforco::Medium
            ]
        );
        assert_eq!(
            ModoEsforco::Profundo.niveis(),
            &[NivelEsforco::High, NivelEsforco::Xhigh, NivelEsforco::Ultra]
        );
        for nivel in NivelEsforco::TODOS {
            assert!(ModoEsforco::do_nivel(nivel).niveis().contains(&nivel));
        }
        assert_eq!(
            ModoEsforco::Raso.limitar(NivelEsforco::Ultra),
            NivelEsforco::Medium
        );
        assert_eq!(
            ModoEsforco::Profundo.limitar(NivelEsforco::Low),
            NivelEsforco::High
        );
        assert_eq!(
            ModoEsforco::Profundo.limitar(NivelEsforco::Xhigh),
            NivelEsforco::Xhigh
        );
        assert_eq!(ModoEsforco::de_texto("depth"), Some(ModoEsforco::Profundo));
        assert_eq!(ModoEsforco::de_texto("raso"), Some(ModoEsforco::Raso));
        assert_eq!(NivelEsforco::de_texto(" XHigh "), Some(NivelEsforco::Xhigh));
        assert_eq!(NivelEsforco::de_texto("max"), None);
    }

    #[test]
    fn pensar_e_orcamento_vao_para_os_campos_do_modelo() {
        let m = modelos();
        let minimo = resolver(&m, "cerebro", NivelEsforco::Minimal).unwrap();
        assert!(minimo.confirmado);
        assert_eq!(minimo.modelo.max_tokens, Some(200));
        assert_eq!(
            Value::Object(minimo.modelo.extra),
            json!({"chat_template_kwargs": {"enable_thinking": false, "clear_thinking": false}})
        );

        let medio = resolver(&m, "cerebro", NivelEsforco::Medium).unwrap();
        assert_eq!(medio.modelo.max_tokens, Some(1000));
        assert_eq!(
            medio.modelo.extra["chat_template_kwargs"],
            json!({"enable_thinking": true, "clear_thinking": false, "thinking_budget": 4096})
        );
    }

    #[test]
    fn placeholders_ficam_marcados() {
        let m = modelos();
        let ultra = resolver(&m, "cerebro", NivelEsforco::Ultra).unwrap();
        assert!(!ultra.confirmado);
        assert_eq!(ultra.nota.as_deref(), Some("formato do esforço máximo"));
        assert_eq!(ultra.modelo.extra["reasoning_effort"], json!("max"));

        // Nível ausente da tabela: parâmetros padrão + aviso.
        let alto = resolver(&m, "cerebro", NivelEsforco::High).unwrap();
        assert!(!alto.confirmado);
        assert!(alto.nota.unwrap().contains("não configurado"));
        assert_eq!(alto.modelo.extra, m.cerebro.extra);
    }

    #[test]
    fn substituto_usa_o_outro_modelo_com_os_campos_dele() {
        let m = modelos();
        let r = resolver(&m, "sub_low", NivelEsforco::Xhigh).unwrap();
        assert_eq!(r.papel_usado, "cerebro");
        assert_eq!(r.modelo.id, "teste/cerebro");
        assert_eq!(
            r.modelo.extra["chat_template_kwargs"]["enable_thinking"],
            json!(true)
        );

        // Pelo modo profundo, sem nível pedido, cai em `high` (não configurado).
        let r = resolver_modo(&m, "sub_low", ModoEsforco::Profundo, None).unwrap();
        assert_eq!(r.nivel, NivelEsforco::High);
        assert_eq!(r.modelo.id, "teste/low");
        // E pedir `ultra` no modo raso vira `medium`.
        let r = resolver_modo(&m, "cerebro", ModoEsforco::Raso, Some(NivelEsforco::Ultra)).unwrap();
        assert_eq!(r.nivel, NivelEsforco::Medium);
    }

    #[test]
    fn config_invalida_e_recusada_ao_carregar() {
        // `pensar` num modelo sem campo_pensar.
        let texto = CONFIG.replace(
            "xhigh = { usar_modelo = \"cerebro\", pensar = true }",
            "xhigh = { pensar = true }",
        );
        assert!(Config::de_texto(&texto).is_err());
        // Substituto inexistente.
        let texto = CONFIG.replace("usar_modelo = \"cerebro\"", "usar_modelo = \"gpt\"");
        assert!(Config::de_texto(&texto).is_err());
        // Nível com nome errado.
        let texto = CONFIG.replace("minimal = {", "maximo = {");
        assert!(Config::de_texto(&texto).is_err());
        // Campo desconhecido num nível (erro de digitação não passa calado).
        let texto = CONFIG.replace("pensar = false, max_tokens", "pensa = false, max_tokens");
        assert!(Config::de_texto(&texto).is_err());
    }

    #[test]
    fn abiyss_toml_do_repositorio_e_valido_e_completo() {
        let config = Config::de_texto(include_str!("../../abiyss.toml")).unwrap();
        for papel in ConfigModelos::PAPEIS {
            let tabela = &config.modelos.por_papel(papel).unwrap().esforco.niveis;
            // Todo nível está escrito (mesmo que como placeholder documentado).
            assert_eq!(tabela.len(), 6, "modelos.{papel} sem os seis níveis");
            for nivel in NivelEsforco::TODOS {
                resolver(&config.modelos, papel, nivel).unwrap();
            }
        }
    }

    #[test]
    fn descrever_mostra_os_seis_niveis() {
        let linhas = descrever(&modelos(), "cerebro").unwrap();
        assert_eq!(linhas.len(), 6);
        assert!(linhas[5].contains("A CONFIRMAR"));
        assert!(linhas[0].starts_with("minimal"));
    }
}
