//! E3: ritmo do dia e orçamento diário do trabalho autônomo.
//!
//! - orçamento esgotado: o heartbeat não chama o modelo (eventos ficam na
//!   fila), mas a conversa com o dono continua;
//! - no limite, a delegação vinda do heartbeat é recusada;
//! - o sono tem origem própria no registro de chamadas;
//! - o abiyss.toml do repositório carrega com as seções novas.

mod comum;

use std::path::Path;

use abiyss::chat::SessaoChat;
use abiyss::config::Config;
use abiyss::eventos;
use abiyss::heartbeat::Heartbeat;
use abiyss::nim::mock::RespostaMock;
use abiyss::nim::{self, Mensagem};
use abiyss::orquestrador::Origem;
use abiyss::tempo::agora_ms;
use comum::Ambiente;
use rusqlite::params;
use serde_json::json;

/// Registra `n` chamadas autônomas "de hoje" no log de chamadas.
fn gastar(amb: &Ambiente, n: usize) {
    for _ in 0..n {
        amb.banco
            .conexao()
            .execute(
                "INSERT INTO chamadas_modelo (momento_ms, pool, origem, modelo, tentativa, status,
                   tokens_entrada, tokens_saida, duracao_ms)
                 VALUES (?1, 'cerebro', 'autonomo', 'm', 1, 'ok', 10, 10, 1)",
                params![agora_ms()],
            )
            .unwrap();
    }
}

#[tokio::test]
async fn orcamento_esgotado_para_o_heartbeat_mas_nao_a_conversa() {
    let mut amb = Ambiente::novo().await;
    amb.config.orcamento.chamadas_autonomas_por_dia = 5;
    amb.config.orcamento.reserva_sono_chamadas = 1;
    gastar(&amb, 4); // limite do heartbeat: 5 - 1 = 4
    eventos::publicar(&amb.banco, "teste", "t", "acorde").unwrap();

    let hb = Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    );
    let r = hb.ciclo().await.unwrap();
    assert!(!r.chamou_modelo);
    assert!(r.motivo.contains("orçamento diário"), "{}", r.motivo);
    assert_eq!(amb.mock.total_requisicoes(), 0);
    assert_eq!(eventos::contar_pendentes(&amb.banco).unwrap(), 1);

    // A conversa com o dono nunca é cortada pelo orçamento autônomo.
    amb.mock.enfileirar(RespostaMock::texto("Oi!"));
    let resposta = SessaoChat::nova(
        amb.config.clone(),
        amb.orquestrador.clone(),
        amb.banco.clone(),
        amb.ferramentas.clone(),
    )
    .unwrap()
    .enviar("oi", None)
    .await
    .unwrap();
    assert_eq!(resposta.texto, "Oi!");
    // E a interocepção do chat mostra o orçamento esgotado.
    let sistema = amb.mock.requisicoes()[0].corpo["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(sistema.contains("ESGOTADO"), "{sistema}");
}

#[tokio::test]
async fn delegacao_do_heartbeat_e_recusada_no_limite() {
    let mut amb = Ambiente::novo().await;
    amb.config.orcamento.chamadas_autonomas_por_dia = 3;
    amb.config.orcamento.reserva_sono_chamadas = 1;
    gastar(&amb, 1); // limite 2: a chamada deste ciclo é a última
    eventos::publicar(&amb.banco, "teste", "t", "acorde").unwrap();
    amb.mock.enfileirar(RespostaMock::texto(
        json!({"acoes": [{"tipo": "delegar", "nivel": "low", "tarefa": "x", "prazo_segundos": 60}]})
            .to_string(),
    ));
    let hb = Heartbeat::novo(
        amb.config.clone(),
        amb.banco.clone(),
        amb.orquestrador.clone(),
    );
    let r = hb.ciclo().await.unwrap();
    assert!(r.chamou_modelo);
    assert!(
        r.resultados[0].starts_with("erro: orçamento diário de trabalho autônomo esgotado"),
        "{:?}",
        r.resultados
    );
    assert!(abiyss::subagentes::ativos(&amb.banco).unwrap().is_empty());
}

#[tokio::test]
async fn sono_tem_origem_propria_no_registro_de_chamadas() {
    let amb = Ambiente::novo().await;
    amb.mock.enfileirar(RespostaMock::texto("ok"));
    let pedido = nim::montar_pedido(
        &amb.config.modelos.cerebro,
        vec![Mensagem::usuario("revise o dia")],
        vec![],
    );
    amb.orquestrador
        .cerebro
        .chamar(Origem::Sono, &pedido, None)
        .await
        .unwrap();
    let origem: String = amb
        .banco
        .conexao()
        .query_row("SELECT origem FROM chamadas_modelo", [], |l| l.get(0))
        .unwrap();
    assert_eq!(origem, "sono");
}

#[test]
fn abiyss_toml_do_repositorio_carrega() {
    let caminho = Path::new(env!("CARGO_MANIFEST_DIR")).join("../abiyss.toml");
    let config = Config::carregar(&caminho).unwrap();
    assert_eq!(config.ritmo.horas_ativas, "07:00-23:00");
    assert!(config.orcamento.chamadas_autonomas_por_dia > 0);
    assert_eq!(config.skills.automaticas.sono, "dormir-bem");
}
