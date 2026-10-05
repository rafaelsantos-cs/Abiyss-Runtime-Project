---
name: sair-de-loops
description: Ensina o Abiyss a sair de um laço improdutivo — quando o kernel avisar estagnação, quando a mesma ação falhar duas vezes ou quando vários ciclos seguidos decidirem a mesma coisa sem progresso. Cobre diagnóstico, mudança de abordagem, passo menor, troca de nível de sub-agente e quando bloquear ou abandonar o goal com motivo.
---
# Sair de loops

Repetir a mesma ação esperando outro resultado gasta orçamento e não
resolve nada. Quando perceber o padrão (ou o kernel avisar), PARE e
diagnostique antes de agir de novo.

## Sinais de laço

- o kernel publicou um aviso de estagnação (ações repetidas sem progresso);
- a mesma delegação voltou `parcial` ou `falhou` duas vezes;
- a mesma transição foi recusada pelo kernel;
- os últimos ciclos têm a mesma `decisao` com outras palavras;
- o diário mostra expectativa × resultado divergindo do mesmo jeito.

## Diagnóstico (escreva na `orientacao`)

Responda, em uma frase cada:

1. O que eu esperava e o que aconteceu, das últimas vezes?
2. A causa é **falta de informação**, **falta de capacidade** (nível do
   sub-agente), **tarefa grande demais** ou **algo fora do meu alcance**?
3. O critério de pronto ainda está claro?

## O que fazer, conforme a causa

| Causa | Saída |
|---|---|
| falta de informação | delegar SÓ a coleta do que falta, com contexto completo |
| falta de capacidade | subir um nível (`low` → `medium` → `ultra`) |
| tarefa grande demais | dividir: delegar um passo menor, com critério próprio |
| fora do alcance | `bloqueado` com o motivo exato do que destrava |
| só o dono sabe ou decide | pergunte (veja a skill `pedir-ao-usuario`) e bloqueie até a resposta |
| o goal não faz mais sentido | `abandonado`, explicando por quê |

Mude UMA coisa por vez e diga na `expectativa` o que vai mostrar que
funcionou. Se a mesma saída também falhar duas vezes, bloqueie o goal: é
hora de o usuário decidir.

## O que não fazer

- Não repita a ação idêntica "para ver se agora vai".
- Não suba direto para `ultra` sem diagnóstico: é o nível mais caro.
- Não abandone um goal só porque está difícil.
- Não fique em `aguardar` para sempre: se nada vai mudar sozinho, bloqueie.

## Ferramentas e ações

- `delegar`: com outra abordagem, outro nível ou um passo menor.
- `cancelar_subagente`: o sub-agente atual está seguindo o caminho errado.
- `transicionar_goal`: para `bloqueado` ou `abandonado`, sempre com motivo.
- `pedir_ao_usuario`: quando só o dono destrava (decisão, acesso, preferência).
- `consultar_skill`: `delegar-bem`, para reescrever a delegação.
- `aguardar`: só quando algo externo vai mudar a situação sozinho.
