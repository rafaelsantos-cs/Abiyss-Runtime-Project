# Recursos do Abiyss

Esta pasta guarda os **recursos** do Abiyss: servidores MCP em Python que o
kernel sobe como processos filhos (stdio) e cujas ferramentas ficam
disponíveis para o modelo.

É a parte que o Abiyss **poderá** editar no futuro (o kernel em `kernel/`
nunca). Por enquanto, nenhuma ferramenta do Abiyss escreve aqui: o workspace
dele é recusado se contiver `recursos/`.

Cada servidor é um projeto `uv` independente:

```
recursos/mcp/<nome>/
  pyproject.toml   # dependências (inclui o SDK oficial `mcp`)
  uv.lock          # versões travadas (gerado por `uv lock`)
  servidor.py      # o servidor MCP
```

e é registrado no `abiyss.toml`:

```toml
[[mcp.servidores]]
nome = "exemplo"
comando = "uv"
args = ["run", "--frozen", "--quiet", "servidor.py"]
diretorio = "recursos/mcp/exemplo"
```

As ferramentas aparecem para o modelo como `<nome>__<ferramenta>`
(ex.: `exemplo__contar_palavras`).

Os servidores recebem um ambiente LIMPO: as chaves do NIM não são repassadas.
