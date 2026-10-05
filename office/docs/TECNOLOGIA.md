# Tecnologia: inspeção da VM, engines avaliadas e escolha

Este documento registra **o que foi medido** na VM antes de escolher a
tecnologia, **o que foi pesquisado** sobre as engines candidatas e **por que**
a escolha foi Three.js (cliente web) + Node.js (simulação autoritativa).

## 1. Inspeção da VM (2026-10-05)

Comandos usados: `uname -a`, `nproc`, `lscpu`, `free -h`, `df -h`,
`/etc/os-release`, `ls /dev/dri`, `ls /usr/lib/x86_64-linux-gnu/dri`,
`dpkg -l`, `which …`, e um teste de WebGL2 no Chromium headless (Playwright).

| Item | Resultado |
|---|---|
| SO | Ubuntu 24.04.4 LTS, kernel 6.18 |
| CPU | Intel Xeon @ 2,10 GHz, **4 vCPUs** (4 núcleos, 1 thread/núcleo), AVX‑512, x86_64 |
| Virtualização | KVM, virtualização completa (`Hypervisor vendor: KVM`) |
| RAM | **15 GiB**, sem swap |
| Disco | ~30 GB livres na cota da sessão |
| GPU | **nenhuma** — não existe `/dev/dri`; nenhuma placa exposta pelo KVM |
| Display | **headless**: `DISPLAY` e `WAYLAND_DISPLAY` vazios; `Xvfb` 21.1.12 instalado |
| OpenGL | Mesa 25.2.8 (`libGL`, `libgallium`), drivers **só de software** (`swrast_dri.so`, `kms_swrast_dri.so`, `zink`) — llvmpipe |
| Vulkan | loader `libvulkan.so.1` (1.3.275) presente, mas **nenhum ICD** em `/usr/share/vulkan/icd.d` (sem lavapipe): nenhum dispositivo Vulkan para apps nativos |
| WebGL no navegador | Chromium 1194 (Playwright 1.56.1) headless → **WebGL2 OK** via ANGLE + SwiftShader (Vulkan 1.3 em CPU), textura máx. 8192 |
| Ferramentas de build | gcc, clang, cmake, make |
| Linguagens/runtimes | Node.js 22.22.0 (com `node:sqlite`, SQLite 3.50.4), npm 10.9.4, Python 3.11.15, Rust 1.97 / cargo, Go, Java, Bun |
| Rede | npm e crates.io acessíveis; **downloads do GitHub bloqueados (403)**; APIs de clima bloqueadas (api.open-meteo.com 403, api.met.no, wttr.in, INMET) |

**VM alvo do runtime** (do README do runtime): Oracle Cloud, 2 OCPU, 12 GB,
provavelmente **aarch64**. A escolha precisa funcionar lá também.

### Conclusões da inspeção

1. Não há aceleração gráfica. Qualquer renderização *na VM* é em CPU
   (llvmpipe ou SwiftShader).
2. A VM é headless: um app nativo com janela só seria visto por VNC/noVNC,
   transmitindo um framebuffer renderizado em CPU — caro e ruim de usar.
3. O navegador do usuário tem GPU. Uma aplicação **web** desloca a
   renderização para quem assiste; a VM só simula.
4. Screenshots reais são possíveis na VM pelo Chromium headless (WebGL2 em
   SwiftShader) — confirmado antes de começar.

## 2. Engines avaliadas

Pesquisa feita em 2026-10-05 (fontes no fim).

| Critério | Godot 4.7 | Bevy 0.19 | **Three.js r186** | Babylon.js 8 |
|---|---|---|---|---|
| Roda na VM sem GPU | só com OpenGL de software (llvmpipe) sob Xvfb | wgpu precisa de Vulkan/GL; aqui não há ICD Vulkan; GL só por software | **sim** (a VM só serve arquivos e simula) | sim |
| Headless + screenshot | `--headless` troca o renderizador por um *dummy* que não desenha nada; screenshot exige janela (Xvfb) | possível com adaptador de software, mas sem ICD aqui e com compilação longa | **Chromium headless + WebGL2 (testado)** | igual ao Three.js |
| Desenvolver rápido | editor + GDScript; projeto binário (.tscn) | compilação Rust pesada (centenas de crates) — minutos a cada build; mais ainda na VM ARM de 2 OCPU | módulos ES, sem bundler, recarregar a página | bom, API maior |
| Obter na VM | **download do GitHub bloqueado (403)** | crates.io acessível | npm acessível | npm acessível |
| ARM64 | binários Linux ARM64 oficiais | compila | Node.js tem builds ARM64 oficiais; cliente é JS | idem |
| Web | exportação web só com o renderizador *Compatibility* (WebGL2) e templates de exportação | WebGL2/WebGPU via wasm | **nativo** | nativo |
| Tamanho | runtime + export templates grandes | binário grande | ~1,3 MB (`three.module.js` + `three.core.js`) | vários MB |
| Integração com o runtime | precisaria de ponte (HTTP/SQLite) | mesma linguagem do kernel (Rust) | ponte simples: ler o SQLite do runtime (`node:sqlite`) e enviar por SSE | igual ao Three.js |

