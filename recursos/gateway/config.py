"""Configuração do adaptador.

- O caminho do socket vem do próprio ``abiyss.toml`` (``[gateway] socket``,
  relativo à pasta dele): o kernel e o adaptador nunca discordam.
- Quem é o dono e qual canal ouvir, o KERNEL diz no ``ola`` (não fica
  duplicado aqui).
- O token do bot vem SÓ da variável de ambiente ``ABIYSS_DISCORD_TOKEN``
  (o systemd lê do ``.env`` com ``EnvironmentFile=``; rodando à mão, o
  ``.env`` ao lado do ``abiyss.toml`` também é lido, sem sobrescrever o
  ambiente). Nunca vai para log nem para o repositório.
"""

from __future__ import annotations

import tomllib
from collections.abc import Mapping
from dataclasses import dataclass, field
from pathlib import Path

PASTA_DO_ADAPTADOR = Path(__file__).resolve().parent
VARIAVEL_TOKEN = "ABIYSS_DISCORD_TOKEN"
SOCKET_PADRAO = "data/gateway/abiyss.sock"
TOKEN_DE_EXEMPLO = "COLOQUE-AQUI"


class ErroConfig(Exception):
    """Configuração inválida: o adaptador não sobe."""


@dataclass(frozen=True)
class Config:
    raiz: Path
    socket: Path
    # Fora do repr: o token nunca aparece num log de erro.
    token: str = field(repr=False)


def achar_abiyss_toml(env: Mapping[str, str]) -> Path:
    explicito = env.get("ABIYSS_CONFIG", "").strip()
    if explicito:
        caminho = Path(explicito)
        if not caminho.is_file():
            raise ErroConfig(f"ABIYSS_CONFIG aponta para {caminho}, que não existe")
        return caminho.resolve()
    for pasta in (PASTA_DO_ADAPTADOR, *PASTA_DO_ADAPTADOR.parents):
        if (pasta / "abiyss.toml").is_file():
            return pasta / "abiyss.toml"
    raise ErroConfig("não achei o abiyss.toml (defina ABIYSS_CONFIG)")


def ler_dotenv(caminho: Path, nome: str) -> str | None:
    """Só a variável pedida do ``.env`` (formato NOME=valor; aspas opcionais)."""
    try:
        linhas = caminho.read_text(encoding="utf-8").splitlines()
    except OSError:
        return None
    for linha in linhas:
        linha = linha.strip()
        if linha.startswith("export "):
            linha = linha[len("export ") :].strip()
        chave, igual, valor = linha.partition("=")
        if igual and chave.strip() == nome:
            valor = valor.strip()
            if len(valor) >= 2 and valor[0] == valor[-1] and valor[0] in "\"'":
                valor = valor[1:-1]
            return valor
    return None


def carregar(env: Mapping[str, str]) -> Config:
    arquivo = achar_abiyss_toml(env)
    raiz = arquivo.parent
    try:
        dados = tomllib.loads(arquivo.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise ErroConfig(f"não consegui ler {arquivo}: {e}") from None
    gateway = dados.get("gateway", {})
    if not gateway.get("ativo", False):
        raise ErroConfig(
            "[gateway] ativo = false no abiyss.toml: o daemon não abre o socket "
            "(ligue e preencha dono_discord_id; veja docs/GATEWAY.md)"
        )
    socket = Path(gateway.get("socket", SOCKET_PADRAO))
    if not socket.is_absolute():
        socket = raiz / socket
    token = env.get(VARIAVEL_TOKEN) or ler_dotenv(raiz / ".env", VARIAVEL_TOKEN) or ""
    token = token.strip()
    if not token or TOKEN_DE_EXEMPLO in token:
        raise ErroConfig(
            f"variável {VARIAVEL_TOKEN} não definida (ou com o valor de exemplo): "
            "coloque o token do bot no .env"
        )
    return Config(raiz=raiz, socket=socket, token=token)
