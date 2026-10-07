"""Configuração do web_rapido.

Como no terminal, duas fontes:

- o ``abiyss.toml`` do projeto (a mesma fonte do kernel): a pasta de dados
  (onde fica o cache) e os limites que o kernel impõe a este servidor
  (``timeout_segundos`` do item, ``[mcp] max_memoria_mb`` e
  ``max_bytes_resultado``);
- variáveis ``WEB_*`` (a tabela ``env`` do item no ``abiyss.toml``).

O servidor recusa subir se os prazos ou o tamanho das respostas não
couberem nos limites do kernel.
"""

from __future__ import annotations

import os
import tomllib
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

PADRAO_KERNEL_TIMEOUT = 60
PADRAO_KERNEL_MEMORIA_MB = 512
PADRAO_KERNEL_BYTES_RESULTADO = 1_000_000

# Entre o prazo de uma chamada e o timeout do kernel: montar a resposta,
# gravar no cache, uma extração que ainda estava terminando.
FOLGA_PRAZO_SEGUNDOS = 8
# Cabeçalho da resposta e avisos, além do texto.
FOLGA_RESULTADO_BYTES = 8192
# Memória que o lxml/trafilatura usa por MiB de HTML (medido: ~34×).
MEMORIA_POR_MIB_DE_HTML = 40

PASTA_DO_SERVIDOR = Path(__file__).resolve().parent
NOME_PADRAO = "web_rapido"
MIB = 1024 * 1024


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
    searxng_url: str
    prazo_segundos: int
    timeout_requisicao: int
    cache_arquivo: Path
    cache_max_mb: int
    ttl_paginas_horas: float
    ttl_buscas_horas: float
    max_pagina_mb: int
    max_texto_bytes: int
    intervalo_dominio: float
    max_concorrentes: int
    reserva_servidor_mb: int
    permitir_rede_local: bool
    agente: str
    avisos: tuple[str, ...] = ()

    @property
    def produto(self) -> str:
        """O "nome de robô" para o robots.txt (ex.: AbiyssBot)."""
        return self.agente.split("/")[0].split()[0]


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


def _numero(env: Mapping[str, str], nome: str, padrao: float, minimo: float, inteiro: bool = True):
    bruto = env.get(nome, "").strip()
    if not bruto:
        return padrao
    try:
        valor = int(bruto) if inteiro else float(bruto.replace(",", "."))
    except ValueError:
        raise ErroConfig(f"{nome} = {bruto!r} não é um número{' inteiro' if inteiro else ''}") from None
    if valor < minimo:
        raise ErroConfig(f"{nome} = {valor}: o mínimo é {minimo}")
    return valor


def _booleano(env: Mapping[str, str], nome: str, padrao: bool) -> bool:
    bruto = env.get(nome, "").strip().lower()
    if not bruto:
        return padrao
    if bruto in ("1", "sim", "s", "true", "yes", "on"):
        return True
    if bruto in ("0", "nao", "não", "n", "false", "no", "off"):
        return False
    raise ErroConfig(f"{nome} = {bruto!r}: use sim ou nao")


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

    searxng = env.get("WEB_SEARXNG_URL", "").strip() or "http://127.0.0.1:8888"
    partes = urlsplit(searxng)
    if partes.scheme not in ("http", "https") or not partes.hostname:
        raise ErroConfig(f"WEB_SEARXNG_URL = {searxng!r}: use http(s)://host:porta")
    dados_dir = _resolver(raiz, dados.get("caminhos", {}).get("dados", "data"))
    cache = env.get("WEB_CACHE_ARQUIVO", "").strip()
    config = Config(
        arquivo=arquivo,
        kernel=kernel,
        searxng_url=searxng.rstrip("/"),
        prazo_segundos=_numero(env, "WEB_PRAZO_SEGUNDOS", max(5, kernel.timeout_segundos - FOLGA_PRAZO_SEGUNDOS), 5),
        timeout_requisicao=_numero(env, "WEB_TIMEOUT_REQUISICAO", 15, 1),
        cache_arquivo=_resolver(raiz, cache) if cache else dados_dir / "web_rapido" / "cache.sqlite3",
        cache_max_mb=_numero(env, "WEB_CACHE_MAX_MB", 200, 1),
        ttl_paginas_horas=_numero(env, "WEB_CACHE_TTL_PAGINAS_HORAS", 24.0, 0, inteiro=False),
        ttl_buscas_horas=_numero(env, "WEB_CACHE_TTL_BUSCAS_HORAS", 6.0, 0, inteiro=False),
        max_pagina_mb=_numero(env, "WEB_MAX_PAGINA_MB", 2, 1),
        max_texto_bytes=_numero(env, "WEB_MAX_TEXTO_BYTES", 40_000, 1000),
        intervalo_dominio=_numero(env, "WEB_INTERVALO_DOMINIO_SEGUNDOS", 2.0, 0, inteiro=False),
        max_concorrentes=_numero(env, "WEB_MAX_CONCORRENTES", 2, 1),
        reserva_servidor_mb=_numero(env, "WEB_RESERVA_SERVIDOR_MB", 160, 1),
        permitir_rede_local=_booleano(env, "WEB_PERMITIR_REDE_LOCAL", False),
        agente=env.get("WEB_AGENTE", "").strip() or "AbiyssBot/0.1 (agente pessoal; respeita robots.txt)",
        avisos=tuple(avisos),
    )
    validar_limites(config)
    return config


def validar_limites(c: Config) -> None:
    k = c.kernel
    if c.prazo_segundos + FOLGA_PRAZO_SEGUNDOS > k.timeout_segundos:
        raise ErroConfig(
            f"WEB_PRAZO_SEGUNDOS ({c.prazo_segundos} s) + folga de {FOLGA_PRAZO_SEGUNDOS} s passa do "
            f"timeout_segundos que o kernel dá a este servidor ({k.timeout_segundos} s): o kernel "
            "mataria o servidor no meio de uma leitura. Diminua WEB_PRAZO_SEGUNDOS ou aumente "
            "timeout_segundos no item do web_rapido"
        )
    if c.timeout_requisicao > c.prazo_segundos:
        raise ErroConfig(
            f"WEB_TIMEOUT_REQUISICAO ({c.timeout_requisicao} s) maior que WEB_PRAZO_SEGUNDOS ({c.prazo_segundos} s)"
        )
    if c.max_texto_bytes + FOLGA_RESULTADO_BYTES > k.max_bytes_resultado:
        raise ErroConfig(
            f"WEB_MAX_TEXTO_BYTES ({c.max_texto_bytes}) + {FOLGA_RESULTADO_BYTES} bytes de cabeçalho passa "
            f"de [mcp] max_bytes_resultado ({k.max_bytes_resultado}): o kernel cortaria a resposta"
        )
    memoria = c.max_concorrentes * c.max_pagina_mb * MEMORIA_POR_MIB_DE_HTML + c.reserva_servidor_mb
    if memoria > k.max_memoria_mb:
        raise ErroConfig(
            f"{c.max_concorrentes} leitura(s) × {c.max_pagina_mb} MiB de HTML × {MEMORIA_POR_MIB_DE_HTML} "
            f"(memória da extração) + {c.reserva_servidor_mb} MiB do servidor = {memoria} MiB passa de "
            f"[mcp] max_memoria_mb ({k.max_memoria_mb}): o kernel mataria o servidor. Diminua "
            "WEB_MAX_CONCORRENTES ou WEB_MAX_PAGINA_MB"
        )
