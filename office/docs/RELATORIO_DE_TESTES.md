# Relatório de testes

Ambiente: VM KVM x86_64, 4 vCPUs, 15 GiB, sem GPU, headless; Node 22.22.0;
Chromium 1194 (Playwright 1.56.1) com WebGL2 por SwiftShader. Data:
2026-10-05.

## Como reproduzir

```bash
cd office
npm ci                       # three + playwright (dev)
npm run check                # sintaxe + imports de todos os módulos
npm test                     # 43 testes (node:test), ~4 s
npm run soak -- --horas 6 --sementes 1,2,3,4
npm run screenshots          # capturas 00–11 (precisa do Chromium do Playwright)
ABIYSS_BIN=/caminho/runtime/target/release/abiyss \
  node scripts/runtime-integration.mjs   # integração com o runtime real + capturas 12–15
```

## 1. Verificação estática

`npm run check`: **53 arquivos, 0 problemas** (`node --check` + todo import
relativo aponta para um arquivo existente).

## 2. Testes automatizados — 43/43 passando

| Arquivo | O que cobre |
|---|---|
| `tests/navigation.test.js` (10) | grade (paredes, janelas, portas, fora do prédio); **todos os 2304 pares de POIs alcançáveis** e cada segmento de cada caminho com linha de visada livre; assentos só para quem reservou; sair da sala do Abiyss só pela porta; entrar só pela porta principal; determinismo; o `Mover` nunca sai da área livre e tem velocidade realista; desvio de obstáculo dinâmico; 4 mesas com assento |
| `tests/dsr.test.js` (13) | **paridade com `ritmo.rs`** (mesmos casos do kernel); leitura de `[ritmo]` do `abiyss.toml`; **adaptador sobre um banco criado com as 10 migrações reais do kernel** (copiadas literalmente em `tests/fixtures/runtime-migrations.sql`): campos, goal em foco, ciclos, eventos, conversa; **não escreve** (hash SHA‑256 do arquivo igual antes/depois e escrita recusada); daemon parado; sono por `origem='sono'`; banco ausente e banco antigo; ponte: atribuição justa e determinística, liberação, transbordo, primeiro instantâneo sem reencenar o passado, falha da fonte, persistência por fonte; emulador demo determinístico, estados válidos, ≤ 4 em execução, abertura roteirizada, **goals só com transições permitidas pelo kernel**, sono pausando o heartbeat |
| `tests/environment.test.js` (7) | sol em Contagem (nascer 05:34, pôr 17:55 em 05/10; sol ao norte em junho; quase a pino em dezembro; leste de manhã); códigos WMO; parser da Open-Meteo; climatologia determinística/plausível/marcada como estimada; serviço com observação real e com falha da API; modelo de luz; ambiente completo |
| `tests/server.test.js` (5) | proteção de caminhos estáticos (`../`, `%2e%2e`); precedência e validação da configuração; SSE (`hello`/`meta`/`tick`) e API; **ponta a ponta**: sobe o processo real, simula, para com SIGTERM (código 0) salvando o estado, **reinicia e restaura**; configuração inválida (código 2) e **porta ocupada** (código 1) com mensagem clara |
| `tests/simulation.test.js` (8) | **soak de 2 h** com invariantes a cada tick (0 violações, 0 recuperações, 0 travamentos, 0 sobreposições; todos trabalham e andam; ≥ 6 atividades por IMMo; alguém descansa e volta à mesa; < 5% do tempo todos fazendo a mesma coisa; Abiyss supervisiona/pensa/delega/recebe); abertura (surgem na calçada, entram, 3 trabalham, o livre passeia → sofá → volta à mesa); noite (dormem no descanso e **acordam na vigília**); sono do runtime; daemon parado; tarefa acorda o IMMo, que trabalha na **própria mesa** e entrega o relatório; determinismo; reinício com estado salvo e arquivo corrompido |

## 3. Soak (sem navegador)

`npm run soak -- --horas 6 --sementes 1,2,3,4` — 24 h de simulação no total,
começando às 06:00 (descanso → vigília):

| semente | resultado | tempo real | violações | recuperações | travamentos | sobreposições | replanejamentos | trabalharam | voltaram à mesa | sub-agentes |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | ✔ | 3,7 s | 0 | 0 | 0 | 0 | 346 | 4/4 | 4/4 | 251 |
| 2 | ✔ | 3,5 s | 0 | 0 | 0 | 0 | 330 | 4/4 | 4/4 | 257 |
| 3 | ✔ | 3,3 s | 0 | 0 | 0 | 0 | 265 | 4/4 | 4/4 | 257 |
| 4 | ✔ | 3,8 s | 0 | 0 | 0 | 0 | 380 | 4/4 | 4/4 | 261 |

Distribuição do tempo (todas as entidades): andando 18%, trabalhando 18%,
ocioso 13%, descansando 11%, dormindo 11%, Abiyss se deslocando 6%, sentado
ocioso 6%, passeando 4%, café 3%, conversando 2%, janela 2%, e o resto.

"Replanejamentos" são desvios de outras entidades (o mecanismo normal de
evitar colisão), não falhas.

## 4. Integração com o runtime REAL

