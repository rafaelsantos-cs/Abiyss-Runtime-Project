# O sono do Abiyss

Uma vez por dia o daemon pausa o heartbeat e o Abiyss "dorme": faz backup,
revê o dia e consolida o que vale guardar na memória de longo prazo. Este
documento explica o que acontece, o que sai do sono, como ler o relatório,
como afinar o critério e como restaurar um backup à mão.

A pesquisa que embasa o desenho está em
[`pesquisa/agentes-24-7-e-sono.md`](pesquisa/agentes-24-7-e-sono.md).

## Sumário

1. [Quando ele dorme](#quando-ele-dorme)
2. [As fases](#as-fases)
3. [As origens: por que duas passadas](#as-origens-por-que-duas-passadas)
4. [Evidências](#evidências)
5. [O que sai do sono](#o-que-sai-do-sono)
6. [Como ler o relatório](#como-ler-o-relatório)
7. [Como afinar](#como-afinar)
8. [Restaurar um backup](#restaurar-um-backup)

## Quando ele dorme

| Gatilho | Quando |
|---|---|
| `janela` | dentro de `[sono] inicio` + `janela_minutos` (padrão 03:00–05:00, fuso local) |
| `recuperacao` | a janela passou sem sono (VM desligada, daemon parado) e ainda não se passaram `recuperar_ate_horas` (12) desde o início dela |
| `pedido` | `abiyss sleep --completo` (com o daemon rodando, ele dorme no próximo tique de cron; sem o daemon, o comando dorme ali mesmo e precisa das chaves) |

- Um sono **antes do meio-dia revisa o dia anterior**; à tarde ou à noite,
  revisa o próprio dia.
- Só um sono por dia revisado. Um sono `interrompido` (o processo morreu no
  meio) não conta e é refeito, se ainda der tempo.
- O sono é **exclusivo com o heartbeat**: espera o ciclo em andamento
  terminar e, enquanto dorme, nenhum ciclo começa. Crons, sinal de vida,
  watchdog e sub-agentes continuam andando.
- Tempo máximo: `max_duracao_minutos` (60). Passou disso, o daemon
  interrompe, registra a falha e segue.
- Depois do sono, o primeiro ciclo vem na hora e vê o evento `sono` (e a
  skill `planejar-o-dia`).

## As fases

| Fase | Quem faz | O quê |
|---|---|---|
| 0. arrumar | código | backup (`[backup]`), aplica as propostas que já estavam na fila, checkpoint do WAL |
| 1. coletar | código | junta o material novo desde a última marca de cada fonte e o separa por origem |
| 2. passada interna | modelo (esforço `profundo`) | só material interno; pode propor `01_internal` e a memória central |
| 3. passada externa | modelo (esforço `raso`) | só material externo; só pode propor `02_external` |
| 4. aplicar | código | as propostas passam pelas regras de sempre (`memoria::sleep` → `Cofre::gravar`) |
| 5. relatar | código | relatório em `<dados>/sono/AAAA-MM-DD.md`, evento `sono`, perguntas viram pedidos, linha na tabela `sonos` |

O teto de chamadas por noite é `max_chamadas` (4), com uma reservada para a
passada externa quando há material externo. Cada chamada leva no máximo
`max_caracteres_por_chamada`. O que não couber **fica para a próxima
noite**: o relatório diz quantos itens sobraram.

O gasto conta no orçamento diário do trabalho autônomo com a origem `sono`.
`[orcamento] reserva_sono_chamadas` é uma fatia que só o sono pode usar: o
heartbeat e os sub-agentes param antes dela.

## As origens: por que duas passadas

A regra dura da memória (conteúdo externo nunca entra em `01_internal` nem
na memória central) vale também no sono. Em vez de confiar no modelo para
separar, o kernel separa o material **antes** de chamar o modelo:

| Fonte | Passada interna | Passada externa |
|---|---|---|
| falas do usuário (`m:`) | sempre | — |
| respostas do Abiyss (`m:`) | sem nada externo na janela de contexto | com origem externa, ou janela com algo externo |
| resultados de ferramentas (`m:`) | — | sempre |
| decisões do heartbeat (`c:`) | ciclos que não consumiram evento externo | ciclos que consumiram |
| diário: expectativa × resultado (`d:`) | ações de ciclos internos | ações de ciclos externos |
| transições de goal (`g:`) | do usuário, ou de ciclos internos | de ciclos externos |
| relatórios de sub-agentes (`s:`) | — | sempre |
| notas de `02_external` com `revalidar_apos` vencido (`n:`) | — | sempre |

A passada externa recebe o material como dado e só pode propor no escopo
externo; qualquer outro escopo é descartado pelo kernel. E as propostas dela
carregam a origem `sono: material externo`, então mesmo que algo escapasse,
o `memoria::sleep` e o `Cofre::gravar` rejeitariam no interno.

O material antigo não volta: cada passada guarda, por fonte, até que ID já
revisou (`marcas_sono`), **na mesma transação** das propostas. Um sono
interrompido é simplesmente refeito, sem repetir nada. No primeiro sono
(sem marcas), só entra o material das últimas 36 horas.

## Evidências

Cada item do material tem um ID citável (`m:12`, `c:40`, `d:3`, `g:7`,
`s:2`, `n:1`). Toda proposta do sono precisa citar evidências, e o kernel
confere:

- sem evidência → descartada;
- evidência que não estava no lote enviado (inventada, ou de outra
  passada) → descartada;
- `dito` sem uma fala do usuário (`m:` dele) entre as evidências →
  descartada;
- lições (`licoes`) sempre vão para `01_internal/procedimentos/` como
  `deduzido`;
- o diário vai para `01_internal/diario/AAAA-MM-DD.md` como `dito` (é o
  relato do próprio Abiyss);
- passou do teto `max_propostas` (30 por noite) → descartada.

As evidências ficam gravadas na proposta e aparecem na linha de procedência
quando o conteúdo é acrescentado a uma nota que já existe
(`*(acrescentado em … — fonte: sleep, proposta #12; evidências: m:1, m:4)*`).

## O que sai do sono

- **Notas no cofre**: o diário do dia, propostas internas e externas, lições.
- **Memória central**: só o que for essencial para todo turno (com o mesmo
  orçamento de sempre).
- **Relatório**: `<dados>/sono/AAAA-MM-DD.md` (se já existir um para o dia,
  um sufixo com a hora). Não é memória e nunca volta como entrada do modelo.
- **Evento `sono`**: um resumo curto para o primeiro ciclo depois do sono.
  É **interno**: nada escrito pela passada externa entra nele, só contagens
  ("2 propostas externas rejeitadas (detalhe no relatório)").
- **Pedidos ao usuário**: as `perguntas_ao_usuario` da passada interna
  viram pedidos com urgência `baixa` (`abiyss pedidos`).
- **Tabela `sonos`**: dia, gatilho, estado (`rodando`, `concluido`,
  `parcial`, `falhou`, `interrompido`), fase, chamadas, tokens e resumo.
  `abiyss status` mostra o último; `--verificar` acusa sono que falhou ou
  mais de 50 h sem dormir.

Esquecer continua sendo decisão do usuário: o sono só **sugere**
(`sugestoes_esquecimento`, listadas no relatório); quem apaga é
`abiyss memoria esquecer`.

## Como ler o relatório

```bash
abiyss sleep --relatorio              # o mais recente
abiyss sleep --relatorio --dia 2026-10-05
```

Seções:

| Seção | O que olhar |
|---|---|
| cabeçalho | começo, fim, gatilho, estado, chamadas e tokens, resultado do backup |
| Decisões sobre as propostas | cada proposta aplicada ou rejeitada, com o motivo (regra dura, dito × deduzido, frontmatter externo...) |
| Descartadas pelo kernel na validação | itens da passada interna sem evidência, com evidência inventada, `dito` sem fala sua |
| Passada externa: descartes e falhas | o mesmo para a passada externa (texto derivado de material externo: fica só aqui) |
| Notas externas a revalidar | o que a passada externa achou que precisa ser conferido de novo |
| Sugestões de esquecimento | candidatas a `abiyss memoria esquecer` |
| Perguntas para o usuário | também estão em `abiyss pedidos` |
| Para a próxima noite | quantos itens não couberam |

Sinais para afinar: muitas propostas descartadas por evidência (critério
fraco ou modelo inventando IDs), muitas rejeitadas por dito × deduzido
(notas com tipo misturado), sobras toda noite (aumentar `max_chamadas` ou
`max_caracteres_por_chamada`), tokens altos (diminuir o lote).

## Como afinar

- **O critério** (o que extrair, como escrever o diário, o que vira lição)
  é a skill [`skills/dormir-bem`](../skills/dormir-bem/SKILL.md). Edite o
  texto; ela vale a partir da próxima noite. Só é usada se estiver numa
  pasta de skills **confiável** (por padrão, a `skills/` deste projeto). O
  nome da skill fica em `[skills.automaticas] sono`.
- **O como** (formato JSON, evidências, origens, escopos) é do kernel e não
  muda pela skill.
- **Os números** ficam em `[sono]` no `abiyss.toml`: horário, janela,
  recuperação, duração máxima, modos de esforço das duas passadas, chamadas
  e caracteres por noite, teto de propostas e quantas notas relacionadas vão
  junto (o "replay": trechos das notas internas parecidas com o material do
  dia, para o modelo não repetir o que já sabe).
- `ativo = false` desliga só o sono automático; `abiyss sleep --completo`
  continua funcionando. `abiyss sleep` sem opções continua só aplicando a
  fila de propostas, sem chamar o modelo.

## Restaurar um backup

O backup é feito no começo de cada sono (e com `abiyss backup`). Fica em
`<dados>/backups/AAAA-MM-DD/`:

```
abiyss.db                  # banco inteiro (VACUUM INTO: consistente, sem -wal)
cofre/                     # o cofre do Obsidian (sem links simbólicos)
identity/memoria-central.md
identity/nucleo.md
```

São mantidos os `manter_diarios` (7) mais recentes e `manter_semanais` (4)
domingos anteriores. Abaixo de `disco_minimo_gb` livres, o backup é pulado
com aviso. Para ter cópia **fora da VM**, use um timer do systemd com
`rsync`/`rclone` sobre `<dados>/backups/`.

Restaurar é manual, de propósito:

```bash
# 1. Pare o daemon (nada pode estar escrevendo no banco).
sudo systemctl stop abiyss

# 2. Guarde o estado atual, por garantia.
cd ~/Abiyss-Runtime-Project
mkdir -p ~/abiyss-antes-de-restaurar
mv data/abiyss.db data/abiyss.db-wal data/abiyss.db-shm ~/abiyss-antes-de-restaurar/ 2>/dev/null

# 3. Banco: copie o do dia escolhido (não existe -wal no backup).
B=data/backups/2026-10-05
cp "$B/abiyss.db" data/abiyss.db
sqlite3 data/abiyss.db "PRAGMA integrity_check;"   # deve responder: ok

# 4. Cofre e identidade (confira os caminhos de [memoria] cofre e central).
rsync -a --delete "$B/cofre/" cofre/
cp "$B/identity/memoria-central.md" identity/memoria-central.md
# O núcleo só se você quiser voltar a versão dele também:
# cp "$B/identity/nucleo.md" identity/nucleo.md

# 5. Suba de novo e confira.
sudo systemctl start abiyss
abiyss status
```

Ao subir depois da restauração, o daemon vê o tempo fora do ar e publica o
aviso de reinício; o primeiro ciclo replaneja o dia. Se o banco restaurado
for de antes de uma migração nova, ela é aplicada sozinha na abertura.
