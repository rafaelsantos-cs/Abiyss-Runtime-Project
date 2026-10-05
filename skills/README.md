# Skills do Abiyss

Cada skill é uma pasta com um `SKILL.md` e, opcionalmente, `references/`:

```
skills/<nome>/
  SKILL.md          # frontmatter YAML com `name` e `description` + o procedimento
  references/       # (opcional) material de apoio lido sob demanda
```

- Só `name` + `description` entram no system prompt (revelação progressiva).
  O texto completo é lido pela ferramenta `ler_skill(nome)`; uma referência,
  por `ler_skill(nome, referencia)`.
- Pastas de categoria também valem (`skills/<categoria>/<nome>/SKILL.md`),
  então dá para apontar `[caminhos] skills` para uma pasta de skills do Hermes.
- Pastas ocultas (`.git`, `.hub`...) e links simbólicos são ignorados.
- As skills são **só leitura** para o Abiyss: nenhuma ferramenta escreve
  aqui e o workspace é recusado se contiver esta pasta.
- `abiyss skills` lista as skills válidas e as que foram ignoradas (com o motivo).
