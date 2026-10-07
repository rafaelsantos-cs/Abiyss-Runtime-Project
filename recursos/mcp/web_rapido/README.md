# web_rapido

A camada **rápida** de pesquisa do Abiyss: nenhum modelo envolvido, só
busca, download e extração de texto. Duas ferramentas:

| Ferramenta | O quê |
|---|---|
| `web_rapido__buscar(consulta, max_resultados=8, idioma="")` | consulta o SearXNG e devolve, de cada resultado, título, URL, trecho, motores e data de publicação (se houver), com a data da busca |
| `web_rapido__ler_pagina(url, inicio=0)` | baixa a página e devolve o texto principal (sem menu, propaganda e rodapé, pelo trafilatura), com URL final, título, data de publicação e data da busca |

Texto maior que `WEB_MAX_TEXTO_BYTES` vem em partes: a resposta termina com
`[… texto cortado pelo web_rapido: faltam N caracteres. Para continuar,
chame ler_pagina com inicio=X …]`, e as partes seguintes saem do cache, sem
baixar de novo. Assim o kernel nunca precisa cortar o resultado no meio.

O kernel trata tudo o que vem daqui como conteúdo externo.

## Proteções

- **robots.txt** (RFC 9309, com os curingas `*` e `$`, que o
  `urllib.robotparser` não entende): grupo `AbiyssBot` (o primeiro nome de
  `WEB_AGENTE`), senão `*`; regra mais longa vence; empate, `Allow`. 404:
  sem regras. 401/403, erro 5xx ou fora do ar: **nada** é lido daquele
  site (por 1 h). Cada salto de redirecionamento é conferido.
- **Um pedido por site** a cada `WEB_INTERVALO_DOMINIO_SEGUNDOS`, ou o
  `Crawl-delay` do site se for maior (até 60 s). Se a espera passar do
  prazo da chamada, a ferramenta diz quando tentar de novo.
- **Tamanho**: lê até `WEB_MAX_PAGINA_MB` (já descomprimido, então gzip
  gigante não estoura); acima disso, usa o começo e avisa.
- **Rede interna bloqueada**: endereços que não são públicos (127.0.0.1,
  10/8, 192.168/16, 169.254.169.254 do metadata da Oracle Cloud, CGNAT,
  IPv6 local...) são recusados antes de conectar (pelo DNS) e depois de
  conectar (pelo endereço de verdade). As variáveis `HTTP(S)_PROXY` são
  ignoradas, para a checagem valer. O SearXNG, configurado pelo dono, é a
  exceção: ele fica na rede local.
- Só `http`/`https`, sem usuário e senha na URL, no máximo 5
  redirecionamentos; só HTML e texto (PDF e afins são recusados com aviso).
- **Prazo** por chamada (`WEB_PRAZO_SEGUNDOS`), menor que o
  `timeout_segundos` do kernel (o servidor recusa subir se não for); cada
  pedido HTTP tem `WEB_TIMEOUT_REQUISICAO`. A extração roda numa thread,
  dentro do mesmo prazo.

## Cache

SQLite em `data/web_rapido/cache.sqlite3` (fora do workspace: o Abiyss não
mexe nele). Buscas ficam `WEB_CACHE_TTL_BUSCAS_HORAS`, páginas
`WEB_CACHE_TTL_PAGINAS_HORAS`, robots.txt 24 h. Acima de `WEB_CACHE_MAX_MB`,
saem as entradas usadas há mais tempo. Busca sem resultado não é guardada
(costuma ser motor fora do ar). Arquivo corrompido é recriado.

## Configuração

Na tabela `env` do item `web_rapido` em `[[mcp.servidores]]` (valores
sempre entre aspas):

