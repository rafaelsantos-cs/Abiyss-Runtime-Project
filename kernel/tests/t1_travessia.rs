//! Travessia de ponta a ponta: o servidor do jogo de verdade (uv + Python),
//! o mock do NIM jogando (`travessia::mock`), sub-agentes no mesmo processo,
//! métricas, replay, linha do tempo e relatório.
//!
//! Precisa do `uv` (o CI instala); sem ele, os testes são pulados.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use abiyss::config::Config;
use abiyss::db::Banco;
use abiyss::nim::mock::MockNim;
use abiyss::travessia::{self, Braco, Modo, OpcoesExecucao, Pastas};

fn uv_disponivel() -> bool {
    let ok = std::process::Command::new("uv")
        .arg("--version")
        .output()
        .is_ok_and(|s| s.status.success());
    if !ok {
        eprintln!("AVISO: 'uv' não encontrado; teste da Travessia pulado");
    }
    ok
}

fn pasta_jogo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../recursos/mcp/jogo")
        .canonicalize()
        .unwrap()
}

fn fase(numero: u32, nevoa: bool, mapa: &[&str], objetos: Value) -> Value {
    json!({
        "formato": 1, "gerador": "teste", "molde": "teste", "seed": "SEGREDO-DA-SEED",
        "numero": numero, "nome": format!("Fase de teste {numero}"),
        "largura": mapa[0].len(), "altura": mapa.len(), "nevoa": nevoa,
        "raios": {"V": 2, "R": 2, "C": 5}, "mapa": mapa, "objetos": objetos,
    })
}

/// Projeto falso: abiyss.toml apontando para o mock e um catálogo de duas
/// fases. A 1ª o mock conclui ("V L", "V L"); a 2ª (com névoa) não.
fn projeto(pasta: &Path, base_url: &str) -> PathBuf {
    std::fs::create_dir_all(pasta.join("identity")).unwrap();
    std::fs::write(pasta.join("identity/nucleo.md"), "Sou o Abiyss de teste.").unwrap();
    std::fs::create_dir_all(pasta.join("skills")).unwrap();
    let toml = pasta.join("abiyss.toml");
    std::fs::write(
        &toml,
        format!(
            r#"
[nim]
base_url = "{base_url}"
timeout_leitura_segundos = 30
timeout_conexao_segundos = 5

[modelos.cerebro]
id = "teste/cerebro"
[modelos.sub_ultra]
id = "teste/ultra"
[modelos.sub_medium]
id = "teste/medium"
[modelos.sub_low]
id = "teste/low"

[pools.cerebro]
api_key_env = "ABIYSS_TESTE_CHAVE_CEREBRO"
requisicoes_por_minuto = 6000
rajada = 100
reserva_conversa_por_minuto = 10
terceiros_por_minuto = 6
[pools.subagentes]
api_key_env = "ABIYSS_TESTE_CHAVE_SUBAGENTES"
requisicoes_por_minuto = 6000
rajada = 100

[ritmo]
horas_ativas = "00:00-00:00"

[subagentes]
verificacao_segundos = 1
[subagentes.low]
ferramentas = ["ler_arquivo", "listar_arquivos", "web_rapido__*"]

[travessia]
jogo = "{jogo}"
fases = [1, 2]
max_chamadas_por_fase = 40
max_minutos_por_execucao = 5
aguardar_max_segundos = 20
"#,
            jogo = pasta_jogo().display()
        ),
    )
    .unwrap();
    let config = Config::carregar(&toml).unwrap();
    let pastas = Pastas::da_config(&config);
    let fases = [
        (
            fase(
                1,
                false,
                &["#####", "#..G#", "#####"],
                json!({
                "avatares": {"V": [1, 1]}, "blocos": [], "itens": [{"item": "dourada", "pos": [2, 1]}],
                "placas": {}, "portas": [], "trancadas": []}),
            ),
            2,
        ),
        (
            fase(
                2,
                true,
                &[
                    "##########",
                    "#........#",
                    "#........#",
                    "#........#",
                    "#........#",
                    "#........#",
                    "#........#",
                    "#........#",
                    "#.......G#",
                    "##########",
                ],
                json!({"avatares": {"V": [1, 1]}, "blocos": [], "itens": [{"item": "dourada", "pos": [8, 7]}],
                       "placas": {}, "portas": [], "trancadas": []}),
            ),
            20,
        ),
    ];
    for (dados, otimo) in fases {
        let numero = dados["numero"].as_u64().unwrap() as u32;
        let cat = pastas.catalogo("teste");
        std::fs::create_dir_all(&cat).unwrap();
        std::fs::write(cat.join(travessia::arquivo_fase(numero)), dados.to_string()).unwrap();
        let gab = pastas.gabarito("teste");
        std::fs::create_dir_all(&gab).unwrap();
        std::fs::write(
            gab.join(travessia::arquivo_fase(numero)),
            json!({"numero": numero, "otimo": otimo, "solucao": ["V L", "V L"]}).to_string(),
        )
        .unwrap();
    }
    toml
}

