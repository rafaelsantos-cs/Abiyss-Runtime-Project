"""Servidor MCP `terminal` do Abiyss: comandos de shell numa caixa de areia.

Cada comando roda num bubblewrap (bwrap) novo, com o workspace gravável em
/workspace, o sistema só para leitura e (por padrão) sem rede. Detalhes da
caixa em isolamento.py; limites e de onde vêm em config.py; instalação na
VM no README.md desta pasta.

Sem bwrap, ou sem namespaces de usuário, o servidor NÃO sobe: escreve o
motivo no stderr (que vai para o journal do daemon) e sai com código 2.

O kernel sobe este arquivo como processo filho e conversa por stdio: NUNCA
use print() para stdout aqui; logs vão para o stderr.
"""

import logging
import sys
from typing import Annotated, Any

import mcp.types as types
from mcp.server import MCPServer
from mcp.server.mcpserver.exceptions import ToolError
from pydantic import Field

import config
import isolamento

log = logging.getLogger("terminal")

# Linha de comando maior que isto vira script no workspace (o Linux aceita
# até 128 KiB por argumento).
MAX_BYTES_COMANDO = 100_000

DICAS = (
    (
        ("Cannot allocate memory", "MemoryError", "out of memory", "bad_alloc", "memory exhausted"),
        "o limite de memória é de {memoria} MiB por comando",
    ),
    (
        ("Cannot fork", "Resource temporarily unavailable", "fork: retry", "can't start new thread"),
        "o limite é de {processos} processos/threads por comando",
    ),
    (
        ("File too large", "File size limit exceeded"),
        "o limite é de {arquivo} MiB por arquivo",
    ),
    (
        ("Network is unreachable", "Temporary failure in name resolution", "Could not resolve host"),
        "a caixa não tem rede",
    ),
)


def descricao(cfg: config.Config) -> str:
    rede = "COM rede" if cfg.rede else "SEM rede"
    return (
        "Executa um comando de shell (bash -c) numa caixa de areia isolada (bubblewrap) e devolve "
        "o código de saída, o stdout e o stderr. A pasta atual é /workspace: o workspace do Abiyss, "
        "o ÚNICO lugar gravável e que persiste entre comandos. O sistema (/usr, /etc) é só leitura; "
        "o resto do projeto e a pasta pessoal não existem aqui. /tmp (também a HOME) e processos "
        f"deixados em segundo plano somem no fim de cada comando. {rede}. Sem sudo e sem entrada "
        f"interativa (stdin vazio). Limites por comando: {cfg.timeout_padrao} s por padrão (até "
        f"{cfg.timeout_maximo} s com timeout_segundos), {cfg.memoria_mb} MiB de memória, "
        f"{cfg.max_processos} processos/threads, arquivos de até {cfg.max_arquivo_mb} MiB e /tmp "
        f"de {cfg.tmp_mb} MiB; stdout e stderr cortados em {cfg.max_saida_bytes} bytes cada "
        f"(começo e fim). Até {cfg.max_concorrentes} comando(s) ao mesmo tempo."
    )


def _resumo(comando: str, limite: int = 200) -> str:
    linhas = comando.strip().splitlines() or [""]
    primeira = linhas[0]
    if len(primeira) > limite or len(linhas) > 1:
        return primeira[:limite] + " …"
    return primeira


def _bytes(n: int) -> str:
    return f"{n:,}".replace(",", ".")


def _segundos(s: float) -> str:
    return f"{s:.2f}".replace(".", ",") + " s"


def dicas(cfg: config.Config, r: isolamento.Resultado) -> list[str]:
    """Erros típicos de bater num limite da caixa, ditos em voz alta."""
    if r.interrupcao or r.codigo_saida == 0:
        return []
    texto = r.stderr + "\n" + r.stdout
    achadas = []
    for padroes, dica in DICAS:
        if cfg.rede and "rede" in dica:
            continue
        if any(p.lower() in texto.lower() for p in padroes):
            achadas.append(
                dica.format(memoria=cfg.memoria_mb, processos=cfg.max_processos, arquivo=cfg.max_arquivo_mb)
            )
    return achadas


def formatar(cfg: config.Config, comando: str, r: isolamento.Resultado, avisos: list[str]) -> str:
    linhas = [f"$ {_resumo(comando)}"]
    if r.codigo_saida is None:
        linhas.append("código de saída: nenhum (o comando foi interrompido)")
    else:
        linhas.append(f"código de saída: {r.codigo_saida}")
    duracao = f"duração: {_segundos(r.duracao_segundos)}"
    if r.espera_fila_segundos >= 0.1:
        duracao += f" (mais {_segundos(r.espera_fila_segundos)} esperando vaga)"
    linhas.append(duracao)
    if r.interrupcao:
        motivo = isolamento.MOTIVOS[r.interrupcao]
        detalhe = {
            "tempo": f" ({r.timeout_segundos} s)",
            "memoria": f" ({cfg.memoria_mb} MiB para o comando inteiro)",
            "processos": f" ({cfg.max_processos})",
        }.get(r.interrupcao, "")
        linhas.append(f"INTERROMPIDO: {motivo}{detalhe}; a saída abaixo é parcial.")
    linhas += [f"aviso: {a}" for a in avisos]
    linhas += [f"dica: {d}" for d in dicas(cfg, r)]
    for nome, texto, total in (("stdout", r.stdout, r.stdout_bytes), ("stderr", r.stderr, r.stderr_bytes)):
        if total == 0:
            linhas.append(f"--- {nome} (vazio) ---")
        else:
            linhas.append(f"--- {nome} ({_bytes(total)} bytes) ---")
            linhas.append(texto.rstrip("\n"))
    return "\n".join(linhas) + "\n"


