# Roadmap do Abiyss

Este documento lista o que já foi entregue e o que vem **depois**. Nada
das seções "Próximas sessões" e "Fases posteriores" está implementado.

> **Sobre as especificações:** a tarefa original mencionava especificações em
> `docs/specs/`, mas essa pasta não existia em nenhuma branch nem no histórico
> do repositório quando o runtime foi construído. Os itens abaixo usam apenas
> os nomes citados nas tarefas; quando as specs forem adicionadas, este roadmap
> deve ser revisado para refletir os nomes e estruturas definidos nelas.

## Estado atual (entregue)

| Fase | Entregue |
|---|---|
| F1 | Cliente NIM (SSE, tool calling) + mock para testes |
| F2 | Orquestrador: dois pools, token bucket no SQLite, Retry-After, backoff, prioridade e concorrência por nível |
| F3 | `abiyss chat`, núcleo de identidade, histórico em SQLite |
| F4 | Tools nativas confinadas a `workspace/`, conteúdo externo rotulado como dado |
| F5 | Ponte MCP (rmcp ↔ servidores Python via uv) |
| F6 | Daemon, heartbeat (1 chamada por ciclo), máquina de estados dos goals, crons, `status` |
| F7 | Sub-agentes assíncronos (`delegar`/`status`/`cancelar`) com orçamento e relatório estruturado |
| F8 | Diário (expectativa → resultado) e interocepção — só registro |
| A1 | Skills (`SKILL.md` + `references/`) com revelação progressiva: só nome + descrição no prompt, `ler_skill` sob demanda; só leitura |
| A2 | Memória num cofre do Obsidian: `01_internal/` (com procedência) e `02_external/` (mapa de fontes); `memoria_buscar`/`ler`/`propor`; fila + `abiyss sleep` mínimo; **regra dura no kernel** (conteúdo externo nunca entra direto em `01_internal`); `abiyss memoria esquecer` |
| A3 | Memória central com orçamento em caracteres, injetada em todo turno (conversa e heartbeat); acima do limite, recusa e relata |
| A4 | Ponte MCP também por HTTP ("streamable"); qmd como motor de `memoria_buscar`, com busca por texto quando falta |
| Infra | Latência por chamada (1º token e total; p50/p95 no `status`); tabela de esforço por modelo (`minimal`…`ultra`, modos raso/profundo — só config/cliente); retenção com agregados diários, checkpoint do WAL e vacuum incremental; limites explícitos em filas/buffers e supervisão dos processos MCP ([`LIMITES.md`](LIMITES.md)); unit do systemd com `Type=notify` + watchdog; teste de resistência ([`RESISTENCIA.md`](RESISTENCIA.md)) |
| A5 | `abiyss importar-hermes`: simulação por padrão, idempotente, sem abrir segredos; goals, journal, diário pessoal, identidade (+ rascunho de núcleo) e memórias |

## Antes das próximas fases: validar na VM

1. Confirmar os IDs de modelo e o formato do modo de raciocínio máximo do GLM
   (`abiyss testar-nim --modelo ...`).
2. Descobrir se o limite de 40 req/min é por chave ou por conta (observar 429
   com os dois pools ativos) e ajustar `pools.*.requisicoes_por_minuto`.
3. Verificar se o NIM aceita `stream_options.include_usage` (tokens no stream)
   e se modelos com raciocínio pedem o `reasoning_content` de volta entre
   rodadas de ferramentas.
4. Rodar o daemon alguns dias com goals reais e calibrar `heartbeat_segundos`,
   `revisao_minima_segundos` e os orçamentos dos sub-agentes.
5. **Migração do Hermes**, começando pela simulação:
   `abiyss importar-hermes --origem ~/.hermes` (roteiro completo em
   [`IMPORTAR_HERMES.md`](IMPORTAR_HERMES.md)). Só depois `--aplicar`.
