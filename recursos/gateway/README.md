# gateway (Discord)

Adaptador do Discord do Abiyss: um processo separado do daemon. Ele leva as
mensagens do Discord ao daemon e entrega o que o daemon manda. **Não decide
nada sobre confiança**: só relata fatos (IDs, DM ou canal, bot ou não,
menção, reply). Quem decide o nível de quem fala (dono, pessoa conhecida ou
canal, resto) é o kernel (`kernel/src/gateway/confianca.rs`). O adaptador
só ouve DMs e os canais que o kernel lista no `ola`, e só manda DM ao dono,
a pessoas conhecidas ou a quem mandou DM ao bot na última hora.

```
Discord ⇄ adaptador.py (discord.py) ⇄ socket Unix local ⇄ daemon (kernel)
```

| Arquivo | O quê |
|---|---|
| `adaptador.py` | ponto de entrada: liga as duas pontas |
| `ponte.py` | socket do daemon: reconexão com espera, mensagens guardadas até o kernel confirmar, entregas em ordem e sem duplicar |
| `discordio.py` | discord.py: fatos de cada mensagem, envio/edição, busca do que chegou com o adaptador fora do ar |
| `saida.py` | entregas no Discord (destino, ritmo) |
| `partes.py` | divide textos no limite de 2000 caracteres do Discord |
| `espera.py` | espera exponencial com teto e sorteio |
| `config.py` | socket (do `abiyss.toml`) e token (só da variável de ambiente) |

Rodar (com o daemon no ar e `[gateway] ativo = true`):

```bash
cd recursos/gateway
uv run --frozen --quiet --no-dev adaptador.py
```

Testes (Discord e daemon de mentira, sem internet):

```bash
uv run --frozen pytest -q
```

Passo a passo (criar o bot, intents, token, unit do systemd): `docs/GATEWAY.md`.
