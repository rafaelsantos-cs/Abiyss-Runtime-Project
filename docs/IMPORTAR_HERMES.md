# Importar a memória do Hermes

`abiyss importar-hermes --origem <pasta do Hermes> [--aplicar]`

O Abiyss rodava no Hermes. Este comando traz goals, diário, diário pessoal,
identidade e memórias para este runtime, **sem perder quem ele é** e sem
levar segredos junto.

> Os dados reais do Hermes não estavam disponíveis no desenvolvimento. Tudo
> foi testado com dados inventados (`kernel/tests/fixtures/hermes/`) no
> formato descrito abaixo. **Cada item desta página é uma suposição** a ser
> conferida na VM com a simulação (seção "Na VM").

## Como funciona

- **Simulação por padrão.** Sem `--aplicar`, só mostra o relatório; nada
  importado é gravado (se o banco ou o cofre ainda não existem, a simulação
  usa um banco em memória e uma pasta temporária). Se o banco já existe, o
  esquema dele pode ser atualizado (migrações), como em qualquer comando.
- **Idempotente.** Cada item importado ganha uma chave estável na tabela
  `importacoes` (ex.: `hermes:goal:g-001`). Rodar de novo não duplica nada:
  o relatório mostra tudo como "já importado".
- **Só lê o que está mapeado.** O resto da pasta é listado pelo nome
  ("não mapeados"), sem abrir.
- **Segredos nunca são abertos.** Arquivos e pastas cujo nome sugere segredo
  são listados como pulados. Links simbólicos não são seguidos.
- **Tolerante.** Cada campo aceita vários nomes (português e inglês). Campos
  desconhecidos são **preservados** (coluna `extras` em JSON, ou a cópia fiel
  da nota) e **listados** no relatório, nunca descartados. O relatório
  também mostra qual nome de origem foi usado para cada campo — é assim que
  se confere, com os dados reais, se as suposições estavam certas.
- Nunca altera `identity/nucleo.md`. Nunca sobrescreve
  `identity/nucleo.proposto.md` se ele já existir com outro conteúdo.
- Nunca traz de volta uma nota que o usuário esqueceu (`abiyss memoria esquecer`).
- Notas importadas guardam `origem: hermes:<arquivo>` no frontmatter: se o
  banco for recriado, a nota que já existe com essa origem não é duplicada.

| Origem (relativa a `--origem`) | Destino |
|---|---|
| `metacognition/goals/*.json` | tabela `goals` |
| `metacognition/journal.jsonl` | tabela `diario` |
| `metacognition/diario-pessoal.md` | `01_internal/diario/` (uma nota por dia) |
| `metacognition/identity/auto-modelo.json` | `01_internal/identidade/auto-modelo.md` + rascunho |
| `SOUL.md` | `01_internal/identidade/soul.md` + rascunho |
| `memories/MEMORY.md`, `memories/USER.md` | memória central; o que não couber → `01_internal/` |

## Suposições sobre os formatos

### Pasta

1. `--origem` é a raiz do Hermes (ex.: `~/.hermes`), com `memories/`,
   `metacognition/` e `SOUL.md` diretamente dentro.
2. `SOUL.md` fica na raiz; se não estiver lá, é procurado em
   `metacognition/identity/SOUL.md`.
3. `metacognition/` é uma estrutura criada para o Abiyss dentro do Hermes
   (não faz parte do Hermes padrão); os nomes dos arquivos são exatamente os
   da tabela acima.
4. Arquivos de texto em UTF-8 (bytes inválidos viram `�`). Até 64 MB cada.
5. As pastas `.git`, `node_modules`, `.venv`, `venv`, `__pycache__` e
   `.cache` não são percorridas (aparecem como "pastas não percorridas").
6. `config.yaml`, `sessions/`, `skills/`, `cron/`, `logs/`, `state.db` etc.
   **não** são importados (só listados). Skills do Hermes podem ser usadas
   apontando `[caminhos] skills` para `~/.hermes/skills` (são só leitura).

### `metacognition/journal.jsonl` → diário

1. JSON Lines: um objeto JSON por linha. Linhas em branco são ignoradas;
   linhas que não são JSON viram erro no relatório (com o número da linha).
