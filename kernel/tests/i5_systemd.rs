//! Infra 5: o binário `abiyss daemon` conversa com o systemd (sd_notify):
//! READY=1 ao subir, WATCHDOG=1 periódico e STOPPING=1 no SIGTERM.
//! O "systemd" aqui é só um socket Unix de datagrama numa pasta temporária.

#![cfg(target_os = "linux")]

use std::os::unix::net::UnixDatagram;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use abiyss::nim::mock::MockNim;

/// Projeto mínimo: abiyss.toml apontando para o mock + núcleo de identidade.
fn criar_projeto(pasta: &std::path::Path, base_url: &str) -> std::path::PathBuf {
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
        [daemon]
        heartbeat_segundos = 1
        cron_verificacao_segundos = 1
        "#
    );
    let caminho = pasta.join("abiyss.toml");
    std::fs::write(&caminho, toml).unwrap();
    caminho
}

/// Próxima mensagem do "systemd" (ou pânico depois de `limite`).
fn receber(socket: &UnixDatagram, limite: Duration) -> String {
    socket.set_read_timeout(Some(limite)).unwrap();
    let mut buffer = [0u8; 512];
    let n = socket
        .recv(&mut buffer)
        .expect("nenhuma mensagem do daemon");
    String::from_utf8_lossy(&buffer[..n]).to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn daemon_avisa_ready_watchdog_e_stopping() {
    let mock = MockNim::iniciar().await;
    let pasta = tempfile::tempdir().unwrap();
    let config = criar_projeto(pasta.path(), &mock.base_url());
    let caminho_socket = pasta.path().join("notify.sock");
    let socket = UnixDatagram::bind(&caminho_socket).unwrap();

    let mut daemon = Command::new(env!("CARGO_BIN_EXE_abiyss"))
        .arg("--config")
        .arg(&config)
        .arg("daemon")
        .env("ABIYSS_TESTE_A", "nvapi-teste-a")
        .env("ABIYSS_TESTE_B", "nvapi-teste-b")
        .env("NOTIFY_SOCKET", &caminho_socket)
        // WatchdogSec=0,4 s → aviso a cada 200 ms.
        .env("WATCHDOG_USEC", "400000")
        .env_remove("WATCHDOG_PID")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

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
    // SAFETY: só envia um sinal ao processo filho que criamos.
    unsafe {
        libc::kill(daemon.id() as i32, libc::SIGTERM);
    }
    let mut parou = false;
    for _ in 0..50 {
        let mensagem = receber(&socket, Duration::from_secs(10));
        if mensagem.starts_with("STOPPING=1") {
            parou = true;
            break;
        }
    }
    assert!(parou, "o daemon não avisou STOPPING=1");
    let status = daemon.wait().unwrap();
    assert!(status.success(), "{status}");
}
