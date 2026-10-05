//! Infra 1: latência das chamadas ao modelo (tempo até o primeiro token e
//! duração total), gravada em `chamadas_modelo` e resumida no `abiyss status`.

mod comum;

use std::time::Duration;

use abiyss::latencia;
use abiyss::nim::mock::RespostaMock;
use abiyss::nim::{self, EventoStream, Mensagem};
use abiyss::orquestrador::{AoReceber, Nivel, Origem};
use comum::Ambiente;

/// (status, primeiro_token_ms, duracao_ms, stream) de cada tentativa, em ordem.
fn registros(amb: &Ambiente) -> Vec<(String, Option<i64>, i64, bool)> {
    let conexao = amb.banco.conexao();
    let mut consulta = conexao
        .prepare(
            "SELECT status, primeiro_token_ms, duracao_ms, stream
               FROM chamadas_modelo ORDER BY id",
        )
        .unwrap();
    consulta
        .query_map([], |l| Ok((l.get(0)?, l.get(1)?, l.get(2)?, l.get(3)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

#[tokio::test]
async fn grava_primeiro_token_e_total_com_e_sem_streaming() {
    let amb = Ambiente::novo().await;
    let modelo = &amb.config.modelos.cerebro;
    let pedido = nim::montar_pedido(modelo, vec![Mensagem::usuario("oi")], vec![]);

    // Com streaming: o mock espera 150 ms antes do primeiro pedaço.
    amb.mock.enfileirar(
        RespostaMock::texto("uma resposta qualquer").atrasada(Duration::from_millis(150)),
    );
    let mut pedacos = 0;
    let mut contar = |_: EventoStream| pedacos += 1;
    let ao_receber: AoReceber<'_> = Some(&mut contar);
    amb.orquestrador
        .cerebro
        .chamar(Origem::Conversa, &pedido, ao_receber)
        .await
        .unwrap();
    assert!(
        pedacos > 0,
        "o callback original continua recebendo os pedaços"
    );

    // Sem streaming, pelo pool dos sub-agentes.
    let pedido_sub = nim::montar_pedido(
        &amb.config.modelos.sub_low,
        vec![Mensagem::usuario("oi")],
        vec![],
    );
    amb.mock
        .enfileirar(RespostaMock::texto("ok").atrasada(Duration::from_millis(100)));
    amb.orquestrador
        .subagentes
        .chamar(Nivel::Low, &pedido_sub, None)
        .await
        .unwrap();

    // Erro não retentável: sem primeiro token.
    amb.mock.enfileirar(RespostaMock::erro(400, None));
    assert!(
        amb.orquestrador
            .cerebro
            .chamar(Origem::Autonomo, &pedido, None)
            .await
            .is_err()
    );

    let r = registros(&amb);
    assert_eq!(r.len(), 3);

    let (status, ttft, total, stream) = &r[0];
    assert_eq!(status, "ok");
    assert!(*stream);
    let ttft_stream = ttft.expect("streaming com sucesso tem primeiro token");
    assert!(ttft_stream >= 140, "ttft = {ttft_stream}");
    assert!(*total >= ttft_stream);

    let (status, ttft, total, stream) = &r[1];
    assert_eq!(status, "ok");
    assert!(!*stream);
    // Sem streaming, o primeiro token chega junto com a resposta inteira.
    assert_eq!(*ttft, Some(*total));
    assert!(*total >= 90);

    let (status, ttft, _, _) = &r[2];
    assert_eq!(status, "erro");
    assert_eq!(*ttft, None);

    // O resumo separa por (modelo, pool) e não conta a falha nos percentis.
    let resumo = latencia::resumo_desde(&amb.banco, 0).unwrap();
    let cerebro = resumo
        .iter()
        .find(|l| l.pool == "cerebro")
        .expect("linha do cérebro");
    assert_eq!(cerebro.modelo, modelo.id);
    assert_eq!((cerebro.sucessos, cerebro.falhas), (1, 1));
    assert_eq!(cerebro.primeiro_token.p50, Some(ttft_stream));
    assert!(
        resumo
            .iter()
            .any(|l| l.pool == "subagentes" && l.sucessos == 1)
    );

    // E aparece no `abiyss status`.
    let texto = abiyss::status::relatorio(&amb.config, &amb.banco).unwrap();
    assert!(texto.contains("Latência (24 h, p50 / p95):"), "{texto}");
    assert!(
        texto.contains(&format!("{} [cerebro]", modelo.id)),
        "{texto}"
    );
}
