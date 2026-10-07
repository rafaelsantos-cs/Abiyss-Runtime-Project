"""Configuração do servidor terminal.

Duas fontes, cada uma com o seu papel:

- o ``abiyss.toml`` do projeto, a MESMA fonte do kernel: onde fica o
  workspace, quais áreas são protegidas e os limites que o kernel impõe a
  este servidor (``[mcp] max_memoria_mb`` e ``max_bytes_resultado``, e o
  ``timeout_segundos`` do próprio item em ``[[mcp.servidores]]``);
- variáveis ``TERMINAL_*`` (a tabela ``env`` do item no ``abiyss.toml``):
  os limites de cada comando.

O servidor recusa subir se os limites dele não couberem nos do kernel: um
comando mais longo que o timeout do kernel faria o kernel matar o servidor
inteiro (com os outros comandos dentro), e o mesmo vale para a memória.
"""

from __future__ import annotations

import os
import tempfile
import tomllib
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any

# Padrões do kernel (kernel/src/mcp.rs), usados quando o abiyss.toml não diz.
PADRAO_KERNEL_TIMEOUT = 60
PADRAO_KERNEL_MEMORIA_MB = 512
PADRAO_KERNEL_BYTES_RESULTADO = 1_000_000

# Folga entre o timeout máximo de um comando e o do kernel: subir a caixa,
# matar, esvaziar os pipes e montar a resposta.
FOLGA_TIMEOUT_SEGUNDOS = 5
# Folga para o cabeçalho e os avisos da resposta (além de stdout e stderr).
FOLGA_RESULTADO_BYTES = 8192
# Abaixo disso nem o bash sobe com o limite de espaço de endereços.
MINIMO_MEMORIA_MB = 32
# Processos da própria caixa (o bwrap e o init dela) entram na conta.
MINIMO_PROCESSOS = 4

PASTA_DO_SERVIDOR = Path(__file__).resolve().parent
NOME_PADRAO = "terminal"
MIB = 1024 * 1024


class ErroConfig(Exception):
    """Configuração inválida: o servidor não sobe."""


@dataclass(frozen=True)
class LimitesKernel:
    """O que o kernel impõe a este servidor (lido do abiyss.toml)."""

    # Tempo máximo de UMA chamada de ferramenta (item em [[mcp.servidores]]).
    timeout_segundos: int
    # Memória da árvore inteira do servidor: uv + Python + comandos.
    max_memoria_mb: int
    # O kernel corta o texto do resultado acima disto.
    max_bytes_resultado: int


@dataclass(frozen=True)
class Config:
    arquivo: Path
    raiz: Path
    workspace: Path
    protegidos: tuple[Path, ...]
    kernel: LimitesKernel
    timeout_padrao: int
    timeout_maximo: int
    max_saida_bytes: int
    max_concorrentes: int
    memoria_mb: int
    max_processos: int
    max_arquivo_mb: int
    tmp_mb: int
    rede: bool
    reserva_servidor_mb: int
    pasta_temporaria: Path
    bwrap: str | None
    avisos: tuple[str, ...] = ()


def canonico(caminho: Path) -> Path:
    """Caminho absoluto com links resolvidos no trecho que existe (como o
    `canonico_aproximado` do kernel): serve também para áreas que ainda não
    existem no disco (o cofre, por exemplo)."""
    return Path(os.path.realpath(caminho))


def dentro_de(caminho: Path, pasta: Path) -> bool:
    """`caminho` é `pasta` ou fica dentro dela (os dois já canônicos)?"""
    return caminho == pasta or pasta in caminho.parents


