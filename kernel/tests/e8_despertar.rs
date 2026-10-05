//! E8: despertar e continuidade — ao subir depois de uma ausência longa, o
//! daemon avisa (queda ou parada limpa) e o primeiro ciclo replaneja o dia
//! com a skill planejar-o-dia; ausência curta não gera aviso.

mod comum;

use abiyss::daemon::{self, Daemon, OpcoesDaemon};
use abiyss::eventos;
use abiyss::nim::mock::RespostaMock;
use abiyss::tempo::agora_ms;
use comum::Ambiente;
use serde_json::{Value, json};

const MIN: i64 = 60_000;

fn instalar_skill(amb: &Ambiente, nome: &str) {
    let origem = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../skills")
        .join(nome)
        .join("SKILL.md");
    let destino = amb.caminho(&format!("skills/{nome}"));
    std::fs::create_dir_all(&destino).unwrap();
    std::fs::copy(origem, destino.join("SKILL.md")).unwrap();
}

fn execucao_anterior(amb: &Ambiente, iniciado: i64, parado: Option<i64>, sinal: Option<i64>) {
    daemon::gravar_estado(&amb.banco, daemon::CHAVE_INICIADO, &iniciado.to_string()).unwrap();
    if let Some(p) = parado {
        daemon::gravar_estado(&amb.banco, daemon::CHAVE_PARADO, &p.to_string()).unwrap();
    }
    if let Some(s) = sinal {
        daemon::gravar_estado(&amb.banco, daemon::CHAVE_SINAL_DE_VIDA, &s.to_string()).unwrap();
    }
}

async fn subir_uma_vez(amb: &Ambiente) {
    let d = Daemon::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
        amb.ferramentas.clone(),
    );
    let opcoes = OpcoesDaemon { uma_vez: true };
    d.rodar_ate(&opcoes, std::future::pending()).await.unwrap();
}

fn texto_da_requisicao(corpo: &Value) -> String {
    corpo["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decisao_vazia() -> RespostaMock {
    RespostaMock::texto(
        json!({"percepcao": "p", "orientacao": "o", "decisao": "d", "acoes": []}).to_string(),
    )
}

#[tokio::test]
async fn queda_gera_aviso_e_o_primeiro_ciclo_replaneja() {
    let amb = Ambiente::novo().await;
    instalar_skill(&amb, "planejar-o-dia");
    let agora = agora_ms();
    // Caiu: sem parada, último sinal de vida há 2 horas.
    execucao_anterior(&amb, agora - 5 * 60 * MIN, None, Some(agora - 120 * MIN));
    amb.mock.enfileirar(decisao_vazia());

    subir_uma_vez(&amb).await;

    let reqs = amb.mock.requisicoes();
    assert_eq!(reqs.len(), 1, "o aviso acorda o modelo");
    let texto = texto_da_requisicao(&reqs[0].corpo);
    assert!(texto.contains("Fiquei fora do ar de"), "{texto}");
    assert!(texto.contains("motivo: queda (último sinal de vida em"));
    assert!(texto.contains("Procedimento sugerido pelo kernel (skill planejar-o-dia)"));
    // Interocepção: no ar desde agora.
    assert!(texto.contains("No ar desde"));
    // O evento foi consumido pelo ciclo.
    assert_eq!(eventos::contar_pendentes(&amb.banco).unwrap(), 0);
}

#[tokio::test]
async fn parada_limpa_longa_gera_aviso() {
    let amb = Ambiente::novo().await;
    let agora = agora_ms();
    execucao_anterior(
        &amb,
        agora - 300 * MIN,
        Some(agora - 90 * MIN),
        Some(agora - 91 * MIN),
    );
    amb.mock.enfileirar(decisao_vazia());

    subir_uma_vez(&amb).await;

    let texto = texto_da_requisicao(&amb.mock.requisicoes()[0].corpo);
    assert!(texto.contains("motivo: parada limpa"), "{texto}");
    assert!(texto.contains("(1 h 30 min)"), "{texto}");
}

#[tokio::test]
async fn ausencia_curta_nao_gera_aviso() {
    let amb = Ambiente::novo().await;
    let agora = agora_ms();
    // Um restart rápido (2 min): nada a avisar, nada a perguntar ao modelo.
    execucao_anterior(&amb, agora - 60 * MIN, Some(agora - 2 * MIN), None);

    subir_uma_vez(&amb).await;

    assert_eq!(amb.mock.total_requisicoes(), 0);
    let avisos = eventos::pendentes(&amb.banco, 10)
        .unwrap()
        .into_iter()
        .filter(|e| e.origem == eventos::ORIGEM_REINICIO)
        .count();
    assert_eq!(avisos, 0);
}