| Variável | Padrão | O quê |
|---|---|---|
| `WEB_SEARXNG_URL` | http://127.0.0.1:8888 | onde o SearXNG responde |
| `WEB_PRAZO_SEGUNDOS` | `timeout_segundos` − 8 | tempo máximo de uma chamada |
| `WEB_TIMEOUT_REQUISICAO` | 15 | cada pedido HTTP |
| `WEB_CACHE_ARQUIVO` | data/web_rapido/cache.sqlite3 | onde fica o cache |
| `WEB_CACHE_MAX_MB` | 200 | tamanho máximo do cache |
| `WEB_CACHE_TTL_PAGINAS_HORAS` | 24 | validade de uma página (0 = sem cache) |
| `WEB_CACHE_TTL_BUSCAS_HORAS` | 6 | validade de uma busca |
| `WEB_MAX_PAGINA_MB` | 2 | quanto baixar de uma página |
| `WEB_MAX_TEXTO_BYTES` | 40000 | texto por resposta (o resto em partes) |
| `WEB_INTERVALO_DOMINIO_SEGUNDOS` | 2 | entre pedidos ao mesmo site |
| `WEB_MAX_CONCORRENTES` | 2 | leituras ao mesmo tempo (memória da extração) |
| `WEB_RESERVA_SERVIDOR_MB` | 160 | memória do uv + Python (medido: ~125 MiB parado) |
| `WEB_PERMITIR_REDE_LOCAL` | nao | `sim` libera endereços internos (só para testes) |
| `WEB_AGENTE` | AbiyssBot/0.1 (...) | User-Agent; o primeiro nome vale para o robots.txt |

O servidor recusa subir se `WEB_PRAZO_SEGUNDOS` + 8 s passar do
`timeout_segundos` do item, se `WEB_MAX_TEXTO_BYTES` + 8 KiB passar de
`[mcp] max_bytes_resultado`, ou se `WEB_MAX_CONCORRENTES` ×
`WEB_MAX_PAGINA_MB` × 40 (a extração usa ~34× o tamanho do HTML) + a
reserva passar de `[mcp] max_memoria_mb`.

## SearXNG na VM (Ubuntu ARM)

Só a busca precisa dele; `ler_pagina` funciona sem. Com Docker (a imagem
oficial tem arm64), ouvindo só no localhost:

```bash
sudo apt update && sudo apt install -y docker.io
sudo mkdir -p /etc/searxng
sudo tee /etc/searxng/settings.yml >/dev/null <<EOF
use_default_settings: true
server:
  secret_key: "$(openssl rand -hex 32)"
  limiter: false        # só o Abiyss usa, pelo localhost
search:
  formats:
    - html
    - json              # o web_rapido usa o formato JSON
EOF
sudo docker run -d --name searxng --restart unless-stopped \
  -p 127.0.0.1:8888:8080 -v /etc/searxng:/etc/searxng \
  docker.io/searxng/searxng:latest

curl -s 'http://127.0.0.1:8888/search?q=teste&format=json' | head -c 300   # deve vir JSON
```

Sem `json` em `search.formats`, o SearXNG responde 403 e a ferramenta diz
isso. Depois, baixe as dependências do servidor antes de reiniciar o
daemon (o lxml vem pronto para arm64):

```bash
cd recursos/mcp/web_rapido && uv sync --frozen --no-dev
sudo systemctl restart abiyss
journalctl -u abiyss | grep web_rapido   # "pronto" (e aviso se o SearXNG não respondeu)
```

## Testes

```bash
cd recursos/mcp/web_rapido
uv run pytest
```

Nenhum teste sai para a internet: um servidor HTTP local faz o papel dos
sites (páginas, robots.txt, redirecionamentos, erro 500, página lenta, PDF)
e do SearXNG (`tests/fixtures/`).

## Limites conhecidos

- Não roda JavaScript: páginas montadas no navegador voltam sem texto
  (a ferramenta diz isso). Para elas, delegar com navegador.
- O limite por site é pelo nome do host (`www.x.com` e `x.com` contam
  separados) e vale só enquanto o servidor está no ar.
- Uma extração que passa do prazo é abandonada, mas a thread termina o
  trabalho em segundo plano (limitado pelo tamanho máximo da página).
