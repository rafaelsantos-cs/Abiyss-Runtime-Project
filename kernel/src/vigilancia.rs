//! Vigilância do heartbeat: estagnação e disjuntor.
//!
//! **Estagnação.** Cada ciclo que chamou o modelo ganha uma IMPRESSÃO
//! DIGITAL (o goal em foco + as ações normalizadas). A mesma impressão em
//! `repeticoes_estagnacao` ciclos seguidos sem o goal em foco mudar, ou a
//! mesma ação falhando `erros_mesma_acao` vezes seguidas, é estagnação: o
//! kernel publica um evento `kernel/estagnacao` (que traz a skill
//! `sair-de-loops` junto) e espaça a revisão periódica daquele goal (2×,
//! 4×, até `max_multiplicador_revisao`×) até ele mudar.
//!
//! **Disjuntor.** `falhas_para_abrir` ciclos seguidos com falha (chamada ao
//! modelo que falhou, resposta fora do formato ou ciclo interrompido) abrem
//! o disjuntor: o heartbeat não chama o modelo por `min(base·2^k, teto)`
//! (com sorteio de ±20%). Depois, UMA tentativa (meio-aberto): deu certo,
//! fecha; falhou, abre de novo por mais tempo. O estado fica em
//! `estado_daemon` e sobrevive a reinícios.
//!
//! Tudo que decide é função pura (recebe `agora`, contagens e config).

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::daemon;
use crate::db::Banco;
use crate::tempo::formatar_ms;

/// Chave em `estado_daemon` do disjuntor.
pub const CHAVE_DISJUNTOR: &str = "disjuntor_heartbeat";
/// Chave em `estado_daemon` da última estagnação.
pub const CHAVE_ESTAGNACAO: &str = "estagnacao";
/// Origem do evento `kernel` de estagnação.
pub const ORIGEM_ESTAGNACAO: &str = "estagnacao";

/// Caracteres da tarefa que entram na impressão (o começo basta).
const PREFIXO_TAREFA: usize = 48;
/// Linhas do diário olhadas para contar erros seguidos da mesma ação.
const JANELA_DIARIO: i64 = 60;

/// `[vigilancia]` no abiyss.toml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigVigilancia {
    /// Mesma impressão neste número de ciclos seguidos = estagnação.
    pub repeticoes_estagnacao: u32,
    /// A mesma ação falhando este número de vezes seguidas = estagnação.
    pub erros_mesma_acao: u32,
    /// Teto do multiplicador da revisão periódica de um goal estagnado.
    pub max_multiplicador_revisao: u32,
    /// Ciclos seguidos com falha que abrem o disjuntor.
    pub falhas_para_abrir: u32,
    pub disjuntor_base_segundos: u64,
    pub disjuntor_teto_segundos: u64,
}

impl Default for ConfigVigilancia {
    fn default() -> Self {
        ConfigVigilancia {
            repeticoes_estagnacao: 3,
            erros_mesma_acao: 3,
            max_multiplicador_revisao: 8,
            falhas_para_abrir: 3,
            disjuntor_base_segundos: 300,
            disjuntor_teto_segundos: 3600,
        }
    }
}

