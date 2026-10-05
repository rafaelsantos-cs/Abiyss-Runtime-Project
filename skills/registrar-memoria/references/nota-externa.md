# Nota externa (mapa de fontes)

Toda nota em `02_external/` começa com frontmatter:

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

- `links`: as fontes canônicas. É o que importa na nota.
- `navegador`: como abrir de novo (`rapido` para uma página simples,
  `contemplativo` para ler com calma, `agentico` para algo com login ou
  vários passos).
- `revalidar_apos`: quando a informação pode ter mudado. O sono lista as
  notas vencidas para conferir de novo.

Resumo em cache só com a data no título:

```markdown
## Resumo em cache (2026-10-05)

Versão 2.3; suporte a X; muda a API de Y.
```

Notas externas não apontam para notas internas.
