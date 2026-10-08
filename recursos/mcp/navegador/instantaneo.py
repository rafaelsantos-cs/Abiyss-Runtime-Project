"""O instantâneo de uma página em TEXTO, com referências estáveis.

O modelo pode não ver imagens: a página vira linhas de texto, na ordem do
documento (o texto visível, os títulos, as imagens com descrição) e cada
elemento com que se interage ganha uma referência:

    # Título da página
    Um parágrafo de texto...
    [link e3] Sobre nós → https://site.com/sobre
    [campo e4] Buscar (vazio)
    [botão e5] Enviar (envia formulário)

A referência fica gravada no próprio elemento (num atributo com nome
sorteado por sessão), então o mesmo elemento mantém a mesma referência nos
instantâneos seguintes, enquanto o documento existir. A numeração é da
sessão inteira e só cresce: uma referência nunca é reaproveitada para outro
elemento, nem depois de navegar. clicar/digitar acham o elemento pelo
atributo e exigem exatamente um.

Limites: no máximo MAX_NOS elementos visitados por quadro e o texto
cortado em NAVEGADOR_MAX_TEXTO_BYTES, com um aviso claro de corte e de como
continuar (``ler`` com ``inicio``).
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field

MAX_NOS = 20_000
MAX_LINHAS = 5_000
MAX_QUADROS = 8
MAX_CARACTERES_POR_QUADRO = 400_000
# Quadro filho que não responde nisso fica de fora (o principal usa o prazo todo).
PRAZO_QUADRO_FILHO = 3.0

JS = r"""
(opcoes) => {
  const { atributo, proximo, maxNos, maxLinhas, maxCaracteres } = opcoes;
  let ref = proximo, nos = 0, caracteres = 0, cortado = false, texto = "";
  const linhas = [];
  const PULAR = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "HEAD", "META", "LINK", "SVG", "CANVAS",
    "OBJECT", "EMBED", "AUDIO", "VIDEO", "SOURCE", "TRACK", "MAP", "DATALIST", "OPTION", "OPTGROUP"]);
  const BLOCOS = new Set(["ADDRESS", "ARTICLE", "ASIDE", "BLOCKQUOTE", "BR", "CAPTION", "DD", "DETAILS", "DIALOG",
    "DIV", "DL", "DT", "FIELDSET", "FIGCAPTION", "FIGURE", "FOOTER", "FORM", "HEADER", "HR", "LEGEND", "LI", "MAIN",
    "NAV", "OL", "P", "PRE", "SECTION", "TABLE", "TBODY", "TFOOT", "THEAD", "TR", "UL"]);
  const ROTULOS = { link: "link", button: "botão", checkbox: "caixa", radio: "opção", switch: "chave", tab: "aba",
    menuitem: "menu", menuitemcheckbox: "menu", menuitemradio: "menu", option: "item", combobox: "seleção",
    listbox: "lista", textbox: "campo", searchbox: "campo", spinbutton: "campo", slider: "controle",
    treeitem: "item", file: "arquivo", clicavel: "clicável" };
  const limpo = (s) => (s || "").replace(/\s+/g, " ").trim();
  const curto = (s, n) => (s.length > n ? s.slice(0, n - 1) + "…" : s);

  function emitir(linha) {
    if (cortado) return;
    if (linhas.length >= maxLinhas || caracteres + linha.length > maxCaracteres) { cortado = true; return; }
    linhas.push(linha);
    caracteres += linha.length + 1;
  }
  function descarregar() {
    const t = limpo(texto);
    texto = "";
    if (t && t !== "•") emitir(t);
  }
  function visivel(el) {
    if (el.getAttribute("aria-hidden") === "true") return false;
    if (typeof el.checkVisibility === "function") {
      if (el.checkVisibility({ checkVisibilityCSS: true, visibilityProperty: true })) return true;
      return getComputedStyle(el).display === "contents";
    }
    return el.getClientRects().length > 0;
  }
  function papel(el) {
    const explicito = (el.getAttribute("role") || "").trim().split(/\s+/)[0].toLowerCase();
    if (explicito && ROTULOS[explicito]) return explicito;
    const t = el.tagName;
    if (t === "A" || t === "AREA") return el.hasAttribute("href") ? "link" : "";
    if (t === "BUTTON" || t === "SUMMARY") return "button";
    if (t === "SELECT") return "combobox";
    if (t === "TEXTAREA") return "textbox";
    if (t === "INPUT") {
      const tipo = (el.getAttribute("type") || "text").toLowerCase();
      if (["button", "submit", "reset", "image"].includes(tipo)) return "button";
      if (tipo === "checkbox" || tipo === "radio") return tipo;
      if (tipo === "range") return "slider";
      if (tipo === "file") return "file";
      if (tipo === "hidden") return "";
      return "textbox";
    }
    if (el.isContentEditable && !(el.parentElement && el.parentElement.isContentEditable)) return "textbox";
    if (el.hasAttribute("onclick")) return "clicavel";
    return "";
  }
  function nome(el, r) {
    let n = el.getAttribute("aria-label") || "";
    if (!n && el.getAttribute("aria-labelledby")) {
      n = el.getAttribute("aria-labelledby").split(/\s+/)
        .map((id) => (document.getElementById(id) || {}).innerText || "").join(" ");
    }
    if (!n && el.labels && el.labels.length) n = Array.from(el.labels).map((l) => l.innerText).join(" ");
    const campo = r === "textbox" || r === "combobox" || r === "slider" || r === "file";
    if (!n && campo) n = el.getAttribute("placeholder") || el.getAttribute("title") || el.getAttribute("name") || "";
    if (!n && el.tagName === "INPUT" && r === "button") n = el.getAttribute("type") === "image" ? el.getAttribute("alt") || "" : el.value || "";
    if (!n && !campo) n = el.innerText || el.textContent || "";
    if (!n) { const img = el.querySelector && el.querySelector("img[alt]"); if (img) n = img.getAttribute("alt"); }
    if (!n) n = el.getAttribute("title") || "";
    return curto(limpo(n), 150);
  }
  function referencia(el) {
    let id = el.getAttribute(atributo);
    if (!id) { id = "e" + ref++; el.setAttribute(atributo, id); }
    return id;
  }
  function interativo(el, r) {
    descarregar();
    let linha = `[${ROTULOS[r]} ${referencia(el)}] ${nome(el, r)}`;
    if (r === "link" && el.href && !String(el.href).startsWith("javascript:")) linha += ` → ${curto(String(el.href), 200)}`;
    if (r === "textbox" || r === "searchbox" || r === "spinbutton") {
      if ((el.getAttribute("type") || "").toLowerCase() === "password") linha += " (senha)";
      else {
        const valor = el.isContentEditable ? el.innerText : el.value;
        linha += valor ? ` = "${curto(limpo(valor), 120)}"` : " (vazio)";
      }
    }
    if (r === "checkbox" || r === "radio" || r === "switch" || r === "menuitemcheckbox" || r === "menuitemradio") {
      const marcado = el.checked === true || el.getAttribute("aria-checked") === "true";
      linha = linha.replace("] ", marcado ? "] ☑ " : "] ☐ ");
    }
    if (el.tagName === "SELECT") {
      const opcoes = Array.from(el.options || []);
      const escolhida = opcoes.filter((o) => o.selected).map((o) => limpo(o.text)).join(", ");
      linha += ` = "${curto(escolhida, 80)}"`;
      const nomes = opcoes.slice(0, 12).map((o) => curto(limpo(o.text), 40));
      linha += ` (opções: ${nomes.join(" | ")}${opcoes.length > 12 ? ` | … mais ${opcoes.length - 12}` : ""})`;
    }
    if (el.getAttribute("aria-expanded")) linha += el.getAttribute("aria-expanded") === "true" ? " (aberto)" : " (fechado)";
    const tipo = (el.getAttribute("type") || "").toLowerCase();
    if (el.form && (el.tagName === "BUTTON" ? tipo === "" || tipo === "submit" : tipo === "submit" || tipo === "image")) {
      linha += " (envia formulário)";
    }
    if (el.disabled || el.getAttribute("aria-disabled") === "true") linha += " (desativado)";
    emitir(linha);
  }
  function filhos(no) {
    if (no.shadowRoot) return no.shadowRoot.childNodes;
    if (no.tagName === "SLOT") { const a = no.assignedNodes({ flatten: true }); return a.length ? a : no.childNodes; }
    return no.childNodes;
  }
  function andar(no) {
    if (cortado) return;
    if (no.nodeType === 3) { texto += no.data; return; }
    if (no.nodeType === 11) { for (const f of no.childNodes) andar(f); return; }
    if (no.nodeType !== 1) return;
    if (++nos > maxNos) { cortado = true; return; }
    const el = no, t = el.tagName;
    if (PULAR.has(t) || !visivel(el)) return;
    const r = papel(el);
    if (r) return interativo(el, r);
    if (/^H[1-6]$/.test(t)) {
      descarregar();
      const titulo = curto(limpo(el.innerText), 300);
      if (titulo) emitir("#".repeat(Number(t[1])) + " " + titulo);
      return;
    }
    if (t === "IMG") {
      const alt = limpo(el.getAttribute("alt"));
      if (alt) { descarregar(); emitir(`[imagem] ${curto(alt, 200)}`); }
      return;
    }
    if (t === "IFRAME" || t === "FRAME") {
      descarregar();
      emitir(`[quadro] ${curto(limpo(el.getAttribute("title") || el.getAttribute("src") || ""), 150)}`);
      return;
    }
    if (t === "TD" || t === "TH") { if (limpo(texto)) texto += " | "; }
    const bloco = BLOCOS.has(t);
    if (bloco) descarregar();
    if (t === "LI") texto = "• ";
    for (const f of filhos(el)) andar(f);
    if (bloco) descarregar();
  }
  if (document.body) andar(document.body);
  descarregar();
  return { linhas, proximo: ref, cortado, nos, titulo: curto(limpo(document.title), 300) };
}
"""


@dataclass
class Instantaneo:
    texto: str
    titulo: str
    cortado_na_pagina: bool
    quadros: int
    erros: list[str] = field(default_factory=list)


async def tirar(pagina, atributo: str, estado: dict, prazo: float) -> Instantaneo:
    """Instantâneo de todos os quadros (o principal primeiro). `estado`
    guarda o próximo número de referência da sessão ("proximo")."""
    partes: list[str] = []
    cortado, erros, quadros, titulo = False, [], 0, ""
    for indice, quadro in enumerate(pagina.frames[:MAX_QUADROS]):
        restante = prazo - asyncio.get_running_loop().time()
        if restante <= 0:
            erros.append("o prazo acabou antes de ler todos os quadros")
            break
        if indice > 0:
            if not quadro.url:
                continue  # carregamento recusado (file:, endereço interno...): não tem documento
            restante = min(restante, PRAZO_QUADRO_FILHO)
        opcoes = {
            "atributo": atributo,
            "proximo": estado["proximo"],
            "maxNos": MAX_NOS,
            "maxLinhas": MAX_LINHAS,
            "maxCaracteres": MAX_CARACTERES_POR_QUADRO,
        }
        try:
            r = await asyncio.wait_for(quadro.evaluate(JS, opcoes), restante)
        except asyncio.TimeoutError:
            if indice == 0:
                raise  # a página inteira travada: quem chamou fecha a aba
            erros.append(f"o quadro {quadro.url[:100] or '(sem URL)'} não respondeu a tempo")
            continue
        except Exception as e:  # quadro que sumiu no meio, página navegando...
            if indice == 0:
                raise
            erros.append(f"não consegui ler o quadro {quadro.url[:100]}: {str(e).splitlines()[0][:150]}")
            continue
        estado["proximo"] = max(estado["proximo"], int(r["proximo"]))
        cortado = cortado or bool(r["cortado"])
        if indice == 0:
            titulo = r["titulo"]
            partes.extend(r["linhas"])
        elif r["linhas"]:
            partes.append(f"--- quadro {indice}: {quadro.url[:150]} ---")
            partes.extend(r["linhas"])
        quadros += 1
    if len(pagina.frames) > MAX_QUADROS:
        erros.append(f"a página tem {len(pagina.frames)} quadros; li só os {MAX_QUADROS} primeiros")
    return Instantaneo(
        texto="\n".join(partes), titulo=titulo, cortado_na_pagina=cortado, quadros=quadros, erros=erros
    )


def cortar_bytes(texto: str, max_bytes: int) -> str:
    return texto.encode("utf-8")[:max_bytes].decode("utf-8", errors="ignore")


def fatia(texto: str, inicio: int, max_bytes: int) -> tuple[str, int]:
    """O pedaço que começa no caractere `inicio`, com até `max_bytes` em
    UTF-8, terminando de preferência numa quebra de linha. Devolve
    (pedaço, onde o próximo começa). Igual ao web_rapido."""
    pedaco = cortar_bytes(texto[inicio : inicio + max_bytes], max_bytes)
    if inicio + len(pedaco) < len(texto):
        quebra = pedaco.rfind("\n", int(len(pedaco) * 0.8))
        if quebra > 0:
            pedaco = pedaco[: quebra + 1]
    return pedaco, inicio + len(pedaco)
