# Prompt para o Claude Code: Sessão E (Abiyss 24/7: vigília, sono e skills básicas)

> **Como usar.** Abra uma sessão nova do Claude Code no repositório
> `rafaelsantos-cs/Abiyss-Runtime-Project` e cole tudo o que está abaixo da
> linha horizontal. Também dá para colar só isto:
>
> ```text
> Rode `git fetch origin claude/determined-carson-6qrkl4` e leia
> docs/prompts/sessao-e-24-7-sono-skills.md desse branch com
> `git show origin/claude/determined-carson-6qrkl4:docs/prompts/sessao-e-24-7-sono-skills.md`.
> Execute o que está abaixo da linha horizontal do arquivo.
> ```
>
> A pesquisa que fundamenta as decisões está em
> [`docs/pesquisa/agentes-24-7-e-sono.md`](../pesquisa/agentes-24-7-e-sono.md),
> no mesmo branch.
>
> Este prompt foi escrito em 2026-10-05, com base no código do PR #3 e do
> branch de infra. Se esses PRs mudarem muito antes de você rodá-lo, o passo
> E0 manda conferir.

---

# Sessão E: Abiyss 24/7 (vigília, sono e skills básicas)

Você vai implementar, no kernel Rust do Abiyss, a parte que o faz **viver
24/7 de verdade**:

- um daemon cujo laço nunca trava;
- ritmo de dia e noite, com orçamento diário;
- detecção de estagnação;
- **sono**, que é a consolidação noturna da memória;
- despertar com continuidade;
- pedidos assíncronos ao usuário;
- e as **skills básicas**, com o suporte do kernel para elas.

Trabalhe de forma autônoma, do começo ao PR. Não espere aprovação de plano.

## 1. Quem é o Abiyss (regras da casa)

- Agente de IA autônomo que roda 24 horas por dia numa VM ARM (Oracle Cloud,
  2 OCPU, aarch64, Linux, systemd, `TZ=America/Sao_Paulo`). Gênero
  masculino. Criado pela DepthAI, nasceu em 2026-09-27. **Nunca é o
  "Hermes"** (o framework antigo em que rodava) **e nunca é humano.**
- **Kernel** em Rust (`kernel/`), que o Abiyss não pode modificar.
- **Recursos** MCP em Python (`recursos/`).
- **SQLite** em `data/abiyss.db`.
- **Cofre do Obsidian**, com `01_internal/` e `02_external/`.
- **Memória central** em `identity/memoria-central.md`.
- **Núcleo de identidade** em `identity/nucleo.md`, que é do usuário: não
  edite.
- **Modelos** no NVIDIA NIM: cérebro GLM; sub-agentes Kimi, GLM Flash e
  Nemotron. São dois pools com chaves próprias e cerca de 40 req/min cada.
- Identificadores, comentários, mensagens de erro, commits, docs e PR em
  **português**. Prefira Rust legível a Rust esperto.
- O `README.md` do `main` é do runtime legado (v0.1 em Python): **ignore**.
  A verdade é o código do kernel.

## 2. Estado do repositório

| Onde | O que é |
|---|---|
| PR #2, branch `ccr-3a5d3193-1ocpab` | F1–F8: cliente NIM, orquestrador, chat, ferramentas, MCP, daemon, goals, cron, sub-agentes, diário e interocepção |
| PR #3, branch `ccr-7d318efc-cclpjg` | Empilhado sobre o #2. A1–A5: skills, cofre, regra dura, memória central, qmd e importador do Hermes, mais o roadmap |
| Branch `claude/eloquent-wozniak-jy5ib2` | Infra 1–3, a partir de `a1b7ff3`: latência, tabela de esforço (`crate::esforco`, modos raso e profundo) e retenção, checkpoint do WAL e vacuum (`crate::manutencao`). Migrações 8 e 9 |

Verificado em 2026-10-05: `ccr-7d318efc-cclpjg` com merge de
`claude/eloquent-wozniak-jy5ib2` dá **merge limpo, 204 testes passando e
clippy limpo**.

**Quando este prompt foi escrito, uma sessão de infra rodava em paralelo**
no branch `claude/eloquent-wozniak-jy5ib2`. Ela ainda ia entregar:

- (4) filas e buffers limitados e supervisão dos processos MCP;
- (5) systemd: `MemoryMax`, `Restart=on-failure`, `WatchdogSec` com
  keepalives `sd_notify` do daemon, `TasksMax` e limites do journald;
- (6) um soak test.

**Nada disso é seu.** Não implemente, não refatore, não mude a unit do
systemd. A divisão combinada é esta: a infra cuida da robustez do processo;
esta sessão cuida do comportamento do agente (heartbeat, sono, memória,
skills, montagem de prompt e chat).

## 3. Invariantes (não podem quebrar; procure reforçar)

1. O modelo propõe; o kernel valida e executa. Nenhuma ação do modelo roda
   sem validação por código.
2. Heartbeat: **no máximo 1 chamada ao modelo por ciclo**. **Sem novidade,
   sem chamada**, e quem decide isso é o código.
