# navegador

O nível **agêntico** do navegador do Abiyss: um Chromium sem tela
(Playwright) dirigido por um sub-agente. Os três níveis:

| Nível | Onde | Para quê |
|---|---|---|
| rápido | `web_rapido` | buscar e ler o texto principal de páginas simples, sem JavaScript |
| contemplativo | (ainda não existe) | — |
| agêntico | `navegador` (este) | páginas montadas por JavaScript, seguir links, abas, formulários (com `interagir`) |

O modelo pode não ver imagens: a saída principal é **texto**, o
*instantâneo* da página.

## Ferramentas

| Ferramenta | O quê |
|---|---|
| `navegador__navegar(url, sessao="")` | abre a URL e devolve o instantâneo; sem `sessao`, abre uma sessão nova (o id vem no cabeçalho) |
| `navegador__ler(sessao, inicio=0)` | o instantâneo da aba atual; `inicio` continua um que veio cortado |
| `navegador__clicar(sessao, ref)` | clica no elemento `ref` (ex.: `e12`) e devolve o instantâneo depois do clique |
| `navegador__digitar(sessao, ref, texto, enter=False)` | escreve num campo (só com `interagir`) |
| `navegador__rolar(sessao, direcao="baixo")` | `baixo`, `cima`, `inicio`, `fim`; diz se a página cresceu |
| `navegador__voltar(sessao)` | página anterior da aba |
| `navegador__abas(sessao, trocar_para=0, fechar=0)` | lista, troca e fecha abas |
| `navegador__capturar_tela(sessao, pagina_inteira=False)` | PNG em `workspace/navegador/`; devolve o caminho (a imagem não vem na resposta) |
| `navegador__fechar(sessao)` | fecha a sessão |

### O instantâneo

```
Sessão s1a2b3c · aba 1 de 1 · modo só leitura
URL: https://site.com/artigo
Título: Sono e memória
---
[link e1] Início → https://site.com/
# Sono e memória
O hipocampo consolida memórias durante o sono profundo.
• Primeiro item
Fase | Duração
[imagem] Gráfico das fases do sono
[campo e4] Buscar (vazio)
[botão e5] Enviar (envia formulário)
```

- Texto visível na ordem do documento: títulos com `#`, listas com `•`,
  tabelas com `|`, imagens pelo texto alternativo. Texto escondido,
  scripts e o valor de campos de senha não aparecem.
- Cada elemento com que se interage tem uma referência (`e1`, `e4`...). Ela
  fica gravada no elemento (num atributo de nome sorteado por sessão), então
  continua a mesma nos instantâneos seguintes; a numeração da sessão só
  cresce, e uma referência nunca passa a apontar para outro elemento.
- Quadros (iframes) aparecem depois da página, com `--- quadro N: url ---`.
- No máximo 20 mil elementos por quadro; acima de `NAVEGADOR_MAX_TEXTO_BYTES`
  a resposta termina com `[… instantâneo cortado pelo navegador: faltam N
  caracteres. Para continuar, chame ler com sessao=… e inicio=X …]`.
- Avisos (diálogos dispensados, pedidos bloqueados, aba recusada) vêm antes
  do `---`.

O kernel trata tudo o que vem daqui como conteúdo externo.

## Modo só leitura (o padrão)

```toml
[navegador]
interagir = false
```

| | só leitura (`false`) | `interagir = true` |
|---|---|---|
| navegar, ler, rolar, voltar, abas, capturar_tela | sim | sim |
| clicar | **só links** | qualquer elemento |
| digitar | recusado | sim, menos em formulário que envia para outra origem |
| envio de formulário (POST ou GET) | recusado | só para a mesma origem da página |
| POST/PUT/... de fetch/XHR da página | só para a mesma origem | só para a mesma origem |

Por quê: o navegador fica com o sub-agente `ultra`, que também lê e escreve
arquivos do workspace. Uma página poderia mandá-lo copiar um arquivo para um
campo e enviar. Sem `interagir`, nada é digitado; com ele, nada sai para
outro site por formulário ou POST. Quem muda o modo é o dono, no
`abiyss.toml` (uma variável de ambiente não muda), e o servidor precisa
subir de novo. Detalhes em `docs/LIMITES.md`.

## Proteções

- **Rede** (as mesmas regras do `web_rapido`, `rede.py`), em duas camadas:
  - *interceptação*: todo pedido que a página faz (navegação, script,
    imagem, fetch...) é conferido antes de sair: só `http`/`https`; o
    endereço literal é conferido antes do DNS e todos os endereços do nome
    depois. Recusa loopback, redes privadas, link-local (o
    169.254.169.254 do metadata da nuvem), CGNAT, IPv6 local;
  - *filtro*: o Chromium usa um proxy local obrigatório (sem a exceção
    implícita do loopback) que resolve o nome, conecta num endereço já
    conferido e confere o endereço conectado. Pega cada salto de
    redirecionamento, WebSocket e qualquer pedido que a interceptação não
    veja. QUIC, pré-resolução de DNS, pings de link e WebRTC por UDP direto
    ficam desligados.
