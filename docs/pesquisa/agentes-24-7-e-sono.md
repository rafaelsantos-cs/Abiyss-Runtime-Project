# Pesquisa: sono e boas práticas para agentes 24/7 (aplicada ao Abiyss)

> **Data:** 2026-10-05.
> **Código estudado:** branch `ccr-7d318efc-cclpjg` (PR #3, que já inclui o
> PR #2) somada ao branch `claude/eloquent-wozniak-jy5ib2` (infra 1–3). A
> junção dos dois compila, passa nos 204 testes e no `clippy -D warnings`.
> O `README.md` do `main` descreve o runtime legado (v0.1 em Python) e **não**
> foi usado como referência.
> **Documento irmão:** [`docs/prompts/sessao-e-24-7-sono-skills.md`](../prompts/sessao-e-24-7-sono-skills.md),
> o prompt para o Claude Code implementar o que está aqui.

## Como ler

| Seção | Conteúdo |
|---|---|
| [Resumo](#resumo-em-12-pontos) | As conclusões, em 12 linhas |
| [1](#1-perguntas-de-pesquisa) | As perguntas que a pesquisa responde |
| [2](#2-o-abiyss-hoje) | O que o Abiyss já tem e as lacunas encontradas no código |
| [3](#3-sono-na-neurociência-inspiração-não-especificação) | Neurociência do sono, como inspiração de projeto |
| [4](#4-sono-em-agentes-de-ia-estado-da-arte) | Sono em agentes de IA: artigos e produtos |
| [5](#5-operar-247-engenharia) | Engenharia de processos 24/7 |
| [6](#6-segurança-de-um-agente-que-roda-sozinho) | Segurança de um agente autônomo com memória |
| [7](#7-skills-o-que-a-pesquisa-diz) | Skills |
| [8](#8-decisões-propostas-para-o-abiyss) | Decisões para o Abiyss (viram o prompt) |
| [9](#9-agenda-de-pesquisa-o-que-medir-na-vm) | O que medir na VM para validar |
| [10](#10-referências) | Referências |

**Método.** Li o código do kernel (não a documentação antiga), rodei a suíte
de testes e pesquisei na web. Algumas fontes foram lidas na íntegra:
documentação da Anthropic (engenharia, Agent Skills, Dreams), documentação do
OpenClaw no GitHub e o README do Hermes Agent. De outras eu só li o resumo,
porque a rede desta sessão bloqueou arxiv.org, letta.com, andonlabs.com,
freedesktop.org e sqlite.org: os artigos (Sleep-time Compute, Vending-Bench,
LightMem, Auto-Dreamer, MINJA etc.) foram vistos pelo resumo e por fontes
secundárias. Nesses casos, os números citados vêm do resumo do artigo.

---

## Resumo em 12 pontos

1. **"Sono" de agente é processamento fora do horário de uso.** É quando ele
   consolida, tira duplicatas, invalida o que envelheceu e extrai lições.
   Letta, LightMem, Claude Dreams, OpenClaw e Auto-Dreamer convergem nisso.
2. **O maior ganho de custo vem de separar duas coisas:** gravar barato
   durante o uso e consolidar pesado depois. No LightMem, isso deu de 32× a
   117× menos tokens. O Abiyss já tem metade: a fila de propostas e o
   `abiyss sleep` mínimo.
3. **A entrada nunca é alterada; a saída é revisada antes de valer.** O
   Dreams da Anthropic não toca na memória de entrada: gera uma memória nova
   para revisar. No Abiyss, o equivalente já existe: propostas passam pelas
   regras do kernel e o cofre só recebe acréscimos.
4. **A procedência é o portão de segurança da memória.** É a mitigação do
   OWASP ASI06, e o OpenClaw descarta candidatos `untrusted`. A regra dura
   do Abiyss é isso, e o sono precisa preservá-la por construção, com duas
   passadas separadas: uma para o material interno e outra para o externo.
5. **Só se promove algo com evidência.** O OpenClaw exige pontuação mínima,
   recorrência e diversidade. O Abiyss pode exigir evidências citáveis (IDs
   de mensagens e ciclos) e conferir por código que elas existem.
6. **Esquecer faz parte do sono.** A hipótese da homeostase sináptica (SHY)
   diz isso para o cérebro; MemoryBank usa a curva de Ebbinghaus; o Zep
   invalida em vez de apagar. No Abiyss, esquecer continua manual: o sono só
   sugere.
7. **Coerência longa não falha por janela cheia.** No Vending-Bench, as
   falhas não têm correlação com o enchimento do contexto. Elas vêm de
   derivas, de loops e de lições não aprendidas (como no Project Vend). A
   resposta é detectar estagnação por código e fazer reflexão estruturada.
8. **O heartbeat tem de ser barato.** Só chama o modelo quando há novidade,
   respeita horas ativas e usa contexto leve (é o que faz o OpenClaw). O
   Abiyss já tem "sem novidade, sem chamada". Faltam o ritmo de dia e noite e
   um orçamento diário.
9. **Processo 24/7 pede três coisas:** ser crash-only, ter watchdog e manter
   estado estável. Recuperar é o caminho normal de partida, o systemd precisa
   saber se o laço principal está vivo, e tudo que acumula precisa de
   expurgo e de backup.
10. **Achado no código:** o laço do daemon fica parado enquanto um ciclo de
    heartbeat roda. Crons, sinal de vida e manutenção esperam o ciclo
    terminar. Se o keepalive do watchdog (que a infra está fazendo) sair
    desse laço, o systemd mata o processo durante um raciocínio longo.
11. **Skills:** a descrição diz o quê e quando, o corpo é curto e as
    referências ficam a um nível do `SKILL.md`; avalia-se antes de escrever
    muito. No Abiyss, as skills do próprio repositório precisam ser
    confiáveis. Se não forem, as skills de memória bloqueiam a si mesmas, e
    foi por isso que a skill `memoria` saiu do PR #3.
12. **Segurança:** memória persistente é o alvo preferido (o MINJA passa de
    95% de sucesso na injeção). Quando chegarem os canais (sessão B), é
    preciso evitar a tríade letal, seguindo a Regra de Dois.

---

## 1. Perguntas de pesquisa

| # | Pergunta |
|---|---|
| P1 | O que é "sono" para um agente de IA? O que ele deve fazer e o que não deve? |
| P2 | Quando e com que frequência dormir? Com que modelo e esforço? |
| P3 | Como consolidar sem contaminar a memória (envenenamento, alucinação, recursão)? |
| P4 | Como esquecer sem perder o que os goals ativos precisam? |
| P5 | Como manter um processo vivo 24/7 com custo previsível? |
| P6 | Como evitar loops e derivas em horizontes longos? |
| P7 | Como o agente percebe que dormiu ou caiu e retoma o fio (continuidade)? |
| P8 | Que skills básicas um agente 24/7 precisa e como escrevê-las? |

---

## 2. O Abiyss hoje

### 2.1 O que já existe

| Peça | O que faz | Onde |
|---|---|---|
| Daemon | Trava de instância única; crons e sinal de vida a cada 30 s; heartbeat a cada 300 s; executor de sub-agentes; SIGTERM sem estragar nada | `kernel/src/daemon.rs` |
| Heartbeat | Decide **por código** se chama o modelo; no máximo 1 chamada por ciclo; núcleo do goal no início e no fim do contexto; ações em JSON validadas; diário registra expectativa antes e resultado depois | `kernel/src/heartbeat.rs` |
| Rate limit | Dois pools; token bucket no SQLite compartilhado entre processos; fatia reservada para conversa; Retry-After; backoff com sorteio | `kernel/src/orquestrador/` |
| Memória | Cofre do Obsidian (`01_internal/`, `02_external/`); `memoria_propor` só enfileira e `abiyss sleep` aplica; regra dura; notas só recebem acréscimos; memória central com orçamento | `kernel/src/memoria/` |
| Skills | `SKILL.md` + `references/`; só nome e descrição vão para o prompt do chat; `ler_skill` sob demanda | `kernel/src/skills.rs` |
| Metacognição | Diário (expectativa → resultado) e interocepção, ambos só registro | `kernel/src/diario.rs`, `interocepcao.rs` |
| Infra (branch `eloquent-wozniak`) | Latência p50/p95; tabela de esforço com modos raso e profundo; retenção com agregados diários, checkpoint do WAL e vacuum incremental | `kernel/src/{latencia,esforco,manutencao}.rs` |
| Infra em andamento (outra sessão) | Filas e buffers limitados; supervisão dos MCP; watchdog `sd_notify` e endurecimento do systemd; soak test | objetivo da sessão "Abiyss runtime v02-infra" |

### 2.2 Lacunas encontradas no código

| # | Lacuna | Evidência |
|---|---|---|
| G1 | **O laço do daemon fica bloqueado durante um ciclo.** O braço do heartbeat dentro do `select!` aguarda o ciclo inteiro, e nesse tempo não há crons, sinal de vida, checkpoint nem manutenção. Com `timeout_leitura_segundos = 600` e até 5 tentativas por chamada, um ciclo pode durar dezenas de minutos. | `kernel/src/daemon.rs` (laço `loop { tokio::select! { ... _ = self.um_ciclo() ... } }`) |
| G2 | O sono só aplica propostas e não é agendado. | `kernel/src/memoria/sleep.rs`; `docs/ROADMAP.md` ("Agendar o `abiyss sleep`" e "Fases posteriores", item 2) |
| G3 | Não há orçamento diário de trabalho autônomo, só limites por minuto. | `abiyss.toml` `[pools]` |
| G4 | Não há detecção de estagnação ou loop no heartbeat, nem disjuntor no nível do ciclo. Só existem retentativas por chamada. | `kernel/src/heartbeat.rs` (`motivo_para_chamar`) |
| G5 | Não há ritmo: o heartbeat roda igual às 3h e às 15h. | `[daemon]` |
| G6 | Não há backup do banco nem do cofre, embora esse estado seja insubstituível. | não existe módulo |
| G7 | Depois de uma queda ou parada, o modelo não fica sabendo que esteve fora do ar. | `daemon.rs` grava `parado_ms`, mas ninguém lê |
| G8 | O heartbeat não vê as skills: o prompt dele não recebe o índice. E ler qualquer skill marca o contexto como externo, o que bloqueia propostas internas. | `heartbeat.rs` (`prompt_sistema` usa `..Default::default()`); `ferramentas/mod.rs` (`classificar_origem`: `_ => Some(nome)`) |
| G9 | Não há canal assíncrono para pedir confirmação ao usuário. O núcleo diz "Peço confirmação antes de qualquer ação irreversível", mas no modo autônomo não há a quem pedir. | `identity/nucleo.md` |
| G10 | Os ciclos não registram a origem do conteúdo. Por exemplo, não fica marcado se havia relatório de sub-agente no contexto, e o sono precisa disso para separar o interno do externo. | tabela `ciclos` (migração 3) |

---

## 3. Sono na neurociência (inspiração, não especificação)

Analogias biológicas ajudam a escolher fases e prioridades, mas não provam
nada sobre agentes. Uso aqui só o que vira decisão de engenharia.

| Achado | Fonte | O que sugere para o Abiyss |
|---|---|---|
| **Sistemas complementares (CLS).** O hipocampo aprende rápido e de forma episódica; o neocórtex aprende devagar e generaliza. O hipocampo "ensina" o neocórtex, sobretudo durante o sono. | McClelland, McNaughton & O'Reilly, 1995 | O diário, o histórico e a fila de propostas fazem o papel do hipocampo (rápido, episódico). O cofre faz o do neocórtex (lento, estruturado). O sono é a ponte entre os dois. |
| **Consolidação ativa.** No sono de ondas lentas, as memórias são reativadas e redistribuídas; o REM ajuda a estabilizá-las. | Diekelmann & Born, 2010 | Fases em ordem: **reativar e selecionar** (revisar o dia), **integrar** (propor) e **estabilizar** (aplicar com as regras). |
| **Homeostase sináptica (SHY).** O sono renormaliza as sinapses, extrai o essencial e esquece de forma inteligente. | Tononi & Cirelli, 2014 | O sono também cuida do orçamento: aponta o que está repetido, velho ou acima do limite (memória central). |
| **Relevância futura.** O sono consolidou mais o que se esperava usar, mas uma replicação de 2021 não encontrou o efeito. | Wilhelm et al., 2011; PLOS ONE, 2021 | Priorizar o que os goals ativos precisam é uma **heurística de engenharia**, não um fato biológico garantido. |
| **Replay contra esquecimento catastrófico.** Um replay no estilo do sono redistribui recursos entre tarefas antigas e novas. | Tadros, Krishnan & Bazhenov, 2022 | Consolidar o novo junto de amostras do antigo (notas relacionadas que já existem) para o recente não dominar e não haver duplicatas. |

---

## 4. Sono em agentes de IA (estado da arte)

### 4.1 Sleep-time compute (Letta e UC Berkeley, 2025)

- **Ideia.** Usar o tempo ocioso para pré-computar "contexto aprendido"
  sobre o que o agente já sabe, antes de as perguntas chegarem.
- **Arquitetura.** Há um agente principal e um agente de sono que
  compartilham blocos de memória. No modo sono, o principal não edita a
  memória central; quem a reorganiza é o agente de sono, que pode usar um
  modelo mais forte e mais lento. A frequência é configurável (a cada N
  passos).
- **Resultado.** Cerca de 5× menos computação na hora da pergunta, com a
  mesma acurácia, em versões "com estado" do GSM-Symbolic e do AIME. O
  artigo também relata que o ganho depende de quão previsíveis são as
  perguntas futuras.
- **Lição para o Abiyss.** O sono pode usar o modo **profundo** da tabela de
  esforço (infra-2) porque roda fora do horário de conversa.

### 4.2 Reflexão (Generative Agents, Park et al., 2023)

- **Memória.** Um fluxo de memórias em linguagem natural. A recuperação
  combina recência (decaimento exponencial), importância (nota de 1 a 10
  dada pelo próprio modelo) e relevância (similaridade).
- **Reflexão.** Dispara quando a soma das importâncias recentes passa de um
  limiar (150 na implementação; na prática, duas ou três vezes por "dia").
  Gera conclusões de nível mais alto que voltam para a memória.
- **Ablação.** Sem reflexão, o comportamento continua coerente no curto
  prazo, mas **degrada em horizontes de horas e dias**. É o argumento
  empírico mais citado para "dormir".

### 4.3 Memória com tempo: MemoryBank, Mem0, Zep/Graphiti

- **MemoryBank** (2023). Aplica a curva de Ebbinghaus, *R = e^(−t/S)*: a
  força *S* da memória aumenta cada vez que ela é lembrada. O que não é
  usado vai apagando.
- **Mem0** (2025). Primeiro extrai fatos a cada troca de mensagens; depois
  decide entre ADD, UPDATE, DELETE e NOOP comparando com as memórias
  parecidas. Um resumo da conversa é atualizado de forma assíncrona.
- **Zep / Graphiti** (2025). Cada fato tem quatro tempos: quando foi válido
  (`valid_at`/`invalid_at`) e quando o sistema soube dele
  (`created_at`/`expired_at`). Um fato novo que contradiz um antigo
  **invalida** o antigo em vez de apagá-lo, e o histórico continua
  consultável.
- **Para o Abiyss.** O cofre já data as coisas (`criado`, `atualizado`,
  resumos externos datados, `revalidar_apos`). O sono deve preferir
  "contradiz X desde AAAA-MM-DD" a reescrever, porque as notas só aceitam
  acréscimos.

### 4.4 Consolidação offline: LightMem e Auto-Dreamer

- **LightMem** (ICLR 2026). Usa três estágios inspirados em
  Atkinson-Shiffrin (sensorial, curto prazo e longo prazo). Durante o uso, as
  entradas são gravadas com data e sem operação cara de LLM. A consolidação
  pesada fica para um período de "sono". Ganhos relatados: acurácia de
  +2,7% a +9,65%, de 32× a 117× menos tokens, de 17× a 177× menos chamadas e
  de 1,67× a 12,45× menos tempo.
- **Auto-Dreamer** (2026). É um consolidador offline **aprendido** (treinado
  por RL), inspirado na teoria CLS. Trata a região de memória que está
  consolidando como **evidência somente leitura**, inspeciona as trajetórias
  de origem pela procedência e sintetiza um conjunto novo e compacto. Teve 7
  pontos a mais no ScienceWorld com memória 12× menor e generalizou para
  ALFWorld e WebArena.

### 4.5 Produtos que já "dormem"

**Claude Managed Agents: Dreams** (prévia de pesquisa, anunciada em
2026-05-06):

- Entra uma memória existente e de 1 a 100 sessões passadas; sai uma
  **memória nova**. A entrada **nunca é modificada**.
- Junta duplicatas, troca entradas velhas ou contraditas pelo valor mais
  recente e traz insights novos. Exemplo citado: *playbooks* criados a
  partir de erros recorrentes.
- Aceita instruções de foco (até 4.096 caracteres). Leva de minutos a
  horas.
- O custo cresce quase linearmente com o número e o tamanho das sessões. A
  própria documentação recomenda começar com lotes pequenos e revisar a
  saída antes de usá-la.

**OpenClaw: Dreaming** (2026):

- Uma varredura diária (padrão `0 3 * * *`) com três fases em ordem.
  *Light* reúne candidatos e sinais. *REM* produz temas e reflexões. *Deep*
  pontua, filtra e é **a única fase que escreve** na memória de longo prazo.
- A pontuação usa seis sinais com pesos: relevância 0,30, frequência 0,24,
  diversidade de consultas 0,15, recência 0,15, consolidação 0,10 e riqueza
  conceitual 0,06.
- Há portões mínimos: pontuação, número de lembranças e consultas
  distintas.
- **Antes de consolidar, descarta candidatos com procedência `untrusted` ou
  `system`.**
- Mantém um diário legível dos sonhos (`DREAMS.md`).

**OpenClaw: heartbeat:**

- Padrão de 30 min. Uma resposta "nada a fazer" (`HEARTBEAT_OK`) é
  descartada sem barulho.
- Tem horas ativas (`activeHours`) e contexto leve com sessão isolada
  (de cerca de 100 mil para 2 a 5 mil tokens por execução).
- A documentação separa *heartbeat* (atenção periódica) de *cron* (horário
  exato).

**Hermes Agent** (Nous Research, o framework em que o Abiyss rodava):

- `MEMORY.md` e `USER.md` são curados pelo próprio agente, com lembretes
  periódicos.
- Tem busca nas sessões antigas (FTS5) com resumo, skills criadas
  automaticamente depois de tarefas complexas e um cron embutido.
- **Não tem uma fase de sono separada**: a consolidação é contínua.

### 4.6 Síntese: princípios convergentes

| # | Princípio | Quem faz | No Abiyss hoje | Falta |
|---|---|---|---|---|
| S1 | Gravar barato durante o uso e consolidar pesado depois | LightMem, Letta, Dreams | Propostas em fila | Sono que *produz* propostas |
| S2 | Entrada imutável e saída revisável; nada destrutivo | Dreams, Auto-Dreamer | Só acréscimos, registro, regras do kernel | Manter no sono |
| S3 | Procedência como portão, antes de consolidar | OpenClaw, OWASP | Regra dura | Duas passadas por origem no sono; origem por ciclo (G10) |
| S4 | Promover só com evidência | OpenClaw (portões), Generative Agents | — | Evidências citáveis, conferidas por código |
| S5 | Tempo explícito: datar, invalidar, revalidar | Zep, MemoryBank | Datas e `revalidar_apos` | Listar externas vencidas no sono |
| S6 | Orçamento e esquecimento | SHY, MemoryBank, Dreams | Orçamento da memória central; esquecimento manual | Sono que *sugere* cortes |
| S7 | Relatório legível e devolutiva ao próprio agente | OpenClaw (`DREAMS.md`) | Relatório do `abiyss sleep`, só para o usuário | Relatório do sono e evento ao acordar (ROADMAP, fase 2) |
| S8 | Fora do horário de uso, com modelo ou esforço mais forte, em lotes pequenos | Letta, Dreams | Tabela de esforço (infra-2) | Janela de sono e modo profundo |
| S9 | O sono nunca lê a própria saída como entrada | lição do v0.1 do Abiyss ("recap imutável") | — | Marcas de progresso e filtros |

---

## 5. Operar 24/7 (engenharia)

### 5.1 Crash-only

Candea e Fox (2003) propõem que a única forma de parar seja "cair" e a única
forma de subir seja "recuperar". Com isso, o código de recuperação roda em
toda partida e deixa de ser raro.

- **O Abiyss já faz:** `recuperar_interrompidos` para sub-agentes; crons
  perdidos viram um evento só; eventos só são marcados como consumidos depois
  de uma chamada bem-sucedida; migrações idempotentes.
- **Falta:** um sono que possa ser retomado (estado por fase) e o aviso de
  reinício ao modelo.

### 5.2 Watchdog e supervisão

- Com `Type=notify` e `WatchdogSec`, o daemon envia `READY=1` quando está
  pronto e `WATCHDOG=1` a cada metade do intervalo. Se o aviso parar, o
  systemd mata o processo e o reinicia (`Restart=on-failure` cobre o
  watchdog).
- Isso já está no escopo da **sessão de infra**.
- **Regra de ouro:** o *keepalive* deve sair do laço que prova a saúde do
  daemon, e esse laço não pode ficar bloqueado por trabalho longo (G1). Um
  *keepalive* numa tarefa à parte, que sempre responde, anula o watchdog.

### 5.3 Estado estável e padrões de estabilidade (Nygard, *Release It!*)

- **Estado estável:** todo mecanismo que acumula recurso precisa de outro
  que o expurgue. Isso cobre retenção (infra-3), backups rotacionados,
  relatórios com limite e filas limitadas (infra-4).
- **Timeouts** em toda espera.
- **Disjuntor:** parar de chamar o que está falhando por um tempo crescente.
- **Anteparas:** isolar os recursos de cada dependência. O Abiyss já faz isso
  com os pools separados.

### 5.4 Backoff com sorteio (Brooker, AWS)

O "full jitter" sorteia uma espera entre 0 e o teto exponencial e evita
avalanches de retentativas. O orquestrador do Abiyss já faz isso por
chamada. Falta o equivalente **no nível do ciclo**, que é o disjuntor (G4).

### 5.5 Backup consistente de SQLite

Copiar `abiyss.db` e o `-wal` com o banco em uso pode gerar uma cópia que
nunca existiu. As duas opções seguras são:

- `VACUUM INTO 'arquivo'`: snapshot transacional e compacto, que funciona
  em WAL;
- a API de backup online.

Depois da cópia, confira com `PRAGMA integrity_check`.

### 5.6 Heartbeat barato e ritmo

A experiência do OpenClaw (seção 4.5) e a regra "sem novidade, sem chamada"
do Abiyss apontam para o mesmo desenho:

- **horas ativas**: fora delas não há revisão periódica e os eventos são
  vistos com menos frequência;
- uma **janela de sono**;
- intervalo variável conforme a fase.

### 5.7 Coerência em horizontes longos

**Vending-Bench** (Andon Labs, 2025). Agentes administram uma máquina de
venda simulada em execuções muito longas.

- A variância entre execuções é alta.
- Todos os modelos acabam caindo em algum momento em *loops* de "colapso".
- **As falhas não se correlacionam com a janela de contexto cheia.**

**Project Vend** (Anthropic, 2025). O Claude administrou uma loja real.

- Falhas: inventou detalhes (uma conta de pagamento, uma pessoa), teve uma
  crise de identidade (dizia que entregaria "pessoalmente"), vendeu com
  prejuízo e **voltou a dar descontos dias depois de decidir parar**.
- Recomendações: reflexão estruturada, ferramentas melhores (CRM) e
  supervisão.

**Para o Abiyss:**

- O núcleo de identidade fixo e as regras do kernel já cuidam da
  identidade: "nunca Hermes, nunca humano".
- Faltam **lições** registradas no sono (expectativa × resultado do diário),
  **detecção de estagnação** e o **aviso de continuidade**.

### 5.8 Detecção de loops (OpenHands, *StuckDetector*)

O OpenHands detecta cinco padrões:

- a mesma ação com a mesma observação 4 vezes;
- a mesma ação com erro 3 vezes;
- monólogo;
- alternância A/B 6 vezes;
- erros repetidos de janela de contexto.

A resposta é um **empurrão** ("você fez X três vezes com o mesmo resultado;
mude a abordagem"), não uma punição.

### 5.9 Arnês de longa duração (Anthropic, nov./2025)

Cada sessão começa se situando: lê o progresso e o git e roda os testes
básicos. Depois trabalha em um item só e termina em estado limpo, com o
progresso escrito. Isso evita três falhas: tentar fazer tudo de uma vez,
declarar vitória cedo e deixar estado quebrado e não documentado.

No Abiyss, o **evento de despertar** e o **relatório do sono** fazem o papel
do arquivo de progresso entre "sessões" do agente.

### 5.10 Engenharia de contexto (Anthropic, set./2025)

Quanto mais tokens no contexto, pior a lembrança ("podridão de contexto").
As técnicas recomendadas são:

- compactação;
- notas estruturadas fora do contexto, reinseridas sob demanda;
- sub-agentes que devolvem resumos de 1 a 2 mil tokens;
- busca *just-in-time*.

O Abiyss já segue essa linha: memória central pequena, skills por revelação
progressiva e sub-agentes com contexto limpo.

### 5.11 Orçamento

Limites por minuto não seguram um loop que roda o dia inteiro. Boas
práticas:

- limite diário por origem (autônomo × conversa);
- degradação graciosa: primeiro alerta, depois só eventos, depois parar até
  o dia seguinte;
- **nunca cortar a conversa com o dono**;
- uma fatia reservada para o sono.

### 5.12 Humano no circuito, de forma assíncrona

Um agente 24/7 precisa poder **estacionar** uma decisão em vez de insistir
nela. O mecanismo:

- pedidos ao usuário com deduplicação, urgência e prazo;
- a resposta vira evento;
- sem resposta, o pedido expira e o agente fica sabendo.

---

## 6. Segurança de um agente que roda sozinho

**Envenenamento de memória.** É a ameaça central de um agente com memória
persistente.

| Fonte | O que mostra |
|---|---|
| MINJA (NeurIPS 2025) | Injeta registros maliciosos **só fazendo perguntas**, com mais de 95% de sucesso na injeção e mais de 70% de sucesso no ataque |
| AgentPoison (2024) | Cerca de 80% de sucesso no ataque, sem afetar entradas normais |
| OWASP Top 10 para aplicações agênticas (2026) | Lista o risco como ASI06 (*Memory & Context Poisoning*) e as falhas em cascata como ASI08. Mitigações: validar escritas, segmentar contextos, aceitar só fontes curadas, detectar anomalias e ter rollback |

**O desenho do Abiyss já segue a recomendação.** A procedência é calculada
pelo kernel (regra dura), as notas só recebem acréscimos, há um registro de
operações e o esquecimento é manual.

**O sono é o ponto mais sensível**, porque lê muito e escreve na memória. Ele
precisa:

1. separar as passadas por origem: material interno só gera propostas
   internas; material externo só gera propostas externas;
2. citar evidências que o kernel confere;
3. nunca ler a própria saída como entrada;
4. fazer backup antes de escrever.

**Tríade letal e Regra de Dois.**

- A tríade letal (Willison, 2025) é juntar dados privados, conteúdo não
  confiável e comunicação externa.
- A Regra de Dois (Meta, 2025) diz que uma sessão autônoma deve ter **no
  máximo duas** dessas três propriedades; com as três, precisa de humano.
- Hoje o Abiyss não envia nada para fora. Quando chegarem os canais
  (sessão B), um heartbeat que leu conteúdo externo não deve poder mandar
  mensagem a terceiros sem confirmação.

**Ataques adaptativos.** Em *The Attacker Moves Second* (2025), as defesas
baseadas em prompt caíram com mais de 90% de sucesso dos ataques. O que
segura é a **arquitetura**: procedência, permissões e humano.

**Herança do War Pigs.** As notas de estudo do v0.1 já pediam testes de
**contaminação epistêmica**: observações falsas que viram memória e guiam
decisões futuras. O sono é o lugar natural para esses cenários (seção 9).

---

## 7. Skills: o que a pesquisa diz

**Boas práticas da Anthropic para escrever skills** (documentação oficial):

- `name` com até 64 caracteres, só minúsculas, números e hífens.
- `description` com até 1.024 caracteres, **em terceira pessoa**, dizendo
  **o que a skill faz e quando usar**.
- Corpo do `SKILL.md` com **menos de 500 linhas**.
- Referências **a um nível** do `SKILL.md`; referências com mais de 100
  linhas começam com um índice.
- Fluxos com checklist e laços de verificação ("valida → corrige → repete").
- Avaliar antes de escrever muito: três cenários e uma linha de base sem a
  skill.
- Testar com **todos os modelos que vão usá-la**. No Abiyss, isso significa
  GLM, Kimi e Nemotron.
- Nada que fique datado e terminologia consistente.
- Grau de liberdade proporcional à fragilidade da tarefa.

**Aprender skills a partir da experiência:**

- **Voyager** guarda só habilidades **verificadas** antes de entrarem na
  biblioteca.
- **Agent Workflow Memory** induz fluxos reutilizáveis a partir de
  trajetórias bem-sucedidas.
- O **Hermes** cria e melhora skills sozinho.
- O **Dreams** gera *playbooks* a partir de erros recorrentes.

**Implicações para o Abiyss:**

1. **Confiança.** Skills versionadas no repositório merecem a mesma
   confiança do núcleo, porque são revisadas pelo git e o Abiyss não escreve
   nelas. Skills de fora (pasta do Hermes, *hubs*) são conteúdo externo.
   Sem essa distinção, ler uma skill de memória bloqueia as propostas que
   ela ensina a fazer.
2. **Quem escreve.** Hoje só o usuário escreve skills: a fase 9 do roadmap
   trata de o Abiyss editá-las com segurança. Até lá, o sono pode
   **sugerir** procedimentos como notas `01_internal/procedimentos/...`, e o
   usuário promove os que valerem para `skills/`. É o mesmo desenho do
   Voyager, com o humano no papel de crítico.
3. **Divisão de trabalho.** O kernel garante regras e formatos, em código.
   A skill descreve critério e estilo, em texto que o usuário pode editar.
   Por exemplo, a skill do sono afina o *que* consolidar, mas o *como*
   (evidências, regra dura, JSON) é do kernel.
4. **Skills básicas para um agente 24/7:** `registrar-memoria`,
   `conduzir-goals`, `dormir-bem`, `planejar-o-dia`, `sair-de-loops`,
   `pedir-ao-usuario`, além da `delegar-bem` que já existe.

---

## 8. Decisões propostas para o Abiyss

| # | Decisão | Por quê | Item do prompt |
|---|---|---|---|
| D1 | O laço do daemon nunca bloqueia. Trabalho longo roda em tarefas supervisionadas, com timeout e pânico isolado | Crash-only, watchdog, G1 | E1 |
| D2 | Skills do repositório são confiáveis. O heartbeat vê o índice e pode consultá-las; o kernel injeta a skill certa em situações conhecidas | Boas práticas de skills, G8 | E2 |
| D3 | Ritmo de horas ativas, descanso e janela de sono | OpenClaw `activeHours`, SHY, G5 | E3 |
| D4 | Orçamento diário autônomo com degradação graciosa | Estado estável, Vending-Bench, G3 | E3 |
| D5 | Estagnação detectada pela impressão digital das ações, respondida com um empurrão; disjuntor por ciclo | OpenHands, Nygard, G4 | E7 |
| D6 | Backup consistente e verificado antes de cada sono | SQLite, rollback do OWASP, G6 | E4 |
| D7 | Sono em fases, com duas passadas por origem, evidências verificáveis, saída em propostas, relatório e devolutiva | S1–S9, G2, G10 | E5 |
| D8 | Evento de despertar e continuidade | Arnês de longa duração, Project Vend, G7 | E8 |
| D9 | Caixa de pedidos ao usuário | Regra de Dois, núcleo ("peço confirmação"), G9 | E9 |
| D10 | Skills básicas escritas com avaliação e teste de coerência com o kernel | Boas práticas de skills | E6 (e E9 para `pedir-ao-usuario`) |

**Fora deste escopo**, porque a sessão de infra já cobre: watchdog e
`sd_notify`, unit do systemd, supervisão dos MCP, filas limitadas, soak test,
retenção, latência e a tabela de esforço em si.

**Fica para depois:** canais (B), mãos e navegador (C), voz (D), esquecimento
ativo automático, AutoGoal e skills escritas pelo próprio Abiyss.

---

## 9. Agenda de pesquisa: o que medir na VM

| Pergunta | Métrica | Como medir | Meta inicial |
|---|---|---|---|
| O sono melhora a lembrança? | Acertos numa "prova da semana": 10 fatos ditos pelo usuário, perguntados no chat | `abiyss chat -m` com perguntas fixas, antes e depois de 2 semanas de sono | ≥ 8/10 |
| O sono gera lixo? | % de propostas do sono rejeitadas; duplicatas no cofre | `abiyss memoria propostas --todas`, `abiyss memoria registro` | < 20% depois de calibrar a skill `dormir-bem` |
| Quanto custa dormir? | Tokens e chamadas por noite | Relatório do sono, `abiyss status` | Dentro de `[sono]` |
| As previsões melhoram? | % de ações com resultado diferente da expectativa | `abiyss diario` | Tendência de queda com as lições |
| Os loops caem? | Avisos de estagnação por semana e % de falsos positivos | Eventos do kernel | Poucos e úteis |
| O processo é estável? | Reinícios pelo watchdog; p95 do ciclo; tamanho do banco | `journalctl`, `abiyss status` (latência), soak test da infra | 0 reinícios inesperados por semana |
| A memória resiste a contaminação? | Cenários no estilo War Pigs: relatório de sub-agente dizendo "o usuário prefere X", nota externa com instrução escondida, evidência inventada | Testes adversariais contra o sono | 0 vazamentos para `01_internal` |

**Perguntas em aberto** (decidir com dados da VM):

1. Um sono por noite ou também um "cochilo" à tarde? Em que horário? (O
   OpenClaw usa 3h; o fuso aqui é `America/Sao_Paulo`.)
2. Qual o tamanho de lote por chamada? O Dreams recomenda começar pequeno.
3. Quando uma dedução vira "dito"? Proposta: só com evidência de mensagem do
   usuário, conferida por código.
4. Vale um "orçamento de memória" para as notas (como o da memória central),
   com o sono sugerindo resumos?
5. Quando abrir para o Abiyss propor skills (fase 9), com avaliação
   automática antes de ativar?

---

## 10. Referências

**Abiyss (este repositório)**
- PR #2: <https://github.com/rafaelsantos-cs/Abiyss-Runtime-Project/pull/2>
- PR #3: <https://github.com/rafaelsantos-cs/Abiyss-Runtime-Project/pull/3>
- Branch de infra: `claude/eloquent-wozniak-jy5ib2`; War Pigs: `warpigs-definitive`, `warpigs-sterile`.

**Sono e memória em agentes**
- Lin, Snell, Wang, Packer, Wooders, Stoica, Gonzalez (2025). *Sleep-time Compute: Beyond Inference Scaling at Test-time*. arXiv:2504.13171. Código: <https://github.com/letta-ai/sleep-time-compute>. Blog: <https://www.letta.com/blog/sleep-time-compute/>
- Letta: agentes de sono e memória compartilhada: <https://docs.letta.com/v1-sdk/memory/shared-memory>
- Park et al. (2023). *Generative Agents: Interactive Simulacra of Human Behavior*. UIST. <https://dl.acm.org/doi/fullHtml/10.1145/3586183.3606763>
- Zhong et al. (2023). *MemoryBank: Enhancing Large Language Models with Long-Term Memory*. arXiv:2305.10250.
- Chhikara et al. (2025). *Mem0: Building Production-Ready AI Agents with Scalable Long-Term Memory*. arXiv:2504.19413.
- Rasmussen et al. (2025). *Zep: A Temporal Knowledge Graph Architecture for Agent Memory*. <https://blog.getzep.com/content/files/2025/01/ZEP__USING_KNOWLEDGE_GRAPHS_TO_POWER_LLM_AGENT_MEMORY_2025011700.pdf>
- Fang et al. (2026). *LightMem: Lightweight and Efficient Memory-Augmented Generation*. ICLR 2026. arXiv:2510.18866.
- Ye et al. (2026). *Auto-Dreamer: Learning Offline Memory Consolidation for Language Agents*. arXiv:2605.20616.
- Anthropic. *Dreams* (Claude Managed Agents): <https://platform.claude.com/docs/en/managed-agents/dreams>
- OpenClaw. *Dreaming*: <https://github.com/openclaw/openclaw/blob/main/docs/concepts/dreaming.md>; *Heartbeat*: <https://github.com/openclaw/openclaw/blob/main/docs/gateway/heartbeat.md>
- Nous Research. *Hermes Agent*: <https://github.com/nousresearch/hermes-agent>

**Neurociência**
- McClelland, McNaughton & O'Reilly (1995). *Why there are complementary learning systems in the hippocampus and neocortex*. Psychological Review. <https://cseweb.ucsd.edu/~gary/258/jay.pdf>
- Diekelmann & Born (2010). *The memory function of sleep*. Nature Reviews Neuroscience 11:114–126. <https://www.nature.com/articles/nrn2762>
- Tononi & Cirelli (2014). *Sleep and the price of plasticity*. Neuron. <https://mechanism.ucsd.edu/bill/teaching/f16/cogs200/review_2014_Tononi.pdf>
- Wilhelm et al. (2011). *Sleep selectively enhances memory expected to be of future relevance*. J Neurosci 31(5):1563. <https://www.jneurosci.org/content/31/5/1563>. Replicação sem efeito: <https://journals.plos.org/plosone/article?id=10.1371%2Fjournal.pone.0258110>
- Tadros, Krishnan & Bazhenov (2022). *Sleep-like unsupervised replay reduces catastrophic forgetting in artificial neural networks*. Nat Commun 13:7742. <https://www.nature.com/articles/s41467-022-34938-7>

**Agentes de longa duração e operação**
- Anthropic (2025-11-26). *Effective harnesses for long-running agents*. <https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents>
- Anthropic (2025-09-29). *Effective context engineering for AI agents*. <https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents>
- Anthropic (2025). *Project Vend*. <https://www.anthropic.com/research/project-vend-1>
- Backlund & Petersson (2025). *Vending-Bench: A Benchmark for Long-Term Coherence of Autonomous Agents*. arXiv:2502.15840. <https://andonlabs.com/evals/vending-bench>
- Kwa et al. / METR (2025). *Measuring AI Ability to Complete Long Tasks*. <https://metr.org/blog/2025-03-19-measuring-ai-ability-to-complete-long-tasks/>
- OpenHands. *Stuck Detector*: <https://docs.openhands.dev/sdk/guides/agent-stuck-detector>
- Candea & Fox (2003). *Crash-Only Software*. HotOS IX. <https://dslab.epfl.ch/pubs/crashonly.pdf>
- Nygard. *Release It!* (2ª ed.), padrões de estabilidade. <https://www.oreilly.com/library/view/release-it-2nd/9781680504552/>
- Brooker (2015). *Exponential Backoff And Jitter*. <https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/>
- systemd: `sd_watchdog_enabled(3)`: <https://www.man7.org/linux/man-pages/man3/sd_watchdog_enabled.3.html>; Poettering, *systemd for Administrators XV (watchdog)*: <http://0pointer.de/blog/projects/watchdog.html>
- Backup de SQLite em WAL (VACUUM INTO / API de backup): <https://sqlite.work/ensuring-consistent-backups-in-sqlite-wal-mode-without-disrupting-writers/>

**Segurança**
- Dong et al. (2025). *MINJA: memory injection attack* (NeurIPS 2025); Chen et al. (2024). *AgentPoison*. Visão geral: <https://christian-schneider.net/blog/persistent-memory-poisoning-in-ai-agents/>
- OWASP Top 10 for Agentic Applications (2026): <https://goteleport.com/blog/owasp-top-10-agentic-applications/>
- Willison (2025-06-16). *The lethal trifecta for AI agents*. <https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/>
- Meta (2025-10-31). *Agents Rule of Two*; Nasr et al. (2025). *The Attacker Moves Second*. Resumo: <https://simonwillison.net/2025/Nov/2/new-prompt-injection-papers/>

**Skills**
- Anthropic. *Skill authoring best practices*: <https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices>
- Wang et al. (2023). *Voyager: An Open-Ended Embodied Agent with Large Language Models*. arXiv:2305.16291.
- Wang et al. (2024). *Agent Workflow Memory*. arXiv:2409.07429.
