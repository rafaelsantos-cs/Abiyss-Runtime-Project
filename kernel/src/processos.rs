//! Processos filhos (servidores MCP): memória usada e encerramento.
//!
//! Cada servidor MCP por stdio roda no SEU grupo de processos (o PID do
//! filho é também o ID do grupo). Isso importa porque `uv run servidor.py`
//! sobe o Python como NETO: para medir a memória e para matar tudo, olhamos
//! a árvore inteira, não só o `uv`.
//!
//! Só Linux (lê `/proc`). Em outros sistemas a memória fica desconhecida
//! (`None`) e a supervisão só reage a quedas e ao tempo de vida.

/// Memória residente (RSS) somada do processo e de todos os descendentes,
/// em bytes. `None` se o processo não existe ou o sistema não tem `/proc`.
#[cfg(target_os = "linux")]
pub fn rss_da_arvore_bytes(pid: u32) -> Option<u64> {
    use std::collections::HashMap;

    // Pai de cada processo do sistema.
    let mut filhos_de: HashMap<u32, Vec<u32>> = HashMap::new();
    for entrada in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(outro) = entrada
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if let Some(pai) = pai_de(outro) {
            filhos_de.entry(pai).or_default().push(outro);
        }
    }

    let mut total = rss_bytes(pid)?;
    let mut pendentes = filhos_de.get(&pid).cloned().unwrap_or_default();
    while let Some(atual) = pendentes.pop() {
        total += rss_bytes(atual).unwrap_or(0);
        if let Some(netos) = filhos_de.get(&atual) {
            pendentes.extend(netos);
        }
    }
    Some(total)
}

#[cfg(not(target_os = "linux"))]
pub fn rss_da_arvore_bytes(_pid: u32) -> Option<u64> {
    None
}

/// PID do pai, lido de `/proc/<pid>/stat`. O nome do programa (2º campo)
/// vem entre parênteses e pode ter espaços, então lemos depois do último ')'.
#[cfg(target_os = "linux")]
fn pai_de(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let depois_do_nome = &stat[stat.rfind(')')? + 1..];
    // Campos depois do nome: estado, pai, ...
    depois_do_nome.split_whitespace().nth(1)?.parse().ok()
}

/// `VmRSS` de `/proc/<pid>/status` (em kB no arquivo), em bytes.
#[cfg(target_os = "linux")]
fn rss_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let linha = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = linha.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// Memória residente do PRÓPRIO processo, em bytes (para o teste de
/// resistência e diagnósticos).
pub fn rss_proprio_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        rss_bytes(std::process::id())
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Mata (SIGKILL) o grupo de processos inteiro. Sem efeito se o grupo já
/// não existe.
#[cfg(unix)]
pub fn matar_grupo(pgid: u32) {
    let Ok(pgid) = i32::try_from(pgid) else {
        return;
    };
    if pgid <= 1 {
        return; // nunca o grupo 0 (o nosso) nem o init
    }
    // SAFETY: `kill` só envia um sinal; PID negativo = o grupo inteiro.
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
pub fn matar_grupo(_pgid: u32) {}

#[cfg(all(test, target_os = "linux"))]
mod testes {
    use super::*;

    #[test]
    fn mede_a_arvore_e_mata_o_grupo() {
        use std::os::unix::process::CommandExt;
        // sh (grupo novo) com um filho sleep: a árvore tem 2 processos.
        let mut filho = std::process::Command::new("sh")
            .args(["-c", "sleep 30 & wait"])
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = filho.id();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let arvore = rss_da_arvore_bytes(pid).unwrap();
        let so_o_sh = rss_bytes(pid).unwrap();
        assert!(
            arvore > so_o_sh,
            "a árvore inclui o sleep ({arvore} > {so_o_sh})"
        );

        matar_grupo(pid);
        let status = filho.wait().unwrap();
        assert!(!status.success());
        assert!(rss_proprio_bytes().unwrap() > 0);
    }
}
