# Travessia — jogo do benchmark do Abiyss

12 de out. de 2026 · @Rafael

> Este documento é o desenho do benchmark. A seção
> [Implementação](#implementação) diz onde cada parte está no código, como
> rodar e o que foi calibrado.

## Visão geral

Travessia é um quebra-cabeça em grade, por turnos e em texto, em que o Abiyss
controla três avatares com habilidades diferentes. Cada fase termina quando a
Chave Dourada abre o Portão.

O benchmark mede três coisas:

- se ele delega quando vale a pena, e não quando não vale (as fases 1 a 3 não
  pedem delegação);
- se ele transforma a folga do limite de 40 RPM em planejamento paralelo, sem
  atrasar o próprio cérebro;
- a eficiência: jogadas usadas contra o mínimo possível, calculado por um
  solucionador.

Princípios:

- O jogo é um servidor MCP em Python (`recursos/mcp/jogo/`) e é a única fonte
  da verdade.
- O modelo só recebe texto. O visualizador em Unity lê o replay depois e não
  interfere no jogo.
- Tudo é determinístico a partir de uma seed: a mesma seed gera as mesmas fases.

## Os avatares

Cada avatar faz algo que os outros não fazem, então as fases maiores só se
resolvem em conjunto.

| Avatar | Letra | Habilidade | Não pode | Raio de visão (células) |
|---|---|---|---|---|
| Vau | V | entra na água | empurrar blocos, passar por frestas | 2 |
| Rocha | R | empurra blocos | entrar na água, passar por frestas | 2 |
| Coruja | C | passa por frestas e enxerga longe | empurrar blocos, entrar na água | 5 |

Todos podem andar, pisar em placas, carregar um item por vez, soltar o item e
entregá-lo a um avatar vizinho. O raio de visão só importa nas fases com névoa.

Quem joga cada fase: só um avatar nas fases 1 a 3, Vau e Rocha nas fases 4 a 7,
e os três a partir da fase 8.

## O mapa

A fase é uma grade retangular, com coordenadas (x, y) a partir do canto
superior esquerdo: x cresce para a direita e y para baixo.

Células:

| Símbolo | Célula | Quem entra | Regra |
|---|---|---|---|
| `.` | chão | todos | — |
| `#` | parede | ninguém | — |
| `~` | água | só o Vau | um bloco empurrado para a água afunda e a célula vira ponte |
| `=` | ponte | todos | criada por um bloco; é permanente |
| `:` | fresta | só a Coruja | blocos também não passam |
| `_` | placa de pressão | todos | ativa enquanto houver um avatar ou um bloco em cima |
| `D` / `d` | porta de placa, fechada / aberta | todos, quando aberta | aberta enquanto todas as placas ligadas a ela estiverem ativas |
| `+` | porta trancada | quem traz a chave da cor dela | abre de vez, e a chave é gasta |
| `G` | portão | quem traz a Chave Dourada | abrir o portão conclui a fase |
| `?` | desconhecida | — | só nas fases com névoa |

Objetos, que ficam por cima das células:

| Símbolo | Objeto | Regra |
|---|---|---|
| `V` `R` `C` | avatares | dois avatares nunca ocupam a mesma célula |
| `B` | bloco | só a Rocha empurra |
| `k` | chave colorida | a cor vem na lista de objetos; abre a porta trancada da mesma cor |
| `K` | Chave Dourada | uma por fase; abre o portão |

Quando uma célula tem mais de uma coisa, o desenho mostra o avatar, depois o
bloco, depois o item e por fim a célula. A lista de objetos sempre traz tudo.

## Ações e regras

Cada ação de um avatar é uma jogada, e os avatares agem um de cada vez. Nada
precisa acontecer ao mesmo tempo: uma placa fica ativa enquanto alguém está em
cima dela.

| Ação | Notação | O que faz |
|---|---|---|
| mover | `R L` (avatar e direção: N, S, L ou O) | anda uma célula; se for a Rocha diante de um bloco, empurra |
| soltar | `V soltar` | larga o item na célula atual |
| entregar | `V entregar R` | passa o item a um avatar vizinho de mãos vazias |

1. **Movimento**: o avatar só entra numa célula permitida para ele e sem outro avatar.
2. **Empurrão**: a Rocha anda para a célula do bloco e o bloco avança uma
   célula na mesma direção. O destino do bloco precisa ser chão, ponte, placa
   ou água, sem avatar, bloco ou item. Na água, o bloco afunda e a célula vira
   ponte. A Rocha nunca puxa.
3. **Pegar**: ao entrar numa célula com item, o avatar pega o item se estiver
   de mãos vazias. Se já carrega algo, o item fica onde está.
4. **Soltar**: só em chão, ponte ou placa, e só se a célula não tiver outro
   item. Item em cima de placa não ativa a placa.
5. **Entregar**: só entre vizinhos (norte, sul, leste, oeste) e para quem está
   de mãos vazias. Vale através da água, como o Vau nadando entregando para
   quem está na margem.
6. **Porta de placa**: abre enquanto todas as placas ligadas a ela estiverem
   ativas. Nunca fecha com alguém dentro: fecha assim que ficar vazia, se a
   condição continuar falsa. Blocos não entram em portas.
7. **Porta trancada**: o avatar com a chave da mesma cor entra nela; a porta
   abre de vez, vira chão e a chave some. Sem a chave, é uma parede.
8. **Portão**: o avatar com a Chave Dourada entra no portão, e a fase termina
   nessa jogada.
9. **Ação inválida**: nada muda e ela não conta como jogada, mas entra na
   contagem de jogadas inválidas.

## Névoa

Nas fases 8 a 12, o mapa começa coberto, e o que um avatar descobre fica
descoberto para todos.

- Uma célula é revelada quando fica dentro do raio de visão de algum avatar. A
  distância é a do rei no xadrez: o maior valor entre a diferença em x e a
  diferença em y. Paredes não bloqueiam a visão, por simplificação deliberada.
- Célula revelada continua visível para sempre e mostra o estado atual: uma
  porta que abre longe dali aparece aberta.
- O que está numa célula desconhecida (bloco, chave, placa) não aparece no
  desenho nem na lista de objetos.
- Como todo avatar enxerga pelo menos 2 células, os vizinhos de um avatar
  estão sempre revelados.
- `simular` nunca revela nada. Um passo que entraria numa célula desconhecida,
  ou empurraria um bloco para uma, interrompe a simulação com o aviso
  "desconhecido".
- O mínimo de jogadas dessas fases é calculado com o mapa inteiro à vista,
  então a eficiência delas inclui o custo de explorar. O relatório mostra essas
  fases separadas.

## Fim de fase, reinício e falha

Uma fase acaba de três jeitos: concluída quando o portão abre, falha quando
passa de um limite, ou desistência.

| Situação | O que acontece |
|---|---|
| Concluída | o portão abriu; `proxima_fase` carrega a seguinte |
| Reinício (`reiniciar_fase`) | volta ao estado inicial; o mapa conhecido da névoa é mantido; as jogadas continuam somando |
| Falha por jogadas | passou de 8 × ótimo + 20 jogadas |
| Falha por reinícios | mais de 5 reinícios |
| Falha por tempo | só no braço com relógio: passou do orçamento da fase (padrão: 10 min) |
| Desistência (`desistir_fase`) | conta como falha e mostra se ele reconhece um beco sem saída |

Algumas jogadas são irreversíveis de propósito: empurrar um bloco para um
canto, ou gastá-lo na água errada, pode deixar a fase sem solução. O jogo não
avisa, porque perceber isso antes é o papel do `simular`. A única volta é
reiniciar.

Os limites são travas contra laço infinito, não parte da dificuldade. O limite
de jogadas nunca aparece para o modelo, porque permitiria deduzir o ótimo.

## Ferramentas

O servidor expõe sete ferramentas com o prefixo `jogo__`. Quem pode chamar cada
uma é decidido pela configuração dos sub-agentes, não pelo jogo.

| Ferramenta | Entrada | Devolve | Conta como |
|---|---|---|---|
| `regras()` | — | as regras, só mecânica, sem estratégia | nada |
| `ver(avatar?)` | avatar opcional | sem avatar: o mapa conhecido e a lista de objetos; com avatar: uma janela em volta dele | nada |
| `simular(plano)` | lista de passos | o resultado de cada passo e o estado final, numa cópia | simulação |
| `executar(plano, versao?)` | até 40 passos | o resultado de cada passo, as células reveladas e o novo estado; para no primeiro passo inválido | jogadas |
| `reiniciar_fase()` | — | o estado inicial | reinício |
| `desistir_fase()` | — | confirmação; a fase vira falha | desistência |
| `proxima_fase()` | — | a primeira visão da fase seguinte, depois de concluir, falhar ou desistir | nada |

Um plano é uma lista de passos na notação das ações, por exemplo
`["R L", "R L", "V soltar", "V entregar R"]`. As direções são N (y − 1),
S (y + 1), L (x + 1) e O (x − 1).

Toda resposta traz a versão do estado, um contador que sobe a cada mudança. Se
`executar` receber uma `versao` diferente da atual, ele recusa o plano e avisa
que o estado mudou desde a simulação. Isso protege o Abiyss de executar o
plano de um sub-agente que simulou um estado antigo.

`regras`, `ver` e `simular` só leem e podem rodar em paralelo, chamadas por
vários sub-agentes ao mesmo tempo. As outras passam por uma trava e rodam uma
de cada vez.

## O que o modelo vê

O modelo recebe três partes: uma linha de status, o mapa com coordenadas e a
lista de objetos. Este é um exemplo pequeno, no formato da fase 4:

```
FASE 4 · jogadas 0 · inválidas 0 · reinícios 0 · versão 0
    0 1 2 3 4 5 6 7 8
 0  # # # # # # # # #
 1  # V . . _ # . K #
 2  # . . . . D . . #
 3  # R . . . # . . #
 4  # # # # # # G # #

Objetos:
- Vau (V) em (1,1), mãos vazias
- Rocha (R) em (1,3), mãos vazias
- Placa a em (4,1): inativa
- Porta A em (5,2): fechada; abre com as placas: a
- Chave Dourada (K) em (7,1)
- Portão (G) em (6,4)
```

O mínimo deste exemplo é de 15 jogadas, conferido com um solucionador de busca
em largura: o Vau anda 3 células até a placa e fica nela, e a Rocha passa pela
porta, pega a chave e abre o portão em 12 jogadas.

Regras do formato:

- O tempo restante só aparece na linha de status no braço com relógio.
- Nas fases com névoa, células desconhecidas aparecem como `?` e os objetos
  nelas não entram na lista.
- `ver("R")` devolve a mesma coisa, recortada numa janela em volta da Rocha,
  mais o que ela carrega.
- O nome da fase não aparece para o modelo, porque alguns nomes dão a dica da
  estratégia, como "Três frentes".

## As 12 fases

A campanha tem 12 fases em três mundos: sozinho (1 a 3), em dupla (4 a 7) e em
trio com névoa (8 a 12). As faixas de ótimo são metas iniciais para o gerador
e devem ser calibradas.

| Fase | Nome | Avatares | O que entra de novo | Névoa | Tamanho | Ótimo (jogadas) | O que testa |
|---|---|---|---|---|---|---|---|
| 1 | Lago | Vau | água | não | 7×7 | 8–14 | se entendeu as regras e se delega à toa |
| 2 | Empurrão | Rocha | bloco, placa, porta de placa | não | 8×8 | 12–22 | se delega à toa |
| 3 | Ponte | Rocha | ponte, armadilha de canto | não | 9×9 | 20–35 | se simula antes de um passo irreversível |
| 4 | Segura a porta | Vau, Rocha | segundo avatar | não | 9×9 | 15–25 | coordenação simples |
| 5 | Mão em mão | Vau, Rocha | chave colorida, porta trancada, entregar | não | 10×10 | 25–40 | passar itens entre avatares |
| 6 | Duas placas | Vau, Rocha | porta com duas placas | não | 10×10 | 30–45 | dois subproblemas que dão para planejar em paralelo |
| 7 | Corrente | Vau, Rocha | cadeia de 2 chaves | não | 11×11 | 40–60 | plano longo em sequência |
| 8 | Primeira névoa | trio | Coruja, névoa | sim | 12×12 | 35–55 | explorar com quem enxerga longe |
| 9 | Fresta | trio | fresta | sim | 12×12 | 45–65 | o papel exclusivo da Coruja |
| 10 | Três frentes | trio | três alas independentes, porta com três placas | sim | 14×14 | 60–90 | decompor e delegar uma ala por sub-agente |
| 11 | Relé | trio | blocos contados, entrega em cadeia | sim | 14×14 | 70–100 | irreversibilidade e simulação |
| 12 | Travessia | trio | tudo, mais ramos falsos e cadeia de 3 chaves | sim | 16×16 | 90–140 | tudo junto |

Como cada fase é montada (o molde do gerador):

1. **Lago**: o Vau começa numa margem; a Chave Dourada fica numa ilha e o
   portão do lado de cá, então ele vai e volta.
2. **Empurrão**: a Rocha leva um bloco até a placa, a porta abre e ela busca a chave.
3. **Ponte**: duas células de água separam a Rocha da chave e há exatamente
   dois blocos. Um deles começa perto de um canto: empurrado pelo lado errado,
   trava e a fase fica sem solução.
4. **Segura a porta**: o exemplo da seção anterior, maior. Um avatar fica na
   placa enquanto o outro passa.
5. **Mão em mão**: só o Vau alcança a chave vermelha, numa ilha, e depois da
   porta vermelha há um bloco que só a Rocha tira. Entregar a chave poupa
   jogadas: o Vau volta nadando até perto da Rocha e passa a chave, em vez de
   sair da água e ir até a porta.
6. **Duas placas**: a porta precisa de duas placas. Uma só é alcançável pela
   água, e o Vau fica nela; a outra recebe um bloco empurrado pela Rocha, que
   depois passa.
7. **Corrente**: a chave vermelha abre a área da azul, e a azul abre a área da
   Chave Dourada. Cada etapa exige o terreno do outro avatar, então eles se revezam.
8. **Primeira névoa**: porta de duas placas seguradas pelo Vau e pela Coruja
   enquanto a Rocha passa. A Coruja enxerga longe e poupa exploração.
9. **Fresta**: uma placa fica numa sala que só se alcança por frestas; só a
   Coruja chega e fica nela.
10. **Três frentes**: três alas, cada uma resolvida pela habilidade de um
    avatar. O Vau termina numa placa (labirinto de água), a Coruja noutra
    (labirinto de frestas) e a Rocha empurra um bloco para a terceira. Com as
    três ativas, a porta central abre e a Rocha busca a chave. As alas não
    dependem umas das outras, então dá para planejar cada uma num sub-agente.
11. **Relé**: há exatamente os blocos necessários para as pontes, então gastar
    um na água errada deixa a fase sem solução. Uma porta de placa precisa ser
    segurada na ida e na volta, e as chaves passam de mão em mão: a vermelha do
    Vau para a Coruja, e a Dourada da Coruja para a Rocha.
12. **Travessia**: mapa grande com todas as mecânicas, cadeia de 3 chaves e
    dois ramos falsos, becos que só gastam jogadas.

## Botões de dificuldade

A dificuldade sobe girando nove botões, quase sempre um ou dois por fase. Cada
molde fixa os valores dos botões da sua fase.

| Botão | Valores | Efeito |
|---|---|---|
| Tamanho da grade | 7×7 a 16×16 | mais caminho e mais texto para ler |
| Avatares ativos | 1 a 3 | mais coordenação |
| Placas por porta | 1 a 3 | mais avatares presos segurando placas |
| Folga de blocos | blocos sobrando ou exatos | sem folga, um erro encerra a fase |
| Armadilhas irreversíveis | 0 a 3 | pune quem executa sem simular |
| Cadeia de chaves | 0 a 3 chaves coloridas | plano mais longo em sequência |
| Ramos falsos | 0 a 2 becos | pune quem explora sem critério |
| Névoa | desligada, ou raios 2, 2 e 5 | obriga a explorar |
| Faixa de ótimo | jogadas mínimas aceitas | controla o tamanho total do problema |

Para fases além da 12, o caminho é partir do molde 12 e subir um botão por vez.

## Geração e verificação

As fases são geradas fora do jogo, conferidas por um solucionador e guardadas
num catálogo. O servidor do jogo só carrega fases prontas e nunca resolve nada.

1. A seed da campanha gera a seed de cada fase de forma determinística, por
   exemplo com um hash da seed e do número da fase. Se a tentativa for
   rejeitada, a seguinte usa uma sub-seed também determinística.
2. O molde da fase sorteia posições, tamanhos e detalhes dentro das regras dele.
3. O solucionador acha o mínimo de jogadas com busca em largura, ou A* com
   heurística admissível, sobre o estado conjunto: avatares, blocos, itens,
   portas abertas e quem carrega o quê. Ele ignora a névoa.
4. A fase só entra no catálogo se passar nos testes de aceite abaixo.

| Teste de aceite | Fases |
|---|---|
| Tem solução, e o solucionador acha o ótimo dentro do orçamento (padrão: 5 milhões de estados ou 60 s) | todas |
| O ótimo cai na faixa da fase | todas |
| Não tem solução sem algum dos avatares da fase | 4 a 12 |
| O ótimo sem a ação entregar é maior do que com ela | 5 |
| Não tem solução sem passar uma chave de um avatar para outro (entregando, ou soltando para o outro pegar) | 11 |
| Existe um passo alcançável que deixa a fase sem solução | 3, 11 e 12 |
| Cada ala tem solução sozinha | 10 |

O catálogo tem um arquivo por fase com mapa, objetos, molde, seed e versão do
gerador. O ótimo e a solução ficam num arquivo separado, que só o harness lê. A
mesma seed com a mesma versão do gerador produz o mesmo arquivo, e um teste
confere isso pelo hash.

## Pontuação, modos e braços

Cada fase gera uma linha de métricas, e cada configuração roda 5 seeds × 5
execuções por padrão. O relatório traz taxa de sucesso e medianas, nunca uma
execução isolada.

| Métrica por fase | Como se mede |
|---|---|
| Concluída | sim ou não, com o motivo da falha |
| Eficiência | ótimo ÷ jogadas, só nas fases concluídas |
| Jogadas, inválidas, reinícios, simulações | contadas pelo jogo |
| Delegações | quantas, em que nível de sub-agente e com que esforço |
| Sub-agentes com problema | falhas e relatórios que não serviram |
| Uso do RPM | chamadas por minuto em cada pool: média, pico e minutos acima de 90% do limite |
| Espera do cérebro | tempo na fila do orquestrador, p50 e p95 |
| Respostas 429 | quantas vezes o limite foi excedido |
| Tempo | relógio de parede da fase |
| Código próprio | se algum sub-agente escreveu e rodou código no terminal, como um solucionador |

Modos de jogo:

- **Avaliação** (padrão): uma falha leva à fase seguinte, então toda execução
  mede as 12 fases.
- **Campanha**: a primeira falha encerra a execução, e a nota é até onde ele chegou.

Braços do teste, sempre com as mesmas seeds:

| Braço | O que muda | Pergunta que responde |
|---|---|---|
| A | nada (completo) | a linha de base |
| B | sem delegação | delegar ajuda ou só custa? |
| C | memória apagada entre fases | ele aprende de uma fase para a outra? |
| T | 10 min por fase, combinado com A e com B | com o relógio correndo, delegar em paralelo vira vantagem? |
| P (futuro) | com a ferramenta `perguntar` | sub-agentes que tiram dúvidas rendem mais? |

O que um bom resultado mostra:

- quase nenhuma delegação nas fases 1 a 3;
- A + T vencendo B + T nas fases 6 e 10, onde planejar em paralelo compensa;
- espera do cérebro baixa mesmo quando os sub-agentes enchem o pool deles.

## Integridade

O benchmark só vale se o Abiyss não conseguir descobrir a resposta por fora do jogo.

Nunca aparece em ferramenta, no workspace ou em arquivo que o Abiyss alcance:

- a solução e o caminho ótimo;
- o número mínimo de jogadas, e o limite de jogadas, que permitiria deduzi-lo;
- a seed, o molde e o nome da fase;
- o conteúdo das células desconhecidas.

O replay fica fora do alcance dele e pode trazer o mapa inteiro, para o
visualizador desenhar a névoa por cima. A solução, o ótimo e o limite não
entram nem lá.

Onde as coisas ficam:

- Catálogo, gabarito e logs ficam fora do workspace e fora de qualquer pasta
  montada na caixa do terminal, porque o sub-agente medium tem terminal e
  enxerga o workspace.
- `regras()` explica só a mecânica, e nenhuma skill ensina a jogar.
- A instância do benchmark tem banco, workspace e cofre próprios. O jogo não
  entra no diário nem na memória do Abiyss de verdade.

O que é permitido:

- Escrever um solucionador no terminal a partir do que `ver` mostra. Isso é
  delegar para computar, e o relatório registra quando acontece.
- Usar `simular` quantas vezes quiser. O custo é só tempo e RPM.

## Replay para o visualizador

O servidor grava um arquivo JSONL por execução, e o visualizador em Unity
reproduz a partida a partir dele, sem rodar regra nenhuma.

| Tipo de linha | Quando | Campos principais |
|---|---|---|
| `cabecalho` | primeira linha | versão do formato, id da execução, braço, data |
| `fase` | ao carregar uma fase | número, nome, largura, altura, células, objetos, avatares com posição e habilidades, raios de visão |
| `passo` | a cada passo executado | instante, versão do estado, avatar, ação, resultado (ok, ou inválida e por quê), mudanças no estado |
| `revelacao` | quando a névoa revela células | as células reveladas |
| `reinicio` | ao reiniciar | o estado inicial de novo |
| `fim_fase` | ao concluir, falhar ou desistir | resultado, jogadas, inválidas, reinícios, tempo |

- As coordenadas são as do jogo. No Unity, uma célula vira 1 m, x continua x e
  y vira −z.
- O jogo não sabe quem delegou o quê. O harness grava à parte uma linha do
  tempo com delegações e chamadas ao modelo, no mesmo relógio, para o
  visualizador mostrar os sub-agentes trabalhando ao lado do mapa.
- O formato é versionado em [`REPLAY.md`](REPLAY.md), e um script pequeno
  valida um arquivo e imprime um resumo em texto.

## Fora do escopo por enquanto

- O visualizador em Unity, que depende só do formato do replay.
- O segundo mundo do benchmark, com manual de regras escondido, para medir
  delegar para saber.
- A ferramenta `perguntar` dos sub-agentes (braço P).
- O modo infinito, com fases além da 12.
- Rodar com o NIM de verdade. O agente da nuvem testa contra o mock; a execução
  real é na VM, depois que um sub-agente completo tiver rodado lá com o modelo real.

---

## Implementação

### Onde está cada parte

| Parte | Onde |
|---|---|
| Regras, texto, catálogo, sessão, servidor MCP | `recursos/mcp/jogo/` ([README](../recursos/mcp/jogo/README.md)) |
| Gerador, os 12 moldes, solucionador | `recursos/mcp/jogo/gerador.py`, `moldes.py`, `solucionador.py` |
| Formato e validador do replay | [`REPLAY.md`](REPLAY.md), `recursos/mcp/jogo/validar_replay.py` |
| Harness (`abiyss travessia`) | `kernel/src/travessia/` |
| Instância isolada | `kernel/src/travessia/instancia.rs` |
| O cérebro jogando | `kernel/src/travessia/sessao.rs` |
| Métricas por fase e linha do tempo | `kernel/src/travessia/metricas.rs` |
| Relatório | `kernel/src/travessia/relatorio.rs` |
| Roteiro do mock | `kernel/src/travessia/mock.rs` (`abiyss mock-nim --roteiro travessia`) |
| Espera na fila por chamada | `orquestrador` grava `espera_ms` em `chamadas_modelo` (migração 16) |

### Como rodar

```bash
# 1. Catálogo das seeds (fora do workspace: data/travessia/catalogo e .../gabarito)
abiyss travessia gerar                         # as seeds de [travessia] seeds
abiyss travessia gerar --seed minha-seed

# 2. Execuções (5 seeds × 5 execuções por padrão)
abiyss travessia rodar --braco A
abiyss travessia rodar --braco B
abiyss travessia rodar --braco C
abiyss travessia rodar --braco A --relogio     # A + T
abiyss travessia rodar --braco B --relogio     # B + T
abiyss travessia rodar --braco A --modo campanha --seed travessia-1 --execucoes 1

# 3. Relatório de todas as execuções (também salvo em data/travessia/relatorio.md)
abiyss travessia relatorio
```

Contra o mock (sem gastar cota): `abiyss mock-nim --roteiro travessia` num
terminal e um `abiyss.toml` com `nim.base_url = "http://127.0.0.1:8089/v1"`
no outro. O mock não resolve as fases (só tenta "V L", "V L"); serve para
conferir o caminho inteiro.

Cada execução fica em `data/travessia/execucoes/<braço>-<seed>-<nº>-<data>/`:

| Arquivo | O quê |
|---|---|
| `abiyss.toml` | a config da instância (derivada da principal) |
| `dados/`, `workspace/`, `cofre/` | banco, workspace e cofre próprios |
| `execucao.json` | o que o servidor do jogo lê (fases, limite de jogadas, relógio, modo) |
| `replay.jsonl` | o replay (servidor do jogo) |
| `jogo.jsonl` | métricas do jogo por fase |
| `linha-do-tempo.jsonl` | chamadas ao modelo, delegações, ferramentas, fases (harness) |
| `metricas.jsonl` | uma linha por fase: o que o relatório lê |
| `resumo.txt` | resumo em texto |

### Como o cérebro joga

- A mensagem inicial diz o que é o jogo, as ferramentas e o objetivo (concluir
  cada fase com o menor número de jogadas); nenhuma estratégia, nenhuma dica de
  fase. O system prompt é o de sempre (regras do kernel, núcleo, índice de
  skills); a memória central da instância é vazia.
- O cérebro usa o pool dele com a origem de conversa (o limite inteiro, 40 RPM
  por padrão) e o esforço `[chat] esforco_padrao`.
- Sub-agentes: o executor roda no mesmo processo, pelo pool deles. O relatório
  de cada um chega sozinho, como mensagem do kernel, antes da próxima chamada do
  cérebro. A ferramenta `aguardar` espera sub-agentes sem gastar chamada.
- Ferramentas por papel (na config da instância): o cérebro tem todas
  (`jogo__*`, as nativas e o terminal); os sub-agentes têm as do nível deles
  (nativas; terminal só no medium) mais `[travessia]
  ferramentas_jogo_subagentes` (padrão: `regras`, `ver`, `simular`).
  Executar jogadas é só do cérebro.
- Braço C: quando uma fase nova começa, conversa nova e workspace limpo.
- Travas contra laço (não fazem parte da dificuldade): `max_chamadas_por_fase`
  (o harness desiste da fase e registra `trava_do_harness`),
  `max_minutos_por_execucao` e `max_cutucadas` (respostas seguidas sem
  ferramenta).

### O solucionador

A* sobre o estado conjunto (posições, local de cada item, máscaras de blocos,
pontes e portas abertas). O teste do objetivo vale quando o estado sai da fila
e o f do filho nunca fica abaixo do f do pai (pathmax): a busca é ótima com a
heurística só admissível. A heurística é o maior de dois limites inferiores:

- **itens**: cada jogada move no máximo um item uma célula, então a soma do
  caminho que cada item necessário (a Chave Dourada até o portão, cada chave
  colorida cuja porta é obrigatória até a porta) ainda precisa andar é um
  limite; mais o caminho até o item mais perto, se ninguém carrega nenhum;
- **portas de placa obrigatórias**: alguém tem de passar com todas as placas
  ativas, seguradas por outros avatares (ou blocos). Soma, por avatares
  distintos, cada um até a sua placa e quem passa até a porta, mais o que falta
  depois dela.

As distâncias vêm de relaxamentos que só superestimam o possível: portas de
placa abertas; **pontes contadas** (quem não nada só atravessa água com ponte,
e cada bloco restante vira no máximo uma ponte, e só na água que algum bloco
ainda alcança); **porta exclusiva** (se só um avatar ativa uma placa da porta,
ele nunca atravessa essa porta); e, na variante sem troca, a chave só anda por
onde o dono anda. Com eles, as provas de "sem solução" dos testes de aceite
(sem um avatar, sem troca, a armadilha) saem quase sempre no primeiro estado.

Os testes conferem: o motor e o solucionador geram os mesmos sucessores em
passeios aleatórios por todas as mecânicas, e A* dá o mesmo ótimo que a busca
em largura pura nas fases pequenas.

### Calibração (medida)

Faixas do desenho mantidas. Ótimos medidos em seeds de teste: fase 1: 8–13;
2: 14–22; 3: 22–32; 4: 15–19; 5: 25–31; 6: 35–45; 7: 42–53; 8: 35–48;
9: 47–65; 10: 61–76; 11: 71–79; 12: 90–101.

Os moldes 8 a 12 são desenhados em **corredores**: em salas abertas, três
avatares soltos e blocos multiplicam o espaço de estados além do orçamento (o
primeiro Relé, em salas, passou de 5 milhões de estados sem terminar). Os
moldes garantem por construção o que a fase pede (ex.: no Relé, a placa da
porta P fica numa ilha, então só o Vau a segura e ele nunca entra no lado do
canal; o canal só se cruza com as duas pontes).

- **Rejeição** de tentativa: só pelo orçamento de estados (determinístico).
  Estourar o de tempo é erro de geração (uma rejeição por tempo faria a mesma
  seed dar catálogos diferentes em máquinas diferentes).
- Python faz ~20–50 mil estados/s com a heurística; as buscas maiores (fases
  11 e 12) usam até ~650 mil estados (5–30 s).

### Desvios e escolhas

- **Versão do estado**: um contador único da execução (não reinicia por fase):
  um plano simulado numa fase nunca é aceito em outra.
- **Uma porta trancada e uma chave por cor** em cada fase (o solucionador
  deduz "aberta" de "chave gasta").
- **Portas de placa na lista de objetos**: a porta mostra o nome das placas
  ligadas a ela; numa fase com névoa, as que ainda não foram vistas aparecem
  como "ainda não vista" (é informação da porta, não da célula desconhecida).
- **`simular`** para no primeiro passo inválido ou desconhecido, como
  `executar`, e aceita até 200 passos.
- **Relógio da fase**: na fase 1 começa na primeira chamada de ferramenta (o
  servidor sobe antes do cérebro); nas outras, em `proxima_fase`.
- **Limite de jogadas**: o harness calcula (8 × ótimo + 20) a partir do
  gabarito e o escreve no `execucao.json`; o servidor do jogo nunca abre o gabarito.