def conflito_de_montagem(
    origem: Path, protegidos: tuple[Path, ...], raiz: Path, gravavel: bool
) -> str | None:
    """Explica por que `origem` não pode entrar na caixa (ou None).

    Nenhuma montagem pode conter uma área protegida nem a raiz do projeto.
    Uma montagem gravável também não pode ficar DENTRO de uma área
    protegida (ex.: uma pasta qualquer dentro de data/).
    """
    origem = canonico(origem)
    for area in (*protegidos, raiz):
        if dentro_de(area, origem):
            return f"{origem} contém {area}"
    if gravavel:
        for area in protegidos:
            if dentro_de(origem, area):
                return f"{origem} fica dentro da área protegida {area}"
    return None


def achar_abiyss_toml(env: Mapping[str, str]) -> Path:
    """O abiyss.toml do projeto: ABIYSS_CONFIG ou o primeiro subindo a partir
    da pasta deste servidor (recursos/mcp/terminal → raiz do projeto)."""
    explicito = env.get("ABIYSS_CONFIG", "").strip()
    if explicito:
        caminho = Path(explicito)
        if not caminho.is_file():
            raise ErroConfig(f"ABIYSS_CONFIG aponta para {caminho}, que não existe")
        return caminho.resolve()
    for pasta in (PASTA_DO_SERVIDOR, *PASTA_DO_SERVIDOR.parents):
        candidato = pasta / "abiyss.toml"
        if candidato.is_file():
            return candidato
    raise ErroConfig(
        f"não achei o abiyss.toml subindo a partir de {PASTA_DO_SERVIDOR}; "
        "defina ABIYSS_CONFIG na tabela env do servidor"
    )


def _resolver(raiz: Path, valor: Any) -> Path:
    caminho = Path(str(valor))
    return caminho if caminho.is_absolute() else raiz / caminho


def _item_do_servidor(
    dados: dict[str, Any], raiz: Path, env: Mapping[str, str]
) -> tuple[dict[str, Any] | None, list[str]]:
    """O item deste servidor em [[mcp.servidores]]: pelo nome em
    ABIYSS_MCP_NOME, senão pelo `diretorio` (que aponta para esta pasta)."""
    servidores = [s for s in dados.get("mcp", {}).get("servidores", []) if isinstance(s, dict)]
    nome = env.get("ABIYSS_MCP_NOME", "").strip()
    if nome:
        for item in servidores:
            if item.get("nome") == nome:
                return item, []
        raise ErroConfig(f"ABIYSS_MCP_NOME = {nome!r}, mas não há servidor com esse nome no abiyss.toml")
    aqui = canonico(PASTA_DO_SERVIDOR)
    meus = [
        item
        for item in servidores
        if item.get("diretorio") and canonico(_resolver(raiz, item["diretorio"])) == aqui
    ]
    if len(meus) > 1:
        raise ErroConfig(
            "mais de um item de [[mcp.servidores]] aponta para esta pasta; "
            "defina ABIYSS_MCP_NOME na tabela env de cada um"
        )
    if meus:
        return meus[0], []
    for item in servidores:
        if item.get("nome") == NOME_PADRAO:
            return item, []
    return None, [
        "este servidor não está em [[mcp.servidores]]; usando o timeout padrão "
        f"do kernel ({PADRAO_KERNEL_TIMEOUT} s)"
    ]


def _inteiro_toml(tabela: dict[str, Any], chave: str, padrao: int, onde: str) -> int:
    valor = tabela.get(chave, padrao)
    if isinstance(valor, bool) or not isinstance(valor, int) or valor <= 0:
        raise ErroConfig(f"{onde}.{chave} = {valor!r} não é um inteiro positivo")
    return valor


def _inteiro(env: Mapping[str, str], nome: str, padrao: int, minimo: int = 1) -> int:
    bruto = env.get(nome, "").strip()
    if not bruto:
        return padrao
    try:
        valor = int(bruto)
    except ValueError:
        raise ErroConfig(f"{nome} = {bruto!r} não é um número inteiro") from None
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