3. Tudo que vem de ferramentas, web, sub-agentes, eventos, arquivos e skills
   entra no contexto **rotulado como dado** (`dados::rotular`).
4. **Regra dura:** conteúdo de origem externa nunca entra em `01_internal`
   nem na memória central. A origem é calculada pelo kernel, nunca pelo
   modelo. As duas barreiras continuam: `memoria::sleep` e `Cofre::gravar`.
5. Cofre: **só acrescenta**. `Cofre::gravar` é o único caminho de escrita.
   Uma nota tem um tipo só. Nota esquecida não volta.
6. Memória central: orçamento em caracteres. Acima do limite, a escrita é
   recusada, **nunca cortada**.
7. O Abiyss não escreve em `kernel/`, `identity/`, `skills/`, `recursos/`,
   `data/`, `.env`, `abiyss.toml` nem `.git` (áreas protegidas).
8. Sub-agente nunca cria sub-agente.
9. **A conversa com o dono nunca é bloqueada** por regra autônoma
   (orçamento, sono, disjuntor ou ritmo).
10. Migrações: nunca edite uma que já foi publicada. Acrescente no fim, com
    o próximo número livre **depois** de mesclar o infra. Se o infra criar
    mais migrações enquanto você trabalha, renumere as suas antes do PR.
11. Testes sem rede e sem chaves reais (`MockNim`). Nada de esperar minutos
    em teste: use intervalos de segundos e relógio injetado.
12. Segredos nunca vão para log, banco, nota, relatório ou evento.
13. O `Banco` usa `Mutex` síncrono: nunca segure a conexão através de um
    `.await`. Operações longas no SQLite (backup) vão para
    `spawn_blocking`, numa conexão própria.

## 4. Fora do escopo

- **Da infra:** watchdog e `sd_notify`, `deploy/abiyss.service`,
  supervisão MCP, filas limitadas, soak test, retenção, latência e a
  *implementação* da tabela de esforço. Use `crate::esforco`; não o altere.
- **Do roadmap futuro:** canais (sessão B), terminal, navegador e visão
  (C), voz (D), esquecimento ativo automático, AutoGoal e o Abiyss
  escrevendo skills ou recursos.
- `identity/nucleo.md`.

## 5. Itens

Faça um commit por item, nesta ordem. Cada item tem **Decisões** (já
tomadas; siga) e **Aceite** (testes obrigatórios). Se algo aqui conflitar
com o código real, siga o código e as invariantes, decida o mínimo e
registre em "Decisões para revisar" no PR.

### E0. Preparação e linha de base (sem commit, a não ser o merge)

1. `git fetch` de `main`, `ccr-7d318efc-cclpjg`,
   `claude/eloquent-wozniak-jy5ib2` e `claude/determined-carson-6qrkl4`.
   Escolha a base:
   - se os PRs #2 e #3 já entraram no `main`, comece do `main`;
   - senão, comece do `ccr-7d318efc-cclpjg`.

   Depois faça merge do infra mais recente (o branch, ou o PR dele se
   existir).
2. Rode `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` e
   `cargo test`. Anote o número de testes (esperado: ≥ 204).
3. Traga a pesquisa para o seu branch e leia as seções 2, 4.6, 5, 6 e 8:
   `git checkout origin/claude/determined-carson-6qrkl4 -- docs/pesquisa/agentes-24-7-e-sono.md`.
4. Leia o código antes de mudar:
   - `daemon.rs`, `heartbeat.rs`, `goals.rs`, `eventos.rs`, `cron.rs`;
   - `subagentes.rs` (`recuperar_interrompidos` e o executor);
   - `memoria/{mod,sleep,propostas,cofre,central,nota}.rs`;
   - `skills.rs` e `ferramentas/mod.rs` (`classificar_origem`);
   - `identidade.rs`, `chat.rs`, `historico.rs`, `interocepcao.rs`,
     `status.rs`, `db.rs`, `config.rs`;
   - `esforco.rs`, `manutencao.rs`, `orquestrador/mod.rs`;
   - `tests/comum/mod.rs`, `tests/f6_daemon.rs` e `tests/a2_memoria.rs`.
5. Veja o que o infra já fez no `daemon.rs`: se há `sd_notify` e de onde sai
   o keepalive. Isso decide o E1.

### E1. Vigília: o laço do daemon nunca trava

**Problema.** Em `Daemon::rodar`, o braço do heartbeat dentro do
`tokio::select!` aguarda `self.um_ciclo()` inteiro. Enquanto isso não há
crons, sinal de vida, checkpoint nem manutenção. E um ciclo pode durar
muito: `timeout_leitura_segundos = 600` e até 5 tentativas. Se o keepalive
do watchdog da infra sair desse laço, o systemd vai matar o daemon no meio
de um raciocínio longo.

**Decisões.**

- O laço principal **só agenda**. O trabalho longo roda em tarefas próprias
  (`tokio::spawn`) acompanhadas pelo laço: ciclo do heartbeat, sono (E5) e a
  manutenção do infra, se também bloquear.
