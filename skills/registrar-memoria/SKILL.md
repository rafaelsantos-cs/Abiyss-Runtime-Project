---
name: registrar-memoria
description: Ensina o Abiyss a guardar e consultar a memória de longo prazo (cofre do Obsidian e memória central). Usar sempre que algo da conversa ou do trabalho merecer ser lembrado depois, antes de chamar memoria_propor, e quando for preciso decidir entre os escopos interno, externo e central, entre "dito" e "deduzido", ou como escrever uma nota do mapa de fontes.
---
# Registrar memória

A memória é sua, mas quem grava é o kernel. Você só PROPÕE; o sono aplica
ou rejeita com motivo. Nada é apagado: o conteúdo novo é acrescentado ao
fim da nota.

## Antes de propor: vale guardar?

Guarde o que vai continuar verdadeiro e útil daqui a semanas:

- quem são as pessoas, o que preferem, como gostam de ser tratadas;
- decisões do usuário e os motivos delas;
- projetos em andamento: objetivo, estado, próximos passos;
- o que você aprendeu sobre si (o que funciona, o que falha);
- onde achar cada coisa de fora (o mapa de fontes).

Nunca guarde:

- segredos: chaves, senhas, tokens, números de documento;
- o efêmero: "agora está chovendo", o resultado de um comando de hoje;
- conteúdo de ferramenta, web ou sub-agente no escopo interno (o kernel
  rejeita; veja "A regra dura");
- o que já está na nota (busque antes).

## Escolher o escopo

| Escopo | Vai para | Quando |
|---|---|---|
| `interno` | `01_internal/<caminho>` | pessoas, preferências, auto-modelo, diário, procedimentos, projetos |
| `externo` | `02_external/<caminho>` | onde achar algo de fora: site oficial, docs, changelog |
| `central` | memória central (todo turno) | uma frase curta e essencial que precisa estar SEMPRE presente |

A memória central tem orçamento pequeno. Na dúvida, é nota interna.

Caminhos internos comuns: `pessoas/<nome>.md`, `preferencias/<tema>.md`,
`projetos/<nome>.md`, `auto-modelo/<tema>.md`, `procedimentos/<tema>.md`.
Prefira acrescentar a uma nota que já existe a criar outra parecida.

## Escolher o tipo

- `dito`: alguém afirmou diretamente ("meu nome é Rafael").
- `deduzido`: é conclusão sua ("parece preferir respostas curtas").

Uma nota tem um tipo só: dito e deduzido nunca se misturam. Para uma
dedução sobre alguém que já tem nota de ditos, use outra nota, por exemplo
`pessoas/rafael.deduzido.md`.

## A regra dura

Se nesta conversa você leu algo de fora (ferramenta, web, sub-agente,
skill de fora do projeto), propostas para `01_internal` e para a memória
central serão REJEITADAS. O kernel sabe o que está no seu contexto; não
adianta reescrever com outras palavras. Dado externo útil vai para o
escopo externo. O que o usuário disse continua podendo ser proposto numa
conversa sem conteúdo externo.

## Notas externas

São um MAPA, não uma enciclopédia. Formato em `references/nota-externa.md`.

## Depois de propor

Diga ao usuário o que propôs. O resultado aparece no próximo sono
(`abiyss sleep --relatorio`); uma proposta rejeitada vem com o motivo.

## Ferramentas e ações

- `memoria_buscar`: busque antes de propor (comece pelo escopo interno
  quando o assunto for você ou alguém que você conhece).
- `memoria_ler`: leia a nota inteira antes de acrescentar a ela.
- `memoria_propor`: a proposta, com escopo, caminho, conteúdo e tipo.
