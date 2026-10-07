//! Infra 5: o binário `abiyss daemon` conversa com o systemd (sd_notify):
//! READY=1 ao subir, WATCHDOG=1 periódico e STOPPING=1 no SIGTERM. E um
//! modelo lento não para os WATCHDOG=1 (o loop não espera a resposta).
//! O "systemd" aqui é só um socket Unix de datagrama numa pasta temporária.

#![cfg(target_os = "linux")]

use std::os::unix::net::UnixDatagram;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use abiyss::db::Banco;
use abiyss::eventos;
use abiyss::nim::mock::{MockNim, RespostaMock};

/// Projeto mínimo: abiyss.toml apontando para o mock + núcleo de identidade.
fn criar_projeto(pasta: &std::path::Path, base_url: &str) -> std::path::PathBuf {
    criar_projeto_com(pasta, base_url, "heartbeat_segundos = 1")
}

/// Como `criar_projeto`, com linhas extras em `[daemon]`.
fn criar_projeto_com(
    pasta: &std::path::Path,
    base_url: &str,
    daemon_extra: &str,
) -> std::path::PathBuf {
    std::fs::create_dir_all(pasta.join("identity")).unwrap();
    std::fs::write(pasta.join("identity/nucleo.md"), "Sou o Abiyss de teste.").unwrap();
    let toml = format!(
        r#"
        [nim]
        base_url = "{base_url}"
        [modelos.cerebro]
        id = "teste/cerebro"
        [modelos.sub_ultra]
        id = "teste/ultra"
        [modelos.sub_medium]
        id = "teste/medium"
        [modelos.sub_low]
        id = "teste/low"
        [pools.cerebro]
        api_key_env = "ABIYSS_TESTE_A"
        [pools.subagentes]
        api_key_env = "ABIYSS_TESTE_B"
        # O sono de recuperação depende da hora em que o teste roda: fora.
        [sono]
        ativo = false
        [daemon]
        cron_verificacao_segundos = 1
        {daemon_extra}
        "#
    );
    let caminho = pasta.join("abiyss.toml");
    std::fs::write(&caminho, toml).unwrap();
    caminho
}

/// Próxima mensagem do "systemd" (ou pânico depois de `limite`).
fn receber(socket: &UnixDatagram, limite: Duration) -> String {
    tentar_receber(socket, limite).expect("nenhuma mensagem do daemon")
}

/// Próxima mensagem do "systemd", se chegar em até `limite`.
fn tentar_receber(socket: &UnixDatagram, limite: Duration) -> Option<String> {
    socket.set_read_timeout(Some(limite)).unwrap();
    let mut buffer = [0u8; 512];
    let n = socket.recv(&mut buffer).ok()?;
    Some(String::from_utf8_lossy(&buffer[..n]).to_string())
}

/// O daemon filho; morto se o teste falhar no meio (senão ficaria órfão).
struct DaemonFilho(Option<Child>);

impl Drop for DaemonFilho {
    fn drop(&mut self) {
        if let Some(filho) = self.0.as_mut() {
            let _ = filho.kill();
            let _ = filho.wait();
        }
    }
}

/// Sobe o binário `abiyss daemon` com o "systemd" de teste: aviso do
/// watchdog a cada metade de `watchdog_usec`.
fn subir_daemon(
    config: &std::path::Path,
    socket: &std::path::Path,
    watchdog_usec: &str,
) -> DaemonFilho {
    let filho = Command::new(env!("CARGO_BIN_EXE_abiyss"))
        .arg("--config")
        .arg(config)
        .arg("daemon")
        .env("ABIYSS_TESTE_A", "nvapi-teste-a")
        .env("ABIYSS_TESTE_B", "nvapi-teste-b")
        .env("NOTIFY_SOCKET", socket)
        .env("WATCHDOG_USEC", watchdog_usec)
        .env_remove("WATCHDOG_PID")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    DaemonFilho(Some(filho))
}

/// SIGTERM, espera o STOPPING=1 e a saída com sucesso.
fn parar_daemon(mut filho: DaemonFilho, socket: &UnixDatagram) {
    let mut daemon = filho.0.take().expect("daemon já parado");
    // SAFETY: só envia um sinal ao processo filho que criamos.
    unsafe {
        libc::kill(daemon.id() as i32, libc::SIGTERM);
    }
    let mut parou = false;
    for _ in 0..200 {
        let mensagem = receber(socket, Duration::from_secs(10));
        if mensagem.starts_with("STOPPING=1") {
            parou = true;
            break;
        }
    }
    assert!(parou, "o daemon não avisou STOPPING=1");
    let status = daemon.wait().unwrap();
    assert!(status.success(), "{status}");
}

