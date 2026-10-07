"""Configuração do ambiente.

Como nos outros servidores: os limites do kernel (``timeout_segundos`` do
item e ``[mcp] max_bytes_resultado``) vêm do próprio ``abiyss.toml``; o
resto, de variáveis ``AMBIENTE_*`` (a tabela ``env`` do item).
"""

from __future__ import annotations

import os
import re
import shutil
import tomllib
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any

PADRAO_KERNEL_TIMEOUT = 60
PADRAO_KERNEL_BYTES_RESULTADO = 1_000_000
FOLGA_TIMEOUT_SEGUNDOS = 5
FOLGA_RESULTADO_BYTES = 8192
MAX_LINHAS_DIARIO = 500

PASTA_DO_SERVIDOR = Path(__file__).resolve().parent
NOME_PADRAO = "ambiente"
# Nome de unidade do systemd: letras, números e : _ . @ - (sem espaço, sem barra).
NOME_DE_UNIDADE = re.compile(r"^[A-Za-z0-9:_.@-]+$")
TIPOS_DE_UNIDADE = (".service", ".socket", ".timer", ".target", ".mount", ".path")


class ErroConfig(Exception):
    """Configuração inválida: o servidor não sobe."""


@dataclass(frozen=True)
class Config:
    arquivo: Path
    raiz: Path
    timeout_kernel: int
    max_bytes_resultado: int
    servicos: tuple[str, ...]
    max_linhas_diario: int
    timeout_comando: int
    max_bytes: int
    systemctl: str | None
    journalctl: str | None
    avisos: tuple[str, ...] = ()


def achar_abiyss_toml(env: Mapping[str, str]) -> Path:
    explicito = env.get("ABIYSS_CONFIG", "").strip()
    if explicito:
        caminho = Path(explicito)
        if not caminho.is_file():
            raise ErroConfig(f"ABIYSS_CONFIG aponta para {caminho}, que não existe")
        return caminho.resolve()
    for pasta in (PASTA_DO_SERVIDOR, *PASTA_DO_SERVIDOR.parents):
        if (pasta / "abiyss.toml").is_file():
            return pasta / "abiyss.toml"
    raise ErroConfig(f"não achei o abiyss.toml subindo a partir de {PASTA_DO_SERVIDOR}; defina ABIYSS_CONFIG")


def _item_do_servidor(dados: dict[str, Any], raiz: Path, env: Mapping[str, str]) -> tuple[dict[str, Any] | None, list[str]]:
    servidores = [s for s in dados.get("mcp", {}).get("servidores", []) if isinstance(s, dict)]
    nome = env.get("ABIYSS_MCP_NOME", "").strip()
    if nome:
        for item in servidores:
            if item.get("nome") == nome:
                return item, []
        raise ErroConfig(f"ABIYSS_MCP_NOME = {nome!r}, mas não há servidor com esse nome no abiyss.toml")
    aqui = PASTA_DO_SERVIDOR.resolve()
    meus = []
    for item in servidores:
        diretorio = item.get("diretorio")
        if diretorio:
            caminho = Path(str(diretorio))
            if (caminho if caminho.is_absolute() else raiz / caminho).resolve() == aqui:
                meus.append(item)
    if len(meus) > 1:
        raise ErroConfig("mais de um item de [[mcp.servidores]] aponta para esta pasta; defina ABIYSS_MCP_NOME")
    if meus:
        return meus[0], []
    for item in servidores:
        if item.get("nome") == NOME_PADRAO:
            return item, []
    return None, [f"este servidor não está em [[mcp.servidores]]; usando o timeout padrão do kernel ({PADRAO_KERNEL_TIMEOUT} s)"]


def _inteiro_toml(tabela: dict[str, Any], chave: str, padrao: int, onde: str) -> int:
    valor = tabela.get(chave, padrao)
    if isinstance(valor, bool) or not isinstance(valor, int) or valor <= 0:
        raise ErroConfig(f"{onde}.{chave} = {valor!r} não é um inteiro positivo")
    return valor


