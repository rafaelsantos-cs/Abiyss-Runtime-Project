"""Configuração do navegador.

Como no web_rapido, duas fontes:

- o ``abiyss.toml`` do projeto (a mesma fonte do kernel): o modo
  (``[navegador] interagir``), as pastas de dados e do workspace (onde ficam
  as capturas de tela) e os limites que o kernel impõe a este servidor
  (``timeout_segundos`` do item, ``[mcp] max_memoria_mb`` e
  ``max_bytes_resultado``);
- variáveis ``NAVEGADOR_*`` (a tabela ``env`` do item no ``abiyss.toml``).

O servidor recusa subir se os prazos, a memória (Chromium + o próprio
servidor) ou o tamanho das respostas não couberem nos limites do kernel.
"""

from __future__ import annotations

import ipaddress
import os
import tomllib
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any

PADRAO_KERNEL_TIMEOUT = 60
PADRAO_KERNEL_MEMORIA_MB = 512
PADRAO_KERNEL_BYTES_RESULTADO = 1_000_000

# Entre o prazo de uma chamada e o timeout do kernel: fechar uma sessão
# travada (ou matar o Chromium) e montar a resposta.
FOLGA_PRAZO_SEGUNDOS = 10
# Entre o carregamento da página e o prazo da chamada: esperar o JavaScript
# assentar e tirar o instantâneo.
FOLGA_CARREGAMENTO_SEGUNDOS = 10
# Cabeçalho da resposta (URL, título, avisos), além do instantâneo.
FOLGA_RESULTADO_BYTES = 8192

PASTA_DO_SERVIDOR = Path(__file__).resolve().parent
NOME_PADRAO = "navegador"
SANDBOX = ("auto", "sim", "nao")


class ErroConfig(Exception):
    """Configuração inválida: o servidor não sobe."""


@dataclass(frozen=True)
class LimitesKernel:
    timeout_segundos: int
    max_memoria_mb: int
    max_bytes_resultado: int