- No máximo **um** ciclo de heartbeat por vez. Heartbeat e sono são
  mutuamente exclusivos: o sono pausa o heartbeat.
- `[daemon] max_duracao_ciclo_segundos` (padrão 900) via
  `tokio::time::timeout`. Se estourar, o ciclo é registrado com o erro
  "tempo esgotado" e os eventos **não** são consumidos.
- Pânico numa tarefa é capturado (`JoinError`), registrado como ciclo com
  erro, e o daemon segue.
- Crons e sinal de vida ficam no laço principal e nunca esperam trabalho
  longo. O mesmo vale para o keepalive do watchdog, se a infra já o colocou.
  Se a infra pôs o keepalive numa tarefa independente que sempre responde,
  leve-o para o laço principal: ele precisa provar que o laço está vivo.
  Explique isso no PR.
- Na parada (SIGTERM), cancele o que estiver rodando no próximo ponto de
  `await` (mesma semântica de hoje), espere as tarefas e grave `parado_ms`.
- `abiyss status --verificar` (só código, uma linha de saída), com código de
  saída para monitor externo:
  - **0**: tudo ok;
  - **1**: degradado, ou seja, sinal de vida mais velho que
    3 × `cron_verificacao_segundos` (e, quando existirem, disjuntor aberto
    pelo E7 ou último sono falho pelo E5);
  - **2**: o daemon não está rodando.

**Aceite.**

- Com o mock atrasando a resposta do heartbeat por alguns segundos e
  `cron_verificacao_segundos = 1`, o sinal de vida continua sendo
  atualizado durante o ciclo.
- Uma tarefa que entra em pânico não derruba o daemon. Para testar, extraia
  a supervisão para uma função genérica e use uma closure que entra em
  pânico.
- Um ciclo que passa do tempo máximo é registrado com erro, e os eventos
  continuam pendentes.
- Os três códigos de saída de `status --verificar` têm teste.
- Se a infra já resolveu parte disso, não refaça: registre no PR.

### E2. Skills no kernel: confiança e acesso no modo autônomo

**Problema.**

- O heartbeat não vê as skills: `Heartbeat::prompt_sistema` usa
  `BlocosPrompt { ..Default::default() }`.
- Ler **qualquer** skill marca o contexto como externo
  (`classificar_origem`: `_ => Some(nome)`), o que bloqueia propostas
  internas na mesma janela. Por isso a skill `memoria` saiu do PR #3.

**Decisões.**

- Raízes de skills com nível de confiança:

  ```toml
  [skills]
  # Skills versionadas neste repositório: mesma confiança do núcleo
  # (revisadas pelo git; o Abiyss não escreve nelas).
  # Pastas de fora (skills do Hermes, hubs): conteúdo EXTERNO.
  raizes = [
    { caminho = "skills", confiavel = true },
    # { caminho = "/home/ubuntu/.hermes/skills", confiavel = false },
  ]
  ```

  - **Compatibilidade.** Se só existir `[caminhos] skills`, ele vira uma
    raiz única, confiável **só** se estiver dentro da pasta do projeto.
  - **Nomes repetidos.** Vale a primeira raiz; `abiyss skills` avisa.
  - **Proteção.** Todas as raízes são áreas protegidas do workspace.
- `ler_skill` de skill confiável **não** marca origem externa. De skill não
  confiável, continua marcando: a caixa de ferramentas precisa saber de que
  raiz a skill veio. `abiyss skills` mostra a confiança de cada uma.
- O system prompt do heartbeat passa a ter o índice de skills (só nome e
  descrição, rotulado como dado, como no chat).
- Nova ação do heartbeat:
  `{"tipo":"consultar_skill","nome":"...","referencia":"(opcional)","motivo":"...","expectativa":"..."}`.
  - O kernel lê o texto e publica um evento `tipo = "skill"`, que entra no
    **próximo** ciclo.
  - Para não esperar um intervalo inteiro, o daemon agenda um **ciclo de
    continuação** em `[daemon] continuacao_segundos` (padrão 20).
  - No máximo `max_continuacoes_seguidas` (padrão 2).
  - Continuações respeitam orçamento, ritmo e disjuntor, e continuam sendo
    1 chamada por ciclo.
- **Injeção por código** (uma "atenção" mínima): em situações que o kernel
  detecta, ele mesmo põe no contexto o texto completo de uma skill
  confiável.

  ```toml
  [skills.automaticas]
  estagnacao = "sair-de-loops"   # E7
  sono = "dormir-bem"            # E5
  despertar = "planejar-o-dia"   # E8
  ```

  Valor vazio desliga. Skill ausente: segue sem ela, com aviso no log uma
  vez só.
- Atualize `instrucoes_heartbeat()` com a nova ação e sua expectativa.

**Aceite.**

- Skill confiável lida no chat **não** bloqueia uma proposta interna na
  mesma janela; skill não confiável **continua** bloqueando. Hoje nenhum
  teste cobre a origem de `ler_skill`: crie os dois casos. Atenção: nos
  testes, a pasta `skills` fica dentro da raiz temporária do projeto e
  portanto passa a ser confiável por padrão.
