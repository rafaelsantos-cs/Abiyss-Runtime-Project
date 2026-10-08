"""O lado Discord do adaptador (discord.py).

Daqui para o kernel só vão FATOS (IDs, se é DM, se é bot, se menciona o
bot, a qual mensagem responde). Quem decide se é o dono falando é o kernel.
Por economia e menos exposição, o adaptador nem encaminha o que vem de
canais de servidor que não são o permitido (o kernel ignoraria).

O bot nunca menciona ninguém (``AllowedMentions.none()``): um texto com
@everyone vindo do modelo não vira ping.
"""

from __future__ import annotations

import logging
import os
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any

import discord

from espera import Espera

log = logging.getLogger("abiyss.gateway.discord")

# Mensagens buscadas por canal ao voltar (o adaptador ficou fora do ar).
MAX_RECUPERAR = 50
SEM_MENCOES = discord.AllowedMentions.none()


def fatos(mensagem: Any, eu_id: int, canal_permitido: str | None) -> dict[str, Any] | None:
    """Os fatos de uma mensagem do Discord, ou ``None`` se não for para o kernel."""
    if mensagem.author.id == eu_id:
        return None  # o próprio bot
    if mensagem.type not in (discord.MessageType.default, discord.MessageType.reply):
        return None  # entrou no servidor, fixou mensagem...
    dm = mensagem.guild is None
    canal_id = str(mensagem.channel.id)
    if not dm and canal_id != canal_permitido:
        return None
    referencia = getattr(mensagem, "reference", None)
    citada = getattr(referencia, "resolved", None) if referencia else None
    texto = mensagem.content or ""
    anexos = [a.filename for a in getattr(mensagem, "attachments", [])]
    if anexos:
        texto = f"{texto}\n(anexos que não consigo abrir: {', '.join(anexos)})".strip()
    return {
        "id": str(mensagem.id),
        "canal_id": canal_id,
        "dm": dm,
        "autor_id": str(mensagem.author.id),
        "autor_nome": getattr(mensagem.author, "display_name", "") or "",
        "autor_bot": bool(mensagem.author.bot or getattr(mensagem, "webhook_id", None)),
        "menciona_bot": any(u.id == eu_id for u in getattr(mensagem, "mentions", [])),
        "responde_a": str(referencia.message_id) if referencia and referencia.message_id else None,
        "responde_ao_bot": bool(citada is not None and getattr(getattr(citada, "author", None), "id", None) == eu_id),
        "texto": texto,
    }


class MensagemReal:
    def __init__(self, mensagem: discord.Message) -> None:
        self._m = mensagem
        self.id = mensagem.id

    async def editar(self, texto: str) -> None:
        self._m = await self._m.edit(content=texto, allowed_mentions=SEM_MENCOES)

    async def apagar(self) -> None:
        await self._m.delete()


class CanalReal:
    def __init__(self, canal: discord.abc.Messageable) -> None:
        self._canal = canal

    async def enviar(self, texto: str, responder_a: str | None, arquivo: Path | None = None) -> MensagemReal:
        referencia = None
        if responder_a:
            referencia = discord.MessageReference(
                message_id=int(responder_a),
                channel_id=self._canal.id,
                fail_if_not_exists=False,
            )
        if arquivo is None:
            m = await self._canal.send(texto, reference=referencia, allowed_mentions=SEM_MENCOES)
            return MensagemReal(m)
        # O caminho já foi conferido (saida.anexo_seguro); O_NOFOLLOW
        # recusa um link trocado no lugar depois da conferência.
        fd = os.open(arquivo, os.O_RDONLY | os.O_NOFOLLOW)
        with os.fdopen(fd, "rb") as f:
            m = await self._canal.send(
                texto,
                file=discord.File(f, filename=arquivo.name),
                reference=referencia,
                allowed_mentions=SEM_MENCOES,
            )
        return MensagemReal(m)


