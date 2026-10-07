"""Cache em SQLite de buscas, páginas e robots.txt.

Cada entrada tem um prazo (TTL) e o tamanho do que guarda. Quando a soma
dos tamanhos passa do máximo, saem primeiro as entradas usadas há mais
tempo (e as vencidas). O arquivo é só cache: se estiver corrompido, é
recriado.
"""

from __future__ import annotations

import json
import logging
import sqlite3
import time
from pathlib import Path
from typing import Any

log = logging.getLogger("web_rapido")

ESQUEMA = """
CREATE TABLE IF NOT EXISTS entradas (
    tipo TEXT NOT NULL,          -- 'busca', 'pagina' ou 'robots'
    chave TEXT NOT NULL,
    valor TEXT NOT NULL,         -- JSON
    buscado_em REAL NOT NULL,    -- quando veio da rede (epoch)
    expira_em REAL NOT NULL,
    acessado_em REAL NOT NULL,
    tamanho INTEGER NOT NULL,    -- bytes do valor
    PRIMARY KEY (tipo, chave)
);
CREATE INDEX IF NOT EXISTS entradas_acesso ON entradas (acessado_em);
"""


class Cache:
    def __init__(self, arquivo: Path, max_bytes: int, relogio=time.time):
        self.arquivo = arquivo
        self.max_bytes = max_bytes
        self._agora = relogio
        arquivo.parent.mkdir(parents=True, exist_ok=True)
        try:
            self._db = self._abrir()
        except sqlite3.DatabaseError as e:
            ruim = arquivo.with_name(arquivo.name + ".corrompido")
            log.warning("cache %s ilegível (%s); recriando (o antigo fica em %s)", arquivo, e, ruim)
            arquivo.replace(ruim)
            self._db = self._abrir()

    def _abrir(self) -> sqlite3.Connection:
        db = sqlite3.connect(self.arquivo, isolation_level=None)
        try:
            db.execute("PRAGMA auto_vacuum = INCREMENTAL")
            db.execute("PRAGMA journal_mode = WAL")
            db.execute("PRAGMA synchronous = NORMAL")
            db.executescript(ESQUEMA)
            db.execute("SELECT count(*) FROM entradas").fetchone()
        except sqlite3.DatabaseError:
            db.close()
            raise
        return db

    def ler(self, tipo: str, chave: str) -> tuple[Any, float] | None:
        """(valor, buscado_em) se existir e não tiver vencido."""
        agora = self._agora()
        linha = self._db.execute(
            "SELECT valor, buscado_em, expira_em FROM entradas WHERE tipo = ? AND chave = ?", (tipo, chave)
        ).fetchone()
        if linha is None:
            return None
        valor, buscado_em, expira_em = linha
        if expira_em <= agora:
            self._db.execute("DELETE FROM entradas WHERE tipo = ? AND chave = ?", (tipo, chave))
            return None
        self._db.execute(
            "UPDATE entradas SET acessado_em = ? WHERE tipo = ? AND chave = ?", (agora, tipo, chave)
        )
        return json.loads(valor), buscado_em

    def gravar(self, tipo: str, chave: str, valor: Any, ttl_segundos: float, buscado_em: float | None = None) -> None:
        if ttl_segundos <= 0:
            return
        agora = self._agora()
        texto = json.dumps(valor, ensure_ascii=False)
        tamanho = len(texto.encode("utf-8")) + len(chave.encode("utf-8"))
        if tamanho > self.max_bytes:
            return  # maior que o cache inteiro: nem tenta
        self._db.execute(
            "INSERT OR REPLACE INTO entradas VALUES (?, ?, ?, ?, ?, ?, ?)",
            (tipo, chave, texto, buscado_em or agora, agora + ttl_segundos, agora, tamanho),
        )
        self._caber()

    def tamanho_total(self) -> int:
        return self._db.execute("SELECT coalesce(sum(tamanho), 0) FROM entradas").fetchone()[0]

    def _caber(self) -> None:
        """Tira as vencidas e, se ainda passar do máximo, as menos usadas."""
        total = self.tamanho_total()
        if total <= self.max_bytes:
            return
        self._db.execute("DELETE FROM entradas WHERE expira_em <= ?", (self._agora(),))
        total = self.tamanho_total()
        alvo = int(self.max_bytes * 0.9)  # folga para não limpar a cada gravação
        if total > alvo:
            removidas = []
            for tipo, chave, tamanho in self._db.execute(
                "SELECT tipo, chave, tamanho FROM entradas ORDER BY acessado_em"
            ):
                if total <= alvo:
                    break
                removidas.append((tipo, chave))
                total -= tamanho
            self._db.executemany("DELETE FROM entradas WHERE tipo = ? AND chave = ?", removidas)
        self._db.execute("PRAGMA incremental_vacuum")

    def fechar(self) -> None:
        self._db.close()