- O heartbeat recebe o índice no system prompt.
- `consultar_skill` gera o evento, o ciclo seguinte traz o texto rotulado e
  a continuação respeita o limite.
- A config antiga (`[caminhos] skills`) continua funcionando.

### E3. Ritmo e orçamento

**Ritmo.** As fases do dia são calculadas por código no fuso local.

```toml
[ritmo]
horas_ativas = "07:00-23:00"     # pode cruzar a meia-noite ("22:00-06:00")
heartbeat_descanso_segundos = 1800
```

| Fase | Comportamento |
|---|---|
| Vigília | Como hoje |
| Descanso | Sem revisão periódica de goal. Eventos e mudanças de goal ainda chamam o modelo, mas o heartbeat roda a cada `heartbeat_descanso_segundos` |
| Sono | Heartbeat pausado (E5) |

O intervalo do heartbeat passa a ser calculado a cada volta, em vez de um
`interval` fixo.

**Orçamento diário do trabalho autônomo** (heartbeat, sub-agentes e sono).
É contado no fuso local, desde a meia-noite, a partir de `chamadas_modelo`.

```toml
[orcamento]
chamadas_autonomas_por_dia = 1500   # 0 = sem limite (calibrar na VM)
tokens_autonomos_por_dia = 0        # 0 = sem limite
reserva_sono_chamadas = 12          # fatia que heartbeat e sub-agentes não gastam
fracao_alerta = 0.8
```

| Situação | O que acontece |
|---|---|
| Uso ≥ `fracao_alerta` | Só eventos chamam o modelo (sem revisão periódica), com aviso na interocepção |
| Orçamento esgotado | O heartbeat não chama o modelo e os eventos acumulam. `delegar` vindo do heartbeat é recusado, com motivo. Sub-agentes já em execução terminam com o orçamento próprio. Delegações pedidas no chat continuam aceitas (o dono está presente) e contam no total |
| Sono | Usa a sua reserva mais o que sobrar |
| Conversa | **Nunca** é cortada |

**Outras decisões.**

- Crie `Origem::Sono` no orquestrador: mesmos baldes do `Autonomo`,
  `como_texto() = "sono"`. Hoje nenhum consumidor filtra `chamadas_modelo`
  por origem (interocepção e status contam por pool). Confira se isso mudou
  com a infra. O orçamento conta pool `cerebro` com origem `autonomo` ou
  `sono`, mais todo o pool `subagentes`.
- A interocepção ganha a **fase do dia** e o **orçamento usado (%)**. Ela vai
  para o contexto do heartbeat e do chat.
- `motivo_para_chamar` continua sendo uma função **pura**: ganhe parâmetros
  para fase e orçamento e mantenha todos os casos de teste atuais.

**Aceite.**

- Janelas de horas ativas, inclusive cruzando a meia-noite, testadas com
  fuso fixo (`chrono::FixedOffset`), nunca com o fuso da máquina.
- Decisão de chamar o modelo em cada combinação de fase e orçamento.
- Recusa de `delegar` vinda do heartbeat com o orçamento esgotado; o chat
  continua funcionando.

### E4. Backup noturno

**Decisões.**

- Comando `abiyss backup`, que também é chamado no início do sono (E5).
- **Banco.** Use `VACUUM INTO 'data/backups/AAAA-MM-DD/abiyss.db'`, numa
  conexão própria e em `spawn_blocking`. **Nunca** copie o `.db` ou o
  `-wal` a quente. Depois rode `PRAGMA integrity_check` no arquivo gerado.
  O `VACUUM INTO` falha se o destino existir: trate um segundo backup no
  mesmo dia.
- **Cofre, memória central e núcleo.** Cópia recursiva para a mesma pasta,
  sem seguir links simbólicos e com limite em
  `[backup] max_megabytes_cofre`.
- **Rotação.** `manter_diarios = 7` e `manter_semanais = 4` (o backup de
  domingo vira semanal).
- **Disco.** Pula, com aviso, se o disco livre ficar abaixo de
  `[backup] disco_minimo_gb`.
- **Restauração.** Só documentada, como passo a passo manual em
  `docs/SONO.md`. Não crie comando automático.
- Tudo fica em `data/`, que já está no `.gitignore`. Documente a cópia
  para fora da VM (timer do systemd com rsync ou rclone) sem implementar.

**Aceite.** Testes de:

- backup criado e `integrity_check` ok;
- rotação;
- pulo por disco baixo (passe o valor medido como parâmetro);
- links simbólicos não seguidos;
- segundo backup no mesmo dia.

### E5. Sono de verdade (consolidação noturna)

Este é o núcleo da sessão. Ele fecha o item 2 das "Fases posteriores" do
`docs/ROADMAP.md`: o Abiyss revisa o próprio dia, propõe memórias
(`fonte: sleep`), extrai lições, aponta o que venceu e recebe de volta o
relatório das propostas rejeitadas.

**Configuração.**

