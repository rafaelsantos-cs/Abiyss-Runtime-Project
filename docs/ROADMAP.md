# Roadmap do Abiyss

Este documento lista o que vem **depois** do runtime atual (fases F1–F8).
Nada daqui está implementado.

> **Sobre as especificações:** a tarefa original mencionava especificações em
> `docs/specs/`, mas essa pasta não existia em nenhuma branch nem no histórico
> do repositório quando o runtime foi construído. Os itens abaixo usam apenas
> os nomes citados na tarefa; quando as specs forem adicionadas, este roadmap
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

## Próximas fases

Ordem sugerida (cada uma depende das anteriores):

### 1. Escalonamento da metacognição
Hoje o diário só registra. Próximo passo: comparar expectativa × resultado por
código, medir a taxa de acerto das previsões e, quando cair, escalar (ex.:
subir o nível do sub-agente, pedir revisão do plano, avisar o usuário).

### 2. Memória (OpenViking / Obsidian)
Memória de longo prazo fora do histórico de conversa: notas em formato
Obsidian (Markdown com links) e/ou OpenViking como armazenamento. Exposta ao
modelo como servidor MCP em `recursos/` (a parte editável), com recuperação
por relevância e o conteúdo sempre rotulado como dado.

### 3. Esquecimento ativo
Política para resumir, arquivar ou descartar memórias e histórico antigos
(tamanho do contexto, custo de tokens, relevância), sem perder o que os goals
ativos precisam.

### 4. Atenção
Mecanismo para decidir o que entra no contexto de cada ciclo além do goal em
foco (eventos, memórias, sub-agentes), com orçamento de tokens por seção.

### 5. AutoGoal
O próprio Abiyss propor goals (hoje só o usuário cria). Os goals propostos
entram como `proposto` e passam pela mesma máquina de estados; limites de
quantos podem existir e regras de aprovação.

### 6. Know'Seeking e agenda de curiosidade
Busca ativa de conhecimento: o Abiyss identifica lacunas ("não sei X") e agenda
investigações (crons + sub-agentes), priorizando pelo valor para os goals.

### 7. DeepWork
Modo de trabalho longo e focado num único goal: janelas sem interrupção,
heartbeat dedicado e checkpoints frequentes.

### 8. SLEEP
Período de consolidação (ex.: madrugada): sem conversa, o Abiyss revisa o
diário, consolida memórias, aplica o esquecimento ativo e prepara o dia
seguinte, com orçamento próprio de chamadas.

### 9. S-A&U
A definir com base nas especificações (o nome foi citado, mas não há descrição
disponível no repositório).

### 10. Edição dos próprios recursos
Permitir que o Abiyss edite `recursos/` (servidores MCP) com segurança:
sandbox, testes automáticos antes de ativar, versionamento e rollback. O
kernel continua inalterável.

### 11. Computer use
Controle de navegador/desktop numa sandbox isolada, com permissões explícitas
e todo conteúdo de tela rotulado como dado.

### 12. Frontend
Interface web (painel de goals, diário, sub-agentes, interocepção e chat).
Hoje tudo é CLI.

## Dívidas técnicas conhecidas

- Sub-agentes só rodam no daemon; `abiyss chat` apenas registra o pedido.
- A prioridade da fila é por processo; entre processos, só o token bucket é
  compartilhado.
- O diário registra apenas ações do heartbeat (não as ferramentas do chat).
- Não há rotação/limpeza das tabelas do SQLite (`chamadas_modelo`, `ciclos`).
- Interocepção usa carga média do sistema (não o uso de CPU do processo).
