"""robots.txt pelo RFC 9309.

O `urllib.robotparser` da biblioteca padrão não entende os curingas `*` e
`$` (trata como texto), então regras comuns como `Disallow: /*?sessao=`
passariam batido. Aqui:

- o grupo é escolhido pelo nome do robô (sem diferenciar maiúsculas);
  grupos com o mesmo nome se somam; sem grupo do robô, vale o `*`;
- vence a regra mais longa que casa com o caminho (com a query); empate
  entre Allow e Disallow: Allow; nenhuma regra: permitido;
- `/robots.txt` sempre pode ser lido;
- `Crawl-delay` (fora do RFC, mas comum) do grupo escolhido é respeitado.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from urllib.parse import unquote, urlsplit


@dataclass
class Grupo:
    agentes: list[str] = field(default_factory=list)
    regras: list[tuple[bool, str]] = field(default_factory=list)  # (permite, padrão)
    atraso: float | None = None


@dataclass
class Robots:
    grupos: list[Grupo]
    # Sem robots.txt utilizável: tudo permitido (4xx) ou tudo proibido (5xx, erro).
    tudo: bool | None = None

    @classmethod
    def permite_tudo(cls) -> "Robots":
        return cls([], tudo=True)

    @classmethod
    def proibe_tudo(cls) -> "Robots":
        return cls([], tudo=False)

    def _grupos_do(self, produto: str) -> list[Grupo]:
        produto = produto.lower()
        meus = [g for g in self.grupos if produto in g.agentes]
        return meus or [g for g in self.grupos if "*" in g.agentes]

    def permite(self, produto: str, url: str) -> bool:
        if self.tudo is not None:
            return self.tudo
        partes = urlsplit(url)
        caminho = _normalizar(partes.path or "/") + (f"?{partes.query}" if partes.query else "")
        if caminho == "/robots.txt":
            return True
        melhor: tuple[int, bool] | None = None
        for grupo in self._grupos_do(produto):
            for permite, padrao in grupo.regras:
                if not padrao:
                    continue  # "Disallow:" vazio não proíbe nada
                if _casa(padrao, caminho):
                    chave = (len(padrao), permite)  # mais longa vence; empate: Allow
                    if melhor is None or chave > melhor:
                        melhor = chave
        return True if melhor is None else melhor[1]

    def atraso(self, produto: str) -> float | None:
        atrasos = [g.atraso for g in self._grupos_do(produto) if g.atraso is not None]
        return max(atrasos) if atrasos else None


def _normalizar(caminho: str) -> str:
    # Compara sem codificação %XX nos dois lados (o RFC manda comparar os octetos).
    return unquote(caminho)


def _casa(padrao: str, caminho: str) -> bool:
    ancorado = padrao.endswith("$")
    corpo = _normalizar(padrao[:-1] if ancorado else padrao)
    regex = "".join(".*" if c == "*" else re.escape(c) for c in corpo)
    return re.match(regex + ("$" if ancorado else ""), caminho, re.DOTALL) is not None


def interpretar(texto: str) -> Robots:
    grupos: list[Grupo] = []
    atual: Grupo | None = None
    lendo_agentes = False
    for linha in texto.splitlines():
        linha = linha.split("#", 1)[0].strip()
        if ":" not in linha:
            continue
        campo, valor = (p.strip() for p in linha.split(":", 1))
        campo = campo.lower()
        if campo == "user-agent":
            if not lendo_agentes:
                atual = Grupo()
                grupos.append(atual)
                lendo_agentes = True
            atual.agentes.append(valor.lower())
            continue
        if atual is None:
            continue  # regra antes de qualquer user-agent: ignorada
        lendo_agentes = False
        if campo == "allow":
            atual.regras.append((True, valor))
        elif campo == "disallow":
            atual.regras.append((False, valor))
        elif campo == "crawl-delay":
            try:
                atual.atraso = float(valor.replace(",", "."))
            except ValueError:
                pass
    return Robots(grupos)