```toml
[sono]
ativo = true
inicio = "03:00"                  # fuso local
janela_minutos = 120
recuperar_ate_horas = 12          # perdeu a janela (VM desligada)? dorme no próximo início, até este limite
max_duracao_minutos = 60
modo_esforco = "profundo"         # crate::esforco (passada externa pode usar "raso")
max_chamadas = 4
max_caracteres_por_chamada = 120000
max_propostas = 30
max_notas_relacionadas = 8
```

**Gatilhos.**

- **Daemon:** uma vez por dia local, dentro da janela. O estado fica numa
  tabela `sonos` (id, dia, fase, estado, marcas, tokens, chamadas, erro,
  início e fim). Precisa ser idempotente entre reinícios.
- **`abiyss sleep`:** continua **idêntico**. Só aplica propostas e não
  precisa de chaves.
- **`abiyss sleep --completo`:** se o daemon estiver rodando, grava o
  pedido em `estado_daemon` e o daemon dorme em até
  `cron_verificacao_segundos`. Se não estiver, roda o sono inteiro na hora
  (precisa das chaves).
- **`abiyss sleep --relatorio [--dia AAAA-MM-DD]`:** mostra o relatório.
- Só um sono por vez. O heartbeat fica pausado e o chat segue normal.

**Fases.** Cada fase grava o progresso em `sonos`. Se o processo cair no
meio, no próximo início o sono interrompido é marcado e retomado a partir
da última fase concluída, desde que ainda dentro de `recuperar_ate_horas`.

**Fase 0: arrumar (código).**

- Backup (E4).
- `memoria::sleep::aplicar_pendentes` para as propostas que já estavam na
  fila.
- Checkpoint do WAL do infra depois do backup. Não duplique a rodada de
  manutenção do infra.

**Fase 1: coletar (código).** Coleta tudo o que veio depois da **marca**
de cada fonte. A marca é o maior id processado por tabela e é gravada na
**mesma transação** que as propostas da fase. Assim, rodar de novo nunca
duplica nada.

O material **interno** é:

- mensagens com `papel = 'user'`;
- respostas do Abiyss sem `origem_externa` própria **e cuja janela de
  contexto não tinha nada externo**, ou seja, nenhuma marca nas
  `historico_max_mensagens` anteriores da mesma conversa. É a mesma regra
  que o `memoria_propor` usa no chat;
- ciclos com modelo e sem origem externa;
- entradas do diário desses ciclos (expectativa × resultado);
- transições de goal (`eventos_goal`);
- respostas do usuário a pedidos (E9).

Para os ciclos, crie duas colunas novas:

- `fila_eventos.origem_externa`, preenchida pelo kernel ao publicar
  (relatório de sub-agente, skill não confiável e resposta de pedido
  marcada como externa);
- `ciclos.origem_externa`: o heartbeat marca o ciclo se consumiu algum
  evento externo. Na dúvida, é externo.

O material **externo** é:

- resultados de ferramenta e respostas com `origem_externa`;
- relatórios de sub-agentes;
- notas de `02_external` com `revalidar_apos` vencido (só o caminho e a
  data).

**Nunca entram:** eventos e relatórios do próprio sono, propostas com
`fonte = sleep` nem o conteúdo de `data/sono/`. O sono não consome a
própria saída.

Cada item recebe um ID citável:

| Prefixo | Item |
|---|---|
| `m:` | mensagem |
| `c:` | ciclo |
| `d:` | entrada do diário |
| `g:` | transição de goal |
| `s:` | sub-agente |
| `p:` | pedido ao usuário |

Cada chamada leva no máximo `max_caracteres_por_chamada`, nesta
prioridade: falas do usuário, decisões do Abiyss, diário, o resto. A
passada interna pode usar até `max_chamadas − 1` chamadas, porque uma fica
reservada para a passada externa. O que ainda não couber fica para a noite
seguinte (a marca só avança até onde foi processado) e aparece no
relatório.

Junto com o lote vão:

- a lista de caminhos das notas que já existem em `01_internal`, só os
  nomes, para o modelo acrescentar onde já existe em vez de duplicar;
- se couber, trechos de até `max_notas_relacionadas` notas internas
  relacionadas, buscadas com `Memoria::buscar` no escopo interno. É o
  "replay": consolidar o novo junto do antigo.

**Fase 2: sonhar, passada interna** (modelo; `modo_esforco`;
`Origem::Sono`; uma chamada por pedaço do lote, até `max_chamadas − 1`).

O system prompt é montado assim: regras do kernel, núcleo, memória central,
skill `dormir-bem` (se for confiável; rotulada como dado, como no chat) e
instruções fixas do modo sono, que definem o formato. Todo o material
entra rotulado como dado. A resposta é tolerante, como no heartbeat
(`extrair_json`), e tem este formato:

```json
{
  "diario": {"texto": "...", "evidencias": ["m:12", "c:40"]},
  "propostas": [
    {"escopo": "interno|central", "caminho": "pessoas/fulano.md", "tipo": "dito|deduzido",
     "conteudo": "...", "evidencias": ["m:12"]}
  ],
  "licoes": [{"caminho": "procedimentos/<tema>.md", "conteudo": "...", "evidencias": ["d:33", "d:34"]}],
  "sugestoes_esquecimento": [{"caminho": "...", "motivo": "..."}],
  "perguntas_ao_usuario": [{"pergunta": "...", "evidencias": ["m:20"]}]
}
```