class ClienteDiscord(discord.Client):
    def __init__(self) -> None:
        intents = discord.Intents.default()
        # Intent privilegiada: ligue em Bot > Privileged Gateway Intents.
        intents.message_content = True
        intents.dm_messages = True
        intents.guild_messages = True
        super().__init__(intents=intents, allowed_mentions=SEM_MENCOES)
        self.ponte: Any = None

    def _ola(self) -> dict[str, Any]:
        return (self.ponte.ola if self.ponte else None) or {}

    async def canal(self, canal_id: str | None) -> CanalReal:
        if canal_id is None:
            dono_id = self._ola().get("dono_id")
            if not dono_id:
                raise RuntimeError("o kernel ainda não disse quem é o dono")
            dono = self.get_user(int(dono_id)) or await self.fetch_user(int(dono_id))
            return CanalReal(dono.dm_channel or await dono.create_dm())
        canal = self.get_channel(int(canal_id)) or await self.fetch_channel(int(canal_id))
        return CanalReal(canal)

    async def on_ready(self) -> None:
        log.info("no Discord como %s (id %s)", self.user, self.user.id if self.user else "?")
        if self.ponte and self.ponte.ola:
            await self.recuperar(self.ponte.ola)

    async def on_message(self, mensagem: discord.Message) -> None:
        await self._encaminhar(mensagem)

    async def _encaminhar(self, mensagem: discord.Message) -> None:
        if self.user is None or self.ponte is None:
            return
        f = fatos(mensagem, self.user.id, self._ola().get("canal_id"))
        if f is not None:
            await self.ponte.mensagem(f)

    async def recuperar(self, ola: dict[str, Any]) -> None:
        """Busca o que chegou enquanto o adaptador estava fora do ar (só
        depois da última mensagem que o kernel já tem; sem marca, nada: no
        primeiro uso, mensagens antigas não viram conversa nova)."""
        if not self.is_ready():
            return
        alvos: list[tuple[str, Callable[[], Awaitable[Any]]]] = []
        if ola.get("ultimo_dm"):
            alvos.append((ola["ultimo_dm"], lambda: self._dm_do_dono(ola)))
        if ola.get("canal_id") and ola.get("ultimo_canal"):
            canal_id = int(ola["canal_id"])
            alvos.append((ola["ultimo_canal"], lambda: self._canal_por_id(canal_id)))
        for depois, achar in alvos:
            try:
                canal = await achar()
                async for m in canal.history(after=discord.Object(id=int(depois)), limit=MAX_RECUPERAR, oldest_first=True):
                    await self._encaminhar(m)
            except discord.DiscordException as e:
                log.warning("não consegui buscar mensagens perdidas: %s", e)

    async def _dm_do_dono(self, ola: dict[str, Any]) -> Any:
        dono_id = int(ola["dono_id"])
        dono = self.get_user(dono_id) or await self.fetch_user(dono_id)
        return dono.dm_channel or await dono.create_dm()

    async def _canal_por_id(self, canal_id: int) -> Any:
        return self.get_channel(canal_id) or await self.fetch_channel(canal_id)

    async def ao_ola(self, ola: dict[str, Any]) -> None:
        await self.recuperar(ola)


async def manter(
    iniciar: Callable[[], Awaitable[None]],
    espera: Espera,
    dormir: Callable[[float], Awaitable[None]],
    fatais: tuple[type[BaseException], ...] = (),
) -> None:
    """Mantém a conexão com o Discord. O discord.py já reconecta sozinho
    em quedas normais; isto cobre o resto (falha ao subir, rede fora por
    muito tempo). Erros de ``fatais`` (token errado, intent desligada)
    sobem: insistir não resolve."""
    while True:
        try:
            await iniciar()
            log.warning("a conexão com o Discord terminou")
        except fatais:
            raise
        except Exception as e:  # noqa: BLE001 — qualquer outra falha: espera e tenta de novo
            log.warning("falha na conexão com o Discord: %s", e)
        else:
            espera.zerar()
        await dormir(espera.proxima())
