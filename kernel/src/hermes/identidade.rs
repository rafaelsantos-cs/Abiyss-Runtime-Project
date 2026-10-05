//! `metacognition/identity/auto-modelo.json` e `SOUL.md` →
//! `01_internal/identidade/` (cópias fiéis) E `identity/nucleo.proposto.md`
//! (um RASCUNHO de núcleo para o usuário revisar).
//!
//! `identity/nucleo.md` nunca é tocado. O rascunho também nunca é
//! sobrescrito se já existir com outro conteúdo (o usuário pode tê-lo editado).

use serde_json::{Map, Value};

use super::campos::{Leitor, valor_como_texto};
use super::segredos::parece_conter_segredo;
use super::{Contexto, Secao};
use crate::frontmatter::Documento;
use crate::identidade::QUEM_SOU;
use crate::memoria::nota::{Fonte, Procedencia, Tipo};

const AUTO_MODELO: &str = "metacognition/identity/auto-modelo.json";
/// Onde procurar o SOUL.md, em ordem.
const SOULS: &[&str] = &["SOUL.md", "metacognition/identity/SOUL.md"];

const NOME: &[&str] = &["nome", "name"];
const PROPOSITO: &[&str] = &[
    "proposito",
    "propósito",
    "purpose",
    "mission",
    "missao",
    "missão",
];
const VALORES: &[&str] = &["valores", "values"];
const TRACOS: &[&str] = &["tracos", "traços", "traits", "personalidade", "personality"];
const CAPACIDADES: &[&str] = &[
    "capacidades",
    "strengths",
    "forcas",
    "forças",
    "capabilities",
    "skills",
];
const LIMITACOES: &[&str] = &[
    "limitacoes",
    "limitações",
    "weaknesses",
    "fraquezas",
    "limitations",
];
const ESTILO: &[&str] = &[
    "estilo_comunicacao",
    "communication_style",
    "estilo",
    "tom",
    "tone",
    "voz",
    "voice",
];

fn procedencia(tipo: Tipo, arquivo: &str) -> Procedencia {
    Procedencia {
        fonte: Fonte::Importacao,
        tipo,
        origem_externa: None,
        criado: None,
        rotulo: format!("importado de {arquivo}"),
    }
}

pub(super) fn importar(ctx: &mut Contexto<'_>) -> anyhow::Result<Secao> {
    let mut secao = Secao::nova(
        "Identidade: auto-modelo.json e SOUL.md → 01_internal/identidade/ + identity/nucleo.proposto.md",
    );

    // 1. Cópias fiéis no cofre.
    let mut auto_modelo: Option<String> = None;
    if ctx.tem(AUTO_MODELO) {
        match ctx.ler(AUTO_MODELO) {
            Ok(texto) => auto_modelo = Some(texto),
            Err(e) => secao.erros.push(format!("{e:#}")),
        }
    }
    let mut soul: Option<(&str, String)> = None;
    if let Some(arquivo) = SOULS.iter().copied().find(|a| ctx.tem(a)) {
        match ctx.ler(arquivo) {
            Ok(texto) => soul = Some((arquivo, texto)),
            Err(e) => secao.erros.push(format!("{e:#}")),
        }
    }
    if auto_modelo.is_none() && soul.is_none() {
        secao
            .linhas
            .push("(nenhum arquivo de identidade encontrado; rascunho não gerado)".into());
        return Ok(secao);
    }

    let mut objeto: Option<Map<String, Value>> = None;
    if let Some(texto) = &auto_modelo {
        match serde_json::from_str::<Value>(texto) {
            Ok(Value::Object(o)) => objeto = Some(o),
            _ => secao.linhas.push(format!(
                "! {AUTO_MODELO} não é um objeto JSON válido; copiado como texto e fora do rascunho"
            )),
        }
        // A cerca do bloco de código não pode aparecer dentro do texto.
        let cerca = if texto.contains("```") { "````" } else { "```" };
        let corpo = format!(
            "# Auto-modelo (importado do Hermes)\n\nCópia fiel de `{AUTO_MODELO}`, sem alterações. \
É como o Abiyss se descrevia no Hermes: inferências dele sobre si mesmo (por isso `tipo: deduzido`).\n\n\
{cerca}json\n{}\n{cerca}\n",
            texto.trim_end()
        );
        ctx.importar_nota(
            &mut secao,
            "hermes:identidade:auto-modelo",
            "01_internal/identidade/auto-modelo.md",
            Documento::sem_campos(&corpo),
            &procedencia(Tipo::Deduzido, AUTO_MODELO),
            AUTO_MODELO,
        )?;
    }
    if let Some((arquivo, texto)) = &soul {
        // O SOUL.md foi escrito pelo usuário para definir quem o Abiyss é.
        ctx.importar_nota(
            &mut secao,
            "hermes:identidade:soul",
            "01_internal/identidade/soul.md",
            Documento::sem_campos(texto),
            &procedencia(Tipo::Dito, arquivo),
            arquivo,
        )?;
        for (i, linha) in texto.lines().enumerate() {
            if linha.to_lowercase().contains("hermes") {
                secao.linhas.push(format!(
                    "⚠ {arquivo} linha {} menciona \"Hermes\": revise no rascunho (o Abiyss nunca se apresenta como Hermes)",
                    i + 1
                ));
            }
        }
    }

    // 2. Rascunho do núcleo (sem nada que tenha cara de segredo).
    if auto_modelo
        .as_deref()
        .and_then(parece_conter_segredo)
        .is_some()
    {
        objeto = None;
    }
    let soul_texto = soul
        .as_ref()
        .map(|(_, t)| t.as_str())
        .filter(|t| parece_conter_segredo(t).is_none());
    let rascunho = montar_rascunho(objeto.as_ref(), soul_texto, &mut secao);
    let destino = ctx.caminho_proposto.display().to_string();
    match std::fs::read_to_string(&ctx.caminho_proposto) {
        Ok(atual) if atual == rascunho => {
            secao.ja_importados += 1;
            secao.linhas.push(format!("= {destino} (igual ao atual)"));
        }
        Ok(_) => secao.linhas.push(format!(
            "! {destino} já existe e é diferente: NÃO sobrescrito (apague-o para gerar de novo)"
        )),
        Err(_) => {
            if ctx.aplicar {
                if let Some(pasta) = ctx.caminho_proposto.parent() {
                    std::fs::create_dir_all(pasta)?;
                }
                std::fs::write(&ctx.caminho_proposto, &rascunho)?;
            }
            secao.novos += 1;
            secao.linhas.push(format!(
                "+ {destino} (rascunho para revisar; identity/nucleo.md não é tocado)"
            ));
        }
    }
    Ok(secao)
}

