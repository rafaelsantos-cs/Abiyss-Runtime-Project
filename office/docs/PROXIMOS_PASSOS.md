# Próximos passos: integração com o runtime do Abiyss

O escritório já funciona como frontend do runtime **sem nenhuma mudança no
kernel**: lê `data/abiyss.db` em modo somente leitura. Estes são os passos
para torná-lo mais fiel e mais barato, em ordem sugerida.

## 1. Definir o DSR oficial (prioridade)

Hoje a camada é provisória (`server/dsr/`, formato
`abiyss-office/dsr-provisorio@1`). Quando o DSR/DSR Sync for especificado:

1. substituir `snapshot.js` pelo formato oficial (ou mapear um no outro);
2. trocar a leitura por polling (`runtime-sqlite.js`) por **eventos
   empurrados** pelo runtime, mantendo a mesma interface de fonte
   (`start()`, `poll(now)` → instantâneo, `status()`, `key`) — o resto do
   escritório não muda;
3. decidir se "IMMo" passa a ser um conceito do runtime (identidade
   persistente por vaga do executor) ou continua só no escritório.

## 2. Pequenas adições no kernel que ajudariam

Nenhuma é obrigatória; todas são leitura para o escritório.

| Adição | Por quê |
|---|---|
| gravar o **início** do ciclo do heartbeat (ex.: `estado_daemon.ciclo_em_andamento_ms`) | mostrar "pensando" durante o ciclo, não depois |
| gravar a **fase** atual em `estado_daemon` | hoje o escritório recalcula pela mesma regra; com o sono (E5) a fonte da verdade fica no daemon |
| publicar a tabela `sonos` (E5) com estados fixos | trocar a detecção provisória do sono (`origem='sono'`) pela oficial |
| expor a **vaga** do executor em `subagentes` (índice 0..max−1) | o IMMo seria literalmente a vaga, sem a heurística de atribuição |
| `abiyss status --json` | alternativa à leitura do banco para máquinas sem acesso ao arquivo |

## 3. Rodar junto do runtime na VM (aarch64)

1. Clonar o repositório do runtime e colocar `office/` na raiz (o caminho
   padrão `../data/abiyss.db` já aponta para o banco do runtime).
2. Instalar Node.js ≥ 22.13 (há builds oficiais para arm64) e `npm ci`.
3. `npm run runtime` (ou `npm start`, que detecta o banco).
4. Criar uma unit do systemd para o escritório (sem privilégios, só leitura
   em `data/`), depois do `abiyss.service`.
5. Acessar pelo túnel SSH (`ssh -L 8090:127.0.0.1:8090 vm`) — o servidor
   escuta só em `127.0.0.1` por padrão, porque os rótulos mostram textos de
   tarefas.
6. Liberar `api.open-meteo.com` na rede da VM para o clima real de Contagem.

## 4. Evoluções do escritório

- Mais vagas: gerar mesas a partir de `[subagentes] max_simultaneos` (hoje
  são 4 fixas, com transbordo).
- Clicar numa entidade para ver o sub-agente/goal completo (hoje: seguir +
  painel).
- Painel de goals, diário e interocepção (fase 10 "Frontend" do roadmap do
  runtime) aproveitando a mesma conexão SSE.
- Autenticação, se o escritório for exposto além do `localhost`.
- Unir os pivôs dos personagens para reduzir draw calls em máquinas fracas.
