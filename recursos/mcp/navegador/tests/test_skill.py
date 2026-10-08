"""A skill usar-o-navegador (skills/) cita as ferramentas deste servidor:
elas existem? O e6_skills do kernel só conhece as ferramentas nativas; os
nomes MCP da seção "## Ferramentas MCP" são conferidos aqui, por quem sabe
quais são (os do web_rapido, pelos testes do web_rapido)."""

import re
import tomllib

import pytest
from mcp import Client

import servidor
from conftest import PASTA_SERVIDOR, RAIZ_DO_REPOSITORIO

pytestmark = pytest.mark.anyio

SKILL = RAIZ_DO_REPOSITORIO / "skills" / "usar-o-navegador" / "SKILL.md"


def citadas() -> list[str]:
    if not SKILL.is_file():
        pytest.skip("a skill usar-o-navegador não existe")
    secao = SKILL.read_text().split("\n## Ferramentas MCP\n", 1)[1].split("\n## ", 1)[0]
    return re.findall(r"`([^`]+)`", secao)


async def test_skill_cita_so_ferramentas_que_existem(fazer_config):
    servidores = tomllib.loads((RAIZ_DO_REPOSITORIO / "abiyss.toml").read_text())["mcp"]["servidores"]
    nomes = {s["nome"] for s in servidores}
    meu = next(s["nome"] for s in servidores if s.get("diretorio") == f"recursos/mcp/{PASTA_SERVIDOR.name}")
    citados = [c for c in citadas() if "__" in c]
    for nome in citados:
        prefixo, _, _ = nome.partition("__")
        assert prefixo in nomes, f"{nome}: não é <servidor>__<ferramenta> de um servidor do abiyss.toml"
    async with Client(servidor.criar_servidor(fazer_config())[0]) as cliente:
        reais = {f"{meu}__{f.name}" for f in (await cliente.list_tools()).tools}
    deste = {n for n in citados if n.startswith(f"{meu}__")}
    assert deste == reais, f"faltam {sorted(reais - deste)}; não existem {sorted(deste - reais)}"
    assert {n.partition("__")[0] for n in citados} <= {meu, "web_rapido"}, "e nunca o terminal"


def test_skill_diz_que_o_padrao_e_so_leitura():
    texto = SKILL.read_text()
    assert "interagir = false" in texto and "só segue LINKS" in texto
