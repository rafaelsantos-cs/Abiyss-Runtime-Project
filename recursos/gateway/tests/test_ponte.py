"""A ponte com o daemon: reconexão com espera, nada se perde, nada duplica."""

import asyncio

from espera import Espera
from ponte import Ponte

from conftest import DaemonFalso


def fatos(id: str, texto: str = "oi") -> dict:
    return {"id": id, "canal_id": "5", "dm": True, "autor_id": "111", "texto": texto}


async def esperar(condicao, prazo: float = 5.0) -> None:
    async def laco():
        while not condicao():
            await asyncio.sleep(0.01)

    await asyncio.wait_for(laco(), prazo)


def test_reconecta_com_espera_crescente_ate_o_daemon_subir(caminho_socket):
    async def cenario():
        daemon = DaemonFalso(caminho_socket)
        dormidas: list[float] = []

        async def dormir(s: float) -> None:
            dormidas.append(s)
            if len(dormidas) == 3:
                await daemon.iniciar()  # o daemon sobe depois de 3 tentativas
            await asyncio.sleep(0)

        ponte = Ponte(caminho_socket, processar=None, espera=Espera(base=1, teto=60, sorteio=0), dormir=dormir)
        tarefa = asyncio.create_task(ponte.rodar())
        await asyncio.wait_for(ponte.conectado.wait(), 5)
        assert dormidas == [1, 2, 4]
        assert ponte.espera.tentativas == 0, "conectou: a espera zera"
        tarefa.cancel()
        await daemon.parar()

    asyncio.run(cenario())


def test_mensagens_esperam_o_daemon_e_so_as_nao_confirmadas_vao_de_novo(caminho_socket):
    async def cenario():
        daemon = DaemonFalso(caminho_socket)

        async def dormir(s: float) -> None:
            await asyncio.sleep(0.01)

        ponte = Ponte(caminho_socket, processar=None, espera=Espera(sorteio=0), dormir=dormir)
        tarefa = asyncio.create_task(ponte.rodar())
        # Daemon fora do ar: guardadas.
        await ponte.mensagem(fatos("1"))
        await ponte.mensagem(fatos("2"))
        assert ponte.esperando() == ["1", "2"]

        await daemon.iniciar()
        assert (await daemon.proxima("mensagem"))["mensagem"]["id"] == "1"
        assert (await daemon.proxima("mensagem"))["mensagem"]["id"] == "2"
        await daemon.mandar({"tipo": "recebido", "id": "1", "estado": "recebida"})
        await esperar(lambda: ponte.esperando() == ["2"])

        # Daemon reinicia: só a "2" (sem confirmação) vai de novo.
        await daemon.parar()
        await daemon.iniciar()
        reenviada = await daemon.proxima("mensagem")
        assert reenviada["mensagem"]["id"] == "2"
        assert daemon.conexoes == 2
        tarefa.cancel()
        await daemon.parar()

    asyncio.run(cenario())


def test_entrega_confirma_falha_e_nao_duplica(caminho_socket):
    async def cenario():
        daemon = DaemonFalso(caminho_socket)
        await daemon.iniciar()
        chamadas: list[dict] = []

        async def processar(pedido: dict):
            chamadas.append(pedido)
            if pedido["texto"] == "quebra":
                raise RuntimeError("Discord fora")
            return [f"d{pedido['ref']}"]

        ponte = Ponte(caminho_socket, processar=processar)
        tarefa = asyncio.create_task(ponte.rodar())
        await asyncio.wait_for(ponte.conectado.wait(), 5)

        await daemon.mandar({"tipo": "enviar", "ref": 7, "canal_id": None, "texto": "olá"})
        assert await daemon.proxima() == {"tipo": "enviado", "ref": 7, "ids": ["d7"]}
        # O kernel não viu a confirmação e manda de novo: confirma sem duplicar.
        await daemon.mandar({"tipo": "enviar", "ref": 7, "canal_id": None, "texto": "olá"})
        assert await daemon.proxima() == {"tipo": "enviado", "ref": 7, "ids": ["d7"]}
        assert len(chamadas) == 1
        # Falha vira `falhou` (o kernel tenta de novo mais tarde).
        await daemon.mandar({"tipo": "enviar", "ref": 8, "canal_id": None, "texto": "quebra"})
        falhou = await daemon.proxima()
        assert falhou["tipo"] == "falhou" and falhou["ref"] == 8 and "Discord fora" in falhou["erro"]
        tarefa.cancel()
        await daemon.parar()

    asyncio.run(cenario())