6. **qmd**: confirmar o caminho do endpoint (`/mcp`), o nome da ferramenta de
   busca e o formato da resposta (`abiyss memoria buscar "x"` mostra o motor
   usado; com `expor = true`, `abiyss ferramentas` lista as ferramentas do
   qmd). Criar as coleções `abiyss-interno` e `abiyss-externo`.
7. Apontar `[memoria] cofre` para o cofre do Obsidian e `[caminhos] skills`
   para a pasta de skills desejada; ajustar `limite_central_caracteres`.
8. Agendar o `abiyss sleep` (ex.: timer do systemd de madrugada) — hoje ele
   só roda quando chamado.

## Próximas sessões

### B — Canais: WhatsApp e Discord

O Abiyss conversar fora do terminal.

- Um processo de canal por plataforma (ou um servidor MCP de canal), falando
  com o kernel pela mesma `SessaoChat` (uma conversa por contato/canal).
- Identificar o remetente: o usuário (dono) × outras pessoas. Mensagens de
  outras pessoas são conteúdo **externo** para a regra dura da memória (só o
  que o dono diz é "conversa" para `01_internal`).
- Tokens das plataformas só no `.env`; nada de segredo no `abiyss.toml`.
- Rate limit próprio por canal e respeito à fatia de conversa do pool cérebro.
- Mídia (áudio, imagem) fica para C/D; aqui, só texto.

### C — Mãos: terminal isolado, navegador em três níveis e visão

- **Terminal isolado**: comandos numa sandbox (contêiner/namespace sem rede
  por padrão, sem acesso a `kernel/`, `identity/`, `data/`, `.env`), com
  tempo e saída limitados; saída sempre rotulada como dado.
- **Navegador em três níveis** (o campo `navegador` das notas de
  `02_external/` já registra qual usar para cada fonte):
  1. **rápido** — sem modelo: SearXNG (metabusca local) + extração do texto
     da página, com cache em disco.
  2. **contemplativo** — um modelo de 120B (Nemotron) refina as buscas num
     loop curto e com orçamento (consultas, páginas e tokens limitados),
     usando o nível rápido por baixo.
  3. **agêntico** — sub-agentes dirigindo o Playwright: **árvore de
     acessibilidade primeiro**; screenshot + visão só quando a árvore não
     basta; **perfil isolado, sem nenhuma sessão logada**; **tomada de
     controle humana ao vivo** por um visualizador de tela virtual (ex.:
     Xvfb + noVNC) que **pausa o sub-agente e o retoma** quando o humano
     devolve o controle.
- Todo conteúdo de página entra como **dado**, nunca como instrução, e conta
  como origem **externa** para a regra dura da memória.
- **Visão**: descrever imagens/capturas sob demanda (modelo de visão do NIM),
  com o resultado também rotulado como dado.

### D — Voz

- **STT**: Whisper (local, ex.: whisper.cpp na VM ARM) para áudios recebidos
  pelos canais da sessão B.
- **TTS**: Piper (rápido, local) + **conversão de voz com RVC** para a voz
  própria do Abiyss (masculina).
- Orçamento de CPU: a VM tem 2 OCPU; filas e limites para não travar o
  heartbeat.

## Crons do Hermes

Os crons do Hermes que são **só scripts** (RSS, checagens de saúde, backups)
não devem virar crons do runtime: viram **timers do systemd** independentes
(`.service` + `.timer`), que rodam sem o Abiyss e sem gastar chamadas ao
modelo. Se o resultado interessar ao Abiyss, o script grava um arquivo ou
publica um evento — que ele lê como **dado**. Crons do runtime (`abiyss cron`)
ficam para lembretes que precisam do Abiyss pensando (viram evento na fila).
O mesmo vale para agendar o `abiyss sleep`.

## Fases posteriores

Ordem sugerida (cada uma depende das anteriores):

### 1. Escalonamento da metacognição
Hoje o diário só registra. Próximo passo: comparar expectativa × resultado por
código, medir a taxa de acerto das previsões (o journal importado do Hermes
já traz confiança e resultado) e, quando cair, escalar (ex.: subir o nível do
sub-agente, pedir revisão do plano, avisar o usuário).