def areas_protegidas(dados: dict[str, Any], raiz: Path, arquivo: Path) -> tuple[Path, ...]:
    """As mesmas áreas de `Config::areas_protegidas` do kernel, mais o próprio
    abiyss.toml lido (se não for o da raiz)."""
    caminhos = dados.get("caminhos", {})
    memoria = dados.get("memoria", {})
    skills = dados.get("skills", {})
    identidade = _resolver(raiz, caminhos.get("identidade", "identity/nucleo.md"))
    areas = [
        raiz / "kernel",
        raiz / "recursos",
        raiz / ".git",
        raiz / ".env",
        raiz / "abiyss.toml",
        raiz / "Cargo.toml",
        arquivo,
        _resolver(raiz, caminhos.get("dados", "data")),
        identidade,
        _resolver(raiz, memoria.get("cofre", "cofre")),
        _resolver(raiz, memoria.get("central", "identity/memoria-central.md")),
    ]
    raizes = skills.get("raizes") or [{"caminho": caminhos.get("skills", "skills")}]
    areas += [_resolver(raiz, r["caminho"]) for r in raizes if isinstance(r, dict) and r.get("caminho")]
    if identidade.parent != raiz:
        areas.append(identidade.parent)
    unicas: list[Path] = []
    for area in areas:
        area = canonico(area)
        if area not in unicas:
            unicas.append(area)
    return tuple(unicas)


