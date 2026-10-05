"""Servidor MCP usado SÓ nos testes: devolve variáveis de ambiente.

Serve para provar que o kernel entrega um ambiente limpo aos servidores
MCP (sem as chaves do NIM nem outras variáveis do processo pai).
"""

import os

from mcp.server import MCPServer

mcp = MCPServer("eco_env")


@mcp.tool()
def ler_env(nome: str) -> str:
    """Valor da variável de ambiente `nome` ("<ausente>" se não existir)."""
    return os.environ.get(nome, "<ausente>")


if __name__ == "__main__":
    mcp.run()
