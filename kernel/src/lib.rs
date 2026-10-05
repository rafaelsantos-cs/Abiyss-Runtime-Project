//! Kernel do Abiyss.
//!
//! O kernel é a parte do Abiyss que ele NÃO pode modificar: cliente do NIM,
//! orquestrador de chamadas, persistência, regras de segurança das
//! ferramentas e o loop do daemon. O que o Abiyss poderá editar no futuro
//! fica fora daqui, em `recursos/` (servidores MCP em Python).

pub mod backup;
pub mod caminho_seguro;
pub mod chat;
pub mod config;
pub mod cron;
pub mod dados;
pub mod daemon;
pub mod db;
pub mod diario;
pub mod esforco;
pub mod eventos;
pub mod ferramentas;
pub mod frontmatter;
pub mod goals;
pub mod heartbeat;
pub mod hermes;
pub mod historico;
pub mod identidade;
pub mod importacoes;
pub mod interocepcao;
pub mod latencia;
pub mod manutencao;
pub mod mcp;
pub mod memoria;
pub mod nim;
pub mod orcamento;
pub mod orquestrador;
pub mod processos;
pub mod ritmo;
pub mod skills;
pub mod sono;
pub mod status;
pub mod subagentes;
#[cfg(unix)]
pub mod systemd;
pub mod tempo;