impl ConfigVigilancia {
    pub fn validar(&self) -> anyhow::Result<()> {
        if self.repeticoes_estagnacao < 2 || self.erros_mesma_acao < 2 {
            anyhow::bail!(
                "vigilancia.repeticoes_estagnacao e vigilancia.erros_mesma_acao precisam ser >= 2"
            );
        }
        if self.max_multiplicador_revisao == 0 || self.falhas_para_abrir == 0 {
            anyhow::bail!(
                "vigilancia.max_multiplicador_revisao e vigilancia.falhas_para_abrir precisam ser > 0"
            );
        }
        if self.disjuntor_base_segundos == 0
            || self.disjuntor_teto_segundos < self.disjuntor_base_segundos
        {
            anyhow::bail!(
                "vigilancia.disjuntor_base_segundos precisa ser > 0 e <= disjuntor_teto_segundos"
            );
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Impressão digital
// ---------------------------------------------------------------------------

/// Minúsculas, só letras e números, espaços colapsados, até `max` caracteres.
fn normalizar(texto: &str, max: usize) -> String {
    texto
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

/// Chave normalizada de uma ação: tipo, goal, destino ou nível e o começo
/// da tarefa. `aguardar` não tem chave (esperar não é repetir uma tentativa).
pub fn chave_acao(acao: &Value) -> Option<String> {
    let tipo = acao["tipo"].as_str().unwrap_or("").trim().to_lowercase();
    if tipo.is_empty() || tipo == "aguardar" {
        return None;
    }
    let mut partes = vec![tipo];
    if let Some(g) = acao["goal_id"].as_i64() {
        partes.push(format!("goal={g}"));
    }
    for campo in ["para", "nivel", "nome", "referencia"] {
        if let Some(v) = acao[campo].as_str().filter(|v| !v.trim().is_empty()) {
            partes.push(normalizar(v, PREFIXO_TAREFA));
        }
    }
    if let Some(id) = acao["id"].as_i64() {
        partes.push(format!("id={id}"));
    }
    if let Some(t) = acao["tarefa"].as_str() {
        partes.push(normalizar(t, PREFIXO_TAREFA));
    }
    Some(partes.join(":"))
}

/// Impressão digital de um ciclo: o goal em foco + as ações normalizadas,
/// em ordem. `None` quando não há ação (além de aguardar).
pub fn impressao(foco: Option<i64>, acoes: &[Value]) -> Option<String> {
    let mut chaves: Vec<String> = acoes.iter().filter_map(chave_acao).collect();
    if chaves.is_empty() {
        return None;
    }
    chaves.sort();
    let foco = foco.map(|g| format!("#{g}")).unwrap_or_else(|| "-".into());
    Some(format!("foco {foco} | {}", chaves.join(" ; ")))
}

/// Quantas das impressões (da mais recente para a mais antiga) são iguais
/// à primeira, seguidas. Impressão vazia não conta.
pub fn repeticoes(impressoes: &[Option<String>]) -> usize {
    let Some(Some(primeira)) = impressoes.first() else {
        return 0;
    };
    impressoes
        .iter()
        .take_while(|i| i.as_deref() == Some(primeira.as_str()))
        .count()
}

/// Tentativas seguidas que falharam desta ação, olhando as tentativas da
/// mais recente para a mais antiga: (chave, falhou).
pub fn erros_seguidos(tentativas: &[(Option<String>, bool)], chave: &str) -> usize {
    tentativas
        .iter()
        .filter(|(c, _)| c.as_deref() == Some(chave))
        .take_while(|(_, falhou)| *falhou)
        .count()
}

/// Estado guardado da última estagnação (para espaçar a revisão).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EstadoEstagnacao {
    pub goal_id: Option<i64>,
    /// `atualizado_ms` do goal quando estagnou: mudou = acabou a estagnação.
    pub goal_atualizado_ms: i64,
    pub multiplicador: u32,
    /// Ciclos antes disto não contam para a próxima estagnação.
    pub desde_ms: i64,
}

/// Multiplicador da revisão periódica de um goal: 1 normalmente; o da
/// estagnação enquanto o goal não mudar.
pub fn multiplicador_revisao(
    estado: Option<&EstadoEstagnacao>,
    goal_id: i64,
    goal_atualizado_ms: i64,
) -> u32 {
    match estado {
        Some(e) if e.goal_id == Some(goal_id) && e.goal_atualizado_ms == goal_atualizado_ms => {
            e.multiplicador.max(1)
        }
        _ => 1,
    }
}

/// Estado depois de uma nova estagnação: dobra o multiplicador se é o
/// mesmo goal parado; senão começa em 2×.
pub fn nova_estagnacao(
    anterior: Option<&EstadoEstagnacao>,
    goal_id: Option<i64>,
    goal_atualizado_ms: i64,
    agora: i64,
    max: u32,
) -> EstadoEstagnacao {
    let multiplicador = match anterior {
        Some(e) if e.goal_id == goal_id && e.goal_atualizado_ms == goal_atualizado_ms => {
            (e.multiplicador.max(1) * 2).min(max)
        }
        _ => 2.min(max),
    };
    EstadoEstagnacao {
        goal_id,
        goal_atualizado_ms,
        multiplicador,
        desde_ms: agora,
    }
}

pub fn ler_estagnacao(banco: &Banco) -> anyhow::Result<Option<EstadoEstagnacao>> {
    Ok(daemon::ler_estado(banco, CHAVE_ESTAGNACAO)?.and_then(|v| serde_json::from_str(&v).ok()))
}

/// Impressões dos últimos `n` ciclos com modelo desde `desde_ms` (mais
/// recente primeiro), com o início de cada um.
pub fn ultimas_impressoes(
    banco: &Banco,
    desde_ms: i64,
    n: usize,
) -> anyhow::Result<Vec<(i64, Option<String>)>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT inicio_ms, impressao FROM ciclos
         WHERE chamou_modelo = 1 AND inicio_ms >= ?1
         ORDER BY id DESC LIMIT ?2",
    )?;
    let linhas = consulta
        .query_map(params![desde_ms, n as i64], |l| Ok((l.get(0)?, l.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(linhas)
}

/// Tentativas recentes do heartbeat no diário: (chave da ação, falhou).
pub fn tentativas_recentes(banco: &Banco) -> anyhow::Result<Vec<(Option<String>, bool)>> {
    let conexao = banco.conexao();
    let mut consulta = conexao.prepare(
        "SELECT acao, resultado FROM diario
         WHERE origem = 'heartbeat' AND resultado IS NOT NULL
         ORDER BY id DESC LIMIT ?1",
    )?;
    let linhas: Vec<(String, String)> = consulta
        .query_map(params![JANELA_DIARIO], |l| Ok((l.get(0)?, l.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(linhas
        .into_iter()
        .map(|(acao, resultado)| {
            let chave = serde_json::from_str::<Value>(&acao)
                .ok()
                .and_then(|v| chave_acao(&v));
            (chave, resultado.starts_with("erro:"))
        })
        .collect())
}

/// Texto do empurrão de estagnação por decisão repetida.
pub fn aviso_repeticao(vezes: usize, goal: Option<i64>, impressao: &str) -> String {
    let goal = goal
        .map(|g| format!("o goal #{g}"))
        .unwrap_or_else(|| "nada".into());
    format!(
        "Estagnação: você decidiu a mesma coisa {vezes} vezes seguidas sem {goal} mudar \
         ({impressao}). Não repita: mude a abordagem, dê um passo menor, delegue de outro \
         jeito (outro nível ou mais contexto), bloqueie o goal com motivo ou peça ajuda ao \
         usuário. A revisão periódica deste goal vai ficar mais espaçada até ele mudar."
    )
}

/// Texto do empurrão de estagnação por erro repetido.
pub fn aviso_erros(vezes: usize, chave: &str, ultimo_erro: &str) -> String {
    format!(
        "Estagnação: a ação \"{chave}\" falhou {vezes} vezes seguidas (último erro: \
         {ultimo_erro}). Não tente de novo do mesmo jeito: leia o erro, mude a ação ou os \
         argumentos, bloqueie o goal com motivo ou peça ajuda ao usuário."
    )
}

// ---------------------------------------------------------------------------
// Disjuntor
// ---------------------------------------------------------------------------

/// Estado do disjuntor do heartbeat (guardado como JSON).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EstadoDisjuntor {
    /// Ciclos seguidos com falha.
    pub falhas_seguidas: u32,
    /// Aberto até este instante (`None` = fechado).
    pub aberto_ate_ms: Option<i64>,
    /// Quantas vezes abriu seguidas (o expoente da espera).
    pub aberturas: u32,
    /// Último erro (para o status).
    pub ultimo_erro: Option<String>,
}

/// Situação do disjuntor agora.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Situacao {
    Fechado,
    /// Aberto: nenhuma chamada até `ate_ms`.
    Aberto {
        ate_ms: i64,
    },
    /// A espera passou: uma tentativa decide.
    MeioAberto,
}

pub fn situacao(estado: &EstadoDisjuntor, agora: i64) -> Situacao {
    match estado.aberto_ate_ms {
        None => Situacao::Fechado,
        Some(ate) if agora < ate => Situacao::Aberto { ate_ms: ate },
        Some(_) => Situacao::MeioAberto,
    }
}

/// Espera da `k`-ésima abertura seguida (k começa em 0), em ms:
/// `min(base·2^k, teto)` vezes um sorteio entre 0,8 e 1,2 (`sorteio` em [0, 1)).
pub fn espera_ms(config: &ConfigVigilancia, k: u32, sorteio: f64) -> i64 {
    let base = config.disjuntor_base_segundos as f64 * 1000.0;
    let teto = config.disjuntor_teto_segundos as f64 * 1000.0;
    let bruto = (base * 2f64.powi(k.min(30) as i32)).min(teto);
    (bruto * (0.8 + 0.4 * sorteio.clamp(0.0, 1.0))) as i64
}

/// Estado depois de um ciclo com chamada ao modelo.
pub fn apos_ciclo(
    estado: &EstadoDisjuntor,
    sucesso: bool,
    erro: Option<&str>,
    agora: i64,
    config: &ConfigVigilancia,
    sorteio: f64,
) -> EstadoDisjuntor {
    if sucesso {
        return EstadoDisjuntor::default();
    }
    let falhas = estado.falhas_seguidas + 1;
    let meio_aberto = situacao(estado, agora) == Situacao::MeioAberto;
    let mut novo = EstadoDisjuntor {
        falhas_seguidas: falhas,
        aberto_ate_ms: None,
        aberturas: estado.aberturas,
        ultimo_erro: erro.map(|e| e.chars().take(300).collect()),
    };
    // Abre ao chegar no limite; no meio-aberto, uma falha já reabre.
    if meio_aberto || falhas >= config.falhas_para_abrir {
        novo.aberto_ate_ms = Some(agora + espera_ms(config, estado.aberturas, sorteio));
        novo.aberturas = estado.aberturas + 1;
    }
    novo
}

pub fn ler_disjuntor(banco: &Banco) -> anyhow::Result<EstadoDisjuntor> {
    Ok(daemon::ler_estado(banco, CHAVE_DISJUNTOR)?
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default())
}

pub fn gravar_disjuntor(banco: &Banco, estado: &EstadoDisjuntor) -> anyhow::Result<()> {
    daemon::gravar_estado(banco, CHAVE_DISJUNTOR, &serde_json::to_string(estado)?)
}

/// Registra o desfecho de um ciclo que chamou (ou tentou chamar) o modelo.
pub fn registrar_no_disjuntor(
    banco: &Banco,
    config: &ConfigVigilancia,
    sucesso: bool,
    erro: Option<&str>,
    agora: i64,
) -> anyhow::Result<EstadoDisjuntor> {
    let antes = ler_disjuntor(banco)?;
    let depois = apos_ciclo(&antes, sucesso, erro, agora, config, fastrand::f64());
    if depois != antes {
        gravar_disjuntor(banco, &depois)?;
        if depois.aberto_ate_ms.is_some() && antes.aberto_ate_ms != depois.aberto_ate_ms {
            tracing::warn!(
                "disjuntor do heartbeat ABERTO até {} ({} falha(s) seguida(s))",
                formatar_ms(depois.aberto_ate_ms.unwrap_or(agora)),
                depois.falhas_seguidas
            );
        } else if sucesso && antes.aberto_ate_ms.is_some() {
            tracing::info!("disjuntor do heartbeat fechado: o modelo voltou a responder");
        }
    }
    Ok(depois)
}

/// Uma linha para status e interocepção (`None` = fechado e sem falhas).
pub fn descrever_disjuntor(estado: &EstadoDisjuntor, agora: i64) -> Option<String> {
    match situacao(estado, agora) {
        Situacao::Fechado if estado.falhas_seguidas == 0 => None,
        Situacao::Fechado => Some(format!(
            "fechado, {} falha(s) seguida(s) no heartbeat",
            estado.falhas_seguidas
        )),
        Situacao::Aberto { ate_ms } => Some(format!(
            "ABERTO até {} ({} falha(s) seguida(s); último erro: {})",
            formatar_ms(ate_ms),
            estado.falhas_seguidas,
            estado.ultimo_erro.as_deref().unwrap_or("?")
        )),
        Situacao::MeioAberto => Some(format!(
            "meio-aberto: a próxima chamada decide ({} falha(s) seguida(s))",
            estado.falhas_seguidas
        )),
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn impressao_normaliza_e_ignora_aguardar() {
        let a = vec![
            json!({"tipo": "delegar", "nivel": "low", "goal_id": 3,
                   "tarefa": "Pesquisar  PREÇOS de VPS!", "expectativa": "x"}),
            json!({"tipo": "aguardar", "motivo": "m"}),
        ];
        let b = vec![
            json!({"tipo": "aguardar", "motivo": "outro"}),
            json!({"tipo": "delegar", "nivel": "low", "goal_id": 3,
                   "tarefa": "pesquisar preços de vps", "expectativa": "y", "prazo_segundos": 60}),
        ];
        let ia = impressao(Some(3), &a).unwrap();
        assert_eq!(ia, impressao(Some(3), &b).unwrap());
        assert_eq!(ia, "foco #3 | delegar:goal=3:low:pesquisar preços de vps");
        // Outro foco = outra impressão; só aguardar = sem impressão.
        assert_ne!(Some(ia), impressao(Some(4), &a));
        assert_eq!(impressao(Some(3), &[json!({"tipo": "aguardar"})]), None);
        assert_eq!(impressao(Some(3), &[]), None);
    }

    #[test]
    fn conta_repeticoes_e_erros_seguidos() {
        let s = |t: &str| Some(t.to_string());
        assert_eq!(repeticoes(&[s("a"), s("a"), s("a"), s("b")]), 3);
        assert_eq!(repeticoes(&[s("a"), None, s("a")]), 1);
        assert_eq!(repeticoes(&[None, None]), 0);
        assert_eq!(repeticoes(&[]), 0);

        let t = vec![
            (s("x"), true),
            (s("y"), false),
            (s("x"), true),
            (None, false),
            (s("x"), true),
            (s("x"), false),
            (s("x"), true),
        ];
        assert_eq!(erros_seguidos(&t, "x"), 3);
        assert_eq!(erros_seguidos(&t, "y"), 0);
        assert_eq!(erros_seguidos(&t, "z"), 0);
    }

    #[test]
    fn revisao_espaca_ate_o_teto_e_volta_quando_o_goal_muda() {
        let e1 = nova_estagnacao(None, Some(3), 100, 1_000, 8);
        assert_eq!(e1.multiplicador, 2);
        let e2 = nova_estagnacao(Some(&e1), Some(3), 100, 2_000, 8);
        assert_eq!(e2.multiplicador, 4);
        let e3 = nova_estagnacao(Some(&e2), Some(3), 100, 3_000, 8);
        let e4 = nova_estagnacao(Some(&e3), Some(3), 100, 4_000, 8);
        assert_eq!((e3.multiplicador, e4.multiplicador), (8, 8));
        assert_eq!(multiplicador_revisao(Some(&e4), 3, 100), 8);
        // O goal mudou (atualizado_ms novo) ou é outro goal: volta a 1×.
        assert_eq!(multiplicador_revisao(Some(&e4), 3, 150), 1);
        assert_eq!(multiplicador_revisao(Some(&e4), 4, 100), 1);
        assert_eq!(multiplicador_revisao(None, 3, 100), 1);
        // Estagnação de outro goal recomeça em 2×.
        assert_eq!(
            nova_estagnacao(Some(&e4), Some(5), 100, 5_000, 8).multiplicador,
            2
        );
    }

    #[test]
    fn disjuntor_abre_espera_e_fecha() {
        let c = ConfigVigilancia::default(); // 3 falhas; 300 s; teto 3600 s
        let mut e = EstadoDisjuntor::default();
        assert_eq!(situacao(&e, 0), Situacao::Fechado);
        e = apos_ciclo(&e, false, Some("500"), 1_000, &c, 0.5);
        e = apos_ciclo(&e, false, Some("500"), 2_000, &c, 0.5);
        assert_eq!(situacao(&e, 2_000), Situacao::Fechado);
        e = apos_ciclo(&e, false, Some("500"), 3_000, &c, 0.5);
        // Sorteio 0,5 = exatamente a base.
        assert_eq!(e.aberto_ate_ms, Some(3_000 + 300_000));
        assert_eq!(situacao(&e, 100_000), Situacao::Aberto { ate_ms: 303_000 });
        assert_eq!(situacao(&e, 303_000), Situacao::MeioAberto);
        // Meio-aberto e falhou: reabre pelo dobro.
        e = apos_ciclo(&e, false, Some("500"), 303_000, &c, 0.5);
        assert_eq!(e.aberto_ate_ms, Some(303_000 + 600_000));
        assert_eq!(e.aberturas, 2);
        // Um sucesso fecha e zera tudo.
        e = apos_ciclo(&e, true, None, 904_000, &c, 0.5);
        assert_eq!(e, EstadoDisjuntor::default());
        assert_eq!(descrever_disjuntor(&e, 0), None);
    }

    #[test]
    fn espera_tem_teto_e_sorteio() {
        let c = ConfigVigilancia::default();
        assert_eq!(espera_ms(&c, 0, 0.5), 300_000);
        assert_eq!(espera_ms(&c, 1, 0.5), 600_000);
        assert_eq!(espera_ms(&c, 10, 0.5), 3_600_000);
        assert_eq!(espera_ms(&c, 0, 0.0), 240_000);
        assert_eq!(espera_ms(&c, 0, 0.999_999), 359_999);
        assert_eq!(espera_ms(&c, 99, 0.5), 3_600_000);
    }
}
