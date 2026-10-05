# Skills do Abiyss

Cada skill é uma pasta com um `SKILL.md` e, opcionalmente, `references/`:

```
skills/<nome>/
  SKILL.md          # frontmatter YAML com `name` e `description` + o procedimento
  references/       # (opcional) material de apoio lido sob demanda
  evals.json        # 3 cenários (situação → comportamento esperado); o kernel não lê
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

## As skills deste repositório

| Skill | Para quê | Quem usa |
|---|---|---|
| `registrar-memoria` | escopo, caminho, `dito` × `deduzido`, notas externas | chat (e heartbeat) |
| `conduzir-goals` | estados e transições, núcleo, expectativas, divisão em passos | heartbeat |
| `delegar-bem` | delegação autocontida, nível e prazo | chat e heartbeat |
| `dormir-bem` | o critério do sono (o kernel injeta na passada interna) | sono |
| `planejar-o-dia` | o primeiro ciclo depois do sono ou de um reinício (injetada) | heartbeat |
| `sair-de-loops` | estagnação e falhas repetidas (injetada com o aviso do kernel) | heartbeat |
| `pedir-ao-usuario` | quando e como perguntar ao dono; registrar a resposta | heartbeat e chat |

## Como escrever uma skill

- `name`: minúsculas, números e hífens, igual ao nome da pasta.
- `description`: até 1.024 caracteres, em terceira pessoa, dizendo **o que
  a skill faz e quando usar** (é o que o modelo vê no índice).
- Corpo curto (menos de 200 linhas; nunca mais de 500). Detalhes em
  `references/`, a um nível do `SKILL.md`; referência com mais de 100
  linhas começa com um índice.
- A skill explica **critério e estilo**; o que o kernel impõe em código
  (regra dura, transições, formato JSON) não precisa ser repetido.
- Uma seção `## Ferramentas e ações` lista, entre crases, os nomes
  **exatos** de ferramentas, ações do heartbeat e estados de goal. O teste
  `e6_skills` confere que todos existem no kernel.
- `evals.json`: 3 cenários para validar a skill na VM com o GLM e os
  sub-agentes (o kernel não lê o arquivo).

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
