//! Kernel do Abiyss.
//!
//! O kernel é a parte do Abiyss que ele NÃO pode modificar: cliente do NIM,
//! orquestrador de chamadas, persistência, regras de segurança das
//! ferramentas e o loop do daemon. O que o Abiyss poderá editar no futuro
//! fica fora daqui, em `recursos/` (servidores MCP em Python).

pub mod caminho_seguro;
pub mod chat;
pub mod config;
pub mod cron;
pub mod dados;
pub mod daemon;
pub mod db;
pub mod diario;
pub mod eventos;
pub mod ferramentas;
pub mod frontmatter;
pub mod goals;
pub mod heartbeat;
pub mod historico;
pub mod identidade;
pub mod interocepcao;
pub mod mcp;
pub mod memoria;
pub mod nim;
pub mod orquestrador;
pub mod skills;
pub mod status;
pub mod subagentes;
pub mod tempo;