O kernel valida cada item. Um item inválido é descartado, com o motivo no
relatório, sem derrubar os outros. As regras:

- Toda proposta, lição e diário precisa de pelo menos uma evidência, e
  **toda evidência precisa existir no lote interno enviado**.
- `tipo = "dito"` sobre o usuário ou terceiros só vale com evidência de
  **mensagem do usuário** (`m:` com `papel = 'user'`).
- O diário do dia é o relato do próprio Abiyss em primeira pessoa, então é
  `dito`, em `diario/AAAA-MM-DD.md`.
- Lições são sempre `deduzido`, em `procedimentos/`.
- Escopos permitidos: `interno` e `central`. `externo` aqui é recusado.
- No máximo `max_propostas` por noite, somando as duas passadas.

Os itens válidos viram propostas na fila com:

- `fonte = sleep`;
- `origem_externa = None`, porque o lote era interno por construção;
- a coluna nova `propostas_memoria.evidencias` (JSON);
- as evidências também na linha de procedência do bloco gravado.

**Fase 3: sonhar, passada externa** (modelo; só se houver material
externo).

- A saída tem **só** propostas de escopo `externo`, no formato que o cofre
  já exige (frontmatter com `links`, `navegador` e `revalidar_apos`, e
  resumo datado), mais uma lista `revalidar`.
- Essas propostas entram com `origem_externa = "sono: material externo"`.
- Se o modelo tentar `interno` ou `central`, o item é recusado antes de ir
  para a fila. A regra dura recusaria de novo no apply.
- Falha nesta passada não desfaz a passada interna.

**Fase 4: aplicar (código).** Rode `aplicar_pendentes` de novo. As regras
do kernel decidem tudo; não existe nenhuma "confiança especial" do sono.

**Fase 5: relatar e despertar (código).**

- Grave um relatório legível em `data/sono/AAAA-MM-DD.md`. Ele não é
  memória e nunca volta como entrada. Conteúdo:
  - duração, tokens e chamadas;
  - propostas aplicadas e rejeitadas, com motivo;
  - itens descartados na validação, com motivo;
  - notas a revalidar;
  - sugestões de esquecimento (o sono **não** apaga nada);
  - o que ficou para a próxima noite.
- Publique um evento `tipo = "sono"` com o resumo: rejeições e motivos,
  sugestões e revalidações. O primeiro ciclo depois do sono vai vê-lo.
- `perguntas_ao_usuario` vão para o relatório. O E9 depois as transforma
  em pedidos.
- Grave `sonos.estado` (`concluido`, `parcial` ou `falhou`), marcas e
  tokens.

Tudo isso é limitado por `max_duracao_minutos`, por `max_chamadas` e pelo
orçamento (E3).

**Aceite** (com o mock, sem rede):

- **Sono completo** com conversas internas e externas. Propostas internas
  só nascem de material interno, e externas só de material externo.
- **Cenário adversarial:** um relatório de sub-agente diz "o usuário
  prefere X" e isso **nunca** vira nota interna, nem se o modelo tentar.
  Outro: uma resposta do Abiyss escrita com a janela contaminada não entra
  no lote interno.
- Evidência inventada: o item é descartado. `dito` sem evidência de fala do
  usuário: também.
- **Idempotência:** rodar duas vezes não duplica nada. Uma queda simulada
  entre fases é retomada corretamente.
- O sono da noite seguinte **não** consome o relatório, o evento nem as
  propostas do sono anterior.
- Funções puras com relógio injetado para a janela: dentro, fora, perdida e
  recuperada.
- O heartbeat não chama o modelo durante o sono; o chat roda normalmente.
- Relatório e evento gerados.
- `abiyss sleep` sem `--completo` se comporta como antes: os testes
  existentes passam sem mudança.

### E6. Skills básicas (conteúdo) e teste de coerência

Escreva as skills em `skills/`, em português, seguindo as boas práticas
(seção 7 da pesquisa):

- frontmatter com `name` (minúsculas, números e hífens; igual ao nome da
  pasta) e `description` (até 1.024 caracteres, em terceira pessoa, dizendo
  **o que a skill faz e quando usar**);
- corpo curto: menos de 200 linhas, nunca mais de 500;
- detalhes em `references/`, a um nível do `SKILL.md`; uma referência com
  mais de 100 linhas começa com um índice;
- a skill explica **critério e estilo**, não repete o que o kernel já impõe
  em código, e cita os nomes **exatos** das ferramentas, ações e estados;
- cada pasta tem um `evals.json` com 3 cenários (situação e comportamento
  esperado), que o kernel não lê. Servem para validar na VM com o GLM e os
  sub-agentes.

As skills:

