//! Funções de tempo usadas em vários módulos.
//!
//! Tudo que vai para o banco usa milissegundos desde 1970 (UTC), porque
//! o relógio precisa ser o mesmo entre processos diferentes (chat e daemon).

/// Agora, em milissegundos desde 1970 (UTC).
pub fn agora_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Converte milissegundos (UTC) para texto legível no fuso local.
pub fn formatar_ms(ms: i64) -> String {
    match chrono::DateTime::from_timestamp_millis(ms) {
        Some(data) => data
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        None => format!("{ms}ms"),
    }
}

/// Duração legível: "45 s", "12 min", "2 h 05 min", "3 d 4 h".
pub fn formatar_duracao(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (d, h, m) = (s / 86_400, (s % 86_400) / 3_600, (s % 3_600) / 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m:02} min")
    } else if m > 0 {
        format!("{m} min")
    } else {
        format!("{s} s")
    }
}

/// Agora no fuso local, no formato das notas do cofre
/// (ex.: "2026-10-05T14:03:11-03:00").
pub fn agora_iso() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

/// Hoje no fuso local (ex.: "2026-10-05").
pub fn hoje() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn duracoes_legiveis() {
        assert_eq!(formatar_duracao(45_000), "45 s");
        assert_eq!(formatar_duracao(12 * 60_000 + 5_000), "12 min");
        assert_eq!(formatar_duracao(2 * 3_600_000 + 5 * 60_000), "2 h 05 min");
        assert_eq!(formatar_duracao(3 * 86_400_000 + 4 * 3_600_000), "3 d 4 h");
        assert_eq!(formatar_duracao(-5), "0 s");
    }
}
