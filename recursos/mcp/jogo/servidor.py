"""Servidor MCP `jogo` do Abiyss: o Travessia, o jogo do benchmark.

O servidor é a única fonte da verdade do jogo. Ele só carrega fases prontas
do catálogo (nunca resolve nada) e nunca lê o gabarito: o limite de jogadas
de cada fase vem no arquivo da execução, escrito pelo harness
(`abiyss travessia`), cujo caminho chega em JOGO_EXECUCAO.

Sete ferramentas (o kernel expõe como `jogo__<nome>`). `regras`, `ver` e
`simular` só leem e podem rodar em paralelo, chamadas por vários
sub-agentes ao mesmo tempo; as outras passam por uma trava e rodam uma de
cada vez. Quem pode chamar cada uma é decidido pela configuração dos
sub-agentes no abiyss.toml da instância do benchmark, não pelo jogo.

O kernel sobe este arquivo como processo filho e conversa por stdio: NUNCA
use print() para stdout aqui; logs vão para o stderr.
"""

import asyncio
import logging
import os
import sys
from pathlib import Path
from typing import Annotated

import mcp.types as types
from mcp.server import MCPServer
from mcp.server.mcpserver.exceptions import ToolError
from pydantic import Field

import sessao

log = logging.getLogger("jogo")


def _texto(conteudo: str) -> types.CallToolResult:
    return types.CallToolResult(content=[types.TextContent(type="text", text=conteudo)])


def criar_servidor(s: sessao.Sessao) -> MCPServer:
    servidor = MCPServer("jogo")
    trava = asyncio.Lock()
    cfg = s.cfg

    def chamar(funcao, *args):
        try:
            return _texto(funcao(*args))
        except sessao.ErroJogo as e:
            raise ToolError(str(e)) from None

    async def regras() -> types.CallToolResult:
        return chamar(s.regras)

    async def ver(
        avatar: Annotated[
            str | None, Field(description="Opcional: V, R ou C para ver uma janela em volta desse avatar.")
        ] = None,
    ) -> types.CallToolResult:
        return chamar(s.ver, avatar)

    async def simular(
        plano: Annotated[
            list[str],
            Field(description=f'Lista de passos, ex.: ["R L", "R L", "V soltar", "V entregar R"] (até {cfg.max_passos_simular}).'),
        ],
    ) -> types.CallToolResult:
        return chamar(s.simular, plano)

    async def executar(
        plano: Annotated[
            list[str],
            Field(description=f'Lista de passos, ex.: ["R L", "R N"] (até {cfg.max_passos_executar}).'),
        ],
        versao: Annotated[
            int | None,
            Field(description="Opcional: a versão do estado em que o plano foi pensado; se mudou, nada é executado."),
        ] = None,
    ) -> types.CallToolResult:
        async with trava:
            return chamar(s.executar, plano, versao)

    async def reiniciar_fase() -> types.CallToolResult:
        async with trava:
            return chamar(s.reiniciar_fase)

    async def desistir_fase() -> types.CallToolResult:
        async with trava:
            return chamar(s.desistir_fase)

    async def proxima_fase() -> types.CallToolResult:
        async with trava:
            return chamar(s.proxima_fase)

    descricoes = {
        "regras": "As regras do Travessia (só a mecânica). Não conta como jogada.",
        "ver": "O mapa conhecido e a lista de objetos; com um avatar, uma janela em volta dele e o que ele carrega. "
        "Não conta como jogada.",
        "simular": "Aplica um plano numa CÓPIA do estado e mostra o resultado de cada passo e o estado final. Não conta "
        "como jogada e nunca revela células desconhecidas (para no aviso \"desconhecido\").",
        "executar": f"Executa um plano de até {cfg.max_passos_executar} passos e para no primeiro inválido. Cada passo "
        "válido é uma jogada. Com `versao`, recusa o plano se o estado mudou desde então.",
        "reiniciar_fase": "Volta a fase ao estado inicial (o mapa descoberto é mantido; as jogadas continuam somando).",
        "desistir_fase": "Desiste da fase atual (conta como falha).",
        "proxima_fase": "Carrega a fase seguinte, depois de concluir, falhar ou desistir da atual.",
    }
    for funcao in (regras, ver, simular, executar, reiniciar_fase, desistir_fase, proxima_fase):
        servidor.add_tool(funcao, name=funcao.__name__, description=descricoes[funcao.__name__])
    return servidor


def main() -> int:
    logging.basicConfig(level=logging.INFO, stream=sys.stderr, format="jogo: %(levelname)s %(message)s")
    caminho = os.environ.get("JOGO_EXECUCAO", "").strip()
    if not caminho:
        print(
            "jogo: NÃO vou subir: falta JOGO_EXECUCAO (o arquivo da execução, escrito por `abiyss travessia`)",
            file=sys.stderr,
            flush=True,
        )
        return 2
    try:
        cfg = sessao.carregar_execucao(Path(caminho))
        s = sessao.Sessao(cfg)
    except (sessao.ErroExecucao, OSError, ValueError) as e:
        print(f"jogo: NÃO vou subir: {e}", file=sys.stderr, flush=True)
        return 2
    log.info("pronto: execução %s, braço %s, modo %s, %d fase(s)", cfg.id, cfg.braco, cfg.modo, len(cfg.fases))
    criar_servidor(s).run()  # transporte padrão: stdio
    return 0


if __name__ == "__main__":
    sys.exit(main())
