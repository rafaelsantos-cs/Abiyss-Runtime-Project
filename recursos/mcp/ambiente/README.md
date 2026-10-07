# ambiente

Consciência da máquina, **só leitura**. Nada aqui muda o sistema, nada usa
sudo, e o servidor recusa rodar como root.

| Ferramenta | O quê | De onde vem |
|---|---|---|
| `ambiente__resumo()` | tudo abaixo numa chamada (5 processos) | — |
| `ambiente__servicos()` | estado de cada serviço da lista: ativo ou não, desde quando, PID, memória, reinícios, último resultado | `systemctl show` |
| `ambiente__diario(servico, linhas=50, prioridade="")` | linhas mais recentes do diário de UM serviço da lista | `journalctl --unit` |
| `ambiente__disco()` | uso de cada disco de verdade (marca o do projeto) | `statvfs` + `/proc/self/mounts` |
| `ambiente__processos(quantos=10)` | memória, carga, tempo ligado e os processos que mais usam memória | `/proc` |
| `ambiente__portas()` | portas TCP em escuta e UDP abertas, alcance (só local / todas as interfaces) e o processo dono quando visível | `/proc/net` |

- Só as unidades de `AMBIENTE_SERVICOS` podem ser vistas (no estado e no
  diário). Os nomes são conferidos ao subir; a linha de comando é montada
  aqui, sem shell.
- Do systemd, só `systemctl show` e `journalctl` (leitura). Os testes
  conferem que nenhum verbo que muda o sistema (start, stop, restart,
  enable, --vacuum, --rotate...) é usado.
- Linhas de comando de processos e linhas do diário passam por um filtro
  que troca por `***` senhas e tokens óbvios (`--password=`, `token=`,
  `usuario:senha@host`...).
- O diário guarda as linhas **mais novas** quando passa de
  `AMBIENTE_MAX_BYTES` (com aviso de quantas foram cortadas).
- Sem root, o dono de uma porta só aparece se o processo for do mesmo
  usuário; as outras dizem "processo não visível sem root".

## Configuração

Na tabela `env` do item `ambiente` em `[[mcp.servidores]]`:

| Variável | Padrão | O quê |
|---|---|---|
| `AMBIENTE_SERVICOS` | abiyss.service | unidades visíveis (vírgula ou espaço; sem sufixo = `.service`) |
| `AMBIENTE_MAX_LINHAS_DIARIO` | 100 | teto de linhas por chamada (até 500) |
| `AMBIENTE_TIMEOUT_COMANDO` | 10 | segundos para systemctl/journalctl (+ 5 s ≤ `timeout_segundos`) |
| `AMBIENTE_MAX_BYTES` | 30000 | por resposta (+ 8 KiB ≤ `[mcp] max_bytes_resultado`) |
| `AMBIENTE_SYSTEMCTL` / `AMBIENTE_JOURNALCTL` | do PATH | caminhos dos comandos |

## Na VM

Nada para instalar além das dependências (`uv sync --frozen --no-dev` nesta
pasta). Para o diário, o usuário do daemon precisa ler o journal do
sistema, o que exige o grupo `adm` ou `systemd-journal`. O usuário `ubuntu`
criado pelo cloud-init costuma já vir no `adm`. Para conferir e, se faltar,
corrigir (é o dono quem faz):

```bash
id ubuntu                                     # aparece adm ou systemd-journal?
sudo usermod -aG systemd-journal ubuntu && sudo systemctl restart abiyss
```

Sem o grupo, `ambiente__diario` responde dizendo exatamente isso.

## Testes

```bash
cd recursos/mcp/ambiente
uv run pytest
```

`systemctl` e `journalctl` são de mentira nos testes (guardam a linha de
comando recebida); disco, memória, processos e portas são lidos da máquina
de teste de verdade (com um socket aberto pelo próprio teste). Como root, o
teste que sobe o servidor pelo stdio é pulado (o servidor recusa root).
