"""As skills usar-as-maos e usar-o-navegador (skills/) citam as ferramentas
deste servidor: elas existem? O e6_skills do kernel só conhece as
ferramentas nativas; os nomes MCP da seção "## Ferramentas MCP" são
conferidos aqui, por quem sabe quais são."""

import re
import tomllib

import pytest
from mcp import Client

import servidor
from conftest import PASTA_SERVIDOR


pytestmark = pytest.mark.anyio

RAIZ = PASTA_SERVIDOR.parent.parent.parent
SKILLS = ("usar-as-maos", "usar-o-navegador")


def citadas(skill: str) -> list[str]:
    arquivo = RAIZ / "skills" / skill / "SKILL.md"
    if not arquivo.is_file():
        pytest.skip(f"a skill {skill} não existe")
    secao = arquivo.read_text().split("\n## Ferramentas MCP\n", 1)[1].split("\n## ", 1)[0]
    return re.findall(r"`([^`]+)`", secao)


@pytest.mark.parametrize("skill", SKILLS)
async def test_skill_cita_so_ferramentas_que_existem(fazer_config, skill):
    servidores = tomllib.loads((RAIZ / "abiyss.toml").read_text())["mcp"]["servidores"]
    nomes = {s["nome"] for s in servidores}
    meu = next(s["nome"] for s in servidores if s.get("diretorio") == f"recursos/mcp/{PASTA_SERVIDOR.name}")
    citados = citadas(skill)
    for nome in citados:
        prefixo, separador, _ = nome.partition("__")
        assert separador and prefixo in nomes, f"{nome}: não é <servidor>__<ferramenta> de um servidor do abiyss.toml"
    async with Client(servidor.criar_servidor(fazer_config())[0]) as cliente:
        reais = {f"{meu}__{f.name}" for f in (await cliente.list_tools()).tools}
    deste = {n for n in citados if n.startswith(f"{meu}__")}
    assert deste, f"a skill não cita nenhuma ferramenta de {meu}"
    assert deste <= reais, f"a skill cita ferramentas que {meu} não tem: {sorted(deste - reais)}"
