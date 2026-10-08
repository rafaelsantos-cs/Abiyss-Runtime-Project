"""Sessões do navegador: um Chromium sem tela e um contexto anônimo por sessão.

- O Chromium sobe sob demanda (na primeira chamada) e fecha sozinho quando
  fica um minuto sem sessões. Ele usa o Filtro global como proxy; cada
  sessão tem um contexto novo (como uma janela anônima: sem cookies, sem
  cache, sem perfil guardado) com o SEU Filtro e a interceptação de pedidos
  (rede.py). Downloads desligados, service workers bloqueados, diálogos
  (alert, confirm, prompt, beforeunload) dispensados na hora.
- Modo só leitura (``[navegador] interagir = false``, o padrão): navegar,
  ler, rolar, voltar, abas, capturar a tela e clicar em LINKS. digitar e
  envio de formulário são recusados. Com interagir, digitar e enviar
  funcionam, mas nunca para outra origem (formulário, POST de fetch/XHR).
- Limites: sessões ao mesmo tempo, abas por sessão, prazo por chamada,
  tempo de carregamento, tamanho do instantâneo, memória da árvore do
  Chromium (passou: o Chromium inteiro é morto e as sessões fecham) e
  sessão ociosa. Uma chamada que passa do prazo fecha a sessão; se nem isso
  responder, a árvore do Chromium inteira é morta.
"""

from __future__ import annotations

import asyncio
import contextlib
import logging
import os
import re
import secrets
import shutil
import subprocess
import sys
import time
from collections import OrderedDict, deque
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Awaitable, Callable

from mcp.server.mcpserver.exceptions import ToolError
from playwright.async_api import Error as ErroPlaywright
from playwright.async_api import TimeoutError as TempoPlaywright
from playwright.async_api import async_playwright

import config
import instantaneo
import processos
import rede
from rede import Bloqueio, Filtro, Rede

log = logging.getLogger("navegador")

MIB = 1024 * 1024
VIEWPORT = {"width": 1280, "height": 800}
NAVEGADOR_OCIOSO_SEGUNDOS = 60
VERIFICACAO_SEGUNDOS = 1.0
MAX_TEXTO_DIGITADO = 2000
MAX_ALTURA_CAPTURA = 8000
REF = re.compile(r"^e\d{1,7}$")

FLAGS_CHROMIUM = [
    # Menos processos (e menos memória): GPU e rede dentro do processo
    # principal. O Playwright já passa CDPScreenshotNewSurface: repetido
    # aqui porque o último --enable-features vale.
    "--disable-gpu",
    "--in-process-gpu",
    "--enable-features=CDPScreenshotNewSurface,NetworkServiceInProcess2",
    # Nada sai por fora do proxy: sem QUIC, sem pré-resolução de DNS, WebRTC
    # só pelo proxy (sem UDP direto), sem pings de links.
    "--disable-quic",
    "--dns-prefetch-disable",
    "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
    "--no-pings",
    "--mute-audio",
]

# Antes de qualquer script da página: formulários e WebRTC. É só a primeira
# barreira (a página pode tentar contornar); a interceptação de pedidos é a
# que vale para POST, e o clicar/digitar conferem o formulário antes.
INICIO_DA_PAGINA = r"""
(() => {
  const interagir = %s;
  const recusar = (form, botao) => {
    if (!interagir) return true;
    let acao = form.action || location.href;
    if (botao && botao.getAttribute && botao.getAttribute("formaction")) acao = botao.formAction;
    try { return new URL(acao, location.href).origin !== location.origin; } catch (e) { return true; }
  };
  document.addEventListener("submit", (e) => {
    if (recusar(e.target, e.submitter)) { e.preventDefault(); e.stopImmediatePropagation(); }
  }, true);
  const enviar = HTMLFormElement.prototype.submit;
  HTMLFormElement.prototype.submit = function () { if (!recusar(this, null)) return enviar.call(this); };
  for (const nome of ["RTCPeerConnection", "webkitRTCPeerConnection", "RTCDataChannel"]) {
    try { Object.defineProperty(window, nome, { value: undefined, configurable: false, writable: false }); } catch (e) {}
  }
})();
"""