`scripts/runtime-integration.mjs` não compila nada: usa o binário do runtime
(`cargo build --release` na branch `claude/determined-carson-6qrkl4`, 2 min
48 s nesta VM) e sobe `mock-nim --roteiro resistencia` + `abiyss daemon`
reais (heartbeat de 8 s, latência do mock de 15 s) + o escritório em modo
runtime. **10/10 verificações** (`artifacts/screenshots/runtime-integration.json`):

1. lê o banco real do runtime (migração 10);
2. daemon real aparece como "rodando";
3. IMMo trabalha num sub-agente que existe no banco (`subagentes.id` e nível conferidos);
4. o IMMo trabalha na própria mesa;
5. o relatório só aparece depois de o banco marcar o fim (`concluido`);
6. ciclos reais do heartbeat (`chamou_modelo = 1`) viram "pensando";
7. PID do daemon conhecido;
8. **SIGTERM no daemon real → escritório mostra "parado" e o Abiyss desliga**
   (e os sub-agentes interrompidos aparecem como `falhou`, exatamente como o
   kernel grava);
9. nenhuma invariante violada;
10. leitura do runtime sem erros até o fim.

Os sub-agentes do teste vêm de duas fontes: as delegações do próprio
heartbeat do kernel (o mock delega um `low` a cada 4 chamadas) e pedidos
registrados pelo script do mesmo jeito que a ferramenta `delegar` (um
`INSERT` com `estado='pendente'`). Em ambos os casos, quem executa é o
daemon real.

## 5. Screenshots reais (`artifacts/screenshots/`)

Capturadas do software rodando (Chromium headless, 1600×900). Cada uma tem
um `.json` com o estado das entidades no instante.

| # | Arquivo | Mostra |
|---|---|---|
| 00 | `00-chegada.png` | abertura: IMMos chegando pela calçada e entrando pela porta |
| 01 | `01-visao-geral.png` | visão geral: 3 IMMos trabalhando, Abiyss supervisionando, IMMo-4 andando |
| 02 | `02-abiyss-sala.png` | Abiyss em sua sala recebendo um relatório (IMMo com ✔, Abiyss verde) |
| 03 | `03-immos-trabalhando.png` | IMMos sentados digitando, rótulos `#id · nível · executando` |
| 04 | `04-descanso-sofa.png` | IMMo descansando no sofá; TV com hora e clima de Contagem |
| 05 | `05-immo-caminhando.png` | câmera seguindo um IMMo caminhando |
| 06 | `06-estados-simultaneos.png` | vários estados ao mesmo tempo + painel de debug |
| 07 | `07-abiyss-pensando.png` | Abiyss "pensando" (heartbeat que chamou o modelo), com o motivo real |
| 08 | `08-planta-caminhos.png` | planta com caminhos e destinos (debug) |
| 09 | `09-noite-chuva.png` | 23:24, fase descanso, chuva: lâmpadas acesas, IMMos dormindo |
| 10 | `10-dormindo.png` | IMMo deitado no sofá e outro encolhido no pufe, dormindo |
| 11 | `11-por-do-sol.png` | 17:25: luz baixa e quente vinda do oeste |
| 12–15 | `12…15-runtime-real-*.png` | **com o runtime real**: visão geral, mesas, relatório e daemon parado |

O ciclo implementar → executar → capturar → inspecionar → corrigir foi
seguido. Problemas achados **pelas capturas** e corrigidos:

| Problema visto | Correção |
|---|---|
| visão geral cortava o sofá | enquadramento da visão "Geral" |
| 724 draw calls | paredes e LEDs unidos por material (→ ~400) |
| ícones ilegíveis na visão ampla | ícones/rótulos crescem com a distância da câmera |
| armários da copa "flutuando" com a parede cortada | trocados por frontão baixo |
| gotas de chuva projetadas sobre o interior (prédio sem teto) | chuva mais baixa e afastada do prédio |
| "escala de tempo 14×" no debug | taxa de reprodução passa a ser a anunciada pelo servidor |
| câmera "seguir" atrasada em FPS baixo | suavização independente do FPS |
| momentos curtos (relatório, pensando) perdidos | script posiciona a câmera antes e captura no instante |

## 6. Problemas encontrados durante o desenvolvimento (e corrigidos)

| Problema | Causa | Correção |
|---|---|---|
| 9 travamentos e 1 violação na primeira simulação de 10 min | (a) zona de desaceleração atrás de outra entidade criava impasse sem disparar o replanejamento; (b) linha de visada amostrada a cada 5 cm "raspava" quinas de células bloqueadas | (a) quase parado conta como bloqueado; (b) travessia **exata** de grade |
| 4 sobreposições em 2 h | cruzamento lateral fora do cone frontal | regra de espaço pessoal (60 cm) |
| sofá do meio inalcançável | a área do "deitado" bloqueava o lugar do meio | máscara permitida inclui assentos que travam o POI |
| `npm test` com pasta | `node --test tests/` não expande pasta no Node 22 | glob `"tests/*.test.js"` |

## 7. Limitações conhecidas

- Clima **real** não foi exercitado nesta VM (Open-Meteo bloqueada → 403);
  o parser foi testado com uma resposta no formato da API e o caminho de
  reserva com falha simulada.
- Desempenho de renderização com GPU real não foi medido aqui (só
  SwiftShader).
- O "pensando" é mostrado depois que o ciclo é gravado (o runtime não grava
  ciclos em andamento).
- Daemon com laço travado (DEGRADADO no `abiyss status`) aparece como
  "parado".
