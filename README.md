# Abiyss — runtime

O **Abiyss** é um agente de IA autônomo que roda 24 horas por dia numa VM.
Este repositório contém o **runtime** dele: um kernel em Rust, recursos em
Python (servidores MCP) e persistência em SQLite. Tudo funciona pela linha de
comando (`abiyss ...`); não há painel web.

- Cérebro principal: **GLM 5.3** (modo de raciocínio) via NVIDIA NIM.
- Sub-agentes: **SubUltra** (Kimi K3), **SubMedium** (GLM 5.3 Flash), **SubLow** (Nemotron 120b).
- O nome dele é Abiyss. Ele nunca se apresenta como "Hermes" nem como humano
  (regra fixa no kernel, não depende do prompt editável).

> Os IDs de modelo e alguns parâmetros do NIM ainda precisam ser confirmados
> na VM — veja [Itens a confirmar](#itens-a-confirmar).

---

## Sumário

1. [Arquitetura](#arquitetura)
2. [Instalação na VM](#instalação-na-vm)
3. [Configuração](#configuração)
4. [Comandos da CLI](#comandos-da-cli)
5. [Daemon com systemd](#daemon-com-systemd)
6. [Testar sem gastar cota (mock do NIM)](#testar-sem-gastar-cota-mock-do-nim)
7. [Desenvolvimento](#desenvolvimento)
8. [Regras de segurança](#regras-de-segurança)
9. [Itens a confirmar](#itens-a-confirmar)

---

## Arquitetura

```
abiyss.toml  .env            ← configuração (sem segredos) + chaves (fora do git)
identity/nucleo.md           ← núcleo de identidade (o Abiyss NÃO edita)
skills/                      ← skills (SKILL.md); só leitura para o Abiyss
kernel/                      ← Rust: a parte que o Abiyss NÃO pode modificar
  src/nim/                   ← cliente NIM (SSE, tool calling) + mock para testes
  src/orquestrador/          ← dois pools, token bucket, fila de prioridade
  src/chat.rs                ← conversa com tool calling
  src/ferramentas/           ← tools nativas (só dentro de workspace/)
  src/mcp.rs                 ← ponte MCP (cliente rmcp, processos filhos)
  src/heartbeat.rs daemon.rs ← ciclo autônomo e daemon
  src/goals.rs cron.rs       ← máquina de estados dos goals, crons
  src/subagentes.rs          ← sub-agentes assíncronos
  src/diario.rs interocepcao.rs ← base da metacognição
  src/skills.rs frontmatter.rs ← skills com revelação progressiva
  src/memoria/               ← cofre (Obsidian), propostas, sleep, busca
recursos/mcp/exemplo/        ← Python (uv + SDK oficial `mcp`): a parte que o Abiyss poderá editar no futuro
workspace/                   ← única pasta onde as tools de arquivo leem/escrevem (criada sozinha)
cofre/                       ← memória de longo prazo (fora do git; criada sozinha)
  01_internal/               ← pessoas, preferências, auto-modelo, diário, procedimentos
  02_external/               ← mapa de fontes (links canônicos, resumos datados)
data/abiyss.db               ← SQLite (criado sozinho)
```

### Orquestrador de chamadas: dois pools

| Pool | Chave | Quem usa | Regras |
|---|---|---|---|
| **cérebro** | `NIM_API_KEY_CEREBRO` | só o Abiyss | 40 req/min (configurável); **fatia reservada para conversa**: o trabalho autônomo usa no máximo `40 - reserva` |
| **sub-agentes** | `NIM_API_KEY_SUBAGENTES` | só os sub-agentes | 40 req/min; fila com prioridade **Ultra > Medium > Low**; limite de chamadas simultâneas por nível |

- **Token bucket guardado no SQLite**: `abiyss chat` e `abiyss daemon` são
  processos diferentes usando a mesma chave; o balde compartilhado garante que,
  juntos, eles respeitam o limite.
- **429**: o `Retry-After` bloqueia o pool inteiro até o horário pedido.
- **Erros temporários** (5xx, rede): backoff exponencial com sorteio.
- **Um pool nunca usa a capacidade do outro** (baldes, filas e clientes separados).
- Cada tentativa fica registrada (`chamadas_modelo`) e alimenta a interocepção.

### Regra de orçamento

Tudo que pode ser código determinístico **não** vira chamada ao modelo:
crons disparam por código; o heartbeat só chama o modelo se houver evento
novo, mudança no goal em foco ou revisão periódica vencida; data/hora, CPU,
memória, disco e tokens gastos são medidos por código; transições de goal são
validadas por código.

### Heartbeat (daemon)

Cada ciclo faz **no máximo UMA** chamada ao cérebro (perceber + orientar +
decidir juntos). O contexto começa **e termina** com o núcleo do goal em foco.
O modelo responde em JSON com ações (`transicionar_goal`, `delegar`,
`cancelar_subagente`, `aguardar`), cada uma com a sua `expectativa`; o kernel
valida e executa, e anota no diário a expectativa (antes) e o resultado (depois).

Máquina de estados dos goals:

```
proposto → comprometido → executando → validando → concluído
   │            │              │  ▲          │
   │            ▼              ▼  └──────────┘ (validação falhou)
   │        bloqueado ◄────────┴─────────────┘
   ▼   (desbloqueio volta a comprometido/executando)
abandonado ◄── qualquer estado não final
```

---

## Instalação na VM

Testado para Linux (x86_64 e aarch64). A VM alvo é Oracle Cloud, 2 OCPU,
12 GB, provavelmente ARM (aarch64).

### 1. Pacotes do sistema

O SQLite e a criptografia do TLS são compilados junto (partes em C), então
é preciso um compilador C.

```bash
# Ubuntu
sudo apt update && sudo apt install -y build-essential git cmake

# Oracle Linux
sudo dnf install -y gcc gcc-c++ make git cmake
```

### 2. Rust e uv

```bash
# Rust (rustup)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# uv (gerenciador Python usado pelos servidores MCP)
curl -LsSf https://astral.sh/uv/install.sh | sh
```

### 3. Código e build

```bash
git clone https://github.com/rafaelsantos-cs/Abiyss-Runtime-Project.git
cd Abiyss-Runtime-Project
cargo build --release          # ~10–15 min numa VM de 2 OCPU
./target/release/abiyss --help

# Baixa as dependências do servidor MCP de exemplo (só na primeira vez)
(cd recursos/mcp/exemplo && uv sync --frozen)
```

Dica: coloque o binário no PATH (`sudo ln -s "$PWD/target/release/abiyss" /usr/local/bin/abiyss`)
ou use sempre `./target/release/abiyss` dentro da pasta do projeto.

**Alternativa sem compilar:** o CI gera o artefato `abiyss-aarch64-linux-gnu`
(aba *Actions* → execução mais recente → *Artifacts*). Ele é compilado no
Ubuntu mais recente do GitHub e exige glibc ≥ 2.39 (Ubuntu 24.04+). Em
distribuições mais antigas (ex.: Oracle Linux 8/9), compile na VM.

---

## Configuração

Todos os comandos procuram `./abiyss.toml` (ou `--config CAMINHO`, ou a
variável `ABIYSS_CONFIG`). Caminhos relativos são resolvidos a partir da pasta
do `abiyss.toml`.

### 1. Chaves do NIM (`.env`)

```bash
cp .env.example .env
nano .env          # preencha NIM_API_KEY_CEREBRO e NIM_API_KEY_SUBAGENTES
chmod 600 .env
```

O `.env` está no `.gitignore`. Os nomes das variáveis são definidos em
`abiyss.toml` (`api_key_env` de cada pool).

### 2. `abiyss.toml`

O arquivo está comentado seção por seção. Os pontos principais:

| Seção | O que ajustar |
|---|---|
| `[nim]` | `base_url` (padrão: `https://integrate.api.nvidia.com/v1`) e timeouts |
| `[modelos.*]` | IDs dos 4 modelos, `max_tokens` e `extra` (parâmetros específicos, como o modo de raciocínio do GLM) |
| `[pools.*]` | requisições por minuto, rajada, reserva de conversa, concorrência por nível, retentativas |
| `[daemon]` | intervalo do heartbeat, verificação de crons, revisão mínima |
| `[subagentes.*]` | orçamento (tokens, segundos, rodadas) e ferramentas por nível |
| `[[mcp.servidores]]` | servidores MCP em Python |

> **Limite por chave ou por conta?** Ainda não confirmado. Se as duas chaves
> forem da mesma conta NVIDIA e o limite for por conta, divida (ex.: 25 + 15)
> para não tomar 429.

### 3. Núcleo de identidade

Edite `identity/nucleo.md` e troque todos os `{{PREENCHER: ...}}`.
O `abiyss status` avisa enquanto houver placeholders. O arquivo é relido a
cada chamada (não precisa reiniciar).

### 4. Fuso horário

Data/hora, crons e interocepção usam o fuso local da máquina:

```bash
sudo timedatectl set-timezone America/Sao_Paulo
```

(ou `Environment=TZ=...` na unit do systemd).

### 5. Conferir a conexão com o NIM

```bash
./target/release/abiyss testar-nim                    # cérebro
./target/release/abiyss testar-nim --modelo ultra
./target/release/abiyss testar-nim --modelo medium
./target/release/abiyss testar-nim --modelo low
./target/release/abiyss ferramentas                   # tools nativas + MCP
```

---

## Comandos da CLI

| Comando | O que faz |
|---|---|
| `abiyss chat` | Conversa interativa (streaming). `/nova`, `/sair`, `/ajuda` |
| `abiyss chat --continuar` | Continua a conversa mais recente (`--conversa ID` para outra) |
| `abiyss chat -m "texto"` | Uma mensagem só (bom para scripts) |
| `abiyss chat --mostrar-raciocinio` | Mostra o raciocínio do modelo em cinza |
| `abiyss daemon` | Roda o Abiyss 24/7 (heartbeat, crons, sub-agentes) |
| `abiyss daemon --uma-vez` | Um ciclo de heartbeat (+ sub-agentes pendentes) e sai |
| `abiyss status` | Estado geral: daemon, identidade, goals, fila, crons, sub-agentes, interocepção |
| `abiyss goal add "Título" --nucleo "essência + critério de pronto" [--descricao ..] [--prioridade N]` | Cria um goal (estado `proposto`) |
| `abiyss goal list [--todos]` | Lista goals (`*` = em foco) |
| `abiyss goal show ID` | Detalhes e histórico de transições |
| `abiyss goal mover ID ESTADO --motivo "..."` | Transição manual (validada) |
| `abiyss cron add NOME "0 9 * * 1-5" "mensagem"` | Lembrete agendado (vira evento na fila) |
| `abiyss cron list` / `abiyss cron remover NOME` | Lista / remove crons |
| `abiyss diario [--limite N]` | Diário: expectativa antes de cada ação e resultado depois |
| `abiyss ferramentas` | Lista as ferramentas que o modelo enxerga |
| `abiyss skills` | Lista as skills (nome + descrição) e as ignoradas, com o motivo |
| `abiyss sleep` | Aplica as propostas de memória pendentes (ou rejeita, com o motivo) |
| `abiyss memoria propostas [--todas]` | Propostas pendentes (ou recentes, com a decisão) |
| `abiyss memoria buscar "consulta" [--escopo interno\|externo\|ambos]` | Busca no cofre (a mesma da ferramenta) |
| `abiyss memoria esquecer CAMINHO` | Remove uma nota (ex.: `01_internal/pessoas/ana.md`) e registra só que foi removida |
| `abiyss memoria registro [--limite N]` | Registro de operações: criada, atualizada, rejeitada, esquecida |
| `abiyss testar-nim [--modelo cerebro\|ultra\|medium\|low] [--sem-stream] [MSG]` | Uma chamada de diagnóstico |
| `abiyss mock-nim [--porta 8089]` | NIM de mentira local (veja abaixo) |

No chat, o Abiyss tem as ferramentas `ler_arquivo`, `listar_arquivos`,
`escrever_arquivo` (só em `workspace/`), `ler_skill`, `memoria_buscar` /
`memoria_ler` / `memoria_propor` (veja abaixo), as dos servidores MCP
(`exemplo__contar_palavras`, ...) e `delegar` / `status` / `cancelar`
(sub-agentes). **Os sub-agentes são executados pelo daemon**: delegar pelo
chat só registra o pedido; com o daemon rodando, ele começa em segundos e o
relatório chega como evento na fila do Abiyss.

Logs vão para o stderr; controle com `RUST_LOG` (ex.: `RUST_LOG=abiyss=debug`).

### Skills

Cada skill é uma pasta em `skills/` com um `SKILL.md` (frontmatter YAML com
`name` e `description`) e, opcionalmente, `references/`. **Revelação
progressiva:** só nome + descrição entram no system prompt (rotulados como
dado); o texto completo vem pela ferramenta `ler_skill(nome)` e cada
referência por `ler_skill(nome, referencia)`. As skills são só leitura para o
Abiyss. Detalhes em [`skills/README.md`](skills/README.md).

### Memória (cofre do Obsidian)

O cofre (`[memoria] cofre`, padrão `cofre/`, fora do git) tem dois escopos:

- `01_internal/`: pessoas, preferências, auto-modelo, diário pessoal,
  procedimentos. Toda nota tem procedência no frontmatter, preenchida pelo
  kernel: `fonte` (`conversa` | `sleep` | `importacao`), `tipo` (`dito` |
  `deduzido`), `criado`, `atualizado`. Uma nota tem **um tipo só**: dito e
  deduzido nunca se misturam (o sleep rejeita).
- `02_external/`: **mapa de fontes**, não enciclopédia. O frontmatter exige
  `links` (site oficial, changelog, docs), `navegador` (`rapido` |
  `contemplativo` | `agentico`) e `revalidar_apos` (AAAA-MM-DD); resumos em
  cache só com data no título (`## Resumo em cache (2026-10-05)`).
- Notas se ligam por `[[wikilinks]]`; internas podem apontar para externas,
  externas não apontam para internas.

Fluxo de escrita: `memoria_propor` → fila no SQLite → `abiyss sleep` aplica
ou rejeita → `Cofre::gravar` (único caminho de escrita; conteúdo novo é
**acrescentado**, nunca apaga). Só `abiyss memoria esquecer` remove uma
nota, e o registro guarda só o caminho.

**Regra dura (código do kernel):** conteúdo vindo de ferramentas, web,
sub-agentes ou qualquer fonte externa nunca entra direto em `01_internal`.
Cada mensagem do histórico guarda se traz conteúdo externo (resultados de
ferramentas externas e a resposta escrita logo depois deles no mesmo turno).
Quando o modelo chama `memoria_propor`, o kernel olha a janela de contexto:
se houver algo externo, a proposta é marcada com essa origem e o sleep a
rejeita no escopo interno (o `Cofre::gravar` confere de novo). Leituras da
memória interna e confirmações do kernel não contam como externas; memória
externa, arquivos do workspace, skills, MCP e sub-agentes contam. Para
sub-agentes, tudo conta como externo.

---

## Daemon com systemd

A unit está em [`deploy/abiyss.service`](deploy/abiyss.service):

```ini
[Unit]
Description=Abiyss - agente de IA autonomo 24/7
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=ubuntu
Group=ubuntu
WorkingDirectory=/home/ubuntu/Abiyss-Runtime-Project
ExecStart=/home/ubuntu/Abiyss-Runtime-Project/target/release/abiyss daemon
Environment=RUST_LOG=abiyss=info
Environment=TZ=America/Sao_Paulo
Environment=PATH=/home/ubuntu/.local/bin:/home/ubuntu/.cargo/bin:/usr/local/bin:/usr/bin:/bin
Restart=on-failure
RestartSec=10
KillSignal=SIGTERM
TimeoutStopSec=30
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
MemoryMax=4G

[Install]
WantedBy=multi-user.target
```

Troque `ubuntu` pelo seu usuário (no Oracle Linux, `opc`) e instale:

```bash
sudo cp deploy/abiyss.service /etc/systemd/system/abiyss.service
sudo systemctl daemon-reload
sudo systemctl enable --now abiyss
systemctl status abiyss
journalctl -u abiyss -f
```

- Só um daemon roda por vez (trava em `data/daemon.lock`).
- `SIGTERM` (stop/restart) para o daemon com educação, mesmo no meio de um
  ciclo; sub-agentes interrompidos ficam marcados como `falhou` e o resultado
  vira evento.
- O `.env` é lido pelo próprio Abiyss; não precisa de `EnvironmentFile`.

---

## Testar sem gastar cota (mock do NIM)

```bash
# terminal 1
./target/release/abiyss mock-nim --porta 8089

# terminal 2: config apontando para o mock
sed 's#https://integrate.api.nvidia.com/v1#http://127.0.0.1:8089/v1#' abiyss.toml > abiyss.mock.toml
NIM_API_KEY_CEREBRO=x NIM_API_KEY_SUBAGENTES=x ./target/release/abiyss --config abiyss.mock.toml chat
```

O mock ecoa a última mensagem (`mock: ...`). Nos testes automatizados ele
também simula streaming, tool calls, 429 com `Retry-After`, erros 5xx e
atrasos — **nenhum teste usa rede externa nem chaves reais**.

> Use um `data/` separado se não quiser misturar o histórico do mock com o
> real (ajuste `[caminhos] dados` no `abiyss.mock.toml`).

---

## Desenvolvimento

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test                    # os testes MCP precisam do uv no PATH (sem ele, são pulados)
```

Os testes ficam em `kernel/tests/` (um arquivo por fase: `f1_` a `f8_` e
`a1_` em diante) e em
módulos `#[cfg(test)]` dentro de `kernel/src/`. O CI
(`.github/workflows/ci.yml`) roda fmt, clippy e testes em x86_64 e compila o
binário para `aarch64-unknown-linux-gnu`.

O código prioriza legibilidade: dados com dono (`String`, `clone`, `Arc`),
poucos genéricos e comentários em português explicando o que não é óbvio.

---

## Regras de segurança

- **Segredos** só no `.env` (fora do git) ou no ambiente; `Debug` do cliente
  esconde a chave; os servidores MCP recebem um **ambiente limpo** (as chaves
  do NIM não são repassadas).
- **Ferramentas de arquivo** só dentro de `workspace/`: sem caminho absoluto,
  sem `..`, links simbólicos não escapam, nunca escreve através de link. O
  kernel recusa iniciar se o workspace contiver (ou estiver dentro de)
  `kernel/`, `identity/`, `recursos/`, `skills/`, o cofre, `data/`, `.git`, `.env` ou `abiyss.toml`.
- **Memória interna** só recebe conteúdo de conversa, sleep ou importação:
  o que veio de ferramentas/web é rejeitado por código (veja "Memória").
- **Conteúdo externo é dado, não instrução**: todo resultado de ferramenta,
  servidor MCP, evento e relatório de sub-agente entra no contexto dentro de
  `<dados origem="...">...</dados>`; marcas que tentem fechar o bloco são
  neutralizadas.
- **Sub-agente não cria sub-agente**: `delegar`/`status`/`cancelar` nunca entram
  na caixa de ferramentas deles, independentemente da config.
- **Identidade**: as regras do kernel (nome Abiyss, nunca Hermes nem humano)
  ficam no código e vêm antes do núcleo editável.

---

## Itens a confirmar

Não foi possível acessar a documentação oficial da NVIDIA durante o
desenvolvimento (bloqueada pela rede do ambiente). O que não foi confirmado
virou configuração:

| Item | Onde | Valor atual |
|---|---|---|
| ID do GLM 5.3 | `modelos.cerebro.id` | `z-ai/glm-5.3` |
| ID do Kimi K3 | `modelos.sub_ultra.id` | `moonshotai/kimi-k3` (placeholder) |
| ID do GLM 5.3 Flash | `modelos.sub_medium.id` | `z-ai/glm-5.3-flash` |
| ID do Nemotron 120b | `modelos.sub_low.id` | `nvidia/nemotron-3-super-120b-a12b` |
| Modo de raciocínio máximo do GLM | `modelos.cerebro.extra` | `chat_template_kwargs = { enable_thinking = true, clear_thinking = false }` |
| Limite por chave ou por conta | `pools.*.requisicoes_por_minuto` | 40 + 40 |
| `stream_options.include_usage` aceito no streaming | código (`nim/cliente.rs`) | enviado sempre |

Os próximos passos estão em [`docs/ROADMAP.md`](docs/ROADMAP.md).
