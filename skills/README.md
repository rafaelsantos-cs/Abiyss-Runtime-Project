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

## Confiança

- Skills de uma pasta **confiável** têm a mesma confiança do núcleo: lê-las
  não marca a conversa como conteúdo externo, então uma proposta de memória
  interna feita depois continua valendo. Por padrão, é confiável a pasta que
  fica **dentro deste projeto** (versionada no git: só muda por revisão).
- Skills de pastas de fora (ex.: as do Hermes, de *hubs*) são conteúdo
  **externo** para a regra dura da memória. Configure em `[skills] raizes`
  no `abiyss.toml`; `abiyss skills` mostra a confiança de cada uma.
- No modo autônomo (heartbeat), o Abiyss vê o mesmo índice e pede o texto
  com a ação `consultar_skill`. Algumas skills o kernel põe no contexto
  sozinho, em situações que ele detecta (`[skills.automaticas]`).