/// Lista de textos a partir de lista, texto ou outro valor.
fn lista(valor: Option<&Value>) -> Vec<String> {
    match valor {
        Some(Value::Array(itens)) => itens
            .iter()
            .map(valor_como_texto)
            .filter(|t| !t.is_empty())
            .collect(),
        Some(v) => Some(valor_como_texto(v))
            .filter(|t| !t.is_empty())
            .into_iter()
            .collect(),
        None => Vec::new(),
    }
}

/// Monta o rascunho de núcleo. Mesmo conteúdo de entrada → mesmo texto
/// (sem data de geração): rodar de novo não muda nada.
pub fn montar_rascunho(
    auto_modelo: Option<&Map<String, Value>>,
    soul: Option<&str>,
    secao: &mut Secao,
) -> String {
    let mut proposito = None;
    let mut valores = Vec::new();
    let mut tracos = Vec::new();
    let mut capacidades = Vec::new();
    let mut limitacoes = Vec::new();
    let mut estilo = None;
    let mut nao_usados = Map::new();
    if let Some(objeto) = auto_modelo {
        let mut l = Leitor::novo(objeto);
        let nome = l.texto("nome", NOME);
        proposito = l.texto("proposito", PROPOSITO);
        valores = lista(l.valor("valores", VALORES));
        tracos = lista(l.valor("tracos", TRACOS));
        capacidades = lista(l.valor("capacidades", CAPACIDADES));
        limitacoes = lista(l.valor("limitacoes", LIMITACOES));
        estilo = l.texto("estilo", ESTILO);
        secao.anotar(&l);
        nao_usados = l.extras();
        if let Some(nome) = nome
            && !nome.eq_ignore_ascii_case("abiyss")
        {
            secao.linhas.push(format!(
                "⚠ o auto-modelo diz que o nome é '{nome}'; o rascunho usa Abiyss"
            ));
        }
    }
    let preencher = |o_que: &str| format!("{{{{PREENCHER: {o_que}}}}}");
    let itens = |lista: &[String], vazio: &str| -> String {
        if lista.is_empty() {
            format!("- {}", preencher(vazio))
        } else {
            lista
                .iter()
                .map(|v| format!("- {v}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let mut t = String::new();
    t.push_str("# Núcleo de identidade do Abiyss — RASCUNHO PROPOSTO\n\n");
    t.push_str(
        "> Gerado por `abiyss importar-hermes` a partir de `SOUL.md` e\n\
         > `metacognition/identity/auto-modelo.json`. **O kernel NÃO usa este arquivo.**\n\
         > Revise, corrija e copie para `identity/nucleo.md` só o que quiser manter\n\
         > (`identity/nucleo.md` nunca é alterado pela importação). Trechos com ⚠\n\
         > pedem atenção; `{{PREENCHER: ...}}` marca o que o Hermes não tinha.\n\n",
    );
    t.push_str(&format!("## Quem sou\n\n{QUEM_SOU}\n\n"));
    t.push_str(&format!(
        "## Propósito\n\n{}\n\n",
        proposito.unwrap_or_else(|| preencher("qual é a missão de longo prazo do Abiyss?"))
    ));
    t.push_str(&format!("## Valores\n\n{}\n\n", itens(&valores, "valores")));
    if !(tracos.is_empty() && capacidades.is_empty() && limitacoes.is_empty()) {
        t.push_str("## Como me vejo (do auto-modelo do Hermes)\n\n");
        for (titulo, lista) in [
            ("Traços", &tracos),
            ("Capacidades", &capacidades),
            ("Limitações", &limitacoes),
        ] {
            if !lista.is_empty() {
                t.push_str(&format!("- {titulo}: {}\n", lista.join("; ")));
            }
        }
        t.push('\n');
    }
    t.push_str(&format!(
        "## Com quem trabalho\n\n{}\n(Veja também a memória central importada de `memories/USER.md`.)\n\n",
        preencher("quem é o usuário, como prefere ser chamado, fuso horário, idioma preferido")
    ));
    t.push_str(&format!(
        "## Como me comunico\n\n{}\n\n",
        estilo
            .unwrap_or_else(|| preencher("tom, nível de detalhe, formato preferido das respostas"))
    ));
    t.push_str(&format!(
        "## Limites\n\n- Peço confirmação antes de qualquer ação irreversível.\n- {}\n",
        preencher("outros limites")
    ));

    if let Some(soul) = soul {
        t.push_str("\n## Do SOUL.md (Hermes) — revisar e incorporar acima\n\n");
        let mencoes = soul
            .lines()
            .filter(|l| l.to_lowercase().contains("hermes"))
            .count();
        if mencoes > 0 {
            t.push_str(&format!(
                "⚠ {mencoes} linha(s) mencionam \"Hermes\": o Abiyss nunca se apresenta como Hermes.\n\n"
            ));
        }
        // Em citação, para os títulos do SOUL não bagunçarem este arquivo.
        for linha in soul.trim().lines() {
            t.push_str(if linha.is_empty() { ">" } else { "> " });
            t.push_str(linha);
            t.push('\n');
        }
    }
    if !nao_usados.is_empty() {
        t.push_str("\n## Campos do auto-modelo que não entraram acima\n\n");
        for (chave, valor) in &nao_usados {
            t.push_str(&format!("- {chave}: {valor}\n"));
        }
    }
    t
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn rascunho_usa_o_auto_modelo_e_o_soul() {
        let auto = json!({
            "nome": "Abiyss",
            "proposito": "Ser um parceiro confiável.",
            "valores": ["honestidade", "curiosidade"],
            "tracos": ["direto"],
            "humor_atual": "otimista"
        });
        let mut secao = Secao::default();
        let r = montar_rascunho(
            auto.as_object(),
            Some("# SOUL\nVocê roda no Hermes."),
            &mut secao,
        );
        assert!(r.contains("RASCUNHO PROPOSTO"));
        assert!(r.contains("DepthAI"));
        assert!(r.contains("2026-09-27"));
        assert!(r.contains("masculino"));
        assert!(r.contains("Ser um parceiro confiável."));
        assert!(r.contains("- honestidade\n- curiosidade"));
        assert!(r.contains("- Traços: direto"));
        assert!(r.contains("> # SOUL"));
        assert!(r.contains("⚠ 1 linha(s) mencionam \"Hermes\""));
        assert!(r.contains("- humor_atual: \"otimista\""));
        // Sem estilo de comunicação no auto-modelo: fica para preencher.
        assert!(r.contains("{{PREENCHER: tom"));
        // Mesmas entradas, mesmo texto.
        assert_eq!(
            r,
            montar_rascunho(
                auto.as_object(),
                Some("# SOUL\nVocê roda no Hermes."),
                &mut Secao::default()
            )
        );
    }
}