fn opcoes(braco: Braco, relogio: bool) -> OpcoesExecucao {
    OpcoesExecucao {
        braco,
        relogio,
        modo: Modo::Avaliacao,
        seed: "teste".into(),
        numero: 1,
        chaves: Some(("nvapi-cerebro".into(), "nvapi-sub".into())),
    }
}

fn ler_jsonl(caminho: &Path) -> Vec<Value> {
    std::fs::read_to_string(caminho)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bracos_a_b_c_e_relogio_de_ponta_a_ponta() {
    if !uv_disponivel() {
        return;
    }
    let pasta = tempfile::tempdir().unwrap();
    let mock = MockNim::iniciar().await;
    mock.definir_roteiro(travessia::mock::roteiro_travessia(
        std::time::Duration::ZERO,
    ));
    let toml = projeto(pasta.path(), &mock.base_url());
    let config = Config::carregar(&toml).unwrap();

    // Braço A: completo, com delegação.
    let a = travessia::rodar_execucao(&config, &toml, &opcoes(Braco::A, false))
        .await
        .unwrap();
    assert_eq!(a.motivo_fim, "jogo encerrado");
    assert_eq!(a.fases.len(), 2);
    let f1 = &a.fases[0];
    assert!(
        f1.concluida && f1.jogadas == 2 && f1.eficiencia == Some(1.0),
        "{f1:?}"
    );
    assert!(
        !f1.nevoa && f1.delegacoes == 1 && f1.delegacoes_por_nivel["low"] == 1,
        "{f1:?}"
    );
    assert!(
        f1.uso["cerebro"].chamadas >= 5 && f1.uso["subagentes"].chamadas >= 2,
        "{f1:?}"
    );
    assert!(f1.espera_cerebro_p50_ms.is_some() && f1.subagentes_com_problema == 0);
    let f2 = &a.fases[1];
    assert!(
        f2.nevoa && f2.resultado == "desistencia" && f2.eficiencia.is_none(),
        "{f2:?}"
    );

    // Arquivos: replay, linha do tempo, métricas, resumo — fora do workspace.
    let replay = ler_jsonl(&a.pasta.join("replay.jsonl"));
    assert_eq!(replay[0]["tipo"], "cabecalho");
    assert!(replay.iter().any(|l| l["tipo"] == "revelacao"));
    assert_eq!(replay.last().unwrap()["tipo"], "fim_execucao");
    let tempo = ler_jsonl(&a.pasta.join("linha-do-tempo.jsonl"));
    for tipo in [
        "chamada_modelo",
        "delegacao",
        "subagente_fim",
        "ferramenta",
        "fase",
    ] {
        assert!(
            tempo.iter().any(|e| e["tipo"] == tipo),
            "falta {tipo} na linha do tempo"
        );
    }
    assert!(tempo.iter().any(
        |e| e["tipo"] == "ferramenta" && e["quem"].as_str().unwrap().starts_with("subagente:")
    ));
    assert_eq!(ler_jsonl(&a.pasta.join("metricas.jsonl")).len(), 2);
    let resumo = std::fs::read_to_string(a.pasta.join("resumo.txt")).unwrap();
    assert!(resumo.contains("1 de 2 fase(s) concluída(s)"), "{resumo}");
    assert_eq!(
        std::fs::read_dir(a.pasta.join("workspace"))
            .unwrap()
            .count(),
        0
    );
    // O validador do replay (Python) aceita o que o kernel produziu.
    let validacao = std::process::Command::new("uv")
        .args([
            "run",
            "--frozen",
            "--quiet",
            "--no-dev",
            "validar_replay.py",
        ])
        .arg(a.pasta.join("replay.jsonl"))
        .arg("--linha-do-tempo")
        .arg(a.pasta.join("linha-do-tempo.jsonl"))
        .current_dir(pasta_jogo())
        .output()
        .unwrap();
    let saida = String::from_utf8_lossy(&validacao.stdout);
    assert!(validacao.status.success(), "{saida}");
    assert!(saida.contains("1 de 2 fase(s) concluída(s)"), "{saida}");

    // Integridade: nada do gabarito nem da seed chega ao modelo.
    for r in mock.requisicoes() {
        let corpo = r.corpo.to_string();
        for segredo in [
            "otimo",
            "SEGREDO-DA-SEED",
            "Fase de teste",
            "max_jogadas",
            "gabarito",
        ] {
            assert!(!corpo.contains(segredo), "{segredo} vazou para o modelo");
        }
    }
    // Os sub-agentes só têm as ferramentas de leitura do jogo.
    let sub = mock
        .requisicoes()
        .into_iter()
        .find(|r| r.corpo["model"] == "teste/low")
        .expect("o sub-agente chamou o modelo");
    let nomes: Vec<&str> = sub.corpo["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    assert!(
        nomes.contains(&"jogo__simular") && !nomes.contains(&"jogo__executar"),
        "{nomes:?}"
    );
    assert!(!nomes.iter().any(|n| n.starts_with("web_rapido")));

    // Braço B: sem delegação.
    let b = travessia::rodar_execucao(&config, &toml, &opcoes(Braco::B, false))
        .await
        .unwrap();
    assert!(b.fases.iter().all(|m| m.delegacoes == 0));
    assert!(b.fases[0].concluida);

    // Braço C com relógio: conversa nova a cada fase, orçamento de tempo.
    let c = travessia::rodar_execucao(&config, &toml, &opcoes(Braco::C, true))
        .await
        .unwrap();
    assert_eq!(c.fases[0].braco, "C+T");
    let execucao: Value =
        serde_json::from_str(&std::fs::read_to_string(c.pasta.join("execucao.json")).unwrap())
            .unwrap();
    assert_eq!(execucao["relogio_segundos"], 600);
    assert_eq!(execucao["max_jogadas"]["1"], 8 * 2 + 20);
    let banco = Banco::abrir(&c.pasta.join("dados/abiyss.db")).unwrap();
    let conversas: i64 = banco
        .conexao()
        .query_row("SELECT COUNT(*) FROM conversas", [], |l| l.get(0))
        .unwrap();
    assert_eq!(conversas, 2);

    // Relatório das três execuções.
    let pastas = Pastas::da_config(&config);
    let metricas = travessia::relatorio::ler_execucoes(&pastas.execucoes()).unwrap();
    assert_eq!(metricas.len(), 6);
    let relatorio = travessia::relatorio::gerar(&metricas);
    for braco in ["## Braço A", "## Braço B", "## Braço C+T"] {
        assert!(relatorio.contains(braco), "{relatorio}");
    }
    assert!(relatorio.contains("| 2 (névoa) | 1 | 0% |"), "{relatorio}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn trava_do_harness_encerra_fase_em_laco() {
    if !uv_disponivel() {
        return;
    }
    let pasta = tempfile::tempdir().unwrap();
    let mock = MockNim::iniciar().await;
    // Um cérebro que só olha o mapa, para sempre.
    mock.definir_roteiro(|_corpo: &Value| {
        abiyss::nim::mock::RespostaMock::ferramenta("jogo__ver", json!({}))
    });
    let toml = projeto(pasta.path(), &mock.base_url());
    let texto = std::fs::read_to_string(&toml)
        .unwrap()
        .replace("max_chamadas_por_fase = 40", "max_chamadas_por_fase = 3");
    std::fs::write(&toml, texto).unwrap();
    let config = Config::carregar(&toml).unwrap();
    let r = travessia::rodar_execucao(&config, &toml, &opcoes(Braco::B, false))
        .await
        .unwrap();
    assert_eq!(r.motivo_fim, "jogo encerrado");
    assert!(
        r.fases
            .iter()
            .all(|m| m.trava_do_harness && m.motivo == "trava_do_harness"),
        "{:?}",
        r.fases
    );
    assert!(r.fases.iter().all(|m| m.resultado == "desistencia"));
}