# O que o clicar/digitar precisam saber do elemento antes de agir.
INFO_ELEMENTO = r"""
(el) => {
  const tag = el.tagName, papel = (el.getAttribute("role") || "").toLowerCase();
  const tipo = (el.getAttribute("type") || "").toLowerCase();
  const link = ((tag === "A" || tag === "AREA") && el.hasAttribute("href")) || papel === "link";
  const form = el.form || (el.closest && el.closest("form")) || null;
  const envia = !!el.form && (tag === "BUTTON" ? (tipo === "" || tipo === "submit")
                                             : tag === "INPUT" && (tipo === "submit" || tipo === "image"));
  let destino = null;
  if (form) {
    let acao = form.action || location.href;
    if (envia && el.getAttribute("formaction")) acao = el.formAction;
    try { destino = new URL(acao, location.href).origin; } catch (e) { destino = "inválida"; }
  }
  const naoCampo = ["button", "submit", "reset", "image", "checkbox", "radio", "range", "file", "hidden", "color"];
  const campo = tag === "TEXTAREA" || (tag === "INPUT" && !naoCampo.includes(tipo || "text")) ||
    el.isContentEditable || ["textbox", "searchbox", "combobox"].includes(papel);
  return { link, envia, destino, origem: location.origin, campo, senha: tipo === "password" };
}
"""


class SessaoEncerrada(Exception):
    pass


@dataclass
class Sessao:
    id: str
    contexto: Any
    filtro: Filtro
    atributo: str
    abas: list[Any] = field(default_factory=list)
    atual: int = 0
    refs: dict[str, int] = field(default_factory=lambda: {"proximo": 1})
    trava: asyncio.Lock = field(default_factory=asyncio.Lock)
    ultimo_uso: float = field(default_factory=time.monotonic)
    avisos: deque[str] = field(default_factory=lambda: deque(maxlen=30))
    novos_avisos: int = 0
    bloqueios: deque[str] = field(default_factory=lambda: deque(maxlen=30))
    novos_bloqueios: int = 0
    ultimo_texto: str = ""
    ultima_url: str = ""
    ultimo_titulo: str = ""
    capturas: int = 0
    filtro_relatado: int = 0

    @property
    def pagina(self):
        if not self.abas:
            raise ToolError(f"a sessão {self.id} não tem abas abertas; abra uma página com navegar (sessao={self.id})")
        self.atual = min(self.atual, len(self.abas) - 1)
        return self.abas[self.atual]

    def avisar(self, texto: str) -> None:
        self.avisos.append(texto)
        self.novos_avisos = min(self.novos_avisos + 1, self.avisos.maxlen)

    def bloquear(self, url: str, motivo: str) -> None:
        self.bloqueios.append(f"{url[:150]} ({motivo})")
        self.novos_bloqueios = min(self.novos_bloqueios + 1, self.bloqueios.maxlen)

    def notas(self) -> list[str]:
        """Avisos novos desde a última resposta (e zera a contagem)."""
        linhas = [f"aviso: {a}" for a in list(self.avisos)[len(self.avisos) - self.novos_avisos :]]
        novos = list(self.bloqueios)[len(self.bloqueios) - self.novos_bloqueios :]
        filtrados = list(self.filtro.bloqueios)[-(self.filtro.total_bloqueados - self.filtro_relatado) :]
        if self.filtro.total_bloqueados == self.filtro_relatado:
            filtrados = []
        if novos:
            linhas.append(f"aviso: {len(novos)} pedido(s) da página bloqueado(s) pelas regras de rede:")
            linhas.extend(f"  - {b}" for b in novos[-5:])
        if filtrados:
            linhas.append(
                f"aviso: o filtro de rede recusou {self.filtro.total_bloqueados - self.filtro_relatado} "
                "conexão(ões) (redirecionamento ou pedido da página):"
            )
            linhas.extend(f"  - {b.onde} ({b.motivo})" for b in filtrados[-5:])
        self.novos_avisos = self.novos_bloqueios = 0
        self.filtro_relatado = self.filtro.total_bloqueados
        return linhas


