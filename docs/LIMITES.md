# Limites de memória do runtime

Auditoria de toda fila, buffer e cache em memória do kernel. Regra: nada
cresce sem um teto explícito, e todo teto que depende do uso fica no
`abiyss.toml`.

## Filas e buffers do kernel

| Onde | O que guarda | Teto | Configuração |
|---|---|---|---|
| `orquestrador/fila.rs` (`FilaPrioridade`) | chamadas esperando a vez num pool | `max_na_fila` lugares; a próxima recebe erro na hora | `[pools.cerebro] max_na_fila`, `[pools.subagentes] max_na_fila` (64) |
| `orquestrador` (semáforos por nível) | chamadas simultâneas de sub-agentes | `ultra`/`medium`/`low` vagas | `[pools.subagentes.concorrencia]` |
| `nim/cliente.rs` resposta sem streaming | o corpo inteiro | `max_bytes_resposta`; passou, erro | `[nim] max_bytes_resposta` (16 MiB) |
| `nim/sse.rs` (`LeitorSse`, `AcumuladorStream`) | linha SSE pendente, texto, raciocínio, argumentos das ferramentas | indireto: tudo vem dos bytes do stream, e o total de bytes lidos é limitado por `max_bytes_resposta` | idem |
| `reqwest` (pool de conexões) | conexões HTTP ociosas por pool | `max_conexoes_ociosas` por host | `[nim] max_conexoes_ociosas` (4) |
| `mcp.rs` resultado de ferramenta | texto juntado dos blocos | `max_bytes_resultado`; o excedente é cortado e o corte é avisado no texto | `[mcp] max_bytes_resultado` (1 MB) |
| `subagentes.rs` (`JoinSet` do executor) | sub-agentes vivos | `max_simultaneos` (semáforo) | `[subagentes] max_simultaneos` |
| `subagentes.rs` (`pendentes`) | IDs lidos do banco por rodada | `LIMIT max_simultaneos` (mais não caberia nas vagas) | idem |
| `heartbeat.rs` | eventos da fila colocados no contexto | `max_eventos_por_ciclo` | `[daemon] max_eventos_por_ciclo` |
| `ferramentas/workspace.rs` | leitura, escrita e listagem | bytes e itens | `[ferramentas]` |
| `nim/mock.rs` (`recebidas`) | requisições recebidas (para os testes conferirem) | as últimas 1000 (`MAX_RECEBIDAS_GUARDADAS`); o total é só um contador | constante: o mock é ferramenta de teste, mas o teste de resistência o deixa horas no ar |
| SQLite: cache de páginas | páginas lidas do banco, por conexão | `cache_kib` (antes: o padrão implícito de ~2 MB do SQLite) | `[banco] cache_kib` (2048) no daemon; constante `CACHE_PADRAO_KIB` nos outros comandos |
| SQLite: arquivo WAL | páginas ainda não copiadas para o banco | checkpoint `TRUNCATE` periódico + `journal_size_limit` de 64 MiB | `[retencao] checkpoint_minutos` |
| SQLite: tabelas de registro | `chamadas_modelo`, `fila_eventos` (consumidos), `ciclos` | N dias de detalhe; o resto vira agregado diário | `[retencao]` |

Não há canais (`mpsc`/`broadcast`) próprios no kernel. As tarefas do tokio
são fixas (daemon, executor de sub-agentes, supervisão MCP) ou limitadas
pelo semáforo dos sub-agentes.

## Processos filhos MCP

Cada servidor stdio roda no seu próprio grupo de processos (o `uv` e o
Python, que é neto do kernel). A cada `[mcp] supervisao_segundos`, o daemon
reinicia o servidor (mata o grupo inteiro com SIGKILL e sobe de novo) quando:

- o servidor caiu (o transporte fechou);
- uma chamada passou de `timeout_segundos` (provavelmente travou);
- a árvore de processos passou de `[mcp] max_memoria_mb` (RSS, lido de `/proc`);
- ele está no ar há mais de `[mcp] max_vida_segundos` (0 = sem limite).

Se não subir de novo, o próximo ciclo de supervisão tenta outra vez. Os nomes
das ferramentas ficam fixos (os da primeira subida). O `abiyss chat` não
supervisiona os servidores dele (a conversa é curta), mas mata o grupo de
processos ao sair. Servidores HTTP (qmd) só são reconectados quando o
transporte fecha. Não há limite de memória para eles: não são processos do
Abiyss.

## Fora deste escopo

Memória (`memoria/`), skills (`skills.rs`) e a conversa (`chat.rs`,
`historico.rs`) são trabalho de outra sessão e não foram alterados aqui. A
leitura deles mostra que nada fica guardado em memória entre chamadas: tudo
é lido do disco ou do banco, já com limites (`historico_max_mensagens`,
`[memoria] max_resultados`/`max_bytes_*`, `limite_central_caracteres`). O
único conjunto sem teto explícito é o catálogo de skills. Ele é montado de
novo a cada uso (não fica em cache) e cresce com o número de pastas em
`skills/`.
