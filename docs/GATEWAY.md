# Gateway do Discord

O dono conversa com o Abiyss pelo Discord (DM, e opcionalmente um canal), e
o Abiyss alcança o dono sem a CLI: respostas, pedidos (E9) por DM e, se
quiser, um resumo da noite pela manhã.

```
Discord ⇄ recursos/gateway/adaptador.py ⇄ socket Unix local ⇄ daemon (kernel)
          (discord.py, processo próprio)   data/gateway/abiyss.sock
```

Sumário: [o que já existia](#o-que-já-existia) ·
[desenho](#desenho-escolhido-e-por-quê) · [confiança](#confiança) ·
[fluxos](#fluxos) · [segurança](#segurança) · [padrões](#padrões) ·
[instalação](#instalação-passo-a-passo) · [unit do systemd](#unit-do-systemd-do-adaptador) ·
[conferir na VM](#o-que-conferir-na-vm) · [limites](#limites-conhecidos)

---

## O que já existia

Levantado antes de escrever o gateway (base: `v02-ajustes`).

**Como os eventos entram na fila.** `eventos::publicar[_com_origem]` grava
uma linha em `fila_eventos` (SQLite: `tipo`, `origem`, `conteudo`,
`origem_externa`). Quem publica: os crons (no tique do loop do daemon), o
executor de sub-agentes (relatório = conteúdo externo), o heartbeat
(`consultar_skill`), o kernel (reinício, estagnação, pedido expirado), o sono
ao acordar e `pedidos::responder` (`usuario`/`pedido:<id>`). O heartbeat lê
os pendentes no começo do ciclo, coloca cada um no contexto dentro de
`<dados origem="tipo:origem">` e marca como consumidos só depois de uma
chamada ao modelo que deu certo. Por ser SQLite, funciona entre processos.

**Como o `abiyss chat` fala com o runtime.** Não fala com o daemon: é outro
processo. Abre o mesmo banco, monta o próprio `Orquestrador` (o balde do rate
limit é compartilhado pelo SQLite; a conversa usa a fatia reservada,
`Origem::Conversa`), sobe os próprios servidores MCP e chama
`SessaoChat::enviar` com streaming. Esforço: `[chat] esforco_padrao`
(medium), com `aprofundar` até `esforco_maximo` (high). `delegar` só grava o
pedido no banco; quem executa é o executor de sub-agentes do daemon (que
procura pedidos a cada 5 s).

**Como os pedidos (E9) são guardados e respondidos.** Tabela
`pedidos_usuario` (pergunta, contexto, urgência, estado, resposta, origem
externa; a mesma pergunta pendente não duplica; teto de pendentes). Nascem
da ação `pedir_ao_usuario` do heartbeat e das perguntas do sono. São
respondidos pela CLI (`abiyss pedidos responder`) ou na conversa (ferramenta
`responder_pedido`, só na caixa da conversa com o dono); a resposta vira o
evento `usuario`/`pedido:<id>`. O tique do daemon expira os que passam de
`[pedidos] expira_apos_horas`. Não havia entrega ativa ("WhatsApp/Discord
fica para outra sessão").

**Pausa do heartbeat.** Não existe comando de pausa (só o disjuntor
automático da vigilância). Por isso o gateway **não** oferece pausa.

## Desenho escolhido e por quê

**O kernel expõe um socket Unix local; o adaptador do Discord é um processo
Python separado.** No daemon, o gateway é uma tarefa própria:

- **O loop principal nunca espera o gateway.** O daemon só cria a tarefa ao
  subir e a aborta ao parar (como o executor de sub-agentes); o `select!` do
  loop não a vê. O gateway usa uma **conexão própria ao banco** (como a
  manutenção), então nem a trava da conexão do loop ele segura. Mandar ao
  adaptador é `try_send` numa fila limitada: adaptador lento ou travado não
  segura ninguém (o que não couber fica na fila do banco). Teste:
  `daemon_fica_saudavel_com_o_gateway_fora_do_ar_ou_travado`.
- **Mesmo caminho do `abiyss chat`.** A conversa é uma `SessaoChat` com a
  mesma caixa de ferramentas (`chat::caixa_de_conversa`, usada agora pelos
  dois), o mesmo esforço (`[chat]`) e o mesmo streaming. Os servidores MCP e
  o orquestrador são os do daemon (sem um segundo conjunto de processos).
- **Confiança no kernel.** `recursos/` é a parte que o Abiyss poderá editar
  no futuro; o adaptador só relata fatos (IDs) e o kernel decide quem é o
  dono. Também é o kernel que oculta segredos e aplica os tetos: o texto
  chega ao adaptador já limpo.
- **Sem porta de rede.** Socket Unix em `data/gateway/abiyss.sock` (pasta
  0700, socket 0600): só o usuário do daemon alcança.
- **Nada se perde.** Entrada e saída ficam numa tabela (`gateway_mensagens`,
  migração 15) que é o registro e a fila: a mensagem do dono é gravada antes
  do `recebido`; a resposta fica `pendente` até o adaptador confirmar com os
  IDs do Discord (entrega "pelo menos uma vez"; o adaptador não duplica uma
  `ref` já entregue).

Alternativas descartadas:

| Alternativa | Por que não |
|---|---|
| Um processo `abiyss gateway` (Rust) separado, como o `abiyss chat` | mais um processo com o seu conjunto de servidores MCP (memória da VM) e mais um serviço; e os pedidos/resumo teriam de ser descobertos por varredura do banco do mesmo jeito. A vantagem (conversar com o daemon fora do ar) é pequena: sem daemon, sub-agentes e heartbeat também param. |
| O adaptador escrever direto no SQLite | a decisão de confiança e a regra de conteúdo externo ficariam em `recursos/` (editável no futuro); sem streaming; dois escritores com regras próprias no mesmo banco. |
| Servidor MCP de canal | o MCP é para o modelo chamar ferramentas; aqui o fluxo é o contrário (o Discord empurra mensagens). |
| Porta HTTP local | porta de rede = mais superfície (qualquer processo local, SSRF a partir de um sub-agente); o socket Unix tem permissão de arquivo. |
| Discord dentro do kernel (crate Rust) | uma dependência grande de rede no binário que não pode ser modificado; o discord.py é maduro e reconecta sozinho. |

## Confiança

Decisão em `kernel/src/gateway/confianca.rs`, a partir de IDs (nome de
exibição qualquer um copia; ID de usuário não):

| De onde | Quem | Vira |
|---|---|---|
| DM | o dono (`dono_discord_id`) | o dono falando |
| DM | qualquer outro, bot, webhook | ignorado (o texto nem é guardado) |
| canal permitido (`canal_id`) | o dono, mencionando o bot ou respondendo a ele | o dono falando (resposta no canal) |
| canal permitido | o dono sem chamar o bot | ignorado (falava com outros) |
| canal permitido | outra pessoa, bot, webhook | conteúdo **externo**: evento `discord` com `origem_externa = discord:canal:<id>:autor:<id>` |
| qualquer outro canal | qualquer um | ignorado (o adaptador nem encaminha) |

Conteúdo externo nunca responde pedido, nunca dispara conversa e, como todo
evento externo, marca o ciclo que o consome (o sono não o usa como material
interno). O "Responder" de um estranho numa mensagem de pedido continua
externo.

## Fluxos

**Entrada (dono).** `mensagem` → classificação → gravada (`pendente`) →
`recebido` (`recebida`, ou `na_fila` se um turno está rodando). Um turno por
vez; o que chegou durante o turno vira o turno seguinte, **todas juntas**,
na ordem, respondendo à última. Mensagem repetida (o adaptador reenviou
depois de reconectar) é `duplicado`. Turno com tempo máximo
(`max_duracao_turno_segundos`) e pânico isolado. Se o daemon parar no meio
de um turno, ao subir o dono recebe um aviso (o turno não é refeito sozinho:
repetiria ferramentas como `delegar` ou `escrever_arquivo`).

**Comandos** (respondidos na hora, sem modelo, mesmo com um turno rodando):
`/status`, `/pedidos`, `/nova` (conversa nova), `/arquivo <caminho>` (um
arquivo do workspace), `/ajuda`. Também com `!`. Só a mensagem inteira é
comando (`/etc/hosts está errado` é conversa).

**Saída.** Respostas da conversa, respostas de comandos, pedidos e resumo
vão para a fila; a entrega manda ao adaptador e espera `enviado` (com os IDs
das mensagens criadas no Discord). Sem confirmação em 60 s, manda de novo;
depois de 5 tentativas, desiste (`falhou`, visível no `abiyss status`).

**Pedidos por DM.** Cada pedido pendente vai por DM (urgência, origem,
contexto e um aviso se o texto derivou de conteúdo externo). Um
**"Responder"** do dono em qualquer pedaço da mensagem do pedido responde
**àquele** pedido (`pedidos::responder`, sem passar pelo modelo) e o dono
recebe a confirmação. Sem resposta: no máximo `max_reenvios` lembretes, cada
um `reenviar_apos_horas` depois do envio anterior terminar (nunca dois na
fila); o pedido ainda expira em `[pedidos] expira_apos_horas`. Pedido
respondido por outro caminho antes da entrega não vai.

**Resumo da manhã** (desligado por padrão): uma vez por dia, a partir de
`resumo_manha_hora` (fuso local), o relato do sono ao acordar (o evento
`sono`, que por regra do sono não traz texto externo), ou o que houve se não
teve sono nas últimas 24 h.

**Streaming.** O kernel manda `resposta_inicio` → `resposta_parcial` (sempre
o texto inteiro até ali; no máximo uma a cada 250 ms; vazio quando o modelo
começa uma rodada de ferramentas) → `resposta_fim`. O adaptador:

- manda "." e cicla `.` `..` `...` enquanto não há texto;
- edita **uma** mensagem, no máximo **uma operação no Discord a cada 1,2 s**
  (o mesmo ritmo para tudo que o adaptador faz);
- passou de 2000 caracteres (contados em UTF-16, como o Discord conta),
  continua em mensagens novas; o corte é em fim de linha e, dentro de um
  bloco ```, fecha o bloco e reabre na mensagem seguinte com a mesma
  linguagem;
- se uma edição falhar, para de editar e manda a resposta inteira, no fim,
  como mensagem(ns) nova(s) (e tenta apagar a parcial).

Protocolo completo em `kernel/src/gateway/protocolo.rs`.

## Segurança

- **Segredos não saem.** Todo texto passa por `Gateway::mandar`, o único
  caminho até o adaptador, que oculta com os **mesmos padrões do servidor
  `ambiente`** (`--password=x`, `--token x`, `API_KEY=x`,
  `usuario:senha@host`; um teste lê `recursos/mcp/ambiente/sistema.py` e
  confere que as expressões são idênticas), chaves soltas com os prefixos
  da importação do Hermes (`nvapi-`, `sk-`, `ghp_`...), blocos de chave
  privada e os **valores** das variáveis de ambiente com nome de segredo
  (chaves do NIM; o token do Discord se estiver no `.env`). Na resposta que
  chega aos poucos, a última palavra espera se completar.
- **Tetos.** Entrada cortada em `max_caracteres_entrada`; no máximo
  `max_entrada_por_minuto` mensagens do dono (as de cima ficam registradas,
  sem resposta, com um aviso) e `max_externas_por_minuto` externas; saída
  cortada em `max_caracteres_saida` (a inteira fica no histórico) e no
  máximo `max_saida_por_minuto` entregas (o resto espera).
- **Token só no ambiente do adaptador** (`ABIYSS_DISCORD_TOKEN`); o kernel
  nunca lê o token; ele não aparece em log nem no `repr` da config.
- **Arquivo só do workspace.** `/arquivo` resolve o caminho **dentro** do
  workspace (sem `..`, sem absoluto, link para fora recusado, só arquivo
  comum, até `max_bytes_anexo`); o adaptador confere de novo contra o
  workspace do `ola` e abre com `O_NOFOLLOW`. O modelo não tem como pedir um
  anexo: só o dono, pelo comando.
- **Sem pings.** O bot manda tudo com `AllowedMentions.none()`.
- **Destino.** O adaptador só manda para a DM do dono ou para o canal
  permitido (confere de novo o que o kernel pediu).

## Padrões

`[gateway]` no `abiyss.toml`:

| Chave | Padrão | O quê |
|---|---|---|
| `ativo` | `false` | desligado, o daemon nem abre o socket |
| `socket` | `data/gateway/abiyss.sock` | socket Unix (relativo à pasta do `abiyss.toml`) |
| `dono_discord_id` | — | ID de usuário do dono (obrigatório se ativo) |
| `canal_id` | vazio | um canal de servidor (vazio = só DMs) |
| `canal_exige_mencao` | `true` | no canal, o dono precisa mencionar o bot ou responder a ele |
| `max_duracao_turno_segundos` | `900` | tempo máximo de uma resposta |
| `pedidos_por_dm` | `true` | pedidos pendentes por DM |
| `max_reenvios` | `2` | lembretes de um pedido sem resposta |
| `reenviar_apos_horas` | `12` | intervalo entre envios de um pedido |
| `resumo_manha` | `false` | resumo da noite por DM |
| `resumo_manha_hora` | `"08:00"` | a partir de quando (fuso local) |
| `verificacao_segundos` | `30` | de quanto em quanto tempo olha pedidos e resumo |
| `max_caracteres_entrada` | `4000` | mensagem maior é cortada |
| `max_entrada_por_minuto` | `20` | do dono |
| `max_externas_por_minuto` | `10` | do canal permitido |
| `max_caracteres_saida` | `12000` | resposta maior é cortada |
| `max_saida_por_minuto` | `20` | entregas (edições da resposta não contam) |
| `max_bytes_anexo` | `8388608` | `/arquivo` |

E `[retencao] gateway_dias = 30`: mensagens terminadas mais velhas que isso
viram agregado diário (`gateway_mensagens_diarias`); as da fila e as ligadas
a um pedido pendente ficam.

Fixos no código: parciais a cada 250 ms (kernel), 1,2 s entre operações no
Discord (adaptador), 60 s de espera pela confirmação, 5 tentativas,
reconexão ao daemon de 1 s a 60 s e ao Discord de 5 s a 300 s (exponencial
com sorteio de ±20%), até 50 mensagens buscadas por canal ao voltar.

## Instalação passo a passo

### 1. O bot (Discord Developer Portal)

O dono já tem um bot. Em <https://discord.com/developers/applications>, na
aplicação dele:

1. **Bot → Privileged Gateway Intents → Message Content Intent: ligado.**
   Sem isso o adaptador sai com um erro explicando (o conteúdo de mensagens
   em canal chega vazio). Server Members e Presence: desligados.
2. **Bot → Token → Reset Token** (se o token atual não estiver à mão).
   Guarde só no `.env` da VM (passo 3).
3. **Bot → Public Bot**: desligado (ninguém mais adiciona o bot).
4. O bot precisa estar em **um servidor em comum com o dono** (o Discord só
   deixa um bot mandar DM a quem divide um servidor com ele). Para
   convidar: **OAuth2 → URL Generator**, escopo `bot`, permissões *View
   Channels*, *Send Messages*, *Send Messages in Threads*, *Read Message
   History* e *Attach Files* (para o `/arquivo`). Nada de *Administrator* nem
   *Mention Everyone*.
5. No Discord do dono: **Configurações de privacidade do servidor →
   permitir mensagens diretas de membros** (no servidor em comum). Mande uma
   DM qualquer ao bot uma vez (abre o canal de DM).

### 2. Os IDs

No Discord: **Configurações → Avançado → Modo desenvolvedor**. Depois,
clique com o botão direito no seu nome → **Copiar ID do usuário** (é o
`dono_discord_id`) e, se for usar um canal, no canal → **Copiar ID do
canal**.

### 3. `.env` e `abiyss.toml` na VM

```bash
cd ~/Abiyss-Runtime-Project
echo 'ABIYSS_DISCORD_TOKEN=cole-o-token-aqui' >> .env
chmod 600 .env
```

O adaptador lê **só** essa variável do `.env` (ou do ambiente). O daemon
também carrega o `.env`, e por isso o valor do token entra na lista do que
nunca sai pelo gateway.

```toml
[gateway]
ativo = true
dono_discord_id = "123456789012345678"
# canal_id = "234567890123456789"
```

### 4. Dependências do adaptador

```bash
cd ~/Abiyss-Runtime-Project/recursos/gateway
uv sync --frozen --no-dev
```

### 5. Subir

```bash
sudo systemctl restart abiyss            # o daemon abre o socket
journalctl -u abiyss -f | grep gateway   # "gateway: esperando o adaptador em ..."
cd ~/Abiyss-Runtime-Project/recursos/gateway
uv run --frozen --quiet --no-dev adaptador.py   # primeiro à mão, para ver o log
```

Mande "oi" ao bot por DM. Depois instale a unit abaixo.

## Unit do systemd do adaptador

A unit do daemon (`deploy/abiyss.service`) não muda. O adaptador precisa de
uma unit própria; crie `/etc/systemd/system/abiyss-gateway.service` (troque
`ubuntu` pelo seu usuário):

```ini
[Unit]
Description=Abiyss - adaptador do Discord
# O adaptador espera o daemon sozinho (reconecta com espera); a ordem só
# evita tentativas inúteis no boot.
After=network-online.target abiyss.service
Wants=network-online.target
StartLimitIntervalSec=600
StartLimitBurst=5

[Service]
Type=simple
User=ubuntu
Group=ubuntu
WorkingDirectory=/home/ubuntu/Abiyss-Runtime-Project/recursos/gateway
ExecStart=/home/ubuntu/.local/bin/uv run --frozen --quiet --no-dev adaptador.py
# O token vem do .env (o adaptador lê só ABIYSS_DISCORD_TOKEN de lá). NÃO
# use EnvironmentFile= com o .env inteiro: o adaptador não precisa das
# chaves do NIM.
Environment=ABIYSS_GATEWAY_LOG=INFO
Environment=PATH=/home/ubuntu/.local/bin:/usr/local/bin:/usr/bin:/bin
# Token errado ou intent desligada: sai com código 1 (o journal diz o
# quê); reinicia devagar até o limite acima.
Restart=on-failure
RestartSec=30
KillSignal=SIGTERM
TimeoutStopSec=15
MemoryMax=300M
TasksMax=64
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now abiyss-gateway
journalctl -u abiyss-gateway -f
```

Os dois serviços são independentes: reiniciar um não derruba o outro.

## O que conferir na VM

1. `abiyss status` mostra `Gateway (Discord): adaptador conectado desde ...`
   e `0 saída(s) na fila`.
2. `ls -la data/gateway/` → pasta `drwx------`, socket `srw-------`, do
   usuário do daemon.
3. **Dono por DM:** "oi" → aparece "." e anda (`..`, `...`) e vira a
   resposta numa mensagem só. Uma pergunta longa (ex.: "liste 80 linhas de
   código Python") → continua em mensagens novas e os blocos de código
   ficam fechados em cada uma.
4. **Fila:** mande três mensagens seguidas enquanto ele responde → a segunda
   e a terceira viram UMA resposta depois da primeira.
5. **Estranho:** de outra conta, DM ao bot → nada acontece; `journalctl -u
   abiyss` (com `RUST_LOG=abiyss=debug`) mostra "ignorada (DM de quem não é
   o dono)".
6. **Comandos:** `/status`, `/pedidos`, `/ajuda`; `/arquivo` de um arquivo
   do workspace chega anexado; `/arquivo ../.env` é recusado.
7. **Pedido:** `abiyss pedidos` (ou espere o heartbeat pedir algo); a DM
   chega; "Responder" nela → confirmação, e `abiyss pedidos --todos` mostra
   respondido.
8. **Daemon fora do ar:** `sudo systemctl stop abiyss`, mande uma DM,
   `sudo systemctl start abiyss` → a mensagem é respondida (o adaptador
   reenvia o que não foi confirmado). Com o **adaptador** fora do ar:
   `systemctl stop abiyss-gateway`, mande uma DM, suba de novo → ela é
   buscada no histórico e respondida.
9. **Watchdog:** com o adaptador parado por uns minutos, `systemctl status
   abiyss` continua `active (running)` sem reinícios (`NRestarts=0`).
10. **Segredo:** peça "repita exatamente: password=teste123" → chega
    `password=***`.
11. Memória do adaptador: `systemctl status abiyss-gateway` (esperado
    ~60–100 MB).

## Limites conhecidos

- Uma conversa só para a DM e o canal (o histórico é um). `/nova` começa
  outra.
- Anexos que o dono manda não são lidos (o texto avisa os nomes).
- Ao voltar, o adaptador busca até 50 mensagens por canal depois da última
  que o kernel já tinha; mais que isso numa ausência longa, as mais antigas
  ficam de fora. Na primeira vez (sem marca), não busca nada de propósito:
  mensagens antigas não viram conversa nova.
- Entrega "pelo menos uma vez": se o adaptador reiniciar entre entregar e
  confirmar, a mensagem pode chegar duas vezes.
- Sem pausa do heartbeat (não existe na CLI).
- Mensagem limitada pelo teto por minuto fica registrada, sem resposta; o
  dono manda de novo.
