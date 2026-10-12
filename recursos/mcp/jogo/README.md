# jogo — Travessia, o jogo do benchmark

Servidor MCP do **Travessia**, o quebra-cabeça em grade do benchmark do
Abiyss, mais o gerador de fases e o solucionador. Desenho completo, métricas e
braços do teste em [`docs/TRAVESSIA.md`](../../../docs/TRAVESSIA.md); formato do
replay em [`docs/REPLAY.md`](../../../docs/REPLAY.md).

O servidor **não** fica no `abiyss.toml` principal: o Abiyss de verdade nunca
vê o jogo. Quem o sobe é o harness (`abiyss travessia rodar`), numa instância
isolada, com o arquivo da execução em `JOGO_EXECUCAO`.

## Arquivos

| Arquivo | O quê |
|---|---|
| `motor.py` | As regras (única fonte da verdade): estado imutável, ações, placas e portas, chaves, portão, névoa |
| `texto.py` | O que o modelo vê: linha de status, mapa com coordenadas, lista de objetos, janela por avatar |
| `regras.py` | O texto de `regras()`: só mecânica, sem estratégia e sem o limite de jogadas |
| `sessao.py` | Uma execução: fases em sequência, versão do estado, contadores, limites, relógio, modos, replay e métricas |
| `servidor.py` | As sete ferramentas MCP (`regras`, `ver`, `simular`, `executar`, `reiniciar_fase`, `desistir_fase`, `proxima_fase`) |
| `catalogo.py` | Formato canônico de uma fase (um JSON por fase) e o sha256 |
| `desenho.py` / `planta.py` | Fases a partir de desenhos (testes) e a planta dos moldes, com as 8 simetrias |
| `moldes.py` | Os 12 moldes (Lago … Travessia) |
| `gerador.py` | Seed → catálogo + gabarito, com os testes de aceite |
| `solucionador.py` | O mínimo de jogadas (A* admissível sobre o estado conjunto) e as variantes dos testes de aceite |
| `validar_replay.py` | Valida um replay (refaz cada fase no motor) e imprime um resumo |

O servidor só **carrega** fases prontas e nunca lê o gabarito; o gerador e o
solucionador rodam fora do jogo.

## Gerar um catálogo

```bash
uv run gerador.py --seed travessia-1 \
  --catalogo ../../../data/travessia/catalogo/travessia-1 \
  --gabarito ../../../data/travessia/gabarito/travessia-1
```

Imprime o sha256 de cada fase. A mesma seed com a mesma `VERSAO_GERADOR`
produz os mesmos bytes. Mudou um molde ou o solucionador de um jeito que muda
as decisões? Suba `VERSAO_GERADOR` e registre os hashes novos em
`tests/test_gerador.py`.

- A **rejeição** de uma tentativa usa só o orçamento de **estados**
  (`--max-estados`, padrão 5 milhões), que não depende da máquina.
- Estourar o orçamento de **tempo** (`--max-segundos`, padrão 60 por busca)
  é **erro** de geração, não rejeição: uma rejeição por tempo faria a mesma
  seed dar catálogos diferentes em máquinas diferentes.

Tempos medidos neste ambiente (x86_64, uma CPU por busca): fases 1–9 em menos
de 1 s; 10 em até ~2 s; 11 e 12 em 5–30 s por fase (a busca principal; o
resto dos testes de aceite sai quase de graça pelos relaxamentos).

## Jogar à mão

```bash
cat > /tmp/execucao.json <<EOF
{"formato": 1, "id": "manual", "braco": "A", "modo": "avaliacao",
 "catalogo": "$PWD/../../../data/travessia/catalogo/travessia-1",
 "fases": [1, 2, 3], "max_jogadas": {"1": 200, "2": 200, "3": 300}}
EOF
JOGO_EXECUCAO=/tmp/execucao.json uv run servidor.py   # fala MCP por stdio
```

## Testes

```bash
uv run pytest                          # ~10 s
JOGO_TESTES_LENTOS=1 uv run pytest     # + a campanha inteira (minutos)
```

- `test_motor.py`: cada regra, o exemplo da especificação desenhado igual e
  com mínimo 15;
- `test_solucionador.py`: motor e solucionador concordam passo a passo
  (sucessores iguais em passeios aleatórios por todas as mecânicas);
- `test_gerador.py`: determinismo (hashes registrados), catálogo ×
  gabarito, armadilha, entregar poupa, e A* = busca em largura nas fases
  pequenas (a heurística é admissível);
- `test_sessao.py`, `test_servidor.py`, `test_replay.py`: limites, versão,
  névoa, relógio, modos, as sete ferramentas pelo cliente MCP (leituras em
  paralelo), o processo de verdade por stdio e o validador do replay.
