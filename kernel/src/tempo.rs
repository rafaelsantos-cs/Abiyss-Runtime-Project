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
