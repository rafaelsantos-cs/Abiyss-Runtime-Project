---
name: pedir-ao-usuario
description: Ensina o Abiyss a pedir algo ao dono quando ele não está conversando — quando vale pedir (ação irreversível, ambiguidade que muda o resultado, bloqueio que só ele destrava), como pedir (pergunta fechada, contexto mínimo, urgência honesta) e quando não pedir. Usar no heartbeat antes da ação pedir_ao_usuario e na conversa quando o dono responder a um pedido pendente.
---
# Pedir ao usuário

O dono não está olhando agora. O pedido fica numa caixa de entrada e ele
responde quando puder (horas ou dias). Cada pedido custa atenção dele:
pergunte pouco e bem.

## Quando pedir

- **Ação irreversível ou cara**: apagar, publicar, gastar, mexer em algo
  que não é seu. Na dúvida, pergunte antes.
- **Ambiguidade que muda o resultado**: duas leituras do pedido levam a
  trabalhos diferentes e você não consegue escolher com o que sabe.
- **Bloqueio que só ele destrava**: acesso, chave, decisão, preferência
  pessoal, critério de pronto de um goal.

## Quando NÃO pedir

- Dá para descobrir sozinho (memória, arquivos, um sub-agente).
- É uma escolha reversível e barata: escolha, diga qual escolheu e por quê.
- Já há um pedido pendente sobre o mesmo assunto (o kernel não duplica a
  mesma pergunta; não reescreva com outras palavras).
- Só para "manter informado": isso vai no diário, não na caixa dele.

## Como pedir

- **Pergunta fechada**: "Posso apagar workspace/tmp (2 GB, só caches)?
  sim/não" é melhor que "o que faço com o disco?".
- **Contexto mínimo**: o que ele precisa para responder em 10 segundos,
  sem abrir nada. Sem segredos.
- **Urgência honesta**: `alta` só se algo quebra ou se perde sem a
  resposta; `normal` para o que trava um goal; `baixa` para o resto.
- **Um assunto por pedido**, ligado ao goal (`goal_id`) quando houver.

Depois de pedir, não fique parado esperando: se o goal depende da
resposta, mova-o para `bloqueado` com o motivo "esperando o pedido #N".
Pedidos expiram (o kernel avisa): aí decida sem a resposta ou pergunte de
outro jeito.

## Quando ele responde

- Pela CLI, a resposta chega como evento no ciclo autônomo.
- Na conversa, se ele responder a um pedido pendente (listados no
  contexto), registre com `responder_pedido`, nas palavras dele. Depois
  siga a conversa normalmente.

## Ferramentas e ações

- `pedir_ao_usuario`: ação do heartbeat que guarda a pergunta.
- `responder_pedido`: ferramenta da conversa que registra a resposta.
- `transicionar_goal`: para `bloqueado` enquanto espera, e de volta para
  `executando` quando a resposta chegar.
