// Protocolo servidor → cliente (Server-Sent Events em /api/events).
//
// Eventos:
//   hello  { protocol, layoutVersion, server, source }      — uma vez, ao conectar
//   tick   { tick, simMs, wallMs, clock, entities, fx }      — broadcastHz (padrão 10/s)
//   meta   { runtime, journal, env, stats, source, weather } — 1/s
//
// O cliente nunca envia estado: ele só observa. Comandos de depuração da
// demonstração usam POST /api/demo/* e só existem com a fonte "demo".

export const PROTOCOL_VERSION = 1;

/** Separa a visão do mundo nos dois tipos de quadro. */
export function splitView(view) {
  const tick = {
    tick: view.tick,
    simMs: view.simMs,
    wallMs: view.wallMs,
    clock: view.clock,
    entities: view.entities,
    fx: view.fx,
  };
  const meta = {
    tick: view.tick,
    runtime: view.runtime,
    journal: view.journal,
    env: view.env,
    stats: view.stats,
  };
  return { tick, meta };
}