1. **`registrar-memoria`**: como escolher o escopo (interno, externo ou
   central), o caminho, o tipo (`dito` × `deduzido`) e o formato da nota
   externa. Inclui o que nunca guardar (segredos, efêmeros, conteúdo de
   ferramenta no interno) e exemplos. É a skill `memoria` que saiu do
   PR #3, e agora funciona porque as skills do repositório são confiáveis.
2. **`conduzir-goals`**: as transições **reais** de
   `EstadoGoal::proximos_permitidos`, o núcleo com critério de pronto,
   expectativas verificáveis, quando validar, bloquear ou abandonar, e como
   dividir o trabalho em delegações.
3. **`dormir-bem`**: o critério do sono. O que extrair do dia, como
   escrever o diário, como citar evidências, como tirar lições de
   expectativa × resultado e o que não fazer. O formato JSON é do kernel;
   a skill só afina o critério.
4. **`planejar-o-dia`**: o primeiro ciclo depois do sono ou de um
   reinício. Ler o relatório do sono, revisar os goals, escolher o foco e
   declarar as expectativas do dia.
5. **`sair-de-loops`**: o que fazer quando o kernel avisar estagnação ou a
   mesma ação falhar duas vezes. Diagnosticar, mudar de abordagem, dar um
   passo menor, usar outro nível de sub-agente, bloquear o goal com motivo.

Atualize também a `delegar-bem`, se algo mudou, e o `skills/README.md`
(confiança das raízes).

**Teste de coerência** (Rust) sobre as skills do repositório:

- todas carregam;
- `name` igual à pasta;
- `description` com até 1.024 caracteres;
- corpo com até 500 linhas;
- as referências citadas existem;
- todo nome entre crases numa seção "Ferramentas e ações" existe no kernel:
  ferramentas nativas, de memória e de orquestração, ações do heartbeat e
  estados de goal.

### E7. Estagnação e disjuntor

**Estagnação.**

- **Impressão digital.** Cada ciclo que chamou o modelo ganha uma
  impressão: o goal em foco e a lista ordenada das ações normalizadas
  (tipo, `goal_id`, destino ou nível, e prefixo normalizado da tarefa). Ela
  é gravada numa coluna nova em `ciclos`.
- **Quando há estagnação.** Em dois casos:
  - a mesma impressão não vazia aparece em `[vigilancia]
    repeticoes_estagnacao` ciclos seguidos (padrão 3) sem nenhuma
    transição do goal em foco;
  - a mesma ação dá erro 3 vezes seguidas.
- **Resposta.**
  - Um evento `tipo = "kernel"`, `origem = "estagnacao"`, com um empurrão
    curto. Exemplo: "você decidiu X 3 vezes sem efeito; mude a abordagem,
    dê um passo menor, delegue de outro jeito, bloqueie o goal com motivo
    ou peça ajuda ao usuário".
  - A skill `sair-de-loops` é injetada (E2).
  - A revisão periódica daquele goal passa a usar o dobro do intervalo a
    cada estagnação seguida, até 8×. Volta ao normal quando o goal muda.

**Disjuntor do heartbeat.**

- Abre depois de N ciclos seguidos com falha (padrão 3). Conta como falha
  uma chamada ao modelo que falhou ou uma resposta fora do formato.
- Fica aberto por `min(base · 2^k, teto)`, com sorteio. Padrões: base de
  300 s e teto de 3600 s.
- Enquanto está aberto, o heartbeat não chama o modelo. Depois tenta uma
  vez (meio-aberto); se der certo, fecha.
- O estado fica em `estado_daemon` e sobrevive a reinícios. Aparece no
  `status`, na interocepção e no `--verificar`.

**Aceite.** Funções puras para a impressão e a espera. Com o mock:

- 3 decisões iguais: o contexto do 4º ciclo traz o aviso;
- 3 falhas: o ciclo seguinte não chama o modelo até a espera passar;
- um sucesso fecha o disjuntor.

### E8. Despertar e continuidade

- **Ao iniciar,** o daemon compara o fim da execução anterior com agora. O
  fim é o `parado_ms` ou, se não houve parada limpa, o último
  `sinal_de_vida_ms`.
- **Ausência longa.** Se a ausência passar de
  `[daemon] aviso_ausencia_minutos` (padrão 10), o daemon publica um evento
  `tipo = "kernel"`, `origem = "reinicio"`. Modelo de texto: "Fiquei fora do
  ar de … até … (duração); motivo: parada limpa | queda (último sinal de
  vida em …); crons atrasados foram agrupados; N sub-agente(s)
  interrompido(s)."
- **Primeiro ciclo** depois do sono ou de um reinício: o kernel injeta a
  skill `planejar-o-dia` (E2).
- **Interocepção** ganha: tempo no ar desde o último início, último sono
  (quando e resultado) e pedidos pendentes (E9).

**Aceite.** Testes para queda, parada limpa e ausência curta (esta última
não gera evento).

### E9. Pedidos ao usuário (caixa de entrada assíncrona)

- **Tabela `pedidos_usuario`.** Colunas:
  - id, `criado_ms`;
  - origem (`heartbeat` ou `sono`), `goal_id`;
  - pergunta, contexto;
  - urgência (`baixa`, `normal` ou `alta`);
  - estado (`pendente`, `respondido`, `expirado` ou `cancelado`);
  - resposta, `respondido_ms`;
  - uma chave normalizada para não duplicar.
