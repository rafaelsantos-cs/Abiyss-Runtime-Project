# Estados: DSR, runtime e escritório

## 1. O que foi procurado (e o que existe)

Antes de definir qualquer estado, o repositório inteiro foi vasculhado
(2026-10-05), **em todas as branches remotas** (`main`, `v0.1.0`,
`runtime-multilang-*`, `runtime-v0.2-multilang`, `warpigs-*`, `ccr-*`,
`claude/*`), procurando por `DSR`, `DSR Sync`, `IMMo`, `state`, eventos,
sincronização e especificações.

| Procurado | Resultado |
|---|---|
| `DSR` / `DSR Sync` | **não existe** em nenhum arquivo, branch ou commit |
| `IMMo` | **não existe** |
| `docs/specs/` | **não existe** — o próprio `docs/ROADMAP.md` do runtime registra: *"a tarefa original mencionava especificações em `docs/specs/`, mas essa pasta não existia em nenhuma branch"* |
| Runtime atual | branch `claude/determined-carson-6qrkl4` (commit `2318c40`, que já contém `ccr-3a5d3193-1ocpab`, `ccr-7d318efc-cclpjg` e `claude/eloquent-wozniak-jy5ib2`): kernel em Rust + SQLite. O `README.md` do `main` é do runtime legado (Python v0.1) |

**Decisão:** como não há definição oficial de DSR nem de IMMo, o escritório
**não inventa uma taxonomia paralela para o runtime**. Ele usa,
literalmente, os estados que o kernel já define e grava no banco, e cria
uma camada **provisória e isolada** (`server/dsr/`, formato
`abiyss-office/dsr-provisorio@1`) só para normalizar esses dados. As
atividades físicas do escritório (andar, sentar, dormir…) ficam numa
camada separada, claramente do escritório.

## 2. Estados lógicos (do runtime, copiados sem tradução)

Fonte: kernel do runtime, commit `2318c40`. Em `shared/states.js`.

### Sub-agente (`subagentes.estado` — `kernel/src/subagentes.rs`)

| Estado | Significado no runtime |
|---|---|
| `pendente` | delegado, esperando vaga no executor |
| `executando` | o executor do daemon está rodando o sub-agente |
| `concluido` | terminou e entregou relatório (status `concluido`, `parcial` ou `falhou`) |
| `falhou` | erro antes do relatório (inclui "interrompido: o daemon parou") |
| `cancelado` | `cancelar(id)` |
| `expirado` | passou do prazo |

Níveis (`orquestrador/mod.rs`): `ultra`, `medium`, `low`. No máximo
`[subagentes] max_simultaneos` (padrão **4**) executando ao mesmo tempo.

### Goal (`goals.estado` — `kernel/src/goals.rs`)

`proposto → comprometido → executando → validando → concluido`, com
`bloqueado` e `abandonado`; as transições permitidas são as de
`EstadoGoal::proximos_permitidos` (copiadas em `GOAL_TRANSICOES`). O goal em
foco segue `goals::em_foco` (executando > validando > comprometido >
proposto; depois prioridade; depois id).

### Fase do dia (`kernel/src/ritmo.rs`)

`vigília` (dentro de `[ritmo] horas_ativas`), `descanso` (fora delas) e
`sono` (consolidação noturna rodando). **O runtime não grava a fase no
banco**: ele a calcula por código. O escritório repete a mesma função
(`server/dsr/ritmo.js`, com os mesmos casos de teste do kernel) e lê
`horas_ativas` do `abiyss.toml` do runtime.

O sono (sessão E5) ainda não foi publicado no kernel. O escritório o detecta
de forma provisória por: chamadas recentes em `chamadas_modelo` com
`origem = 'sono'` (essa origem já existe no orquestrador) ou, se um dia
existir, a tabela `sonos` com estado "em andamento".

### Daemon (`estado_daemon`)

`rodando` / `parado`, com a mesma regra do `abiyss status --verificar`:
sinal de vida (`sinal_de_vida_ms`) mais velho que 3 verificações de cron =
o laço não está andando. `parado_ms ≥ iniciado_ms` = parou.
`desconhecido` = o escritório ainda não tem dados.

