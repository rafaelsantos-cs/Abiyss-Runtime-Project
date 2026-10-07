"""Servidor MCP `ambiente` do Abiyss: como está a máquina, só leitura.

Serviços do systemd (só os da lista AMBIENTE_SERVICOS), o diário deles,
disco, memória, processos que mais usam memória e portas abertas. Nada aqui
muda o sistema, nada usa sudo, e o servidor não roda como root.

O kernel sobe este arquivo como processo filho e conversa por stdio: NUNCA
use print() para stdout aqui; logs vão para o stderr.
"""

import logging
import os
import sys
from typing import Annotated, Any

import mcp.types as types
from mcp.server import MCPServer
from mcp.server.mcpserver.exceptions import ToolError
from pydantic import Field

import config
import sistema

log = logging.getLogger("ambiente")

MAX_PROCESSOS = 50


def tamanho(n: int | None) -> str:
    if n is None:
        return "?"
    for unidade, fator in (("GiB", 1024**3), ("MiB", 1024**2), ("KiB", 1024)):
        if n >= fator:
            return f"{n / fator:.1f} {unidade}".replace(".", ",")
    return f"{n} B"


def decimal(x: float, casas: int = 1) -> str:
    return f"{x:.{casas}f}".replace(".", ",")


def duracao(segundos: float) -> str:
    dias, resto = divmod(int(segundos), 86400)
    horas, resto = divmod(resto, 3600)
    return (f"{dias} d " if dias else "") + f"{horas} h {resto // 60} min"


def resultado(texto: str, estruturado: dict[str, Any]) -> types.CallToolResult:
    return types.CallToolResult(
        content=[types.TextContent(type="text", text=texto)], structured_content=estruturado
    )


def caber(linhas: list[str], max_bytes: int, do_fim: bool = False) -> str:
    """Junta as linhas sem passar de max_bytes; corta do começo (do_fim:
    guarda as mais novas, para o diário) ou do fim, com aviso."""
    total, escolhidas = 0, []
    for linha in reversed(linhas) if do_fim else linhas:
        tamanho_linha = len(linha.encode("utf-8")) + 1
        if total + tamanho_linha > max_bytes:
            break
        escolhidas.append(linha)
        total += tamanho_linha
    faltam = len(linhas) - len(escolhidas)
    if do_fim:
        escolhidas.reverse()
        if faltam:
            escolhidas.insert(0, f"[… {faltam} linha(s) mais antiga(s) cortada(s) pelo ambiente …]")
    elif faltam:
        escolhidas.append(f"[… {faltam} linha(s) cortada(s) pelo ambiente …]")
    return "\n".join(escolhidas)