2. Nomes aceitos (o primeiro presente vence):

   | Campo do diário | Nomes aceitos no Hermes |
   |---|---|
   | momento | `ts`, `timestamp`, `time`, `created_at`, `criado_em`, `data`, `date`, `quando`, `momento` |
   | ação | `action`, `acao`, `ação`, `decision`, `decisao`, `decisão`, `task`, `tarefa`, `event`, `evento`, `title`, `titulo` |
   | expectativa | `expectation`, `expectativa`, `expected`, `prediction`, `previsao`, `previsão`, `hypothesis`, `hipotese`, `hipótese` |
   | sinais | `signals`, `sinais`, `signal`, `sinal`, `evidence`, `evidencias`, `evidências` |
   | risco | `risk`, `risco`, `risks`, `riscos` |
   | confiança | `confidence`, `confianca`, `confiança`, `certainty`, `certeza` |
   | resultado | `outcome`, `resultado`, `result`, `observed`, `observado` |
   | momento do resultado | `outcome_at`, `resolved_at`, `resultado_em` |
   | goal | `goal_id`, `goal`, `objetivo`, `goal_ref` |
   | ID | `id`, `entry_id`, `uuid` |

3. Sinais: lista (vira `a; b`) ou texto. Risco: texto ou número (guardado
   como texto).
4. Confiança: número de 0 a 1; número de 1 a 100 ou `"70%"` é tratado como
   porcentagem; `"0,8"` também vale. Texto como `"alta"` não vira número:
   fica em `extras` como `confianca_original` (e o relatório avisa).
5. Datas: RFC 3339 (`2026-09-28T09:14:02-03:00`); `AAAA-MM-DD HH:MM[:SS]`
   **sem fuso = fuso local da máquina**; `AAAA-MM-DD` ou `DD/MM/AAAA`
   (meia-noite local); número = segundos (se < 10¹¹) ou milissegundos.
   Sem data reconhecível: usa o momento da importação e guarda o original
   em `extras` (`momento_original`).
6. `"outcome": null` = ainda sem resultado.
7. O goal é referenciado pelo **ID do Hermes**; se aquele goal foi
   importado, a entrada é ligada a ele. Se não, o ID fica em `extras`
   (`goal_hermes`).
8. Chave de importação: o `id`; sem `id`, um hash da linha. Consequência:
   editar no Hermes uma linha SEM `id` e importar de novo cria outra entrada.
9. A origem gravada é `hermes`. Sem ação → `(sem ação registrada no
   Hermes)`; sem expectativa → `(não declarada)`.

### `metacognition/goals/*.json` → goals

1. Cada arquivo `*.json` diretamente em `goals/` é um goal (objeto) ou uma
   lista de goals. Arquivos com JSON inválido viram erro (nada é importado
   daquele arquivo); outros arquivos da pasta são "não mapeados".
2. Nomes aceitos:

   | Campo do goal | Nomes aceitos no Hermes |
   |---|---|
   | ID | `id`, `goal_id`, `slug`, `uuid` (sem ID: nome do arquivo; em listas, `nome#posição`) |
   | título | `title`, `titulo`, `título`, `name`, `nome`, `goal`, `objetivo` |
   | núcleo (critério de pronto) | `core`, `nucleo`, `núcleo`, `essence`, `essencia`, `essência`, `success_criteria`, `criterio`, `critério`, `criterio_de_pronto`, `done_when`, `definition_of_done` |
   | descrição | `description`, `descricao`, `descrição`, `details`, `detalhes`, `why`, `porque`, `context`, `contexto` |
   | prioridade | `priority`, `prioridade`, `importance`, `importancia`, `importância` |
   | estado | `status`, `state`, `estado`, `lifecycle`, `fase`, `stage` |
   | criado em | `created_at`, `criado_em`, `created`, `criado`, `ts`, `timestamp`, `data` |