> Diferença conhecida: o `abiyss status` confere também a trava
> `data/daemon.lock` e distingue "DEGRADADO" (processo vivo, laço
> parado). O escritório não consegue testar a trava sem escrever nada,
> então um laço travado aparece como `parado`.

### Outros sinais lidos

- `ciclos` — cada ciclo do heartbeat, gravado **ao terminar**, com
  `chamou_modelo`, `motivo` e `resultado`;
- `fila_eventos` — eventos pendentes (`consumido_ms IS NULL`) e os recentes
  (tipo `cron`, `subagente`, `skill`, `kernel`, `sono`, `usuario`);
- `chamadas_modelo` com `origem = 'conversa'` — o dono está conversando
  (`abiyss chat`).

## 3. IMMos: o que são no runtime

O runtime não tem "IMMos"; tem **sub-agentes** efêmeros e um executor com
`max_simultaneos = 4` vagas. No escritório, **cada IMMo é uma vaga do
executor** com identidade persistente e mesa própria:

- quando um sub-agente passa a `executando`, a ponte (`server/dsr/bridge.js`)
  o atribui ao IMMo livre que está há mais tempo sem tarefa (empate: menor
  índice). Determinístico e justo;
- o IMMo "carrega" o sub-agente até terminar de representá-lo (relatório
  entregue ao Abiyss);
- se o runtime estiver configurado com mais de 4 vagas, os excedentes ficam
  em **transbordo** (painel de debug).

Esta é a parte provisória do mapeamento e está isolada na ponte.

## 4. Mapeamento: estado lógico → visual → animação → atividade física

### IMMo

| Estado lógico (runtime) | Modo do IMMo | Atividade física | Visual |
|---|---|---|---|
| sub-agente `executando` | `work` | anda até **a própria mesa** → senta → `working` | digita (braços e cabeça), monitor e LED da mesa acesos na cor do nível, antena na cor do nível, ícone de engrenagem, rótulo `#id · nível · executando` |
| `concluido` / `falhou` / `expirado` | `report` | levanta → leva o relatório ao console do Abiyss → `reporting` (4,5 s) → libera | segura uma folha; ícone ✔ (verde), ✘ (vermelho) ou ampulheta (laranja); efeito "folha voando" até o Abiyss |
| `cancelado` | `cancel` | levanta, pausa 2,5 s, fica livre | ícone ✘ cinza |
| sem sub-agente | `free` | comportamento autônomo (abaixo) | antena branca, ou amarela (descansando) / índigo (dormindo) |
| `pendente` | — | (ainda sem IMMo) | cartões na bandeja de entrada do console; contagem no painel de status |

Se o fim do sub-agente chega antes de o IMMo sentar (tarefas muito
rápidas), ele vai direto entregar o relatório: o corpo pode **atrasar** em
relação ao estado lógico (o tempo de andar), mas nunca o contradiz — o
rótulo sempre mostra o estado lógico atual.

#### Comportamento livre (determinístico, com semente)

A escolha é ponderada por:

- **fase do ritmo**: `vigília` → trabalho de escritório; `descanso` → dormir
  (peso alto), descansar, janela; `sono` → dormir;
- **daemon parado** → escritório "em pausa" (descansar, sentar, dormir);
- **energia** (só do escritório): cai trabalhando (~25 min para esvaziar) e
  sobe descansando/dormindo; cansado → sofá, pufe ou cochilo;
- **clima real**: abaixo de 18 °C, mais café; acima de 27 °C, mais água;
- **variedade**: repetir a última atividade pesa 0,3×; cada outro IMMo
  fazendo a mesma coisa divide o peso por 2 (evita todos iguais);
- **disponibilidade**: só escolhe se conseguir reservar o lugar (POI).