def _inteiro(env: Mapping[str, str], nome: str, padrao: int, minimo: int = 1, maximo: int | None = None) -> int:
    bruto = env.get(nome, "").strip()
    if not bruto:
        return padrao
    try:
        valor = int(bruto)
    except ValueError:
        raise ErroConfig(f"{nome} = {bruto!r} não é um número inteiro") from None
    if valor < minimo or (maximo is not None and valor > maximo):
        raise ErroConfig(f"{nome} = {valor}: use de {minimo} a {maximo if maximo is not None else '…'}")
    return valor


def unidades(texto: str) -> tuple[str, ...]:
    """A lista de serviços permitidos: separada por vírgula ou espaço; sem
    sufixo, vira `.service`."""
    lista = []
    for nome in re.split(r"[,\s]+", texto.strip()):
        if not nome:
            continue
        if not NOME_DE_UNIDADE.match(nome) or nome.startswith("-"):
            raise ErroConfig(f"AMBIENTE_SERVICOS: {nome!r} não é um nome de unidade do systemd")
        if not nome.endswith(TIPOS_DE_UNIDADE):
            nome += ".service"
        if nome not in lista:
            lista.append(nome)
    return tuple(lista)


def carregar(env: Mapping[str, str] | None = None) -> Config:
    env = os.environ if env is None else env
    arquivo = achar_abiyss_toml(env)
    try:
        dados = tomllib.loads(arquivo.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise ErroConfig(f"não consegui ler {arquivo}: {e}") from None
    raiz = arquivo.parent.resolve()
    item, avisos = _item_do_servidor(dados, raiz, env)
    tabela_env = (item or {}).get("env", {})
    if isinstance(tabela_env, dict):
        env = {**{k: str(v) for k, v in tabela_env.items()}, **env}
    timeout_kernel = _inteiro_toml(item or {}, "timeout_segundos", PADRAO_KERNEL_TIMEOUT, "mcp.servidores")
    max_bytes_resultado = _inteiro_toml(
        dados.get("mcp", {}), "max_bytes_resultado", PADRAO_KERNEL_BYTES_RESULTADO, "mcp"
    )
    caminho_systemctl = env.get("AMBIENTE_SYSTEMCTL", "").strip() or shutil.which("systemctl")
    caminho_journalctl = env.get("AMBIENTE_JOURNALCTL", "").strip() or shutil.which("journalctl")
    config = Config(
        arquivo=arquivo,
        raiz=raiz,
        timeout_kernel=timeout_kernel,
        max_bytes_resultado=max_bytes_resultado,
        servicos=unidades(env.get("AMBIENTE_SERVICOS", "abiyss.service")),
        max_linhas_diario=_inteiro(env, "AMBIENTE_MAX_LINHAS_DIARIO", 100, 1, MAX_LINHAS_DIARIO),
        timeout_comando=_inteiro(env, "AMBIENTE_TIMEOUT_COMANDO", 10),
        max_bytes=_inteiro(env, "AMBIENTE_MAX_BYTES", 30_000, 2000),
        systemctl=caminho_systemctl or None,
        journalctl=caminho_journalctl or None,
        avisos=tuple(avisos),
    )
    if not config.servicos:
        raise ErroConfig("AMBIENTE_SERVICOS está vazio: diga quais serviços o Abiyss pode ver")
    if config.timeout_comando + FOLGA_TIMEOUT_SEGUNDOS > timeout_kernel:
        raise ErroConfig(
            f"AMBIENTE_TIMEOUT_COMANDO ({config.timeout_comando} s) + folga de {FOLGA_TIMEOUT_SEGUNDOS} s "
            f"passa do timeout_segundos que o kernel dá a este servidor ({timeout_kernel} s)"
        )
    if config.max_bytes + FOLGA_RESULTADO_BYTES > max_bytes_resultado:
        raise ErroConfig(
            f"AMBIENTE_MAX_BYTES ({config.max_bytes}) + {FOLGA_RESULTADO_BYTES} bytes de cabeçalho passa "
            f"de [mcp] max_bytes_resultado ({max_bytes_resultado}): o kernel cortaria a resposta"
        )
    return config