class Ambiente:
    def __init__(self, cfg: config.Config):
        self.cfg = cfg

    def _unidade(self, nome: str) -> str:
        nome = nome.strip()
        candidatos = [nome] if nome.endswith(config.TIPOS_DE_UNIDADE) else [nome, nome + ".service"]
        for candidato in candidatos:
            if candidato in self.cfg.servicos:
                return candidato
        raise ToolError(
            f"o serviço {nome!r} não está na lista que o Abiyss pode ver ({', '.join(self.cfg.servicos)}); "
            "o dono muda a lista em AMBIENTE_SERVICOS"
        )

    async def servicos(self) -> tuple[list[str], list[dict]]:
        try:
            estados = await sistema.estado_dos_servicos(self.cfg.systemctl, self.cfg.servicos, self.cfg.timeout_comando)
        except sistema.ErroSistema as e:
            raise ToolError(str(e)) from None
        linhas = []
        for s in estados:
            if s["carregada"] == "not-found":
                linhas.append(f"- {s['unidade']}: NÃO EXISTE nesta máquina")
                continue
            partes = [f"{s['ativa']} ({s['sub']})"]
            if s["habilitada"]:
                partes.append(s["habilitada"])
            if s["desde"]:
                partes.append(f"desde {s['desde']}")
            if s["pid"]:
                partes.append(f"pid {s['pid']}")
            if s["memoria"] is not None:
                partes.append(f"memória {tamanho(s['memoria'])}")
            if s["reinicios"]:
                partes.append(f"{s['reinicios']} reinício(s)")
            if s["resultado"] and s["resultado"] != "success":
                partes.append(f"último resultado: {s['resultado']}")
            alerta = " ⚠" if s["ativa"] in ("failed", "inactive") else ""
            linhas.append(f"- {s['unidade']}{alerta}: {', '.join(partes)}" + (f" ({s['descricao']})" if s["descricao"] else ""))
        return linhas, estados

    def disco(self) -> tuple[list[str], list[dict]]:
        lista = sistema.discos(self.cfg.raiz)
        linhas = []
        for d in lista:
            alerta = " ⚠ quase cheio" if d["uso_pct"] >= 90 else ""
            inodes = f", inodes {decimal(d['inodes_uso_pct'])}%" if d["inodes_uso_pct"] is not None and d["inodes_uso_pct"] >= 50 else ""
            projeto = " [projeto do Abiyss]" if d["projeto"] else ""
            linhas.append(
                f"- {d['ponto']} ({d['tipo']}){projeto}: {decimal(d['uso_pct'])}% usado, {tamanho(d['livre'])} livres "
                f"de {tamanho(d['total'])}{inodes}{alerta}"
            )
        return linhas, lista

    def memoria_e_carga(self) -> tuple[list[str], dict]:
        m, c = sistema.memoria(), sistema.carga()
        usado = m["total"] - m["disponivel"]
        linhas = [
            f"Memória: {tamanho(usado)} em uso de {tamanho(m['total'])} ({tamanho(m['disponivel'])} disponíveis)"
            + (f"; swap {tamanho(m['swap_total'] - m['swap_livre'])} de {tamanho(m['swap_total'])}" if m["swap_total"] else "; sem swap"),
            f"Carga: {decimal(c['1min'], 2)} {decimal(c['5min'], 2)} {decimal(c['15min'], 2)} ({c['cpus']} CPUs); "
            f"ligada há {duracao(c['ligado_segundos'])}",
        ]
        return linhas, {"memoria": m, "carga": c}

    def processos(self, quantos: int) -> tuple[list[str], list[dict]]:
        total = sistema.memoria()["total"] or 1
        lista = sistema.processos(quantos)
        linhas = [
            f"- {p.pid} {p.usuario}: {tamanho(p.rss)} ({decimal(100 * p.rss / total)}%), {p.threads} thread(s), "
            f"{p.estado}: {p.comando}"
            for p in lista
        ]
        return linhas, [p.__dict__ for p in lista]

    def portas(self) -> tuple[list[str], list[dict]]:
        lista = sistema.portas()
        linhas = []
        for p in lista:
            dono = f"{p['processo']} (pid {p['pid']})" if p["pid"] else "processo não visível sem root"
            endereco = f"[{p['endereco']}]" if ":" in p["endereco"] else p["endereco"]
            linhas.append(f"- {p['protocolo']} {endereco}:{p['porta']} ({p['alcance']}): {dono}, usuário {p['usuario']}")
        return linhas, lista