#[tokio::test(flavor = "multi_thread")]
async fn daemon_avisa_ready_watchdog_e_stopping() {
    let mock = MockNim::iniciar().await;
    let pasta = tempfile::tempdir().unwrap();
    let config = criar_projeto(pasta.path(), &mock.base_url());
    let caminho_socket = pasta.path().join("notify.sock");
    let socket = UnixDatagram::bind(&caminho_socket).unwrap();

    // WatchdogSec=0,4 s → aviso a cada 200 ms.
    let daemon = subir_daemon(&config, &caminho_socket, "400000");

    let primeira = receber(&socket, Duration::from_secs(30));
    assert!(primeira.starts_with("READY=1"), "{primeira}");

    // Vários WATCHDOG=1 seguidos, no ritmo pedido.
    let inicio = Instant::now();
    let mut avisos = 0;
    while avisos < 5 {
        let mensagem = receber(&socket, Duration::from_secs(5));
        if mensagem == "WATCHDOG=1" {
            avisos += 1;
        }
    }
    assert!(
        inicio.elapsed() < Duration::from_secs(3),
        "{:?}",
        inicio.elapsed()
    );

    // SIGTERM (o que o systemd manda no `stop`).
    parar_daemon(daemon, &socket);
}

/// Um ciclo esperando um modelo lento (resposta 5 s depois, mais que o
/// dobro do `max_travado_segundos` de 2 s) não segura o loop principal: os
/// WATCHDOG=1 continuam chegando no ritmo e o daemon nunca se declara travado.
#[tokio::test(flavor = "multi_thread")]
async fn modelo_lento_nao_para_o_watchdog() {
    const ATRASO: Duration = Duration::from_secs(5);
    let mock = MockNim::iniciar().await;
    mock.definir_roteiro(|_| {
        RespostaMock::texto(r#"{"percepcao": "", "orientacao": "", "decisao": "", "acoes": []}"#)
            .atrasada(ATRASO)
    });
    let pasta = tempfile::tempdir().unwrap();
    let config = criar_projeto_com(
        pasta.path(),
        &mock.base_url(),
        "heartbeat_segundos = 3600\nmax_travado_segundos = 2",
    );
    // Um evento na fila: o primeiro ciclo (logo ao subir) chama o modelo.
    {
        let banco = Banco::abrir(&pasta.path().join("data/abiyss.db")).unwrap();
        eventos::publicar(&banco, "teste", "t", "acorde").unwrap();
    }
    let caminho_socket = pasta.path().join("notify.sock");
    let socket = UnixDatagram::bind(&caminho_socket).unwrap();
    let daemon = subir_daemon(&config, &caminho_socket, "400000");
    let primeira = receber(&socket, Duration::from_secs(30));
    assert!(primeira.starts_with("READY=1"), "{primeira}");

    // Espera o ciclo mandar o pedido ao modelo (e começar a esperar). Lendo
    // os avisos enquanto isso: com a fila do socket cheia, o envio do daemon
    // bloquearia (o systemd de verdade sempre lê).
    let limite = Instant::now() + Duration::from_secs(20);
    while mock.total_requisicoes() == 0 {
        assert!(Instant::now() < limite, "o ciclo não chamou o modelo");
        if let Some(mensagem) = tentar_receber(&socket, Duration::from_millis(20)) {
            assert!(!mensagem.contains("travado"), "loop parado: {mensagem}");
        }
    }
    let pedido = Instant::now();

    // Enquanto o modelo "pensa" (bem mais que o max_travado), os avisos seguem.
    let janela = ATRASO - Duration::from_millis(1500);
    let mut avisos = Vec::new();
    while pedido.elapsed() < janela {
        let mensagem = receber(&socket, Duration::from_secs(3));
        assert!(
            !mensagem.contains("travado"),
            "o loop parou esperando o modelo: {mensagem}"
        );
        if mensagem == "WATCHDOG=1" {
            avisos.push(Instant::now());
        }
    }
    // Só o pedido do ciclo, ainda esperando a resposta (nenhuma nova tentativa).
    assert_eq!(mock.total_requisicoes(), 1);
    // ~3,5 s com um aviso a cada 200 ms: uns 17; nenhum buraco perto do limite.
    assert!(
        avisos.len() >= 8,
        "só {} aviso(s) em {janela:?}",
        avisos.len()
    );
    let maior_buraco = avisos.windows(2).map(|par| par[1] - par[0]).max().unwrap();
    assert!(
        maior_buraco < Duration::from_secs(1),
        "buraco de {maior_buraco:?} entre avisos do watchdog"
    );

    parar_daemon(daemon, &socket);
}
