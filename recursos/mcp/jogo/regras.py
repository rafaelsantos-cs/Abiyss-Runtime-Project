"""O texto de `regras()`: só a mecânica do jogo.

Nada de estratégia, nada de dica de fase, nada que permita deduzir o mínimo
de jogadas (o limite de jogadas existe, mas o número nunca aparece).
"""

from __future__ import annotations

REGRAS = """\
TRAVESSIA — regras

Quebra-cabeça em grade, por turnos. Você controla até três avatares. Cada fase
termina quando a Chave Dourada abre o Portão.

COORDENADAS
(x, y) a partir do canto superior esquerdo: x cresce para a direita, y para
baixo. Direções: N (y − 1), S (y + 1), L (x + 1), O (x − 1).

AVATARES
- Vau (V): entra na água. Não empurra blocos, não passa por frestas. Visão 2.
- Rocha (R): empurra blocos. Não entra na água, não passa por frestas. Visão 2.
- Coruja (C): passa por frestas. Não empurra blocos, não entra na água. Visão 5.
Todos andam, pisam em placas, carregam um item por vez, soltam o item e o
entregam a um avatar vizinho de mãos vazias.

CÉLULAS
  .  chão                 todos entram
  #  parede               ninguém entra
  ~  água                 só o Vau; um bloco empurrado para a água afunda e a
                          célula vira ponte
  =  ponte                todos entram; é permanente
  :  fresta               só a Coruja; blocos também não passam
  _  placa de pressão     todos entram; ativa enquanto houver um avatar ou um
                          bloco em cima (item em cima NÃO ativa)
  D  porta de placa       fechada; vira d (aberta) enquanto todas as placas
  d                       ligadas a ela estiverem ativas
  +  porta trancada       só entra quem traz a chave da cor dela; abre de vez,
                          vira chão, e a chave é gasta
  G  portão               só entra quem traz a Chave Dourada; abrir conclui a fase
  ?  desconhecida         só nas fases com névoa

OBJETOS (por cima das células)
  V R C  avatares (dois avatares nunca ocupam a mesma célula)
  B      bloco (só a Rocha empurra)
  k      chave colorida (a cor vem na lista de objetos)
  K      Chave Dourada (uma por fase)
Numa célula com mais de uma coisa, o desenho mostra o avatar, depois o bloco,
depois o item e por fim a célula. A lista de objetos traz tudo o que está em
células conhecidas.

AÇÕES (cada ação válida é uma jogada; os avatares agem um de cada vez)
  mover     "R L"           avatar e direção (N, S, L ou O)
  soltar    "V soltar"      larga o item na célula atual
  entregar  "V entregar R"  passa o item a um avatar vizinho de mãos vazias

1. Movimento: o avatar só entra numa célula permitida para ele e sem outro
   avatar.
2. Empurrão: a Rocha anda para a célula do bloco e o bloco avança uma célula
   na mesma direção. O destino do bloco precisa ser chão, ponte, placa ou
   água, sem avatar, bloco ou item. Na água, o bloco afunda e a célula vira
   ponte. A Rocha nunca puxa.
3. Pegar: ao entrar numa célula com item, o avatar pega o item se estiver de
   mãos vazias. Se já carrega algo, o item fica onde está.
4. Soltar: só em chão, ponte ou placa, e só se a célula não tiver outro item.
5. Entregar: só entre vizinhos (norte, sul, leste, oeste) e para quem está de
   mãos vazias. Vale através da água.
6. Porta de placa: abre enquanto todas as placas ligadas a ela estiverem
   ativas. Nunca fecha com alguém dentro: fecha assim que ficar vazia, se a
   condição continuar falsa. Blocos não entram em portas.
7. Porta trancada: o avatar com a chave da mesma cor entra nela; a porta abre
   de vez, vira chão e a chave some. Sem a chave, é uma parede.
8. Portão: o avatar com a Chave Dourada entra no portão, e a fase termina
   nessa jogada.
9. Ação inválida: nada muda e ela não conta como jogada, mas entra na
   contagem de jogadas inválidas.

Algumas jogadas não têm volta (empurrar um bloco para um canto, gastar um
bloco na água errada). O jogo não avisa. A única volta é reiniciar a fase.

NÉVOA (em algumas fases)
- O mapa começa coberto. Uma célula é revelada quando fica dentro do raio de
  visão de algum avatar: distância do rei no xadrez (o maior valor entre a
  diferença em x e a diferença em y). Paredes não bloqueiam a visão.
- O que um avatar descobre fica descoberto para todos, para sempre, e mostra
  o estado atual (uma porta que abre longe dali aparece aberta).
- O que está numa célula desconhecida não aparece no desenho nem na lista.
- Os vizinhos de um avatar estão sempre revelados.
- Reiniciar a fase mantém o mapa descoberto.

FIM DE FASE
- Concluída: o portão abriu.
- Falha: passou do limite de jogadas da fase (o número não é mostrado), ou
  mais de {max_reinicios} reinícios{relogio_texto}.
- Desistência (desistir_fase): conta como falha.
Depois do fim, proxima_fase carrega a fase seguinte.{modo_texto}
Reiniciar volta ao estado inicial; as jogadas continuam somando.

FERRAMENTAS
- regras(): este texto.
- ver(avatar?): sem avatar, o mapa conhecido e a lista de objetos; com
  avatar (V, R ou C), uma janela em volta dele e o que ele carrega.
- simular(plano): aplica o plano numa CÓPIA e mostra o resultado de cada
  passo e o estado final. Não conta como jogada e nunca revela nada: um
  passo que entraria numa célula desconhecida, ou empurraria um bloco para
  uma, interrompe a simulação com o aviso "desconhecido".
- executar(plano, versao?): executa até {max_passos} passos e para no
  primeiro inválido. Com `versao`, recusa o plano se o estado mudou desde
  então (por exemplo, desde a simulação que gerou o plano).
- reiniciar_fase(), desistir_fase(), proxima_fase().
Um plano é uma lista de passos na notação das ações, por exemplo
["R L", "R L", "V soltar", "V entregar R"].
Toda resposta traz a versão do estado: um contador que sobe a cada mudança.
"""


def texto(max_reinicios: int, max_passos: int, relogio_segundos: float | None, modo: str) -> str:
    relogio_texto = ""
    if relogio_segundos:
        minutos = relogio_segundos / 60
        quanto = f"{minutos:g} min" if minutos >= 1 else f"{relogio_segundos:g} s"
        relogio_texto = f", ou o tempo da fase acabou ({quanto} por fase; o tempo restante aparece na linha de status)"
    modo_texto = ""
    if modo == "campanha":
        modo_texto = " Nesta execução, a primeira falha encerra o jogo."
    return REGRAS.format(
        max_reinicios=max_reinicios, max_passos=max_passos, relogio_texto=relogio_texto, modo_texto=modo_texto
    )
