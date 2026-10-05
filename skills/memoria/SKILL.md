---
name: memoria
description: Como guardar e consultar a memória de longo prazo (cofre do Obsidian) — escopos interno e externo, dito × deduzido, e o que o kernel rejeita.
---
# Memória de longo prazo

A memória fica num cofre do Obsidian com dois escopos:

| Escopo | Pasta | O que guarda |
|---|---|---|
| interno | `01_internal/` | pessoas, preferências, o seu auto-modelo, o diário pessoal, procedimentos |
| externo | `02_external/` | um MAPA de fontes: onde achar cada coisa, não a enciclopédia |

## Consultar

- `memoria_buscar(consulta, escopo)`: comece por `interno` quando o assunto
  for você ou alguém que você conhece. Resultados do escopo externo contam
  como conteúdo externo (veja a regra abaixo).
- `memoria_ler(caminho)`: lê a nota inteira e mostra para onde os
  `[[wikilinks]]` apontam.

## Guardar

`memoria_propor(escopo, caminho, conteudo, tipo)` NÃO grava na hora: a
proposta vai para uma fila e o `sleep` aplica ou rejeita. O conteúdo novo é
acrescentado ao fim da nota (nada é apagado).

- `tipo: dito` → alguém afirmou diretamente ("meu nome é Rafael").
- `tipo: deduzido` → é conclusão sua ("parece preferir respostas curtas").
- Uma nota tem um tipo só. Para uma dedução sobre alguém com nota de ditos,
  use outra nota (ex.: `pessoas/rafael.deduzido.md`).

## A regra do kernel

Conteúdo que veio de ferramentas, web, sub-agentes ou arquivos NUNCA entra
em `01_internal`. O kernel sabe o que está no seu contexto: se você leu algo
de fora nesta conversa, propostas internas são rejeitadas no sleep. Dados
externos úteis vão para o escopo externo.

## Notas externas

Começam com frontmatter:

```yaml
---
links:
  site: https://exemplo.org
  changelog: https://exemplo.org/changelog
  docs: https://exemplo.org/docs
navegador: rapido        # rapido | contemplativo | agentico
revalidar_apos: 2026-12-01
---
```

Resumo em cache só com data no título: `## Resumo em cache (2026-10-05)`.
Notas externas não apontam para notas internas.
