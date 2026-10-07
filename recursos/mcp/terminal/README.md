# terminal

Servidor MCP que dá ao Abiyss um shell **dentro de uma caixa de areia**
(bubblewrap). Uma ferramenta: `terminal__executar`.

| Parâmetro | O quê |
|---|---|
| `comando` | linha do bash (`bash -c`): pipes, `&&`, heredoc |
| `timeout_segundos` | opcional; padrão `TERMINAL_TIMEOUT_PADRAO`, teto `TERMINAL_TIMEOUT_MAXIMO` (conta a espera por vaga) |
| `pasta` | opcional; subpasta do workspace onde o comando começa |

A resposta é texto: o comando, o código de saída, a duração, avisos
(interrompido, timeout reduzido, dica de limite) e o stdout e o stderr,
cada um cortado em `TERMINAL_MAX_SAIDA_BYTES` (começo e fim, com a marca
`[… saída cortada pelo terminal …]`). Os mesmos campos vêm em
`structuredContent`. Código de saída diferente de zero **não** é erro da
ferramenta; erro é pedido inválido (pasta fora do workspace, comando vazio)
ou terminal ocupado.

## A caixa

Cada comando roda num `bwrap` novo:

- **Visível**: `/workspace` (o workspace do Abiyss, gravável, a pasta
  atual), `/usr` e `/etc` só leitura (e `/bin`, `/lib`... como links ou só
  leitura), `/proc` e `/dev` próprios, `/tmp` vazio (tmpfs de
  `TERMINAL_TMP_MB`, também a `HOME`).
- **Nunca montado**: o resto do projeto (kernel/, identity/, data/,
  skills/, recursos/, .env, .git, abiyss.toml, o cofre), a pasta pessoal,
  /run, /sys. Ao subir, o servidor confere com as mesmas áreas protegidas
  do kernel (`Config::areas_protegidas`) que nenhuma montagem contém ou
  fica dentro delas, e recusa subir se o workspace for inválido.
- **Rede**: desligada (`--unshare-all`: só o loopback da própria caixa).
  `TERMINAL_REDE = "sim"` compartilha a rede da máquina, **inclusive os
  serviços locais** (qmd, SearXNG): use só se precisar.
- **Privilégios**: namespace de usuário novo, `--cap-drop ALL`,
  no_new_privs (setuid não eleva), `--disable-userns` (bwrap ≥ 0.8),
  `--new-session` (sem acesso ao terminal de controle). O bwrap não pode
  ser setuid e o servidor não roda como root.
- **Nada sobrevive**: `--die-with-parent` e namespace de PID próprio.
  Quando o comando acaba (ou é interrompido, ou o servidor morre), todos
  os processos da caixa morrem, inclusive os deixados em segundo plano.
- **Ambiente**: montado do zero (PATH, HOME=/tmp, LANG=C.UTF-8, TERM=dumb,
  PAGER=cat...). Nenhuma variável do servidor vaza (e o kernel já não
  repassa as chaves do NIM aos servidores).

## Limites e por que cabem nos do kernel

O kernel supervisiona o grupo de processos do servidor (uv + Python + os
comandos) e o mata se uma chamada passar de `timeout_segundos` do item,
ou se a árvore passar de `[mcp] max_memoria_mb`. Ele também corta o
resultado em `[mcp] max_bytes_resultado`. Por isso, ao subir, o servidor
lê esses valores **do próprio abiyss.toml** e recusa a configuração que não
couber:

| Regra | Padrão |
|---|---|
| `TERMINAL_TIMEOUT_MAXIMO` + 5 s ≤ `timeout_segundos` do item | 110 + 5 ≤ 120 |
| `TERMINAL_MAX_CONCORRENTES` × `TERMINAL_MEMORIA_MB` + 128 MiB (o próprio servidor) ≤ `max_memoria_mb` | 2 × 160 + 128 = 448 ≤ 512 |
| 2 × `TERMINAL_MAX_SAIDA_BYTES` + 8 KiB ≤ `max_bytes_resultado` | 72 KB ≤ 1 MB |

Por comando:

- **Tempo**: o comando é morto no prazo (a espera por vaga conta no prazo).
- **Memória**: cada processo tem `RLIMIT_AS` = `TERMINAL_MEMORIA_MB` (limite
  duro: nenhum processo passa disso), e um vigia mede a memória residente
  da árvore inteira da caixa a cada 0,2 s e mata a caixa que passar do mesmo
  valor (um comando com vários processos).
- **Processos**: `RLIMIT_NPROC` = `TERMINAL_MAX_PROCESSOS` (processos +
  threads), contado só dentro da caixa (Linux ≥ 5.14: o limite vale por
  namespace de usuário). O vigia também confere. Isso protege o
  `TasksMax=256` do `abiyss.service`, que vale para o daemon inteiro.