3. Estado (sem diferenciar maiúsculas, acentos, espaço e hífen):

   | Estado no kernel | Estados aceitos no Hermes |
   |---|---|
   | `proposto` | proposed, proposto, draft, rascunho, idea, ideia, backlog, new, novo, suggested, sugerido, pending, pendente |
   | `comprometido` | committed, comprometido, accepted, aceito, planned, planejado, todo, to_do, ready |
   | `executando` | active, ativo, in_progress, inprogress, em_andamento, andamento, executing, executando, doing, fazendo, running, started, iniciado, working, ongoing |
   | `validando` | validating, validando, validation, review, reviewing, em_revisao, revisao, testing, verifying, verificando |
   | `concluido` | done, completed, complete, concluido, finished, finalizado, achieved, alcancado, success, sucesso |
   | `bloqueado` | blocked, bloqueado, waiting, aguardando, on_hold, paused, pausado, stalled, parado |
   | `abandonado` | abandoned, abandonado, cancelled, canceled, cancelado, dropped, descartado, failed, falhou, rejected, rejeitado |

   **Desconhecido → `proposto`**, com o estado original no motivo do evento
   de criação (`abiyss goal show ID`). Atenção: `archived` e `closed` são
   tratados como desconhecidos de propósito (não dá para saber se o goal foi
   concluído ou abandonado) e, como `proposto`, podem entrar em foco no
   heartbeat — o relatório avisa quantos são.
4. O goal **nasce no estado de origem** (evento de criação com autor
   `importacao`), sem passar pela máquina de estados.
5. Prioridade: número (arredondado) ou `alta`/`high`/`urgente` = 2,
   `média`/`medium`/`normal` = 1, `baixa`/`low` = 0. Outra coisa: 0, com o
   original em `extras`.
6. Sem critério de pronto: núcleo = descrição (até 300 caracteres) ou o
   título. Sem título: `(goal do Hermes sem título: ID)`.
7. `history` e qualquer outro campo vão para `extras` (preservados). **O
   histórico não vira eventos de goal**: só o estado atual é usado.

### `metacognition/diario-pessoal.md` → `01_internal/diario/`

1. Markdown escrito pelo próprio Abiyss. As entradas começam em títulos
   (`#`, `##`...) que contêm uma data `AAAA-MM-DD` ou `DD/MM/AAAA` em
   qualquer lugar do título (`## 28/09/2026 (noite)`).
2. Uma nota por dia (`2026-09-28.md`); dois títulos do mesmo dia vão para a
   mesma nota, na ordem. Títulos sem data continuam no dia em que estão.
3. O texto antes do primeiro título com data (ou o arquivo todo, se não
   houver datas) vai para `diario-pessoal-hermes.md`.
4. `tipo: dito` (é escrita em primeira pessoa, registrada como tal);
   `criado` = o dia da entrada.
5. Se já existir uma nota com o mesmo nome que não veio da importação, a
   importada é gravada como `<dia>-hermes.md`.

### Identidade → `01_internal/identidade/` e `identity/nucleo.proposto.md`

1. `auto-modelo.json` é um objeto JSON com a visão do Abiyss sobre si. A
   nota é uma **cópia fiel** (texto original num bloco de código) com
   `tipo: deduzido` (são inferências dele sobre si mesmo).