class Navegador:
    def __init__(self, cfg: config.Config):
        self.cfg = cfg
        self.rede = Rede(cfg.excecoes_rede)
        self.filtro_global = Filtro(self.rede, "global")
        self.sessoes: dict[str, Sessao] = {}
        self.encerradas: OrderedDict[str, str] = OrderedDict()
        self.sem_sandbox: str | None = None
        self._pw = None
        self._navegador = None
        self._trava = asyncio.Lock()
        # Grupos de processos do Chromium → executáveis vistos em cada um.
        self._grupos: dict[int, set[str]] = {}
        self._criando = 0
        self._vigia: subprocess.Popen | None = None
        self._sem_sessoes_desde = time.monotonic()
        self._tarefa: asyncio.Task | None = None
        self._tmp = cfg.pasta_dados / "tmp" / str(os.getpid())
        self.ao_vivo = None  # ao_vivo.AoVivo, se ligado

    # -- ciclo de vida -------------------------------------------------------

    async def iniciar(self) -> None:
        # Uma pasta temporária por processo (o chat e o daemon podem ter cada
        # um o seu navegador). Perfis que um Chromium morto à força deixou
        # para trás, de processos que já não existem, são apagados.
        base = self.cfg.pasta_dados / "tmp"
        base.mkdir(parents=True, exist_ok=True)
        for velha in base.iterdir():
            if velha.name.isdigit() and processos.inicio_de(int(velha.name)) is None:
                shutil.rmtree(velha, ignore_errors=True)
        self._tmp = base / str(os.getpid())
        self._tmp.mkdir(exist_ok=True)
        await self.filtro_global.iniciar()
        self._vigia = subprocess.Popen(
            [sys.executable, str(config.PASTA_DO_SERVIDOR / "vigia.py"), str(os.getpid())],
            stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL,
            start_new_session=True,
            text=True,
        )
        self._tarefa = asyncio.ensure_future(self._vigiar())

    async def encerrar(self) -> None:
        if self._tarefa:
            self._tarefa.cancel()
            with contextlib.suppress(BaseException):
                await self._tarefa
        await self._fechar_navegador("o servidor está encerrando")
        await self.filtro_global.fechar()
        if self._vigia:
            with contextlib.suppress(Exception):
                self._vigia.stdin.close()
                self._vigia.wait(5)
            self._vigia = None
        shutil.rmtree(self._tmp, ignore_errors=True)

    def _avisar_vigia(self, linha: str) -> None:
        if self._vigia and self._vigia.stdin and not self._vigia.stdin.closed:
            with contextlib.suppress(OSError, ValueError):
                self._vigia.stdin.write(linha + "\n")
                self._vigia.stdin.flush()

    def _grupos_do_chromium(self) -> dict[int, list[int]]:
        excluir = (self._vigia.pid,) if self._vigia else ()
        return processos.grupos_destacados(os.getpid(), excluir=excluir)

    async def _lancar(self):
        async with self._trava:
            if self._navegador is not None and self._navegador.is_connected():
                return self._navegador
            if self._pw is None:
                # O driver do Playwright cria os perfis temporários do Chromium
                # no TMPDIR dele: só o processo do driver recebe esta pasta.
                antes = os.environ.get("TMPDIR")
                os.environ["TMPDIR"] = str(self._tmp)
                try:
                    self._pw = await async_playwright().start()
                finally:
                    if antes is None:
                        os.environ.pop("TMPDIR", None)
                    else:
                        os.environ["TMPDIR"] = antes
            opcoes = dict(
                headless=True,
                args=FLAGS_CHROMIUM + [f"--js-flags=--max-old-space-size={self.cfg.js_heap_mb}"],
                proxy={"server": self.filtro_global.endereco},
                timeout=self.cfg.timeout_carregamento * 1000,
                downloads_path=str(self._tmp / "downloads"),
            )
            if self.cfg.chromium:
                opcoes["executable_path"] = str(self.cfg.chromium)
            try:
                navegador = await self._pw.chromium.launch(chromium_sandbox=self.cfg.sandbox != "nao", **opcoes)
                self.sem_sandbox = "NAVEGADOR_SANDBOX = nao" if self.cfg.sandbox == "nao" else None
            except ErroPlaywright as e:
                motivo = _primeira_linha(e)
                if self.cfg.sandbox != "auto":
                    raise ToolError(_erro_de_lancamento(motivo)) from None
                log.warning(
                    "o Chromium não subiu com a sandbox (%s); subindo SEM a sandbox. Para exigir a sandbox, "
                    "veja o README (perfil do AppArmor) e use NAVEGADOR_SANDBOX = sim",
                    motivo,
                )
                try:
                    navegador = await self._pw.chromium.launch(chromium_sandbox=False, **opcoes)
                except ErroPlaywright as e2:
                    raise ToolError(_erro_de_lancamento(_primeira_linha(e2))) from None
                self.sem_sandbox = motivo
            navegador.on("disconnected", lambda _: self._caiu())
            self._navegador = navegador
            self._anotar_grupos()
            log.info("Chromium %s no ar (grupos %s)", navegador.version, sorted(self._grupos))
            return navegador

    def _caiu(self) -> None:
        if self._navegador is not None:
            log.warning("o Chromium caiu ou foi encerrado")
            self._navegador = None
            self._derrubar_sessoes("o Chromium caiu ou foi encerrado")

    def _derrubar_sessoes(self, motivo: str) -> None:
        for sessao in list(self.sessoes.values()):
            self._esquecer(sessao, motivo)
            asyncio.ensure_future(sessao.filtro.fechar())

    def _esquecer(self, sessao: Sessao, motivo: str) -> None:
        self.sessoes.pop(sessao.id, None)
        self.encerradas[sessao.id] = motivo
        while len(self.encerradas) > 100:
            self.encerradas.popitem(last=False)
        if not self.sessoes:
            self._sem_sessoes_desde = time.monotonic()

    def _anotar_grupos(self) -> None:
        """Anota os grupos do Chromium que descendem deste servidor agora e
        esquece os que já não têm processo vivo (o número pode ser reusado)."""
        atuais = self._grupos_do_chromium()
        for grupo in atuais.keys() - self._grupos.keys():
            self._avisar_vigia(f"grupo {grupo}")
        processos.anotar(self._grupos, atuais)
        vivos = {processos.grupo_de(pid) for pid in processos.membros_verificados(self._grupos)}
        for grupo in self._grupos.keys() - vivos:
            del self._grupos[grupo]
            self._avisar_vigia(f"fim {grupo}")

    def _matar_arvore(self) -> None:
        """SIGKILL em cada processo do Chromium (grupo E executável anotados)."""
        self._anotar_grupos()
        for _ in range(3):  # um processo pode ter criado outro no meio
            if not processos.matar(processos.membros_verificados(self._grupos)):
                break
        for grupo in self._grupos:
            self._avisar_vigia(f"fim {grupo}")
        self._grupos.clear()

    def matar_chromium(self, motivo: str) -> None:
        """A árvore inteira do Chromium, na hora."""
        self._matar_arvore()
        navegador, self._navegador = self._navegador, None
        self._derrubar_sessoes(motivo)
        log.warning("Chromium morto: %s", motivo)
        if navegador is not None:
            asyncio.ensure_future(_sem_erro(navegador.close(), 2))

    async def _fechar_navegador(self, motivo: str) -> None:
        for sessao in list(self.sessoes.values()):
            await self.fechar_sessao(sessao, motivo)
        navegador, self._navegador = self._navegador, None
        if navegador is not None:
            await _sem_erro(navegador.close(), 5)
        self._matar_arvore()  # o que sobrou, se sobrou
        if self._pw is not None:
            await _sem_erro(self._pw.stop(), 5)
            self._pw = None

    async def _vigiar(self) -> None:
        """Memória do Chromium, sessões ociosas e o Chromium sem uso."""
        limite = self.cfg.memoria_chromium_mb * MIB
        while True:
            await asyncio.sleep(VERIFICACAO_SEGUNDOS)
            try:
                self._anotar_grupos()
                usado = processos.rss_bytes_de(processos.membros_verificados(self._grupos))
                if usado > limite:
                    self.matar_chromium(
                        f"o Chromium passou do limite de memória ({usado // MIB} MiB > "
                        f"{self.cfg.memoria_chromium_mb} MiB, NAVEGADOR_MEMORIA_MB) e foi encerrado"
                    )
                    continue
                agora = time.monotonic()
                for sessao in list(self.sessoes.values()):
                    if not sessao.trava.locked() and agora - sessao.ultimo_uso > self.cfg.sessao_ociosa_segundos:
                        await self.fechar_sessao(
                            sessao, f"ficou mais de {self.cfg.sessao_ociosa_segundos} s sem uso e foi fechada"
                        )
                if (
                    self._navegador is not None
                    and not self.sessoes
                    and agora - self._sem_sessoes_desde > NAVEGADOR_OCIOSO_SEGUNDOS
                ):
                    log.info("sem sessões há %d s: fechando o Chromium", NAVEGADOR_OCIOSO_SEGUNDOS)
                    await self._fechar_navegador("sem uso")
            except asyncio.CancelledError:
                raise
            except Exception:
                log.exception("erro na vigilância do Chromium")

    # -- sessões -------------------------------------------------------------

    async def nova_sessao(self) -> Sessao:
        if len(self.sessoes) + self._criando >= self.cfg.max_sessoes:
            ids = ", ".join(sorted(self.sessoes))
            raise ToolError(
                f"limite de {self.cfg.max_sessoes} sessão(ões) ao mesmo tempo (abertas: {ids}). Use uma "
                "delas (passe `sessao`) ou feche uma com fechar."
            )
        self._criando += 1  # a vaga fica reservada enquanto a sessão sobe
        try:
            return await self._criar_sessao()
        finally:
            self._criando -= 1

    async def _criar_sessao(self) -> Sessao:
        navegador = await self._lancar()
        filtro = Filtro(self.rede, "sessão")
        await filtro.iniciar()
        try:
            contexto = await navegador.new_context(
                proxy={"server": filtro.endereco},
                accept_downloads=False,
                service_workers="block",
                java_script_enabled=True,
                viewport=VIEWPORT,
                locale="pt-BR",
                permissions=[],
            )
        except ErroPlaywright as e:
            await filtro.fechar()
            raise ToolError(f"não consegui abrir uma sessão: {_primeira_linha(e)}") from None
        sessao = Sessao(
            id="s" + secrets.token_hex(3),
            contexto=contexto,
            filtro=filtro,
            atributo="data-abiyss-" + secrets.token_hex(4),
        )
        contexto.set_default_timeout(self.cfg.timeout_acao * 1000)
        contexto.set_default_navigation_timeout(self.cfg.timeout_carregamento * 1000)
        await contexto.add_init_script(INICIO_DA_PAGINA % ("true" if self.cfg.interagir else "false"))
        await contexto.route("**/*", lambda route, pedido: self._interceptar(sessao, route, pedido))
        contexto.on("page", lambda pagina: self._aba_nova(sessao, pagina))
        pagina = await contexto.new_page()
        if pagina not in sessao.abas:
            self._aba_nova(sessao, pagina)
        self.sessoes[sessao.id] = sessao
        log.info("sessão %s aberta (%d de %d)", sessao.id, len(self.sessoes), self.cfg.max_sessoes)
        return sessao

    def _aba_nova(self, sessao: Sessao, pagina) -> None:
        if pagina in sessao.abas:
            return
        if len(sessao.abas) >= self.cfg.max_paginas:
            sessao.avisar(
                f"a página tentou abrir outra aba ({pagina.url[:100] or 'em branco'}); recusado: limite de "
                f"{self.cfg.max_paginas} aba(s) por sessão (NAVEGADOR_MAX_PAGINAS)"
            )
            asyncio.ensure_future(_sem_erro(pagina.close(), 5))
            return
        sessao.abas.append(pagina)
        pagina.on("dialog", lambda dialogo: self._dialogo(sessao, dialogo))
        pagina.on("download", lambda d: sessao.avisar("a página tentou baixar um arquivo; downloads estão desligados"))
        pagina.on("close", lambda p: self._aba_fechou(sessao, p))
        if len(sessao.abas) > 1:
            sessao.atual = len(sessao.abas) - 1
            sessao.avisar(f"abriu uma aba nova (aba {len(sessao.abas)}), que passou a ser a atual")

    def _aba_fechou(self, sessao: Sessao, pagina) -> None:
        if pagina in sessao.abas:
            indice = sessao.abas.index(pagina)
            sessao.abas.remove(pagina)
            if sessao.atual >= indice and sessao.atual > 0:
                sessao.atual -= 1

    def _dialogo(self, sessao: Sessao, dialogo) -> None:
        sessao.avisar(f"diálogo {dialogo.type} dispensado: {dialogo.message[:200]!r}")
        asyncio.ensure_future(_sem_erro(dialogo.dismiss(), 5))

    async def _interceptar(self, sessao: Sessao, route, pedido) -> None:
        try:
            try:
                url_da_pagina = pedido.frame.url
            except ErroPlaywright:  # service worker: sem quadro
                url_da_pagina = ""
            await self.rede.conferir_pedido(pedido.url)
            motivo = rede.politica(
                pedido.method, pedido.url, url_da_pagina, pedido.is_navigation_request(), self.cfg.interagir
            )
            if motivo:
                raise Bloqueio(motivo)
        except Bloqueio as e:
            sessao.bloquear(f"{pedido.method} {pedido.url}", str(e))
            log.info("sessão %s: bloqueado %s %s: %s", sessao.id, pedido.method, pedido.url[:200], e)
            await _sem_erro(route.abort("blockedbyclient"), 5)
            return
        except Exception as e:  # nunca deixa um pedido pendurado
            log.warning("interceptação falhou (%s): recusando %s", e, pedido.url[:200])
            await _sem_erro(route.abort("failed"), 5)
            return
        await _sem_erro(route.continue_(), 10)

    def pegar(self, sessao_id: str) -> Sessao:
        sessao = self.sessoes.get((sessao_id or "").strip())
        if sessao is None:
            motivo = self.encerradas.get((sessao_id or "").strip())
            if motivo:
                raise ToolError(f"a sessão {sessao_id} foi encerrada: {motivo}. Abra outra com navegar (sem sessao).")
            raise ToolError(
                f"não existe a sessão {sessao_id!r}. Abra uma com navegar (sem sessao); abertas: "
                f"{', '.join(sorted(self.sessoes)) or 'nenhuma'}"
            )
        return sessao

    async def fechar_sessao(self, sessao: Sessao, motivo: str = "fechada a pedido") -> None:
        if self.sessoes.get(sessao.id) is sessao:  # já encerrada: fica o primeiro motivo
            self._esquecer(sessao, motivo)
        try:
            await asyncio.wait_for(sessao.contexto.close(), 5)
        except Exception:
            self.matar_chromium(f"a sessão {sessao.id} não fechou a tempo; o Chromium foi reiniciado")
        await sessao.filtro.fechar()
        log.info("sessão %s fechada: %s", sessao.id, motivo)

    async def executar(
        self, sessao_id: str, operacao: Callable[[Sessao, float], Awaitable[Any]], criar: bool = False
    ) -> Any:
        """Roda a operação com a trava da sessão e o prazo da chamada. Passou
        do prazo: a sessão é fechada (e, se nem isso responder, o Chromium
        inteiro é morto)."""
        prazo = asyncio.get_running_loop().time() + self.cfg.prazo_segundos
        sessao: Sessao | None = None
        try:
            async with asyncio.timeout_at(prazo):
                sessao = await self.nova_sessao() if criar else self.pegar(sessao_id)
                async with sessao.trava:
                    sessao.ultimo_uso = time.monotonic()
                    try:
                        return await operacao(sessao, prazo)
                    finally:
                        sessao.ultimo_uso = time.monotonic()
        except TimeoutError:
            aberta = sessao is not None and self.sessoes.get(sessao.id) is sessao
            alvo = f"a sessão {sessao.id} foi fechada" if aberta else "nada ficou aberto"
            if aberta:
                await self.fechar_sessao(sessao, f"uma chamada passou do prazo de {self.cfg.prazo_segundos} s")
            raise ToolError(
                f"a operação passou do prazo de {self.cfg.prazo_segundos} s (NAVEGADOR_PRAZO_SEGUNDOS); {alvo}. "
                "A página pode estar travada (JavaScript em laço) ou lenta demais."
            ) from None
        except ErroPlaywright as e:
            if self._navegador is None or not self._navegador.is_connected():
                raise ToolError(
                    "o Chromium caiu no meio da operação (memória? travou?); as sessões foram fechadas. "
                    "Abra outra com navegar."
                ) from None
            raise ToolError(_explicar(e, self.cfg)) from None

    # -- operações (rodam dentro de executar) --------------------------------

    async def assentar(self, pagina, prazo: float) -> None:
        """Depois de carregar: espera o load e um respiro da rede (para o
        JavaScript montar a página), sem passar do prazo."""
        laco = asyncio.get_running_loop()
        for estado, maximo in (("load", 5.0), ("networkidle", 2.0)):
            restante = min(maximo, prazo - laco.time() - 8)
            if restante <= 0:
                return
            with contextlib.suppress(ErroPlaywright):
                await pagina.wait_for_load_state(estado, timeout=restante * 1000)

    async def ir(self, sessao: Sessao, url: str, prazo: float) -> list[str]:
        """Abre a URL na aba atual. Devolve avisos do carregamento."""
        try:
            url = rede.validar_url(url)
            await self.rede.conferir_pedido(url)
        except Bloqueio as e:
            raise ToolError(str(e)) from None
        pagina = sessao.pagina if sessao.abas else await sessao.contexto.new_page()
        avisos, antes = [], pagina.url
        try:
            resposta = await pagina.goto(
                url, wait_until="domcontentloaded", timeout=self.cfg.timeout_carregamento * 1000
            )
            if resposta is not None and resposta.status == 403 and await resposta.header_value(rede.CABECALHO_BLOQUEIO):
                motivo = sessao.filtro.bloqueios[-1].motivo if sessao.filtro.bloqueios else "endereço não permitido"
                sessao.filtro_relatado = sessao.filtro.total_bloqueados
                raise ToolError(f"bloqueado pelo filtro de rede (redirecionamento?) ao abrir {url[:200]}: {motivo}")
        except TempoPlaywright:
            if pagina.url == antes:
                # A navegação ficou pendurada: para ela, senão a aba fica
                # "carregando" e nada mais responde nela.
                with contextlib.suppress(Exception):
                    cdp = await sessao.contexto.new_cdp_session(pagina)
                    await asyncio.wait_for(cdp.send("Page.stopLoading"), 3)
                    await asyncio.wait_for(cdp.detach(), 3)
                raise ToolError(
                    f"{url[:200]} não respondeu em {self.cfg.timeout_carregamento} s "
                    "(NAVEGADOR_TIMEOUT_CARREGAMENTO); a página continua a anterior"
                ) from None
            avisos.append(
                f"a página não terminou de carregar em {self.cfg.timeout_carregamento} s "
                "(NAVEGADOR_TIMEOUT_CARREGAMENTO); abaixo, o que deu para ler"
            )
        except ErroPlaywright as e:
            raise ToolError(self.explicar_navegacao(sessao, url, e)) from None
        await self.assentar(pagina, prazo)
        return avisos

    def explicar_navegacao(self, sessao: Sessao, url: str, e: Exception) -> str:
        texto = str(e)
        if "ERR_BLOCKED_BY_CLIENT" in texto and sessao.bloqueios:
            ultimo = sessao.bloqueios[-1]
            sessao.novos_bloqueios = 0
            return f"bloqueado: {ultimo}"
        if "ERR_TUNNEL_CONNECTION_FAILED" in texto or "ERR_PROXY" in texto or "ERR_BLOCKED_BY_CLIENT" in texto:
            registro = sessao.filtro.bloqueios[-1].motivo if sessao.filtro.bloqueios else "endereço não permitido"
            return f"bloqueado pelo filtro de rede ao abrir {url[:200]}: {registro}"
        if "Download is starting" in texto or "net::ERR_ABORTED" in texto and "download" in texto.lower():
            return f"{url[:200]} é um download; downloads estão desligados no navegador"
        return f"não consegui abrir {url[:200]}: {_explicar(e, self.cfg)}"

    async def instantaneo(self, sessao: Sessao, prazo: float) -> instantaneo.Instantaneo:
        pagina = sessao.pagina
        laco = asyncio.get_running_loop()
        # Folga para fechar a aba se a página estiver travada.
        limite = min(prazo - 5, laco.time() + 15)
        try:
            inst = await instantaneo.tirar(pagina, sessao.atributo, sessao.refs, limite)
        except asyncio.TimeoutError:
            await _sem_erro(pagina.close(run_before_unload=False), 3)
            raise ToolError(
                "a página não respondeu à leitura (JavaScript travado?); a aba foi fechada. "
                + ("Use abas para ver as outras." if sessao.abas else "Abra outra página com navegar.")
            ) from None
        if inst.quadros == 0 and inst.erros:
            await _sem_erro(pagina.close(run_before_unload=False), 3)
            raise ToolError(f"não consegui ler a página ({inst.erros[0]}); a aba foi fechada")
        return inst

    async def responder_com_pagina(self, sessao: Sessao, prazo: float, avisos: list[str] | None = None) -> dict:
        inst = await self.instantaneo(sessao, prazo)
        sessao.ultimo_texto = inst.texto
        sessao.ultima_url = sessao.pagina.url
        sessao.ultimo_titulo = inst.titulo
        cabecalho = self.cabecalho(sessao, inst.titulo)
        notas = list(avisos or []) + [f"aviso: {e}" for e in inst.erros] + sessao.notas()
        if inst.cortado_na_pagina:
            notas.append(
                f"aviso: a página é grande demais; li só os primeiros {instantaneo.MAX_NOS} elementos de cada quadro"
            )
        return self.montar(sessao, cabecalho, notas, inst.texto, 0, inst.titulo)

    def cabecalho(self, sessao: Sessao, titulo: str) -> list[str]:
        modo = "interagir" if self.cfg.interagir else "só leitura"
        return [
            f"Sessão {sessao.id} · aba {sessao.atual + 1} de {len(sessao.abas)} · modo {modo}",
            f"URL: {sessao.pagina.url}",
            f"Título: {titulo or '(sem título)'}",
        ]

    def montar(self, sessao: Sessao, cabecalho: list[str], notas: list[str], texto: str, inicio: int, titulo: str) -> dict:
        pedaco, fim = instantaneo.fatia(texto, inicio, self.cfg.max_texto_bytes)
        linhas = cabecalho + notas
        if inicio:
            linhas.append(f"Instantâneo: continuando de {inicio} (de {len(texto)} caracteres)")
        linhas.append("---")
        linhas.append(pedaco.rstrip("\n") if pedaco.strip() else "(a página não tem texto visível)")
        if fim < len(texto):
            linhas.append(
                f"[… instantâneo cortado pelo navegador: faltam {len(texto) - fim} caracteres. Para continuar, "
                f"chame ler com sessao={sessao.id} e inicio={fim} …]"
            )
        return {
            "texto": "\n".join(linhas) + "\n",
            "estruturado": {
                "sessao": sessao.id,
                "url": sessao.pagina.url if sessao.abas else "",
                "titulo": titulo,
                "aba": sessao.atual + 1,
                "abas": len(sessao.abas),
                "interagir": self.cfg.interagir,
                "total_caracteres": len(texto),
                "inicio": inicio,
                "fim": fim,
            },
        }

    async def achar(self, sessao: Sessao, ref: str):
        ref = (ref or "").strip()
        if not REF.match(ref):
            raise ToolError(f"referência inválida: {ref!r} (use o que aparece no instantâneo, ex.: e12)")
        achados = []
        for indice, quadro in enumerate(sessao.pagina.frames[: instantaneo.MAX_QUADROS]):
            if indice and not quadro.url:
                continue  # quadro sem documento (carregamento recusado)
            localizador = quadro.locator(f'[{sessao.atributo}="{ref}"]')
            with contextlib.suppress(ErroPlaywright, asyncio.TimeoutError):
                quantos = await asyncio.wait_for(localizador.count(), instantaneo.PRAZO_QUADRO_FILHO)
                achados.extend([localizador] * quantos)
        if not achados:
            raise ToolError(
                f"a referência {ref} não existe nesta página (a página mudou ou é de outra aba?); chame ler de novo"
            )
        if len(achados) > 1:
            raise ToolError(f"a referência {ref} aparece {len(achados)} vezes (a página copiou a marca); chame ler de novo")
        return achados[0]

    async def capturar(self, sessao: Sessao, pagina_inteira: bool) -> tuple[Path, int, int]:
        pasta = self.cfg.pasta_capturas
        workspace = self.cfg.workspace.resolve()
        if pasta.is_symlink():
            raise ToolError("workspace/navegador é um link simbólico: recusado (as capturas ficam só no workspace)")
        pasta.mkdir(parents=True, exist_ok=True)
        if not pasta.resolve().is_relative_to(workspace):
            raise ToolError("a pasta das capturas saiu do workspace: recusado")
        sessao.capturas += 1
        destino = pasta / f"{sessao.id}-{sessao.capturas:03d}.png"
        pagina = sessao.pagina
        opcoes: dict[str, Any] = {"type": "png", "timeout": self.cfg.timeout_acao * 1000}
        if pagina_inteira:
            altura = await pagina.evaluate("() => document.documentElement.scrollHeight")
            opcoes.update(
                full_page=True,
                clip={"x": 0, "y": 0, "width": VIEWPORT["width"], "height": min(int(altura), MAX_ALTURA_CAPTURA)},
            )
        imagem = await pagina.screenshot(**opcoes)
        # Pela pasta já aberta (sem seguir link): trocar a pasta ou o arquivo
        # por um link depois da conferência não muda onde a imagem vai parar.
        pasta_fd = os.open(pasta, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            descritor = os.open(
                destino.name, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o644, dir_fd=pasta_fd
            )
        finally:
            os.close(pasta_fd)
        with os.fdopen(descritor, "wb") as arquivo:
            arquivo.write(imagem)
        self._podar_capturas(pasta)
        largura = int.from_bytes(imagem[16:20], "big")
        altura = int.from_bytes(imagem[20:24], "big")
        return destino, largura, altura

    def _podar_capturas(self, pasta: Path) -> None:
        capturas = sorted(
            (p for p in pasta.glob("*.png") if p.is_file() and not p.is_symlink()), key=lambda p: p.stat().st_mtime
        )
        for velha in capturas[: max(0, len(capturas) - self.cfg.max_capturas)]:
            with contextlib.suppress(OSError):
                velha.unlink()


async def _sem_erro(coro, segundos: float) -> None:
    with contextlib.suppress(Exception):
        await asyncio.wait_for(coro, segundos)


def _primeira_linha(e: BaseException) -> str:
    texto = str(e).strip()
    linhas = [l for l in texto.splitlines() if l.strip() and not set(l.strip()) <= set("=╔╗╚╝║ ")]
    return (linhas[0] if linhas else type(e).__name__)[:300]


def _erro_de_lancamento(motivo: str) -> str:
    if "Executable doesn't exist" in motivo or "playwright install" in motivo:
        return (
            "o Chromium do Playwright não está instalado. Na VM: cd recursos/mcp/navegador && "
            "uv run --frozen --no-dev playwright install chromium (veja o README do navegador)"
        )
    return f"o Chromium não subiu: {motivo}"


def _explicar(e: BaseException, cfg: config.Config) -> str:
    texto = _primeira_linha(e)
    if isinstance(e, TempoPlaywright):
        return f"passou do tempo ({texto})"
    return texto
