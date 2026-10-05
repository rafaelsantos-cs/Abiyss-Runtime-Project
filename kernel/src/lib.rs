//! Kernel do Abiyss.
//!
//! O kernel é a parte do Abiyss que ele NÃO pode modificar: cliente do NIM,
//! orquestrador de chamadas, persistência, regras de segurança das
//! ferramentas e o loop do daemon. O que o Abiyss poderá editar no futuro
//! fica fora daqui, em `recursos/` (servidores MCP em Python).

pub mod config;
pub mod db;
pub mod nim;
pub mod orquestrador;
pub mod tempo;