2. Campos usados no rascunho do núcleo: `nome`/`name`;
   `proposito`/`purpose`/`mission`/`missao`; `valores`/`values`;
   `tracos`/`traits`/`personalidade`; `capacidades`/`strengths`/`skills`;
   `limitacoes`/`weaknesses`/`limitations`;
   `estilo_comunicacao`/`communication_style`/`tom`/`tone`/`voz`/`voice`.
   Os demais aparecem no fim do rascunho ("Campos do auto-modelo que não
   entraram acima"). Listas viram itens; texto vira um item.
3. `SOUL.md` é Markdown livre escrito pelo usuário para definir o Abiyss:
   nota com `tipo: dito`, cópia inteira. No rascunho, vai inteiro como
   citação. Linhas que mencionam "Hermes" são sinalizadas (o Abiyss nunca se
   apresenta como Hermes).
4. O rascunho começa com os fatos de origem que **não vêm do Hermes**, mas
   desta migração: o Abiyss é do gênero masculino, foi criado pela DepthAI e
   nasceu em 2026-09-27 (os mesmos de `identity/nucleo.md`).
5. O rascunho não tem data de geração: as mesmas entradas geram o mesmo
   texto (por isso rodar de novo não muda nada).

### `memories/MEMORY.md` e `memories/USER.md` → memória central

1. Formato do Hermes Agent: entradas separadas por uma linha só com `§`.
   Se não houver nenhuma linha assim, qualquer `§` separa.
2. Ordem: todas as de `MEMORY.md`, depois as de `USER.md`. Cada entrada
   entra na memória central enquanto couber em
   `[memoria] limite_central_caracteres` (padrão 4000; o Hermes usa por
   padrão ~2200 + ~1375). As que não couberem vão inteiras para
   `01_internal/memoria/hermes-memory.md` (de `MEMORY.md`) ou
   `01_internal/pessoas/usuario-hermes.md` (de `USER.md`), com
   `fonte: importacao`. Nada é cortado.
3. Todas entram como `[deduzido]` / `tipo: deduzido`: o Hermes não diz se a
   entrada foi afirmada pelo usuário ou deduzida pelo agente. Revise e
   promova a `[dito]` o que foi dito de verdade.
4. Entrada com texto igual a uma que já está na memória central (ou na nota
   de excedente) não entra de novo.

### Segredos

1. **Nome:** o nome de cada arquivo e pasta é quebrado em palavras; se
   alguma for `env`, `auth`, `oauth`, `token(s)`, `secret(s)`, `segredo(s)`,
   `credential(s)`, `credencial`, `credenciais`, `password(s)`, `passwd`,
   `senha(s)`, `apikey(s)`, `key(s)`, `cookie(s)`, `private`, `privkey`,
   `rsa`, `ed25519`, `ecdsa`, `ssh`, `gnupg`, `gpg`, `pem`, `p12`, `pfx`,
   `jks`, `kdbx`, `keystore`, `keychain`, `htpasswd`, `netrc`, `npmrc` ou
   `pypirc`, o item é pulado sem ser aberto (pastas nem são percorridas).
   Ex.: `.env`, `auth.json`, `credentials.json`, `id_rsa`, `api_key.txt`,
   `secrets/`, `.ssh/`. Falsos positivos (ex.: `token-usage.json`) só
   aparecem como pulados.
2. **Conteúdo:** texto com cara de chave (`nvapi-`, `sk-`, `sk_live_`,
   `rk_live_`, `ghp_`, `gho_`, `ghs_`, `github_pat_`, `glpat-`, `xoxb-`,
   `xoxp-`, `hf_`, `AKIA`, `AIza` seguidos de pelo menos 16 caracteres, ou
   `-----BEGIN ... PRIVATE KEY-----`) não é importado; o relatório diz onde
   estava, nunca o valor.

## Na VM

1. Faça backup: `cp -a ~/.hermes ~/hermes-backup-$(date +%F)` e da pasta
   `data/` do projeto (se já existir).
2. **Simulação** (não grava nada):

   ```bash
   ./target/release/abiyss importar-hermes --origem ~/.hermes | tee importacao-simulada.txt
   ```

   Confira no relatório:
   - **Pulados por parecerem segredos**: `.env` e `auth.json` (e tudo que
     for sensível) estão lá?
   - **Não mapeados**: algum arquivo que DEVERIA ser importado está aqui?
     (Se sim, o caminho real é diferente do suposto: me avise.)
   - **mapeamento de campos**: os nomes de origem batem com os dados reais?
   - **campos desconhecidos**: algum é importante a ponto de merecer uma
     coluna própria?
   - **ERROs** (linhas inválidas do journal, JSON quebrado em goals).
   - **Goals com estado desconhecido** (viram `proposto`).
   - **Memória central**: uso em caracteres e quantas entradas iriam para o
     excedente. Ajuste `limite_central_caracteres` antes de aplicar, se
     quiser.
3. **Aplicar**: `./target/release/abiyss importar-hermes --origem ~/.hermes --aplicar`
4. **Rodar de novo** com `--aplicar`: tem de dar `Total: 0 novo(s)`.
5. Revisar:
   - `abiyss goal list --todos` e `abiyss goal show ID` (motivo de criação
     com o estado original);
   - `abiyss diario --limite 50` (sinais, risco, confiança, extras);
   - `abiyss memoria central` (promova a `[dito]` o que foi dito);
   - o cofre no Obsidian (`01_internal/diario/`, `01_internal/identidade/`);
   - `identity/nucleo.proposto.md` → copie à mão para `identity/nucleo.md`
     o que quiser manter (o kernel só lê o `nucleo.md`).
6. Se algo der errado: `abiyss memoria esquecer CAMINHO` remove notas; o
   banco pode voltar do backup de `data/`.
