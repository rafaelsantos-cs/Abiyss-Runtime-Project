//! Conversa com o systemd pelo protocolo `sd_notify`: uma mensagem de texto
//! (ex.: "READY=1") num socket Unix de datagrama cujo caminho vem na
//! variável `NOTIFY_SOCKET`. Fora do systemd (variável ausente), tudo aqui
//! é silenciosamente ignorado.
//!
//! - `READY=1`: o daemon terminou de subir (unit com `Type=notify`).
//! - `WATCHDOG=1`: "ainda estou vivo". Com `WatchdogSec=` na unit, o
//!   systemd mata e reinicia o serviço se isso parar de chegar.
//! - `STOPPING=1`: parada em andamento.
//! - `STATUS=...`: texto livre mostrado no `systemctl status`.

use std::io;
use std::os::unix::net::UnixDatagram;
use std::time::Duration;

/// Envia `mensagem` ao systemd, se houver um `NOTIFY_SOCKET`. Devolve
/// `true` se a mensagem foi entregue.
pub fn notificar(mensagem: &str) -> bool {
    let Some(caminho) = std::env::var_os("NOTIFY_SOCKET") else {
        return false;
    };
    match notificar_em(&caminho.to_string_lossy(), mensagem) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("sd_notify falhou ({mensagem:?}): {e}");
            false
        }
    }
}

/// Envia para um socket específico. Caminhos começando com '@' são
/// sockets "abstratos" do Linux (sem arquivo no disco).
pub fn notificar_em(caminho: &str, mensagem: &str) -> io::Result<()> {
    let socket = UnixDatagram::unbound()?;
    match caminho.strip_prefix('@') {
        #[cfg(target_os = "linux")]
        Some(nome) => {
            use std::os::linux::net::SocketAddrExt;
            let endereco = std::os::unix::net::SocketAddr::from_abstract_name(nome)?;
            socket.send_to_addr(mensagem.as_bytes(), &endereco)?;
        }
        #[cfg(not(target_os = "linux"))]
        Some(_) => return Err(io::Error::other("socket abstrato só existe no Linux")),
        None => {
            socket.send_to(mensagem.as_bytes(), caminho)?;
        }
    }
    Ok(())
}

/// De quanto em quanto tempo mandar `WATCHDOG=1`: metade do `WatchdogSec`
/// (recomendação do systemd). `None` se o watchdog não está ligado para
/// este processo.
pub fn intervalo_watchdog() -> Option<Duration> {
    intervalo_watchdog_de(
        std::env::var("WATCHDOG_USEC").ok().as_deref(),
        std::env::var("WATCHDOG_PID").ok().as_deref(),
        std::process::id(),
    )
}

/// Parte pura de `intervalo_watchdog` (testável sem mexer no ambiente).
pub fn intervalo_watchdog_de(
    watchdog_usec: Option<&str>,
    watchdog_pid: Option<&str>,
    meu_pid: u32,
) -> Option<Duration> {
    let microssegundos: u64 = watchdog_usec?.trim().parse().ok()?;
    if microssegundos == 0 {
        return None;
    }
    // Se WATCHDOG_PID existe, o watchdog é só daquele processo.
    if let Some(pid) = watchdog_pid
        && pid.trim().parse::<u32>().ok() != Some(meu_pid)
    {
        return None;
    }
    Some(Duration::from_micros(microssegundos / 2))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn intervalo_e_metade_do_watchdog_e_respeita_o_pid() {
        assert_eq!(
            intervalo_watchdog_de(Some("120000000"), None, 7),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            intervalo_watchdog_de(Some("120000000"), Some("7"), 7),
            Some(Duration::from_secs(60))
        );
        assert_eq!(intervalo_watchdog_de(Some("120000000"), Some("8"), 7), None);
        assert_eq!(intervalo_watchdog_de(Some("0"), None, 7), None);
        assert_eq!(intervalo_watchdog_de(Some("abc"), None, 7), None);
        assert_eq!(intervalo_watchdog_de(None, None, 7), None);
    }

    #[test]
    fn envia_para_socket_em_arquivo() {
        let pasta = tempfile::tempdir().unwrap();
        let caminho = pasta.path().join("notify.sock");
        let receptor = UnixDatagram::bind(&caminho).unwrap();
        notificar_em(caminho.to_str().unwrap(), "READY=1").unwrap();
        let mut buffer = [0u8; 64];
        let n = receptor.recv(&mut buffer).unwrap();
        assert_eq!(&buffer[..n], b"READY=1");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn envia_para_socket_abstrato() {
        use std::os::linux::net::SocketAddrExt;
        let nome = format!("abiyss-teste-{}", std::process::id());
        let endereco = std::os::unix::net::SocketAddr::from_abstract_name(&nome).unwrap();
        let receptor = UnixDatagram::bind_addr(&endereco).unwrap();
        notificar_em(&format!("@{nome}"), "WATCHDOG=1").unwrap();
        let mut buffer = [0u8; 64];
        let n = receptor.recv(&mut buffer).unwrap();
        assert_eq!(&buffer[..n], b"WATCHDOG=1");
    }
}