- **Sessão anônima**: cada sessão é um contexto novo (sem cookies, cache ou
  armazenamento de outra sessão); o perfil do Chromium é temporário e não
  fica nada guardado entre subidas. Downloads desligados, service workers
  bloqueados, nenhuma permissão (câmera, localização...). Diálogos
  (`alert`, `confirm`, `prompt`, `beforeunload`) são dispensados na hora.
- **Limites**: sessões ao mesmo tempo, abas por sessão (a página que tenta
  abrir mais tem a aba fechada), tempo de carregamento, prazo por chamada,
  tamanho do instantâneo, heap do JavaScript, memória da árvore do Chromium
  e sessão ociosa.
- **Prazo**: a chamada que passa de `NAVEGADOR_PRAZO_SEGUNDOS` fecha a sessão;
  se nem isso responder, a árvore do Chromium inteira é morta (SIGKILL no
  grupo de processos). Página com JavaScript em laço: a aba é fechada.
- **Memória**: a cada segundo o servidor soma o RSS da árvore do Chromium (a
  mesma conta do kernel). Passou de `NAVEGADOR_MEMORIA_MB`: a árvore é morta,
  as sessões fecham com o motivo, e a próxima chamada sobe outro Chromium.
- **Sem órfãos**: o Playwright sobe o Chromium noutro grupo de processos,
  então o SIGKILL do kernel no grupo do servidor não o alcançaria. O
  `vigia.py` roda numa sessão própria, anota os grupos do Chromium e os mata
  quando o servidor morre (de qualquer jeito). Ao encerrar com educação, o
  servidor fecha tudo sozinho. Chromium sem sessões há 1 min é fechado.

## Memória: o kernel precisa de mais que 512 MiB

O kernel mede a árvore inteira do servidor (uv + Python + o driver Node do
Playwright + Chromium) contra `[mcp] max_memoria_mb`. Medido em x86_64: uv
~35 MiB, Python ~30–60 MiB, driver ~130 MiB, Chromium ~270 MiB parado com uma
página simples (RSS somado, como o kernel conta). Com o padrão de 512 MiB o
servidor **recusa subir** e diz por quê. Para ligar:

```toml
[mcp]
max_memoria_mb = 1024        # vale para todos os servidores MCP (é um teto)
```

`NAVEGADOR_MEMORIA_MB` (Chromium) + `NAVEGADOR_RESERVA_SERVIDOR_MB` (o resto)
tem de caber nisso.

## Configuração

No `abiyss.toml`: `[navegador] interagir` (acima) e a tabela `env` do item
`navegador` em `[[mcp.servidores]]` (valores sempre entre aspas):

| Variável | Padrão | O quê |
|---|---|---|
| `NAVEGADOR_PRAZO_SEGUNDOS` | `timeout_segundos` − 10 | tempo máximo de uma chamada |
| `NAVEGADOR_TIMEOUT_CARREGAMENTO` | 30 | carregar uma página (+ 10 s ≤ o prazo) |
| `NAVEGADOR_TIMEOUT_ACAO` | 10 | clicar, digitar, capturar |
| `NAVEGADOR_MAX_SESSOES` | 2 | sessões ao mesmo tempo |
| `NAVEGADOR_MAX_PAGINAS` | 3 | abas por sessão |
| `NAVEGADOR_SESSAO_OCIOSA_SEGUNDOS` | 600 | sessão parada fecha sozinha |
| `NAVEGADOR_MAX_TEXTO_BYTES` | 40000 | instantâneo por resposta (o resto com `inicio`) |
| `NAVEGADOR_MEMORIA_MB` | 768 | árvore do Chromium (o heap do JavaScript fica em ⅓ disso, até 512) |
| `NAVEGADOR_RESERVA_SERVIDOR_MB` | 256 | uv + Python + driver do Playwright |
| `NAVEGADOR_MAX_CAPTURAS` | 50 | PNGs guardados em `workspace/navegador/` (os mais velhos saem) |
| `NAVEGADOR_SANDBOX` | auto | sandbox do Chromium: `auto` tenta e, se não der, sobe sem (aviso no journal); `sim` exige; `nao` desliga |
| `NAVEGADOR_CHROMIUM` | (o do Playwright) | caminho de outro executável do Chromium |
| `NAVEGADOR_AO_VIVO_PORTA` | 0 | visão ao vivo (abaixo); 0 = desligada |
| `NAVEGADOR_EXCECOES_REDE_LOCAL` | (vazio) | `ip:porta` internos liberados — **só para testes** |