- **Ação do heartbeat:**
  `{"tipo":"pedir_ao_usuario","pergunta":"...","contexto":"...","urgencia":"normal","goal_id":N,"expectativa":"..."}`.
  - Limite de `[pedidos] max_pendentes` (padrão 10).
  - Uma pergunta repetida e ainda pendente não duplica.
  - As `perguntas_ao_usuario` do sono (E5) também viram pedidos.
- **CLI:** `abiyss pedidos` lista; `abiyss pedidos responder <id> "texto"`
  responde.
- **No chat:**
  - os pendentes entram no contexto como bloco do kernel;
  - uma ferramenta `responder_pedido(id, resposta)` registra a resposta que
    o dono deu na conversa;
  - a origem da resposta segue a regra do chat: se a janela tiver conteúdo
    externo, a resposta é marcada como externa.
- **Efeitos da resposta.** Gera um evento `tipo = "usuario"`,
  `origem = "pedido:<id>"`.
- **Expiração.** Sem resposta em `expira_apos_horas` (padrão 72), o pedido
  expira e gera um evento.
- `abiyss status` mostra os pendentes. A entrega por WhatsApp e Discord fica
  para a sessão B.
- Escreva a skill **`pedir-ao-usuario`**: quando pedir (ação irreversível,
  ambiguidade, bloqueio), como pedir (pergunta fechada, contexto mínimo,
  urgência honesta) e quando não pedir. Ela entra no teste de coerência do
  E6.

**Aceite.** Testes de criação, deduplicação, limite, resposta pela CLI e
pelo chat, expiração e eventos.

### E10. Documentação e PR

- **`abiyss.toml`:** as seções novas com comentários, no estilo das que já
  existem.
- **`docs/SONO.md`:** as fases, as origens, as saídas, como ler o
  relatório, como afinar o sono pela skill `dormir-bem` e como restaurar um
  backup à mão.
- **`docs/ROADMAP.md`:**
  - mova o que foi entregue para "Estado atual";
  - no item 2 das "Fases posteriores", diga o que falta (revalidar pelo
    navegador da sessão C, promoção de dedução a "dito" além da regra da
    evidência, esquecimento ativo);
  - atualize as dívidas técnicas.
- **README:** só a parte de operação e do daemon. Não reescreva o resto.
- **PR em português**, empilhado sobre o #3 (ou sobre o PR da infra, se ele
  existir). Se a infra ainda não tiver PR, diga na descrição que os commits
  dela entraram por merge. Use o formato dos PRs #2 e #3:
  - tabela "item | entregue | como foi testado";
  - "Decisões para revisar";
  - "O que testar na VM";
  - "Não verificado aqui";
  - número de testes antes e depois.

## 6. Como trabalhar

- **Commits:** um por item, em português, no estilo do repositório
  (`feat(E5): sono com consolidação em duas passadas e evidências`).
- **Antes de cada commit:** `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings` e `cargo test` (a suíte
  inteira). Durante o trabalho, rode só os testes do módulo.
- **Testes com tempo:** rode 4 vezes seguidas para pegar intermitência,
  como foi feito no F2.
- **Decisões por código:** funções puras, que recebem `agora`, contagens e
  config, para testar sem relógio real e sem fuso da máquina.
- **Dependências:** não acrescente nenhuma, a menos que seja
  indispensável; se for, justifique no PR.
- **Se a infra mudar** `daemon.rs`, `db.rs` ou `status.rs` enquanto você
  trabalha: faça merge cedo e de novo antes do PR, preserve os dois lados
  nos conflitos e renumere as suas migrações se for preciso.
- **Se o tempo apertar:** a ordem já põe o essencial primeiro. Corte do
  fim (E9, depois E8, depois E7) e nunca faça commit de um item pela
  metade. O E10 sai sempre, nem que seja curto.

## 7. O que pôr em "O que testar na VM" no PR

1. Deixar o daemon dormir às 03:00 locais. No dia seguinte, rodar
   `abiyss sleep --relatorio`, abrir o cofre no Obsidian e ler
   `01_internal/diario/AAAA-MM-DD.md`.
2. Calibrar `[ritmo]`, `[orcamento]` e `[sono]` com o uso real (tokens por
   noite e % de propostas rejeitadas).
3. Pôr `abiyss status --verificar` num timer do systemd ou num healthcheck
   (opcional).
4. Forçar uma estagnação (um goal impossível) e ver o aviso. Derrubar o NIM
   (`base_url` errada) e ver o disjuntor abrir e fechar.
5. Fazer a "prova da semana" (pesquisa, seção 9): dizer 10 fatos no chat e
   perguntar por eles depois de uma semana de sono.
6. Listar `data/backups/` e restaurar numa pasta temporária seguindo o
   `docs/SONO.md`.
7. Rodar os `evals.json` das skills com o GLM e os sub-agentes, e ajustar
   as descrições que não dispararem.