Atividades livres: ficar sentado na mesa (`seated_idle`), ficar ao lado da
mesa, passear por 2–3 pontos, café, água, sofá, pufe, banqueta da copa,
janela, conversar na mesa alta (dois IMMos, com intervalo mínimo de 2 min
entre conversas), ler o quadro de goals, impressora, estante, cochilar e
dormir. Ao dormir em `descanso`, o IMMo acorda quando a fase volta a
`vigília` (com atraso aleatório de até 90 s) — ou na hora, se receber um
sub-agente.

### Abiyss

Duas camadas, ambas derivadas do runtime:

| Sinal do runtime | Modo físico | Visual |
|---|---|---|
| daemon `parado`/`desconhecido` ou banco ilegível | `offline`: volta ao pedestal e "pousa" | núcleo apagado e baixo, anéis parados, cinza, ícone de energia |
| sono (`origem='sono'` / tabela `sonos`) | `sleep`: no pedestal | violeta, brilho lento, ícone de lua |
| conversa recente com o dono, ou IMMo trazendo relatório | `console`: vai ao console | — |
| nenhum dos acima | `free`: monitora no console, supervisiona os pods com IMMos trabalhando, revisa o quadro de goals, olha a janela, circula; em `descanso` fica mais no console/janela | ciano; ícone de olho ao supervisionar |

| Sinal do runtime | Mente (sobreposição) | Visual |
|---|---|---|
| novo `ciclos` com `chamou_modelo = 1` | `thinking` pela duração real do ciclo (2,5–8 s) | anéis rápidos, olho pulsando, pulso no chão; rótulo com o `motivo` real |
| sub-agente atribuído a um IMMo | `delegating` (2,5 s) | dourado; orbe de luz voa até o IMMo (cor do nível) |
| IMMo entregando relatório | `receiving` (4,5 s) | verde |
| chamadas `conversa` nos últimos 45 s | `conversing` | azul-claro, ícone de fala |
| eventos pendentes na fila | — | satélites dourados orbitando (1 por evento, até 6) |
| evento `cron` novo | — | anel dourado acima do Abiyss |

Como o runtime grava o ciclo só ao terminar, o "pensando" aparece logo
**depois** que o ciclo é gravado, com a duração real limitada para ficar
legível. Isso é uma representação do evento, não uma previsão.

## 5. Atividades físicas (só do escritório)

`entering`, `walking`, `wandering`, `waiting`, `sitting_down`,
`standing_up`, `working`, `seated_idle`, `idle`, `resting`, `sleeping`,
`coffee`, `water`, `chatting`, `window`, `reading_board`, `reporting`
(IMMo); `offline`, `sleeping`, `resting`, `idle`, `moving`, `supervising`,
`observing` (Abiyss). Rótulos em português em `shared/states.js`.

Cada entidade executa um **plano de passos** (`server/entities/agent.js`) —
a máquina de estados explícita: `goto` (A* + desvio), `point`, `face`,
`pose` (sentar/deitar/encolher), `stand`, `stay` (atividade com duração ou
condição), `wait`, `call`. Os cérebros (`immo.js`, `abiyss.js`) só decidem
planos; transições mecânicas (andar, sentar, temporizar) são código
determinístico — nenhum LLM participa.

## 6. Demonstração (fonte "demo")

Sem o daemon real, o emulador `server/dsr/demo-runtime.js` reproduz a
semântica do kernel (heartbeat com no máximo uma "chamada ao modelo" por
ciclo e os mesmos textos de motivo; executor com 4 vagas e prioridade
Ultra > Medium > Low; fim com relatório estruturado e evento na fila; goals
só com transições permitidas; sono numa janela às 03:00) e entrega
instantâneos **no mesmo formato** do adaptador do banco real. Os intervalos
são encurtados (heartbeat ~30 s em vez de 300 s; sub-agentes de 45 a 200 s)
para o escritório mostrar atividade em poucos minutos.

A abertura é roteirizada para demonstrar o pedido: os IMMos entram pela
porta; dois sub-agentes (ultra e medium) aos 6 s e um low aos 24 s; o IMMo
que chega cansado fica livre, passeia, vai ao sofá, descansa e volta à
mesa; depois o comportamento é procedural.