def carregar(env: Mapping[str, str] | None = None) -> Config:
    """Lê e valida tudo. Qualquer problema vira `ErroConfig` (o servidor não sobe)."""
    env = os.environ if env is None else env
    arquivo = achar_abiyss_toml(env)
    try:
        dados = tomllib.loads(arquivo.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise ErroConfig(f"não consegui ler {arquivo}: {e}") from None
    raiz = canonico(arquivo.parent)

    item, avisos = _item_do_servidor(dados, raiz, env)
    # O kernel passa a tabela `env` do item como variáveis de ambiente; quem
    # roda o servidor à mão (para depurar) recebe os mesmos valores daqui.
    tabela_env = (item or {}).get("env", {})
    if isinstance(tabela_env, dict):
        env = {**{k: str(v) for k, v in tabela_env.items()}, **env}
    mcp = dados.get("mcp", {})
    kernel = LimitesKernel(
        timeout_segundos=_inteiro_toml(item or {}, "timeout_segundos", PADRAO_KERNEL_TIMEOUT, "mcp.servidores"),
        max_memoria_mb=_inteiro_toml(mcp, "max_memoria_mb", PADRAO_KERNEL_MEMORIA_MB, "mcp"),
        max_bytes_resultado=_inteiro_toml(mcp, "max_bytes_resultado", PADRAO_KERNEL_BYTES_RESULTADO, "mcp"),
    )

    protegidos = areas_protegidas(dados, raiz, canonico(arquivo))
    workspace = _resolver(raiz, dados.get("caminhos", {}).get("workspace", "workspace"))
    try:
        # O kernel também cria o workspace ao subir; quem chegar primeiro cria.
        workspace.mkdir(parents=True, exist_ok=True)
    except OSError as e:
        raise ErroConfig(f"não consegui criar o workspace {workspace}: {e}") from None
    workspace = canonico(workspace)
    problema = conflito_de_montagem(workspace, protegidos, raiz, gravavel=True)
    if problema:
        raise ErroConfig(f"workspace inválido para a caixa: {problema}")

    pasta_temporaria = Path(env.get("TERMINAL_PASTA_TEMPORARIA", "").strip() or tempfile.gettempdir())
    if not pasta_temporaria.is_dir():
        raise ErroConfig(f"TERMINAL_PASTA_TEMPORARIA: {pasta_temporaria} não é uma pasta")
    pasta_temporaria = canonico(pasta_temporaria)
    problema = conflito_de_montagem(pasta_temporaria, protegidos, raiz, gravavel=True)
    if problema:
        raise ErroConfig(f"pasta temporária inválida para a caixa: {problema}")

    timeout_maximo_padrao = max(1, kernel.timeout_segundos - FOLGA_TIMEOUT_SEGUNDOS)
    timeout_maximo = _inteiro(env, "TERMINAL_TIMEOUT_MAXIMO", timeout_maximo_padrao)
    config = Config(
        arquivo=arquivo,
        raiz=raiz,
        workspace=workspace,
        protegidos=protegidos,
        kernel=kernel,
        timeout_padrao=_inteiro(env, "TERMINAL_TIMEOUT_PADRAO", min(30, timeout_maximo)),
        timeout_maximo=timeout_maximo,
        max_saida_bytes=_inteiro(env, "TERMINAL_MAX_SAIDA_BYTES", 32_000, minimo=1024),
        max_concorrentes=_inteiro(env, "TERMINAL_MAX_CONCORRENTES", 2),
        memoria_mb=_inteiro(env, "TERMINAL_MEMORIA_MB", 160, minimo=MINIMO_MEMORIA_MB),
        max_processos=_inteiro(env, "TERMINAL_MAX_PROCESSOS", 64, minimo=MINIMO_PROCESSOS),
        max_arquivo_mb=_inteiro(env, "TERMINAL_MAX_ARQUIVO_MB", 1024),
        tmp_mb=_inteiro(env, "TERMINAL_TMP_MB", 64),
        rede=_booleano(env, "TERMINAL_REDE", False),
        reserva_servidor_mb=_inteiro(env, "TERMINAL_RESERVA_SERVIDOR_MB", 128),
        pasta_temporaria=pasta_temporaria,
        bwrap=env.get("TERMINAL_BWRAP", "").strip() or None,
        avisos=tuple(avisos),
    )
    validar_limites(config)
    return config


def validar_limites(c: Config) -> None:
    """Os limites do servidor cabem nos do kernel?"""
    k = c.kernel
    if c.timeout_padrao > c.timeout_maximo:
        raise ErroConfig(
            f"TERMINAL_TIMEOUT_PADRAO ({c.timeout_padrao} s) maior que "
            f"TERMINAL_TIMEOUT_MAXIMO ({c.timeout_maximo} s)"
        )
    if c.timeout_maximo + FOLGA_TIMEOUT_SEGUNDOS > k.timeout_segundos:
        raise ErroConfig(
            f"TERMINAL_TIMEOUT_MAXIMO ({c.timeout_maximo} s) + folga de {FOLGA_TIMEOUT_SEGUNDOS} s "
            f"passa do timeout_segundos que o kernel dá a este servidor ({k.timeout_segundos} s): "
            "o kernel mataria o servidor no meio do comando. Diminua TERMINAL_TIMEOUT_MAXIMO "
            "ou aumente timeout_segundos no item do terminal em [[mcp.servidores]]"
        )
    total = c.max_concorrentes * c.memoria_mb + c.reserva_servidor_mb
    if total > k.max_memoria_mb:
        raise ErroConfig(
            f"{c.max_concorrentes} comando(s) × {c.memoria_mb} MiB + reserva do servidor "
            f"{c.reserva_servidor_mb} MiB = {total} MiB passa de [mcp] max_memoria_mb "
            f"({k.max_memoria_mb} MiB): o kernel mataria o servidor. Diminua "
            "TERMINAL_MAX_CONCORRENTES ou TERMINAL_MEMORIA_MB"
        )
    resposta = 2 * c.max_saida_bytes + FOLGA_RESULTADO_BYTES
    if resposta > k.max_bytes_resultado:
        raise ErroConfig(
            f"2 × TERMINAL_MAX_SAIDA_BYTES ({c.max_saida_bytes}) + {FOLGA_RESULTADO_BYTES} bytes de "
            f"cabeçalho passa de [mcp] max_bytes_resultado ({k.max_bytes_resultado}): o kernel "
            "cortaria a resposta no meio. Diminua TERMINAL_MAX_SAIDA_BYTES"
        )
