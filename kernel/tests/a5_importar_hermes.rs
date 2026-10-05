//! A5: `abiyss importar-hermes`.
//!
//! Usa a fixture `tests/fixtures/hermes/` (dados inventados no formato
//! suposto), copiada para uma pasta temporária. Os arquivos com nome de
//! segredo são criados AQUI, com um texto-sentinela que nunca pode
//! aparecer em lugar nenhum do projeto depois da importação.

mod comum;

use std::path::{Path, PathBuf};

use abiyss::config::Config;
use abiyss::diario;
use abiyss::frontmatter;
use abiyss::goals::{self, EstadoGoal};
use abiyss::hermes;
use abiyss::memoria::Memoria;
use abiyss::memoria::central::MemoriaCentral;
use comum::{Ambiente, NUCLEO_DE_TESTE};
use tempfile::TempDir;

const SENTINELA: &str = "SENTINELA-DE-SEGREDO-QUE-NUNCA-PODE-VAZAR";

/// Copia a fixture para uma pasta temporária e acrescenta os segredos.
fn origem_com_segredos() -> (TempDir, PathBuf) {
    let pasta = tempfile::tempdir().unwrap();
    let origem = pasta.path().join("hermes");
    copiar(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hermes"),
        &origem,
    );
    std::fs::write(
        origem.join(".env"),
        format!("OPENROUTER_API_KEY={SENTINELA}\n"),
    )
    .unwrap();
    std::fs::write(
        origem.join("auth.json"),
        format!("{{\"token\": \"{SENTINELA}\"}}"),
    )
    .unwrap();
    std::fs::create_dir_all(origem.join("secrets")).unwrap();
    std::fs::write(origem.join("secrets/notas.md"), SENTINELA).unwrap();
    std::fs::write(origem.join("memories/api_key.txt"), SENTINELA).unwrap();
    // Um link com nome inocente apontando para o .env: não é seguido.
    #[cfg(unix)]
    std::os::unix::fs::symlink(origem.join(".env"), origem.join("metacognition/atalho.md"))
        .unwrap();
    (pasta, origem)
}

fn copiar(de: &Path, para: &Path) {
    std::fs::create_dir_all(para).unwrap();
    for entrada in std::fs::read_dir(de).unwrap() {
        let entrada = entrada.unwrap();
        let destino = para.join(entrada.file_name());
        if entrada.file_type().unwrap().is_dir() {
            copiar(&entrada.path(), &destino);
        } else {
            std::fs::copy(entrada.path(), &destino).unwrap();
        }
    }
}

/// Todos os arquivos do projeto (cofre, banco, identity...), com conteúdo.
fn fotografia(raiz: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut arquivos = Vec::new();
    let mut pendentes = vec![raiz.to_path_buf()];
    while let Some(pasta) = pendentes.pop() {
        for entrada in std::fs::read_dir(&pasta).unwrap() {
            let entrada = entrada.unwrap();
            if entrada.file_type().unwrap().is_dir() {
                pendentes.push(entrada.path());
            } else {
                arquivos.push((entrada.path(), std::fs::read(entrada.path()).unwrap()));
            }
        }
    }
    arquivos.sort();
    arquivos
}

/// Fotografia só das notas, da memória central e do rascunho (o banco
/// muda de bytes por motivos internos do SQLite).
fn fotografia_da_memoria(config: &Config) -> Vec<(PathBuf, Vec<u8>)> {
    let mut tudo = fotografia(&config.caminho_cofre());
    for arquivo in [
        config.caminho_memoria_central(),
        config
            .caminho_identidade()
            .with_file_name("nucleo.proposto.md"),
    ] {
        tudo.push((arquivo.clone(), std::fs::read(&arquivo).unwrap()));
    }
    tudo
}

fn nota(config: &Config, caminho: &str) -> frontmatter::Documento {
    let texto = std::fs::read_to_string(config.caminho_cofre().join(caminho)).unwrap();
    frontmatter::separar(&texto).unwrap()
}