Também foram considerados: **raylib**/OpenGL nativo (mesmo problema de
janela/VNC e GPU) e **PlayCanvas** (equivalente ao Three.js, com menos
material de referência). Unity e Unreal foram descartados: pesados, sem
headless com renderização útil nesta VM e com licenças/editores fora do
escopo de um frontend leve.

## 3. Escolha

**Cliente: Three.js r186 (WebGL2), módulos ES servidos localmente.**
**Servidor/simulação: Node.js ≥ 22.13, sem dependências de execução.**

Motivos, em ordem de peso:

1. **Funciona nesta VM e na VM do runtime sem GPU.** A simulação é CPU pura
   e barata (~0,03 ms por tick); quem desenha é o navegador de quem assiste.
2. **Screenshots reais comprovados** no ambiente (Chromium headless + WebGL2
   por SwiftShader). O ciclo implementar → executar → capturar → corrigir foi
   usado durante todo o desenvolvimento.
3. **Integração direta com o runtime real.** O kernel grava tudo em SQLite
   (`data/abiyss.db`, modo WAL). O Node 22 tem `node:sqlite` embutido: o
   escritório lê o banco em modo somente leitura, sem bibliotecas nativas.
4. **Separação simulação × renderização**: o servidor é autoritativo
   (posições, estados, caminhos); o cliente só interpola e anima. O mesmo
   arquivo de layout (`shared/layout.js`) alimenta a navegação no servidor e
   a geometria no cliente.
5. **Poucas dependências**: `three` (cliente) e `playwright` (só para
   screenshots, dependência de desenvolvimento). Nada de bundler ou
   framework.
6. **Pronto para virar o "Frontend" do roadmap do runtime** (fase 10:
   "Interface web"): é uma página web servida localmente.

Bevy seria a escolha natural pela linguagem (o kernel é Rust), mas sem
dispositivo Vulkan na VM, com compilações longas na VM ARM de 2 OCPU e com a
necessidade de streaming de vídeo para ver a cena remotamente, ele perde em
todos os critérios práticos. Godot não pôde nem ser baixado (GitHub
bloqueado) e seu modo headless não renderiza.

## 4. Limitações encontradas na VM

| Limitação | Efeito | O que foi feito |
|---|---|---|
| Sem GPU | WebGL2 roda em SwiftShader (CPU): ~1–3 FPS a 1600×900 com sombras | só afeta screenshots na VM; num navegador com GPU a cena é leve (~400 draw calls, ~27 mil triângulos). Parâmetro `?q=low` reduz sombras/antialias |
| APIs de clima bloqueadas (403 pelo proxy) | não foi possível obter o clima **real** de Contagem aqui | o serviço tenta a Open-Meteo a cada 10 min; enquanto falha, usa uma **estimativa climatológica claramente marcada como "estimado"** no HUD e na TV. Para liberar: permitir `api.open-meteo.com` na política de rede do ambiente |
| Downloads do GitHub bloqueados | Godot não pôde ser obtido | não afetou a escolha final (seria descartado pelos outros critérios) |
| `PCFSoftShadowMap` removido no three r186 | aviso no console | usado `PCFShadowMap` |
| Node 22: `node:sqlite` ainda é "experimental" | aviso na saída | `--disable-warning=ExperimentalWarning` nos scripts; requer Node ≥ 22.13 |

## Fontes

- Godot — modo headless usa renderizador *dummy*: <https://github.com/godotengine/godot/issues/124139>, <https://github.com/godotengine/godot-proposals/issues/5790>
- Godot — exportação web só com Compatibility/WebGL2: <https://docs.godotengine.org/en/latest/tutorials/export/exporting_for_web.html>
- Godot 4.5 com binários Linux ARM64: <https://godotengine.org/download/archive/4.5-stable/>; Godot 4.7: <https://app.cinevva.com/news/2026-06-19-godot-4-7-released>
- Bevy 0.19 (junho de 2026): <https://alternativeto.net/news/2026/6/bevy-0-19-brings-next-gen-scenes-faster-rendering-contact-shadows-and-new-feathers-widgets/>; adaptador de software no wgpu: <https://docs.rs/bevy/latest/bevy/render/settings/struct.WgpuSettings.html>
- Three.js r186 (remoção do `PCFSoftShadowMap`): <https://newreleases.io/project/github/mrdoob/three.js/release/r186>, <https://github.com/mrdoob/three.js/wiki/Migration-Guide>
- Babylon.js — plugin de navegação V2 (recast): <https://forum.babylonjs.com/t/navigation-plugin-v2-is-here/60751>