### 2. Memória: consolidação no SLEEP
A base está pronta (A2–A4). Falta o sono de verdade: o Abiyss revisar o
diário e a conversa do dia, propor memórias (`fonte: sleep`), promover
deduções confirmadas, revalidar notas externas vencidas (`revalidar_apos`)
pelo navegador da sessão C e mandar o relatório das propostas rejeitadas de
volta para ele (hoje só o usuário vê).

### 3. Esquecimento ativo
Política para resumir, arquivar ou descartar memórias e histórico antigos
(tamanho do contexto, custo de tokens, relevância), sem perder o que os goals
ativos precisam. Hoje só existe o esquecimento manual
(`abiyss memoria esquecer`).

### 4. Atenção
Mecanismo para decidir o que entra no contexto de cada ciclo além do goal em
foco (eventos, memórias buscadas, sub-agentes), com orçamento de tokens por
seção.

### 5. AutoGoal
O próprio Abiyss propor goals (hoje só o usuário cria). Os goals propostos
entram como `proposto` e passam pela mesma máquina de estados; limites de
quantos podem existir e regras de aprovação.

### 6. Know'Seeking e agenda de curiosidade
Busca ativa de conhecimento: o Abiyss identifica lacunas ("não sei X") e agenda
investigações (crons + sub-agentes + navegador), priorizando pelo valor para os
goals; o que aprende vai para o mapa `02_external/`.

### 7. DeepWork
Modo de trabalho longo e focado num único goal: janelas sem interrupção,
heartbeat dedicado e checkpoints frequentes.

### 8. S-A&U
A definir com base nas especificações (o nome foi citado, mas não há descrição
disponível no repositório).

### 9. Edição dos próprios recursos e skills
Permitir que o Abiyss edite `recursos/` (servidores MCP) e escreva skills com
segurança: sandbox, testes automáticos antes de ativar, versionamento e
rollback. O kernel continua inalterável.

### 10. Frontend
Interface web (painel de goals, diário, sub-agentes, interocepção, memória e
chat). Hoje tudo é CLI (e, depois de B, os canais).

## Dívidas técnicas conhecidas

- Sub-agentes só rodam no daemon; `abiyss chat` apenas registra o pedido.
- A prioridade da fila é por processo; entre processos, só o token bucket é
  compartilhado.
- O diário registra apenas ações do heartbeat (não as ferramentas do chat).
- A retenção cobre `chamadas_modelo`, `ciclos` e os eventos consumidos de
  `fila_eventos` (viram agregados diários). `propostas_memoria`,
  `registro_memoria`, `mensagens` e `diario` continuam crescendo sem limpeza.
- A tabela de esforço (`abiyss esforco`) ainda não é usada pelo chat, pelo
  heartbeat nem pelos sub-agentes.
- Interocepção usa carga média do sistema (não o uso de CPU do processo).
- Regra dura da memória: a marca de origem externa vale para resultados de
  ferramentas e para a resposta escrita logo depois deles no mesmo turno, e
  some quando essas mensagens saem da janela de contexto. Uma paráfrase
  repetida turno após turno não é rastreada (escolha consciente para uma
  conversa longa não ficar bloqueada para sempre).
- O sleep acrescenta conteúdo às notas, nunca edita nem resume; notas podem
  crescer com repetições até a consolidação de verdade (fase 2).
- Depois de `abiyss memoria esquecer`, o índice do qmd pode guardar a nota
  até ser reindexado (os resultados são filtrados contra o cofre, então a
  nota esquecida não aparece, mas o índice ainda tem o texto).
- O formato das respostas do qmd é suposto (ver `memoria/qmd.rs`); se não
  bater, a busca cai para texto e o relatório mostra `motor: texto (qmd falhou)`.
- O importador do Hermes não transforma o `history` dos goals em eventos
  (fica preservado em `extras`).