def estruturado(r: isolamento.Resultado) -> dict[str, Any]:
    return {
        "codigo_saida": r.codigo_saida,
        "stdout": r.stdout,
        "stderr": r.stderr,
        "stdout_bytes": r.stdout_bytes,
        "stderr_bytes": r.stderr_bytes,
        "stdout_cortado": r.stdout_cortado,
        "stderr_cortado": r.stderr_cortado,
        "duracao_segundos": r.duracao_segundos,
        "espera_fila_segundos": r.espera_fila_segundos,
        "timeout_segundos": r.timeout_segundos,
        "interrompido": r.interrupcao,
        "pico_memoria_mb": r.pico_memoria_mb,
    }


def criar_servidor(cfg: config.Config, amb: isolamento.Ambiente) -> MCPServer:
    executor = isolamento.Executor(cfg, amb)
    servidor = MCPServer("terminal")

    async def executar(
        comando: Annotated[
            str,
            Field(description="Linha de comando do bash (aceita pipes, &&, redirecionamentos e heredoc)."),
        ],
        timeout_segundos: Annotated[
            int | None,
            Field(
                description=f"Tempo máximo em segundos (padrão {cfg.timeout_padrao}, máximo "
                f"{cfg.timeout_maximo}). Inclui a espera por uma vaga."
            ),
        ] = None,
        pasta: Annotated[
            str,
            Field(description="Subpasta do workspace onde o comando começa (relativa). Vazio = /workspace."),
        ] = "",
    ) -> types.CallToolResult:
        if not comando.strip():
            raise ToolError("comando vazio")
        if "\0" in comando:
            raise ToolError("comando com caractere nulo")
        if len(comando.encode("utf-8")) > MAX_BYTES_COMANDO:
            raise ToolError(
                f"comando com mais de {_bytes(MAX_BYTES_COMANDO)} bytes: grave um script no "
                "workspace e execute o script"
            )
        avisos = []
        timeout = cfg.timeout_padrao if timeout_segundos is None else timeout_segundos
        if timeout < 1:
            raise ToolError("timeout_segundos precisa ser pelo menos 1")
        if timeout > cfg.timeout_maximo:
            avisos.append(f"timeout pedido ({timeout} s) maior que o máximo; usei {cfg.timeout_maximo} s")
            timeout = cfg.timeout_maximo
        try:
            subpasta = isolamento.validar_pasta(cfg, pasta)
        except ValueError as e:
            raise ToolError(f"pasta inválida: {e}") from None
        try:
            resultado = await executor.executar(comando, timeout, subpasta)
        except isolamento.Ocupado as e:
            raise ToolError(f"terminal ocupado: {e}") from None
        log.info(
            "%s → código %s em %.2f s%s",
            _resumo(comando, 120),
            resultado.codigo_saida,
            resultado.duracao_segundos,
            f" (interrompido: {resultado.interrupcao})" if resultado.interrupcao else "",
        )
        return types.CallToolResult(
            content=[types.TextContent(type="text", text=formatar(cfg, comando, resultado, avisos))],
            structured_content=estruturado(resultado),
        )

    servidor.add_tool(executar, name="executar", description=descricao(cfg))
    return servidor


def main() -> int:
    logging.basicConfig(level=logging.INFO, stream=sys.stderr, format="terminal: %(levelname)s %(message)s")
    try:
        cfg = config.carregar()
        amb = isolamento.preparar(cfg)
    except (config.ErroConfig, isolamento.Recusa) as e:
        print(f"terminal: NÃO vou subir: {e}", file=sys.stderr, flush=True)
        return 2
    for aviso in cfg.avisos:
        log.warning(aviso)
    isolamento.limpar_sobras(cfg)
    log.info(
        "caixa pronta (%s): workspace %s, rede %s, %d s padrão / %d s máximo, %d MiB e %d processos "
        "por comando, %d simultâneo(s)",
        amb.bwrap.versao,
        cfg.workspace,
        "ligada" if cfg.rede else "desligada",
        cfg.timeout_padrao,
        cfg.timeout_maximo,
        cfg.memoria_mb,
        cfg.max_processos,
        cfg.max_concorrentes,
    )
    criar_servidor(cfg, amb).run()  # transporte padrão: stdio
    return 0


if __name__ == "__main__":
    sys.exit(main())
