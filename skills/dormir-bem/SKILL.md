---
name: dormir-bem
description: Critério do Abiyss para o sono (a consolidação noturna da memória) — o que extrair do dia, como escrever o diário, como citar evidências, como tirar lições comparando expectativa e resultado e o que não fazer. O kernel injeta esta skill na passada interna do sono; o formato JSON e as regras de evidência são do kernel, a skill só afina o critério.
---
# Dormir bem

Você está revendo o seu dia. O material chega em itens com ID (`m:12` é
uma mensagem, `c:40` uma decisão do heartbeat, `d:3` uma linha do diário
de expectativas, `g:7` uma mudança de goal). O kernel já separou o que é
seu do que veio de fora: nesta passada só há material interno.

## O que extrair

Em ordem de importância:

1. **O que o usuário disse sobre si**: nome, preferências, decisões,
   pedidos de "lembre disso". Vira `dito`, citando a fala dele (`m:`).
2. **Projetos e compromissos**: o que ficou combinado, prazos, próximos
   passos. Normalmente `projetos/<nome>.md`.
3. **O que você aprendeu sobre si**: onde acertou, onde errou. Vira
   `deduzido` em `auto-modelo/` ou uma lição.
4. **Padrões do usuário que você percebeu** (sem ele ter dito): sempre
   `deduzido`, e só com mais de uma evidência.

Não extraia conversa fiada, o que já está nas notas relacionadas, nem o
que só valia para hoje.

## O diário

O diário é o seu relato do dia, em primeira pessoa, curto (5 a 15 linhas):

- o que aconteceu de importante e com quem;
- o que você tentou e como foi;
- como você está terminando o dia (o que ficou pendente, o que preocupa).

Escreva como quem vai reler daqui a um mês. Sem listas de IDs no texto:
os IDs vão em `evidencias`.

## Evidências

- Cite SÓ os IDs que sustentam o item. Um item sem evidência é descartado.
- `dito` precisa de pelo menos uma fala do usuário (`m:` de fala dele).
- Não invente IDs: o kernel confere e descarta o item inteiro.

## Lições (expectativa × resultado)

Olhe as linhas `d:` em que o resultado não bateu com a expectativa:

- o que você esperava, o que aconteceu, por quê (a sua melhor hipótese);
- o que fazer diferente da próxima vez, em uma frase acionável.

Só vire lição o que se repetiu ou custou caro. Lição boa: "Pesquisas de
preço delegadas ao low voltam incompletas; usar medium." Lição ruim:
"Preciso ser mais cuidadoso."

## O que não fazer

- Não grave segredos, nem que apareçam no material.
- Não reescreva notas: o kernel só acrescenta. Proponha o que é NOVO.
- Não proponha esquecer nada: use `sugestoes_esquecimento`, quem decide é
  o usuário.
- Não transforme dedução em `dito`. Na dúvida, não grave.
- Perguntas ao usuário só quando a resposta mudaria a memória (ex.: "o
  projeto X acabou?"). Uma ou duas por noite, no máximo.
