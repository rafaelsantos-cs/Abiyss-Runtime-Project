"""Servidor MCP de exemplo do Abiyss.

Ferramentas determinísticas e baratas: exatamente o tipo de coisa que
NÃO deve gastar uma chamada ao modelo (regra de orçamento do Abiyss).

O kernel sobe este arquivo como processo filho e conversa com ele por
stdio (stdin/stdout). Por isso, NUNCA use print() para stdout aqui:
mensagens de log vão para stderr.
"""

import sys
import unicodedata
from datetime import datetime, timezone

from mcp.server import MCPServer

mcp = MCPServer("exemplo")


@mcp.tool()
def contar_palavras(texto: str) -> dict:
    """Conta palavras, caracteres e linhas de um texto."""
    return {
        "palavras": len(texto.split()),
        "caracteres": len(texto),
        "linhas": texto.count("\n") + 1 if texto else 0,
    }


@mcp.tool()
def somar(numeros: list[float]) -> float:
    """Soma uma lista de números com precisão de ponto flutuante."""
    return float(sum(numeros))


@mcp.tool()
def remover_acentos(texto: str) -> str:
    """Remove acentos de um texto (ex.: "ação" -> "acao")."""
    decomposto = unicodedata.normalize("NFKD", texto)
    return "".join(c for c in decomposto if not unicodedata.combining(c))


@mcp.tool()
def agora_utc() -> str:
    """Data e hora atuais em UTC, no formato ISO 8601."""
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


if __name__ == "__main__":
    print("servidor MCP 'exemplo' iniciando (stdio)", file=sys.stderr)
    mcp.run()  # transporte padrão: stdio