#[tokio::test]
async fn simulacao_relata_tudo_e_nao_grava_nada() {
    let amb = Ambiente::novo().await;
    let (_pasta, origem) = origem_com_segredos();
    let relatorio = hermes::importar(&amb.config, &origem, false).unwrap();
    let texto = relatorio.to_string();

    assert!(texto.contains("SIMULAÇÃO: nada foi gravado"));
    // Segredos: listados pelo nome, nunca abertos.
    for segredo in [".env", "auth.json", "secrets/", "memories/api_key.txt"] {
        assert!(
            relatorio.segredos_pulados.iter().any(|s| s == segredo),
            "faltou {segredo}: {:?}",
            relatorio.segredos_pulados
        );
    }
    #[cfg(unix)]
    assert_eq!(relatorio.links_ignorados, vec!["metacognition/atalho.md"]);
    // Não mapeados: só listados.
    for item in [
        "config.yaml",
        "metacognition/goals/LEIA-ME.txt",
        "sessions/20261001_090000.json",
    ] {
        assert!(
            relatorio.nao_mapeados.iter().any(|n| n == item),
            "faltou {item}"
        );
    }
    // Mapeamentos e campos desconhecidos aparecem para conferir as suposições.
    assert!(texto.contains("'active' → executando"));
    assert!(texto.contains("'hibernando' → proposto ⚠ desconhecido"));
    assert!(texto.contains("expectativa ← expectation (3x), expectativa (1x), expected (1x)"));
    assert!(texto.contains("context (1x), mood (1x), tags (1x)"));
    assert!(texto.contains("history (1x), notes (1x), owner (1x)"));
    assert!(texto.contains("linha 4: JSON inválido"));
    assert!(texto.contains("g-005-cortado.json: JSON inválido"));
    assert!(texto.contains("SOUL.md linha 5 menciona \"Hermes\""));
    assert!(texto.contains("confiança \"alta\" não é número"));
    assert!(!texto.contains(SENTINELA));

    // Nada gravado.
    assert!(!amb.config.caminho_cofre().exists());
    assert!(!amb.config.caminho_memoria_central().exists());
    assert!(!amb.caminho("identity/nucleo.proposto.md").exists());
    assert_eq!(diario::contar(&amb.banco).unwrap(), 0);
    assert!(goals::listar(&amb.banco, true).unwrap().is_empty());
    assert_eq!(relatorio.total_novos(), 22);
}

