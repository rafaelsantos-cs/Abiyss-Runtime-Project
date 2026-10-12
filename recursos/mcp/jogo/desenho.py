"""Monta uma `Fase` a partir de um desenho em texto (moldes e testes).

Letras do desenho:
- células: `#` `.` `~` `=` `:` `_` (placa) `D` (porta de placa) `+` (porta
  trancada) `G` (portão);
- objetos em cima de chão: `V` `R` `C` (avatares), `B` (bloco), `K`
  (Chave Dourada), `k` (chave colorida).

Placas recebem os ids a, b, c... e portas A, B, C... na ordem de leitura
(linha por linha, da esquerda para a direita). As cores das chaves `k` e
das portas `+` vêm em listas, também na ordem de leitura.
"""

from __future__ import annotations

import motor

OBJETOS_SOBRE_CHAO = {"V", "R", "C", "B", "K", "k"}


def fase(
    linhas: list[str] | tuple[str, ...],
    *,
    numero: int = 1,
    nome: str = "teste",
    ligacoes: dict[str, str | list[str]] | None = None,
    cores_chaves: list[str] | None = None,
    cores_trancadas: list[str] | None = None,
    nevoa: bool = False,
    raios: dict[str, int] | None = None,
    extras: dict | None = None,
) -> motor.Fase:
    """`ligacoes`: id da porta → placas (ex.: {"A": "ab"}); sem isso, cada
    porta liga a todas as placas. `extras`: objetos fora do chão, como
    {"blocos": [(x, y)], "avatares": {"V": (x, y)}, "itens": {(x, y): "azul"}}.
    """
    # Aceita o desenho com um espaço entre as células, como o jogo mostra.
    linhas = [l[0::2] if len(l) > 1 and set(l[1::2]) == {" "} else l for l in linhas]
    altura, largura = len(linhas), len(linhas[0])
    celulas, avatares, blocos, itens = [], {}, [], {}
    placas, portas_pos, trancadas = {}, [], {}
    cores_chaves = list(cores_chaves or [])
    cores_trancadas = list(cores_trancadas or [])
    for y, linha in enumerate(linhas):
        if len(linha) != largura:
            raise motor.ErroFase(f"linha {y} com largura diferente")
        base = []
        for x, c in enumerate(linha):
            p = (x, y)
            if c in OBJETOS_SOBRE_CHAO:
                base.append(motor.CHAO)
                if c in motor.ORDEM_AVATARES:
                    avatares[c] = p
                elif c == "B":
                    blocos.append(p)
                elif c == "K":
                    itens[p] = motor.DOURADA
                else:
                    if not cores_chaves:
                        raise motor.ErroFase("falta a cor de uma chave k")
                    itens[p] = cores_chaves.pop(0)
                continue
            base.append(c)
            if c == motor.PLACA:
                placas[chr(ord("a") + len(placas))] = p
            elif c == motor.PORTA:
                portas_pos.append(p)
            elif c == motor.TRANCADA:
                if not cores_trancadas:
                    raise motor.ErroFase("falta a cor de uma porta +")
                trancadas[p] = cores_trancadas.pop(0)
        celulas.append("".join(base))
    portas = []
    for i, p in enumerate(portas_pos):
        id_ = chr(ord("A") + i)
        ligadas = (ligacoes or {}).get(id_)
        portas.append(motor.PortaDePlaca(id_, p, tuple(ligadas) if ligadas else tuple(sorted(placas))))
    extras = extras or {}
    avatares.update(extras.get("avatares", {}))
    blocos.extend(extras.get("blocos", []))
    itens.update(extras.get("itens", {}))
    f = motor.Fase(
        numero=numero,
        nome=nome,
        largura=largura,
        altura=altura,
        celulas=tuple(celulas),
        placas=placas,
        portas=tuple(portas),
        trancadas=trancadas,
        avatares=avatares,
        blocos=tuple(sorted(blocos)),
        itens=itens,
        nevoa=nevoa,
        raios={**motor.RAIOS_PADRAO, **(raios or {})},
    )
    f.validar()
    return f