O servidor recusa subir se o prazo + 10 s passar do `timeout_segundos` do
item, se o carregamento + 10 s passar do prazo, se o instantâneo + 8 KiB
passar de `[mcp] max_bytes_resultado`, ou se Chromium + reserva passar de
`[mcp] max_memoria_mb`.

## Instalação na VM (Ubuntu 24.04, aarch64)

Como o usuário do daemon (o mesmo `HOME`: o Chromium fica em
`~/.cache/ms-playwright`):

```bash
cd ~/Abiyss-Runtime-Project/recursos/mcp/navegador
uv sync --frozen --no-dev
# O Chromium headless shell para arm64 (~100 MiB):
uv run --frozen --no-dev playwright install --only-shell chromium
# As bibliotecas do sistema que ele usa (apt):
sudo .venv/bin/playwright install-deps chromium
```

Depois, no `abiyss.toml`: `[mcp] max_memoria_mb = 1024` e `ativo = true` no
item do navegador; então `sudo systemctl restart abiyss` e
`journalctl -u abiyss | grep navegador` ("pronto: modo só leitura...").

### Sandbox do Chromium (recomendado)

No Ubuntu 24.04 o AppArmor bloqueia os *user namespaces* de que a sandbox do
Chromium precisa, e o servidor (com `auto`) sobe o Chromium **sem** sandbox,
com um aviso no journal. Para ligar, um perfil que libere só o Chromium do
Playwright (caminhos A CONFIRMAR na VM: `ls ~/.cache/ms-playwright/*/`):

```bash
sudo tee /etc/apparmor.d/abiyss-chromium >/dev/null <<'EOF'
abi <abi/4.0>,
include <tunables/global>

profile abiyss-chromium /home/ubuntu/.cache/ms-playwright/chromium{,_headless_shell}-*/**/{chrome,headless_shell} flags=(unconfined) {
  userns,
  include if exists <local/abiyss-chromium>
}
EOF
sudo apparmor_parser -r /etc/apparmor.d/abiyss-chromium
```

Depois, `NAVEGADOR_SANDBOX = "sim"` para exigir a sandbox (sem ela, o
navegador não abre).

### Tarefas do systemd

A unit limita o cgroup inteiro (daemon + todos os servidores MCP) a
`TasksMax=256` (processos **e** threads). Medido: driver + Chromium com duas
sessões simples ~65 tarefas. Somado ao terminal (até 64 por comando, 2 ao
mesmo tempo) dá para chegar perto do teto; se o Chromium falhar com "Resource
temporarily unavailable", aumente o `TasksMax` da unit.

## Visão ao vivo (opcional)

`NAVEGADOR_AO_VIVO_PORTA = "8790"` liga uma página que mostra a captura de
cada sessão aberta, renovada a cada 1,5 s. Só ver (nada de clique ou tecla),
só em 127.0.0.1, com um token sorteado a cada subida (no journal:
`journalctl -u abiyss | grep "visão ao vivo"`). Da sua máquina:

```bash
ssh -L 8790:127.0.0.1:8790 ubuntu@<vm>
# abra a URL do journal: http://127.0.0.1:8790/?t=<token>
```

## Testes

```bash
cd recursos/mcp/navegador
uv run playwright install --only-shell chromium   # uma vez (ou NAVEGADOR_CHROMIUM=/caminho)
uv run pytest
```

Nenhum teste sai para a internet: três servidores HTTP locais fazem o papel
de um site, de outro site e de um serviço da rede interna (página estática,
página montada por JavaScript, redirecionamento para 127.0.0.1 e para o
metadata, página que busca o metadata, formulário com POST para outra
origem, página enorme, página que nunca responde, JavaScript em laço,
diálogos, abas, download). Sem Chromium instalado, os testes que precisam
dele são pulados.

## Limites conhecidos

- Com `interagir`, uma página **maliciosa** recebe o que for digitado nela
  (é o próprio destino) e o JavaScript dela pode mandar adiante por GET. A
  regra de origem impede que uma página mande dados para um TERCEIRO por
  formulário ou POST, não que o próprio site os receba. Por isso o padrão é
  só leitura, e a skill `usar-o-navegador` diz para nunca digitar conteúdo
  do workspace.
- O user agent é o do Chromium sem tela (`HeadlessChrome`): alguns sites
  recusam ou mostram um desafio. O navegador não tenta se disfarçar e não
  resolve CAPTCHA.
- Não respeita robots.txt (é um navegador agindo por uma tarefa, não um
  robô que varre sites); o `web_rapido` respeita.
- O "nível" contemplativo ainda não existe.
- Medidas de memória feitas em x86_64; na VM (arm64) os números podem ser
  outros — confira com `abiyss status`/`ps` e ajuste `NAVEGADOR_MEMORIA_MB`.