#[tokio::test]
async fn aplicar_importa_com_procedencia_e_rodar_de_novo_nao_duplica() {
    let amb = Ambiente::novo().await;
    let (_pasta, origem) = origem_com_segredos();
    let primeira = hermes::importar(&amb.config, &origem, true).unwrap();
    assert_eq!(primeira.total_novos(), 22, "{primeira}");

    // Goals: estado de origem mapeado; desconhecido → proposto com o original no motivo.
    let lista = goals::listar(&amb.banco, true).unwrap();
    let por_titulo = |t: &str| lista.iter().find(|g| g.titulo.contains(t)).unwrap().clone();
    let migrar = por_titulo("Migrar");
    assert_eq!(migrar.estado, EstadoGoal::Executando);
    assert_eq!(migrar.prioridade, 2);
    assert!(migrar.nucleo.contains("memória importada"));
    assert_eq!(por_titulo("aprender Rust").estado, EstadoGoal::Proposto);
    assert_eq!(por_titulo("qmd").estado, EstadoGoal::Concluido);
    let voz = por_titulo("voz");
    assert_eq!(voz.estado, EstadoGoal::Proposto);
    let criacao = &goals::eventos(&amb.banco, voz.id).unwrap()[0];
    assert!(criacao.motivo.contains("'hibernando' desconhecido"));
    assert_eq!(criacao.autor, "importacao");
    assert!(
        goals::extras(&amb.banco, migrar.id)
            .unwrap()
            .unwrap()
            .contains("history")
    );

    // Diário: campos mapeados, extras preservados, ligação com o goal.
    let entradas = diario::recentes(&amb.banco, 10).unwrap();
    assert_eq!(entradas.len(), 5);
    let acao = |t: &str| {
        entradas
            .iter()
            .find(|e| e.acao.contains(t))
            .unwrap()
            .clone()
    };
    let docs = acao("documentação do qmd");
    assert_eq!(docs.origem, "hermes");
    assert_eq!(docs.goal_id, Some(migrar.id));
    assert_eq!(docs.confianca, Some(0.8));
    assert_eq!(
        docs.sinais.as_deref(),
        Some("README menciona collections; existe modo MCP")
    );
    assert_eq!(
        docs.resultado.as_deref(),
        Some("Entendi collections e o modo HTTP")
    );
    let backup = acao("backup");
    assert_eq!(backup.confianca, Some(0.65));
    assert_eq!(backup.risco.as_deref(), Some("médio"));
    assert!(backup.extras.as_deref().unwrap().contains("\"mood\""));
    assert!(acao("limpeza").extras.as_deref().unwrap().contains("38%"));

    // Notas internas com procedência de importação.
    let dia = nota(&amb.config, "01_internal/diario/2026-09-28.md");
    assert_eq!(dia.texto("fonte").as_deref(), Some("importacao"));
    assert_eq!(dia.texto("tipo").as_deref(), Some("dito"));
    assert_eq!(dia.texto("criado").as_deref(), Some("2026-09-28"));
    assert!(dia.corpo.contains("Li sobre o qmd") && dia.corpo.contains("(noite)"));
    let auto = nota(&amb.config, "01_internal/identidade/auto-modelo.md");
    assert_eq!(auto.texto("tipo").as_deref(), Some("deduzido"));
    assert!(auto.corpo.contains("\"humor_atual\": \"otimista\""));
    let soul = nota(&amb.config, "01_internal/identidade/soul.md");
    assert_eq!(soul.texto("tipo").as_deref(), Some("dito"));

    // Rascunho do núcleo gerado; o núcleo de verdade intacto.
    let proposto = std::fs::read_to_string(amb.caminho("identity/nucleo.proposto.md")).unwrap();
    assert!(proposto.contains("RASCUNHO PROPOSTO"));
    assert!(proposto.contains("DepthAI") && proposto.contains("2026-09-27"));
    assert!(proposto.contains("- honestidade sobre incertezas"));
    assert!(proposto.contains("> Hoje roda dentro do framework Hermes"));
    assert_eq!(
        std::fs::read_to_string(amb.caminho("identity/nucleo.md")).unwrap(),
        NUCLEO_DE_TESTE
    );

    // Memória central: as 7 entradas, marcadas como deduzidas.
    let central = MemoriaCentral::da_config(&amb.config);
    let entradas_centrais = central.entradas();
    assert_eq!(entradas_centrais.len(), 7);
    assert!(central.contem("Fuso horário do usuário: America/Sao_Paulo."));
    assert!(
        central
            .texto()
            .starts_with("[deduzido] O servidor roda numa VM")
    );

    // Segunda vez: nada novo, nada muda.
    let antes = fotografia_da_memoria(&amb.config);
    let segunda = hermes::importar(&amb.config, &origem, true).unwrap();
    assert_eq!(segunda.total_novos(), 0, "{segunda}");
    assert_eq!(segunda.total_ja_importados(), 22);
    assert_eq!(fotografia_da_memoria(&amb.config), antes);
    assert_eq!(diario::contar(&amb.banco).unwrap(), 5);
    assert_eq!(goals::listar(&amb.banco, true).unwrap().len(), 4);
}

#[tokio::test]
async fn segredos_nunca_sao_abertos_nem_copiados() {
    let amb = Ambiente::novo().await;
    let (_pasta, origem) = origem_com_segredos();
    #[cfg(unix)]
    {
        // Pior caso: um arquivo MAPEADO que na verdade é link para o .env.
        std::fs::remove_file(origem.join("memories/USER.md")).unwrap();
        std::os::unix::fs::symlink(origem.join(".env"), origem.join("memories/USER.md")).unwrap();
    }
    let relatorio = hermes::importar(&amb.config, &origem, true).unwrap();
    assert!(!relatorio.to_string().contains(SENTINELA));
    // Nenhum arquivo do projeto (cofre, banco, identity, memória central)
    // contém o sentinela.
    for (caminho, bytes) in fotografia(amb.pasta.path()) {
        let texto = String::from_utf8_lossy(&bytes);
        assert!(!texto.contains(SENTINELA), "vazou em {}", caminho.display());
    }
    #[cfg(unix)]
    assert!(
        relatorio
            .links_ignorados
            .iter()
            .any(|l| l == "memories/USER.md")
    );
}