def criar_servidor(cfg: config.Config) -> MCPServer:
    amb = Ambiente(cfg)
    servidor = MCPServer("ambiente")
    lista = ", ".join(cfg.servicos)

    async def resumo() -> types.CallToolResult:
        memoria, dados_memoria = amb.memoria_e_carga()
        try:
            servicos, dados_servicos = await amb.servicos()
        except ToolError as e:
            servicos, dados_servicos = [f"(não consegui ler: {e})"], []
        disco, dados_disco = amb.disco()
        processos, dados_processos = amb.processos(5)
        portas, dados_portas = amb.portas()
        partes = memoria + ["", "Serviços:", *servicos, "", "Disco:", *disco, "", "Processos que mais usam memória:", *processos]
        partes += ["", f"Portas abertas ({len(dados_portas)}):", *portas]
        return resultado(
            caber(partes, cfg.max_bytes) + "\n",
            {**dados_memoria, "servicos": dados_servicos, "discos": dados_disco, "processos": dados_processos, "portas": dados_portas},
        )

    async def servicos() -> types.CallToolResult:
        linhas, dados = await amb.servicos()
        return resultado(caber(linhas, cfg.max_bytes) + "\n", {"servicos": dados})

    async def diario(
        servico: Annotated[str, Field(description=f"Um destes: {lista}.")],
        linhas: Annotated[int, Field(description=f"Quantas linhas, das mais novas (1 a {cfg.max_linhas_diario}).")] = 50,
        prioridade: Annotated[
            str, Field(description="Só desta gravidade para cima: err, warning, notice, info... Vazio = todas.")
        ] = "",
    ) -> types.CallToolResult:
        unidade = amb._unidade(servico)
        linhas = max(1, min(cfg.max_linhas_diario, linhas))
        prioridade = prioridade.strip().lower()
        if prioridade and prioridade not in sistema.PRIORIDADES:
            raise ToolError(f"prioridade {prioridade!r} inválida: use {', '.join(sistema.PRIORIDADES)}")
        try:
            registros, aviso = await sistema.diario(cfg.journalctl, unidade, linhas, prioridade, cfg.timeout_comando)
        except sistema.ErroSistema as e:
            raise ToolError(str(e)) from None
        cabecalho = f"Diário de {unidade}: {len(registros)} linha(s) mais recente(s)" + (f", prioridade {prioridade}+" if prioridade else "")
        if aviso:
            cabecalho += f"\naviso: {aviso}"
        corpo = caber(registros, cfg.max_bytes, do_fim=True) if registros else "(nenhuma linha)"
        return resultado(f"{cabecalho}\n{corpo}\n", {"servico": unidade, "linhas": registros, "aviso": aviso})

    async def disco() -> types.CallToolResult:
        linhas, dados = amb.disco()
        return resultado(caber(linhas, cfg.max_bytes) + "\n", {"discos": dados})

    async def processos(
        quantos: Annotated[int, Field(description=f"Quantos processos (1 a {MAX_PROCESSOS}).")] = 10,
    ) -> types.CallToolResult:
        memoria, dados_memoria = amb.memoria_e_carga()
        linhas, dados = amb.processos(max(1, min(MAX_PROCESSOS, quantos)))
        return resultado(caber(memoria + ["", *linhas], cfg.max_bytes) + "\n", {**dados_memoria, "processos": dados})

    async def portas() -> types.CallToolResult:
        linhas, dados = amb.portas()
        return resultado(caber(linhas or ["(nenhuma porta aberta)"], cfg.max_bytes) + "\n", {"portas": dados})

    descricoes = {
        "resumo": "Visão geral da máquina numa chamada: memória, carga, serviços, disco, os 5 processos que mais usam memória e portas abertas. Só leitura.",
        "servicos": f"Estado dos serviços do systemd que o Abiyss acompanha ({lista}): ativo ou não, desde quando, reinícios, memória. Só leitura.",
        "diario": f"Linhas mais recentes do diário (journalctl) de um serviço da lista ({lista}). Só leitura.",
        "disco": "Uso de cada disco (sistema de arquivos de verdade): espaço livre e % usado; marca o do projeto do Abiyss. Só leitura.",
        "processos": "Memória e carga da máquina e os processos que mais usam memória (linha de comando com senhas e tokens ocultos). Só leitura.",
        "portas": "Portas TCP em escuta e UDP abertas, com o alcance (só local ou todas as interfaces) e o processo dono quando visível. Só leitura.",
    }
    for funcao in (resumo, servicos, diario, disco, processos, portas):
        servidor.add_tool(funcao, name=funcao.__name__, description=descricoes[funcao.__name__])
    return servidor


def verificar_usuario() -> None:
    if os.geteuid() == 0:
        raise config.ErroConfig(
            "o ambiente não roda como root: ele só lê, e um usuário comum basta (o deploy/abiyss.service "
            "usa User=ubuntu; para o diário, o usuário precisa do grupo adm ou systemd-journal)"
        )


def main() -> int:
    logging.basicConfig(level=logging.INFO, stream=sys.stderr, format="ambiente: %(levelname)s %(message)s")
    try:
        verificar_usuario()
        cfg = config.carregar()
    except config.ErroConfig as e:
        print(f"ambiente: NÃO vou subir: {e}", file=sys.stderr, flush=True)
        return 2
    for aviso in cfg.avisos:
        log.warning(aviso)
    for nome, caminho in (("systemctl", cfg.systemctl), ("journalctl", cfg.journalctl)):
        if not caminho:
            log.warning("%s não encontrado: as ferramentas de serviços/diário vão avisar", nome)
    log.info("pronto: serviços %s", ", ".join(cfg.servicos))
    criar_servidor(cfg).run()  # transporte padrão: stdio
    return 0


if __name__ == "__main__":
    sys.exit(main())
