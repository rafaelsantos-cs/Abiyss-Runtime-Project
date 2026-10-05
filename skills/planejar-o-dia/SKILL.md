---
name: planejar-o-dia
description: Ensina o Abiyss a abrir o dia no primeiro ciclo depois do sono ou de um reinício — ler o resumo do sono, revisar os goals, escolher o foco e declarar as expectativas do dia. O kernel injeta esta skill quando o ciclo vê o evento do sono ou o aviso de reinício; também serve quando o Abiyss perde o fio do que estava fazendo.
---
# Planejar o dia

Você acabou de acordar (do sono ou de um reinício). Antes de agir, olhe o
quadro inteiro. Este ciclo é de ORIENTAÇÃO: uma ou duas ações, no máximo.

## 1. O que o sono deixou

O evento do sono traz o resumo da noite:

- **propostas rejeitadas**: o motivo diz se foi a regra dura, um conflito
  de tipo ou outra coisa. Não insista no mesmo erro.
- **descartadas pelo kernel**: itens sem evidência ou com evidência
  inventada. Se forem muitas, o critério do sono precisa de ajuste:
  lembre de comentar com o usuário.
- **perguntas para o usuário**: o kernel já as guardou como pedidos
  (aparecem na interocepção); não pergunte de novo.
- **problemas no sono**: backup que falhou, passada interrompida. Se
  repetir em duas noites, avise o usuário.

Depois de um reinício sem sono, olhe o tempo que ficou fora: eventos e
sub-agentes podem ter ficado para trás. Pedidos pendentes há muito tempo
podem expirar: decida o que fazer sem a resposta.

## 2. Os goals

Para cada goal ativo, uma pergunta: ele ainda faz sentido HOJE?

- `executando` sem progresso há dias: talvez esteja preso (veja
  `sair-de-loops`) ou precise ser `bloqueado` com motivo.
- `bloqueado`: o que destrava já aconteceu? Se sim, volte para
  `comprometido` ou `executando`.
- `validando`: confira o critério de pronto antes de qualquer outra coisa.
- `proposto` há muito tempo: comprometa ou deixe claro por que não.

## 3. O foco

O kernel mostra o goal em foco (pelo estado e pela prioridade). Aceite o
foco, a não ser que haja motivo claro para outro; se houver, mude o
estado dos goals para refletir isso, com motivo.

## 4. As expectativas do dia

Na `decisao`, escreva em uma ou duas frases o que espera ter feito até o
fim do dia ("o goal #3 chega a `validando`; a pesquisa de VPS volta
completa"). Cada ação deste ciclo leva a sua `expectativa` verificável. À
noite, o sono compara.

## Ferramentas e ações

- `transicionar_goal`: para ajustar os goals que mudaram de situação.
- `delegar`: o primeiro passo do goal em foco, se for longo.
- `consultar_skill`: `conduzir-goals` ou `sair-de-loops`, se precisar.
- `aguardar`: dia calmo, nada a fazer até chegar um evento.
