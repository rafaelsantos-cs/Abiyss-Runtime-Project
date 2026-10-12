"""Formato do catálogo: um arquivo JSON por fase.

O arquivo da fase traz mapa, objetos, molde, seed e versão do gerador. O
ótimo e a solução ficam num arquivo SEPARADO (o gabarito), que só o harness
lê: este módulo nunca abre o gabarito, e o servidor do jogo também não.

Os arquivos são gravados de forma canônica (chaves ordenadas, sem espaços
variáveis): a mesma seed com a mesma versão do gerador produz exatamente os
mesmos bytes, e um teste confere isso pelo hash.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

import motor

FORMATO = 1


def _pos(v: Any) -> motor.Pos:
    if not (isinstance(v, list) and len(v) == 2 and all(isinstance(n, int) for n in v)):
        raise motor.ErroFase(f"posição inválida {v!r}")
    return (v[0], v[1])


def fase_para_dict(fase: motor.Fase, *, molde: str, seed: str, gerador: str) -> dict[str, Any]:
    return {
        "formato": FORMATO,
        "gerador": gerador,
        "molde": molde,
        "seed": seed,
        "numero": fase.numero,
        "nome": fase.nome,
        "largura": fase.largura,
        "altura": fase.altura,
        "nevoa": fase.nevoa,
        "raios": {a: fase.raios[a] for a in motor.ORDEM_AVATARES if a in fase.raios},
        "mapa": list(fase.celulas),
        "objetos": {
            "avatares": {a: list(p) for a, p in sorted(fase.avatares.items())},
            "blocos": [list(p) for p in sorted(fase.blocos)],
            "itens": [{"item": i, "pos": list(p)} for p, i in sorted(fase.itens.items())],
            "placas": {id_: list(p) for id_, p in sorted(fase.placas.items())},
            "portas": [
                {"id": p.id, "pos": list(p.pos), "placas": list(p.placas)} for p in sorted(fase.portas, key=lambda p: p.id)
            ],
            "trancadas": [{"cor": c, "pos": list(p)} for p, c in sorted(fase.trancadas.items())],
        },
    }


def fase_de_dict(d: dict[str, Any]) -> motor.Fase:
    try:
        if d.get("formato") != FORMATO:
            raise motor.ErroFase(f"formato {d.get('formato')!r} desconhecido (esperado {FORMATO})")
        o = d["objetos"]
        fase = motor.Fase(
            numero=int(d["numero"]),
            nome=str(d["nome"]),
            largura=int(d["largura"]),
            altura=int(d["altura"]),
            celulas=tuple(d["mapa"]),
            placas={str(k): _pos(v) for k, v in o.get("placas", {}).items()},
            portas=tuple(
                motor.PortaDePlaca(str(p["id"]), _pos(p["pos"]), tuple(p["placas"])) for p in o.get("portas", [])
            ),
            trancadas={_pos(t["pos"]): str(t["cor"]) for t in o.get("trancadas", [])},
            avatares={str(a): _pos(p) for a, p in o["avatares"].items()},
            blocos=tuple(_pos(p) for p in o.get("blocos", [])),
            itens={_pos(i["pos"]): str(i["item"]) for i in o.get("itens", [])},
            nevoa=bool(d.get("nevoa", False)),
            raios={**motor.RAIOS_PADRAO, **{str(a): int(r) for a, r in d.get("raios", {}).items()}},
        )
    except (KeyError, TypeError, ValueError) as e:
        if isinstance(e, motor.ErroFase):
            raise
        raise motor.ErroFase(f"arquivo de fase incompleto: {e}") from None
    fase.validar()
    return fase


def canonico(d: dict[str, Any]) -> bytes:
    return (json.dumps(d, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")


def hash_de(dados: bytes) -> str:
    return hashlib.sha256(dados).hexdigest()


def gravar(caminho: Path, d: dict[str, Any]) -> str:
    """Grava canônico (atômico) e devolve o sha256."""
    dados = canonico(d)
    caminho.parent.mkdir(parents=True, exist_ok=True)
    temporario = caminho.with_suffix(caminho.suffix + ".tmp")
    temporario.write_bytes(dados)
    temporario.replace(caminho)
    return hash_de(dados)


def ler(caminho: Path) -> dict[str, Any]:
    return json.loads(caminho.read_text(encoding="utf-8"))


def carregar_fase(caminho: Path) -> motor.Fase:
    return fase_de_dict(ler(caminho))


def nome_arquivo(numero: int) -> str:
    return f"fase-{numero:02d}.json"
