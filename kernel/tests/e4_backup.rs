//! E4: backup consistente do banco (VACUUM INTO) e do cofre, com rotação,
//! sem seguir links simbólicos e pulando quando falta disco.

mod comum;

use abiyss::backup::{self, fazer_backup};
use abiyss::eventos;
use chrono::NaiveDate;
use comum::Ambiente;
use rusqlite::{Connection, OpenFlags};

fn hoje() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
}

fn preparar(amb: &Ambiente) {
    eventos::publicar(&amb.banco, "teste", "t", "algo para guardar").unwrap();
    let cofre = amb.config.caminho_cofre();
    std::fs::create_dir_all(cofre.join("01_internal/pessoas")).unwrap();
    std::fs::write(
        cofre.join("01_internal/pessoas/ana.md"),
        "Ana gosta de chá.",
    )
    .unwrap();
    std::fs::create_dir_all(amb.caminho("identity")).unwrap();
    std::fs::write(amb.config.caminho_memoria_central(), "[dito] Prefere café.").unwrap();
    // Um link simbólico apontando para fora do cofre: nunca é seguido.
    let fora = amb.caminho("segredo-fora.txt");
    std::fs::write(&fora, "NAO-PODE-IR-PARA-O-BACKUP").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&fora, cofre.join("01_internal/atalho.md")).unwrap();
}

fn conteudo_de_tudo(pasta: &std::path::Path) -> String {
    let mut texto = String::new();
    for entrada in std::fs::read_dir(pasta).unwrap() {
        let entrada = entrada.unwrap();
        let tipo = entrada.file_type().unwrap();
        if tipo.is_dir() {
            texto.push_str(&conteudo_de_tudo(&entrada.path()));
        } else if tipo.is_file() {
            texto.push_str(&String::from_utf8_lossy(
                &std::fs::read(entrada.path()).unwrap(),
            ));
        } else {
            texto.push_str("<link>");
        }
    }
    texto
}

#[tokio::test]
async fn backup_consistente_do_banco_e_do_cofre() {
    let amb = Ambiente::novo().await;
    preparar(&amb);
    let relatorio = fazer_backup(&amb.config, hoje(), 100.0).unwrap();
    assert!(relatorio.pulado.is_none());
    let pasta = backup::pasta_backups(&amb.config).join("2026-10-05");
    assert_eq!(relatorio.pasta, pasta);

    // O banco copiado abre, passa no integrity_check e tem os dados.
    let copia =
        Connection::open_with_flags(pasta.join("abiyss.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let eventos: i64 = copia
        .query_row("SELECT COUNT(*) FROM fila_eventos", [], |l| l.get(0))
        .unwrap();
    assert_eq!(eventos, 1);

    assert_eq!(
        std::fs::read_to_string(pasta.join("cofre/01_internal/pessoas/ana.md")).unwrap(),
        "Ana gosta de chá."
    );
    assert!(pasta.join("identity/memoria-central.md").is_file());
    assert!(pasta.join("identity/nucleo.md").is_file());
    #[cfg(unix)]
    {
        assert_eq!(relatorio.links_ignorados, 1);
        assert!(!conteudo_de_tudo(&pasta).contains("NAO-PODE-IR-PARA-O-BACKUP"));
    }
    assert!(relatorio.resumo().contains("backup em"));
}

#[tokio::test]
async fn segundo_backup_do_dia_substitui_o_primeiro() {
    let amb = Ambiente::novo().await;
    preparar(&amb);
    fazer_backup(&amb.config, hoje(), 100.0).unwrap();
    std::fs::write(
        amb.config
            .caminho_cofre()
            .join("01_internal/pessoas/ana.md"),
        "Ana agora prefere café.",
    )
    .unwrap();
    fazer_backup(&amb.config, hoje(), 100.0).unwrap();
    let raiz = backup::pasta_backups(&amb.config);
    let pastas: Vec<_> = std::fs::read_dir(&raiz).unwrap().collect();
    assert_eq!(
        pastas.len(),
        1,
        "um backup por dia, sem pasta temporária sobrando"
    );
    assert_eq!(
        std::fs::read_to_string(raiz.join("2026-10-05/cofre/01_internal/pessoas/ana.md")).unwrap(),
        "Ana agora prefere café."
    );
}

#[tokio::test]
async fn sem_disco_o_backup_e_pulado() {
    let amb = Ambiente::novo().await;
    let relatorio = fazer_backup(&amb.config, hoje(), 0.5).unwrap();
    assert!(relatorio.pulado.as_deref().unwrap().contains("livres"));
    assert!(!backup::pasta_backups(&amb.config).exists());
}

#[tokio::test]
async fn rotacao_apaga_os_antigos() {
    let amb = Ambiente::novo().await;
    let raiz = backup::pasta_backups(&amb.config);
    for i in 1..30u64 {
        let data = hoje() - chrono::Days::new(i);
        std::fs::create_dir_all(raiz.join(data.format("%Y-%m-%d").to_string())).unwrap();
    }
    // Uma pasta que não é de backup nunca é tocada.
    std::fs::create_dir_all(raiz.join("minha-pasta")).unwrap();
    let relatorio = fazer_backup(&amb.config, hoje(), 100.0).unwrap();
    let restantes = std::fs::read_dir(&raiz).unwrap().count();
    // 7 diários (com o de hoje) + 4 domingos + a pasta do usuário.
    assert_eq!(restantes, 12, "{:?}", relatorio.removidos);
    assert!(raiz.join("minha-pasta").is_dir());
    assert_eq!(relatorio.removidos.len(), 30 - 11);
}

#[tokio::test]
async fn cofre_acima_do_teto_nao_e_copiado_mas_o_banco_sim() {
    let mut amb = Ambiente::novo().await;
    preparar(&amb);
    amb.config.backup.max_megabytes_cofre = 0;
    let relatorio = fazer_backup(&amb.config, hoje(), 100.0).unwrap();
    assert!(
        relatorio
            .avisos
            .iter()
            .any(|a| a.contains("cofre NÃO copiado"))
    );
    assert!(relatorio.pasta.join("abiyss.db").is_file());
    assert!(!relatorio.pasta.join("cofre").exists());
}