#[tokio::test]
async fn excedente_da_memoria_central_vai_para_notas_sem_cortar() {
    let mut amb = Ambiente::novo().await;
    amb.config.memoria.limite_central_caracteres = 200;
    let (_pasta, origem) = origem_com_segredos();
    let relatorio = hermes::importar(&amb.config, &origem, true).unwrap();
    let texto = relatorio.to_string();
    assert!(texto.contains("não couberam no orçamento"));

    let central = MemoriaCentral::da_config(&amb.config);
    assert!(central.uso() <= 200);
    let no_central = central.entradas().len();
    assert!((1..7).contains(&no_central));

    // O que não coube está inteiro em notas internas, com procedência.
    let memoria = nota(&amb.config, "01_internal/memoria/hermes-memory.md");
    assert_eq!(memoria.texto("fonte").as_deref(), Some("importacao"));
    assert_eq!(memoria.texto("tipo").as_deref(), Some("deduzido"));
    let usuario = nota(&amb.config, "01_internal/pessoas/usuario-hermes.md");
    assert!(
        usuario
            .corpo
            .contains("Gosta de relatórios com o que foi feito")
    );
    let todas = format!("{}\n{}\n{}", central.texto(), memoria.corpo, usuario.corpo);
    for entrada in [
        "O servidor roda numa VM Oracle ARM (aarch64), 2 OCPU, 12 GB.",
        "Ferramenta de busca local: qmd, ouvindo na porta 8181.",
        "Está aprendendo Rust; prefere código legível a código esperto.",
    ] {
        assert_eq!(todas.matches(entrada).count(), 1, "{entrada}");
    }

    // De novo: nenhuma entrada repetida em lugar nenhum.
    let antes = fotografia_da_memoria(&amb.config);
    hermes::importar(&amb.config, &origem, true).unwrap();
    assert_eq!(fotografia_da_memoria(&amb.config), antes);
}

#[tokio::test]
async fn conteudo_com_cara_de_chave_nao_e_importado() {
    let amb = Ambiente::novo().await;
    let (_pasta, origem) = origem_com_segredos();
    // Montado aqui (e não na fixture) para nunca parecer uma chave real no git.
    let chave_falsa = format!("ghp_{}", "a1B2".repeat(9));
    let memoria = origem.join("memories/MEMORY.md");
    let mut texto = std::fs::read_to_string(&memoria).unwrap();
    texto.push_str(&format!(
        "\n§\nToken do GitHub para o backup: {chave_falsa}\n"
    ));
    std::fs::write(&memoria, texto).unwrap();

    let relatorio = hermes::importar(&amb.config, &origem, true).unwrap();
    let saida = relatorio.to_string();
    assert!(saida.contains("memories/MEMORY.md, entrada 5: parece conter um segredo (ghp_)"));
    assert!(!saida.contains(&chave_falsa));
    for (caminho, bytes) in fotografia(amb.pasta.path()) {
        assert!(
            !String::from_utf8_lossy(&bytes).contains(&chave_falsa),
            "vazou em {}",
            caminho.display()
        );
    }
}

#[tokio::test]
async fn nao_sobrescreve_rascunho_editado_nem_ressuscita_nota_esquecida() {
    let amb = Ambiente::novo().await;
    let (_pasta, origem) = origem_com_segredos();
    hermes::importar(&amb.config, &origem, true).unwrap();

    let proposto = amb.caminho("identity/nucleo.proposto.md");
    std::fs::write(&proposto, "minha versão editada").unwrap();
    let memoria = Memoria::abrir(&amb.config, amb.banco.clone()).unwrap();
    memoria
        .esquecer("01_internal/diario/2026-09-27.md")
        .unwrap();

    let relatorio = hermes::importar(&amb.config, &origem, true).unwrap();
    assert!(relatorio.to_string().contains("NÃO sobrescrito"));
    assert_eq!(
        std::fs::read_to_string(&proposto).unwrap(),
        "minha versão editada"
    );
    assert!(
        !amb.config
            .caminho_cofre()
            .join("01_internal/diario/2026-09-27.md")
            .exists()
    );
}

#[tokio::test]
async fn banco_recriado_nao_duplica_notas_ja_importadas() {
    let amb = Ambiente::novo().await;
    let (_pasta, origem) = origem_com_segredos();
    hermes::importar(&amb.config, &origem, true).unwrap();
    let antes = fotografia(&amb.config.caminho_cofre());

    // Perde as chaves de importação (como num banco recriado), mantém o cofre.
    amb.banco
        .conexao()
        .execute("DELETE FROM importacoes", [])
        .unwrap();
    let relatorio = hermes::importar(&amb.config, &origem, true).unwrap();
    assert!(relatorio.to_string().contains("(já existia, mesma origem)"));
    // Nenhuma nota "-hermes" duplicada no cofre.
    assert_eq!(fotografia(&amb.config.caminho_cofre()), antes);
}
