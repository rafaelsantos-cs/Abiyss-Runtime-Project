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