- **Arquivo**: `RLIMIT_FSIZE` = `TERMINAL_MAX_ARQUIVO_MB`; sem core dump.
- **Saída**: `TERMINAL_MAX_SAIDA_BYTES` por fluxo na resposta; acima de
  64 MiB lidos o comando é interrompido (um `yes` não fica girando até o
  prazo).
- **Concorrência**: `TERMINAL_MAX_CONCORRENTES` comandos; os outros
  esperam vaga dentro do próprio prazo.

## Configuração

Na tabela `env` do item `terminal` em `[[mcp.servidores]]` (valores sempre
entre aspas). O servidor também lê essa tabela quando é rodado à mão.

| Variável | Padrão | O quê |
|---|---|---|
| `TERMINAL_TIMEOUT_PADRAO` | 30 | segundos, quando o modelo não pede |
| `TERMINAL_TIMEOUT_MAXIMO` | `timeout_segundos` − 5 | teto do que o modelo pode pedir |
| `TERMINAL_MAX_SAIDA_BYTES` | 32000 | por fluxo (stdout, stderr) |
| `TERMINAL_MAX_CONCORRENTES` | 2 | comandos ao mesmo tempo |
| `TERMINAL_MEMORIA_MB` | 160 | por comando |
| `TERMINAL_MAX_PROCESSOS` | 64 | processos + threads por comando |
| `TERMINAL_MAX_ARQUIVO_MB` | 1024 | maior arquivo que um comando escreve |
| `TERMINAL_TMP_MB` | 64 | tamanho do /tmp (tmpfs, em RAM) |
| `TERMINAL_REDE` | nao | `sim` compartilha a rede da máquina |
| `TERMINAL_RESERVA_SERVIDOR_MB` | 128 | memória do uv + Python (medido: ~105 MiB parado) |
| `TERMINAL_PASTA_TEMPORARIA` | `$TMPDIR` ou /tmp | só com bwrap < 0.9 (sem `--size`): o /tmp vira uma pasta em disco |
| `TERMINAL_BWRAP` | `bwrap` do PATH | caminho do bwrap |
| `ABIYSS_CONFIG` | o abiyss.toml acima desta pasta | outro arquivo de configuração |

## Instalação na VM (Ubuntu, ARM ou x86)

```bash
sudo apt update && sudo apt install -y bubblewrap   # util-linux (prlimit) já vem no Ubuntu

# Como o usuário do daemon (ubuntu), conferir que a caixa sobe:
bwrap --unshare-all --die-with-parent --ro-bind /usr /usr --symlink usr/bin /bin \
      --symlink usr/lib /lib --symlink usr/lib64 /lib64 --proc /proc --dev /dev -- /bin/echo ok
```

Se aparecer `setting up uid map: Permission denied` (ou `No permissions to
create new namespace`), o Ubuntu 24.04 está bloqueando namespaces de
usuário pelo AppArmor. Libere só para o bwrap:

```bash
sudo tee /etc/apparmor.d/bwrap >/dev/null <<'EOF'
abi <abi/4.0>,
include <tunables/global>
profile bwrap /usr/bin/bwrap flags=(unconfined) {
  userns,
  include if exists <local/bwrap>
}
EOF
sudo apparmor_parser -r /etc/apparmor.d/bwrap
```

(Alternativa para a máquina toda, menos restrita:
`sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`, e a mesma
linha em `/etc/sysctl.d/60-abiyss-userns.conf`.)

Depois, baixe as dependências antes de reiniciar o daemon (o primeiro
`uv run` baixa pacotes e conta no `timeout_inicio_segundos`):

```bash
cd recursos/mcp/terminal && uv sync --frozen --no-dev
sudo systemctl restart abiyss
journalctl -u abiyss | grep terminal    # "caixa pronta" ou "NÃO vou subir: <motivo>"
abiyss ferramentas | grep terminal
```

## Testes

```bash
cd recursos/mcp/terminal
uv run pytest
```

Rode como usuário comum: como root, os testes da caixa de verdade são
pulados (o servidor recusa root) e só os de configuração, montagem dos
argumentos, corte de saída e recusa rodam.

## Limites conhecidos

- O limite de memória da árvore é medido a cada 0,2 s: um comando com
  muitos processos pode passar dele por um instante antes de ser morto.
  Cada processo, sozinho, nunca passa (`RLIMIT_AS`). Limite exato para a
  árvore exigiria um cgroup por comando (`Delegate=yes` na unit do
  systemd).
- `RLIMIT_AS` conta memória virtual: programas que reservam muito espaço
  de endereços (node, java) não sobem com o limite padrão.
- O /tmp é tmpfs: até `TERMINAL_TMP_MB` por comando em RAM, que o kernel
  não vê como memória do servidor (o cgroup do serviço vê).
- Sem filtro seccomp.
