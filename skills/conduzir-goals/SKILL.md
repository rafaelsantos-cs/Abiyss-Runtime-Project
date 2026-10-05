---
name: conduzir-goals
description: Ensina o Abiyss a conduzir um goal do começo ao fim no heartbeat — quando comprometer, executar, validar, bloquear ou abandonar, como escrever o núcleo com critério de pronto, como declarar expectativas verificáveis e como dividir o trabalho em delegações. Usar quando houver um goal em foco, ao decidir uma transição de estado ou quando um goal parecer parado.
---
# Conduzir goals

Um goal é trabalho de vários ciclos. Em cada ciclo você faz UMA coisa que
o aproxima do critério de pronto e diz o que espera que aconteça.

## O núcleo

O núcleo é a essência do goal em uma ou duas frases, COM o critério de
pronto. Ele aparece no começo e no fim do seu contexto. Um bom núcleo:

- diz o que existe quando o goal acabar ("um resumo de 1 página em
  workspace/relatorio.md com as 3 opções e o custo de cada");
- dá para conferir sem opinião (arquivo existe, teste passa, o usuário
  confirmou);
- cabe numa frase. Se não cabe, o goal é grande demais: proponha dividir.

Quem cria goals é o usuário (`abiyss goal add`). Se o núcleo de um goal
não tiver critério de pronto, peça ao usuário (skill `pedir-ao-usuario`)
antes de comprometer.

## Os estados e as transições

O kernel só aceita estas transições (qualquer outra é recusada):

| De | Para |
|---|---|
| `proposto` | `comprometido`, `abandonado` |
| `comprometido` | `executando`, `bloqueado`, `abandonado` |
| `executando` | `validando`, `bloqueado`, `abandonado` |
| `validando` | `concluido`, `executando`, `bloqueado`, `abandonado` |
| `bloqueado` | `comprometido`, `executando`, `abandonado` |

`concluido` e `abandonado` são finais. Toda transição precisa de motivo.

Quando usar cada uma:

- **comprometer**: você entende o núcleo, tem um primeiro passo e nada
  impede começar.
- **executar**: o primeiro passo concreto vai ser dado AGORA (neste ciclo
  ou por uma delegação deste ciclo).
- **validar**: você acha que o critério de pronto foi atingido. Em
  `validando`, CONFIRA cada parte do critério (leia o arquivo, compare
  com o pedido). Faltou algo: volte para `executando` dizendo o quê.
- **bloquear**: falta algo que você não consegue obter sozinho (decisão do
  usuário, acesso, um recurso fora do ar). O motivo diz o que destrava.
- **abandonar**: o goal deixou de fazer sentido ou é impossível. Diga por
  quê; nunca abandone só porque está difícil (veja a skill `sair-de-loops`).

## Expectativas verificáveis

Toda ação leva uma `expectativa`. Escreva o que dá para conferir depois:

- ruim: "vai dar certo";
- bom: "o sub-agente devolve a lista dos 5 maiores arquivos com tamanhos";
- bom: "o goal passa a `executando` e o próximo ciclo delega a coleta".

O kernel anota a expectativa no diário e compara com o resultado. No sono,
as diferenças viram lições.

## Dividir o trabalho

- Um passo por ciclo. Não tente fazer o goal inteiro numa decisão.
- O que for longo (ler muito, pesquisar, rascunhar) vai para um
  sub-agente com `delegar` e o `goal_id` do goal. Veja `delegar-bem`.
- Enquanto um sub-agente trabalha, `aguardar` é uma decisão válida.
- Mais de um goal ativo: siga o goal em foco (o kernel escolhe pelo estado
  e pela prioridade). Não pule entre goals a cada ciclo.

## Ferramentas e ações

- `transicionar_goal`: muda o estado (goal_id, para e motivo).
- `delegar`: manda um passo para um sub-agente (inclua o goal_id).
- `cancelar_subagente`: quando o passo delegado deixou de fazer sentido.
- `consultar_skill`: para ler `delegar-bem` ou `sair-de-loops` quando precisar.
- `pedir_ao_usuario`: o que só o dono decide (critério de pronto, acesso).
- `aguardar`: nada a fazer agora, com motivo.
