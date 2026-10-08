"""Um Discord de mentira (sem internet, sem discord.py de verdade na
conversa) e um daemon de mentira no socket Unix."""

from __future__ import annotations

import asyncio
import json
from pathlib import Path
from typing import Any

import pytest


class RelogioFalso:
    """Tempo que só anda quando alguém dorme."""

    def __init__(self) -> None:
        self.agora = 1000.0
        self.dormidas: list[float] = []

    def __call__(self) -> float:
        return self.agora

    async def dormir(self, segundos: float) -> None:
        self.dormidas.append(segundos)
        self.agora += max(0.0, segundos)
        await asyncio.sleep(0)


class MensagemFalsa:
    def __init__(self, canal: "CanalFalso", id: int, texto: str, responder_a: str | None) -> None:
        self.canal = canal
        self.id = id
        self.texto = texto
        self.responder_a = responder_a
        self.edicoes: list[tuple[float, str]] = []
        self.apagada = False

    async def editar(self, texto: str) -> None:
        if self.canal.discord.falhar_edicoes:
            raise RuntimeError("403 Forbidden (edição recusada)")
        self.canal.discord.operacoes.append((self.canal.discord.relogio(), "editar", self.id, texto))
        self.edicoes.append((self.canal.discord.relogio(), texto))
        self.texto = texto

    async def apagar(self) -> None:
        self.apagada = True


class CanalFalso:
    def __init__(self, discord: "DiscordFalso", id: str | None) -> None:
        self.discord = discord
        self.id = id
        self.mensagens: list[MensagemFalsa] = []

    async def enviar(self, texto: str, responder_a: str | None) -> MensagemFalsa:
        assert len(texto.encode("utf-16-le")) // 2 <= 2000, "o Discord recusaria"
        self.discord.proximo_id += 1
        m = MensagemFalsa(self, self.discord.proximo_id, texto, responder_a)
        self.mensagens.append(m)
        self.discord.operacoes.append((self.discord.relogio(), "enviar", m.id, texto))
        return m

    def visiveis(self) -> list[str]:
        return [m.texto for m in self.mensagens if not m.apagada]


class DiscordFalso:
    def __init__(self, relogio: RelogioFalso) -> None:
        self.relogio = relogio
        self.proximo_id = 9000
        self.falhar_edicoes = False
        self.operacoes: list[tuple[float, str, int, str]] = []
        self.canais: dict[str | None, CanalFalso] = {None: CanalFalso(self, None)}

    @property
    def dm(self) -> CanalFalso:
        return self.canais[None]

    async def canal(self, canal_id: str | None) -> CanalFalso:
        if canal_id not in self.canais:
            self.canais[canal_id] = CanalFalso(self, canal_id)
        return self.canais[canal_id]


@pytest.fixture
def relogio() -> RelogioFalso:
    return RelogioFalso()


@pytest.fixture
def discord_falso(relogio: RelogioFalso) -> DiscordFalso:
    return DiscordFalso(relogio)


class DaemonFalso:
    """O lado do kernel do protocolo, num socket Unix de verdade."""

    def __init__(self, caminho: Path, ola: dict[str, Any] | None = None) -> None:
        self.caminho = caminho
        self.ola = ola or {"tipo": "ola", "versao": 1, "dono_id": "111", "canal_id": None}
        self.recebidas: asyncio.Queue[dict[str, Any]] = asyncio.Queue()
        self.conexoes = 0
        self._escritor: asyncio.StreamWriter | None = None
        self._servidor: asyncio.base_events.Server | None = None

    async def iniciar(self) -> None:
        self._servidor = await asyncio.start_unix_server(self._atender, path=str(self.caminho))

    async def parar(self) -> None:
        if self._escritor:
            self._escritor.close()
        if self._servidor:
            self._servidor.close()
            await self._servidor.wait_closed()
        self.caminho.unlink(missing_ok=True)

    async def _atender(self, leitor: asyncio.StreamReader, escritor: asyncio.StreamWriter) -> None:
        self.conexoes += 1
        primeira = json.loads(await leitor.readline())
        assert primeira == {"tipo": "ola", "versao": 1}
        self._escritor = escritor
        await self.mandar(self.ola)
        while linha := await leitor.readline():
            await self.recebidas.put(json.loads(linha))

    async def mandar(self, mensagem: dict[str, Any]) -> None:
        assert self._escritor is not None
        self._escritor.write((json.dumps(mensagem) + "\n").encode())
        await self._escritor.drain()

    async def proxima(self, tipo: str | None = None) -> dict[str, Any]:
        while True:
            m = await asyncio.wait_for(self.recebidas.get(), 5)
            if tipo is None or m.get("tipo") == tipo:
                return m


@pytest.fixture
def caminho_socket(tmp_path: Path) -> Path:
    # Caminho curto: sockets Unix têm limite de ~100 bytes.
    import tempfile

    pasta = Path(tempfile.mkdtemp(prefix="gw"))
    return pasta / "s.sock"