@dataclass(frozen=True)
class Config:
    arquivo: Path
    kernel: LimitesKernel
    interagir: bool
    workspace: Path
    pasta_dados: Path
    prazo_segundos: int
    timeout_carregamento: int
    timeout_acao: int
    max_sessoes: int
    max_paginas: int
    sessao_ociosa_segundos: int
    max_texto_bytes: int
    memoria_chromium_mb: int
    reserva_servidor_mb: int
    chromium: Path | None
    sandbox: str
    max_capturas: int
    ao_vivo_porta: int
    excecoes_rede: frozenset[tuple[str, int]]
    avisos: tuple[str, ...] = ()

    @property
    def pasta_capturas(self) -> Path:
        return self.workspace / "navegador"

    @property
    def js_heap_mb(self) -> int:
        """Teto do heap do V8 por página: um script guloso estoura sozinho
        antes de levar a árvore do Chromium ao limite de memória."""
        return max(64, min(512, self.memoria_chromium_mb // 3))


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


def _resolver(raiz: Path, valor: Any) -> Path:
    caminho = Path(str(valor))
    return caminho if caminho.is_absolute() else raiz / caminho


def _item_do_servidor(dados: dict[str, Any], raiz: Path, env: Mapping[str, str]) -> tuple[dict[str, Any] | None, list[str]]:
    """O item deste servidor: pelo nome em ABIYSS_MCP_NOME, senão pelo
    `diretorio` (que aponta para esta pasta), senão pelo nome padrão."""
    servidores = [s for s in dados.get("mcp", {}).get("servidores", []) if isinstance(s, dict)]
    nome = env.get("ABIYSS_MCP_NOME", "").strip()
    if nome:
        for item in servidores:
            if item.get("nome") == nome:
                return item, []
        raise ErroConfig(f"ABIYSS_MCP_NOME = {nome!r}, mas não há servidor com esse nome no abiyss.toml")
    aqui = PASTA_DO_SERVIDOR.resolve()
    meus = [s for s in servidores if s.get("diretorio") and _resolver(raiz, s["diretorio"]).resolve() == aqui]
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


def _numero(env: Mapping[str, str], nome: str, padrao: int, minimo: int, maximo: int | None = None) -> int:
    bruto = env.get(nome, "").strip()
    if not bruto:
        return padrao
    try:
        valor = int(bruto)
    except ValueError:
        raise ErroConfig(f"{nome} = {bruto!r} não é um número inteiro") from None
    if valor < minimo:
        raise ErroConfig(f"{nome} = {valor}: o mínimo é {minimo}")
    if maximo is not None and valor > maximo:
        raise ErroConfig(f"{nome} = {valor}: o máximo é {maximo}")
    return valor


def _excecoes(bruto: str) -> frozenset[tuple[str, int]]:
    """NAVEGADOR_EXCECOES_REDE_LOCAL: "ip:porta, ip:porta" que o filtro de
    rede deixa passar mesmo sendo internos. Só para os testes (o site de
    mentira roda no 127.0.0.1): nada de nomes, nada de faixas."""
    excecoes = set()
    for parte in (p.strip() for p in bruto.split(",")):
        if not parte:
            continue
        ip, separador, porta = parte.rpartition(":")
        try:
            endereco = ipaddress.ip_address(ip.strip("[]"))
            numero = int(porta)
        except ValueError:
            raise ErroConfig(f"NAVEGADOR_EXCECOES_REDE_LOCAL: {parte!r} não é ip:porta") from None
        if not separador or not 0 < numero < 65536:
            raise ErroConfig(f"NAVEGADOR_EXCECOES_REDE_LOCAL: {parte!r} não é ip:porta")
        excecoes.add((str(endereco), numero))
    return frozenset(excecoes)


def _interagir(dados: dict[str, Any]) -> bool:
    tabela = dados.get("navegador", {})
    if not isinstance(tabela, dict):
        raise ErroConfig("[navegador] precisa ser uma tabela")
    valor = tabela.get("interagir", False)
    if not isinstance(valor, bool):
        raise ErroConfig(f"[navegador] interagir = {valor!r}: use true ou false")
    return valor


def carregar(env: Mapping[str, str] | None = None) -> Config:
    env = os.environ if env is None else env
    arquivo = achar_abiyss_toml(env)
    try:
        dados = tomllib.loads(arquivo.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise ErroConfig(f"não consegui ler {arquivo}: {e}") from None
    raiz = arquivo.parent.resolve()
    item, avisos = _item_do_servidor(dados, raiz, env)
    # A tabela `env` do item vale também quando o servidor roda à mão.
    tabela_env = (item or {}).get("env", {})
    if isinstance(tabela_env, dict):
        env = {**{k: str(v) for k, v in tabela_env.items()}, **env}
    mcp = dados.get("mcp", {})
    kernel = LimitesKernel(
        timeout_segundos=_inteiro_toml(item or {}, "timeout_segundos", PADRAO_KERNEL_TIMEOUT, "mcp.servidores"),
        max_memoria_mb=_inteiro_toml(mcp, "max_memoria_mb", PADRAO_KERNEL_MEMORIA_MB, "mcp"),
        max_bytes_resultado=_inteiro_toml(mcp, "max_bytes_resultado", PADRAO_KERNEL_BYTES_RESULTADO, "mcp"),
    )
    caminhos = dados.get("caminhos", {})
    chromium = env.get("NAVEGADOR_CHROMIUM", "").strip()
    if chromium and not Path(chromium).is_file():
        raise ErroConfig(f"NAVEGADOR_CHROMIUM = {chromium!r} não existe")
    sandbox = env.get("NAVEGADOR_SANDBOX", "").strip().lower() or "auto"
    if sandbox == "não":
        sandbox = "nao"
    if sandbox not in SANDBOX:
        raise ErroConfig(f"NAVEGADOR_SANDBOX = {sandbox!r}: use auto, sim ou nao")
    excecoes = _excecoes(env.get("NAVEGADOR_EXCECOES_REDE_LOCAL", ""))
    if excecoes:
        avisos.append(
            "NAVEGADOR_EXCECOES_REDE_LOCAL libera endereços internos ("
            + ", ".join(f"{ip}:{porta}" for ip, porta in sorted(excecoes))
            + "): só para testes"
        )
    config = Config(
        arquivo=arquivo,
        kernel=kernel,
        interagir=_interagir(dados),
        workspace=_resolver(raiz, caminhos.get("workspace", "workspace")),
        pasta_dados=_resolver(raiz, caminhos.get("dados", "data")) / "navegador",
        prazo_segundos=_numero(
            env, "NAVEGADOR_PRAZO_SEGUNDOS", max(15, kernel.timeout_segundos - FOLGA_PRAZO_SEGUNDOS), 15
        ),
        timeout_carregamento=_numero(env, "NAVEGADOR_TIMEOUT_CARREGAMENTO", 30, 5),
        timeout_acao=_numero(env, "NAVEGADOR_TIMEOUT_ACAO", 10, 1),
        max_sessoes=_numero(env, "NAVEGADOR_MAX_SESSOES", 2, 1, 16),
        max_paginas=_numero(env, "NAVEGADOR_MAX_PAGINAS", 3, 1, 20),
        sessao_ociosa_segundos=_numero(env, "NAVEGADOR_SESSAO_OCIOSA_SEGUNDOS", 600, 30),
        max_texto_bytes=_numero(env, "NAVEGADOR_MAX_TEXTO_BYTES", 40_000, 2000),
        memoria_chromium_mb=_numero(env, "NAVEGADOR_MEMORIA_MB", 768, 192),
        reserva_servidor_mb=_numero(env, "NAVEGADOR_RESERVA_SERVIDOR_MB", 256, 1),
        chromium=Path(chromium) if chromium else None,
        sandbox=sandbox,
        max_capturas=_numero(env, "NAVEGADOR_MAX_CAPTURAS", 50, 1, 10_000),
        ao_vivo_porta=_numero(env, "NAVEGADOR_AO_VIVO_PORTA", 0, 0, 65535),
        excecoes_rede=excecoes,
        avisos=tuple(avisos),
    )
    validar_limites(config)
    return config


def validar_limites(c: Config) -> None:
    k = c.kernel
    if c.prazo_segundos + FOLGA_PRAZO_SEGUNDOS > k.timeout_segundos:
        raise ErroConfig(
            f"NAVEGADOR_PRAZO_SEGUNDOS ({c.prazo_segundos} s) + folga de {FOLGA_PRAZO_SEGUNDOS} s passa do "
            f"timeout_segundos que o kernel dá a este servidor ({k.timeout_segundos} s): o kernel "
            "mataria o servidor no meio de uma chamada. Diminua NAVEGADOR_PRAZO_SEGUNDOS ou aumente "
            "timeout_segundos no item do navegador"
        )
    if c.timeout_carregamento + FOLGA_CARREGAMENTO_SEGUNDOS > c.prazo_segundos:
        raise ErroConfig(
            f"NAVEGADOR_TIMEOUT_CARREGAMENTO ({c.timeout_carregamento} s) + {FOLGA_CARREGAMENTO_SEGUNDOS} s "
            f"(esperar o JavaScript e ler a página) passa de NAVEGADOR_PRAZO_SEGUNDOS ({c.prazo_segundos} s)"
        )
    if c.timeout_acao > c.timeout_carregamento:
        raise ErroConfig(
            f"NAVEGADOR_TIMEOUT_ACAO ({c.timeout_acao} s) maior que NAVEGADOR_TIMEOUT_CARREGAMENTO "
            f"({c.timeout_carregamento} s)"
        )
    if c.max_texto_bytes + FOLGA_RESULTADO_BYTES > k.max_bytes_resultado:
        raise ErroConfig(
            f"NAVEGADOR_MAX_TEXTO_BYTES ({c.max_texto_bytes}) + {FOLGA_RESULTADO_BYTES} bytes de cabeçalho "
            f"passa de [mcp] max_bytes_resultado ({k.max_bytes_resultado}): o kernel cortaria a resposta"
        )
    memoria = c.memoria_chromium_mb + c.reserva_servidor_mb
    if memoria > k.max_memoria_mb:
        raise ErroConfig(
            f"Chromium ({c.memoria_chromium_mb} MiB, NAVEGADOR_MEMORIA_MB) + uv, Python e o driver do "
            f"Playwright ({c.reserva_servidor_mb} MiB, NAVEGADOR_RESERVA_SERVIDOR_MB) = {memoria} MiB passa de "
            f"[mcp] max_memoria_mb ({k.max_memoria_mb}): o kernel mataria o servidor inteiro antes do "
            "limite do Chromium agir. Aumente [mcp] max_memoria_mb (o navegador precisa de pelo menos "
            "1024) ou diminua NAVEGADOR_MEMORIA_MB"
        )
