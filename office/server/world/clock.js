// Relógio do escritório.
//
// Dois tempos diferentes:
// - `simMs`: tempo de simulação acumulado pelos ticks (determinístico; é o
//   que temporizadores de comportamento usam);
// - `wallMs()`: instante "do mundo" (epoch ms) usado para hora do dia, sol,
//   clima e fase do ritmo. No modo normal é a hora real; no demo pode começar
//   em outro instante (`--clock`) e andar mais rápido (`--time-scale`).

const formatters = new Map();

function formatterFor(timeZone) {
  let f = formatters.get(timeZone);
  if (!f) {
    f = new Intl.DateTimeFormat('en-GB', {
      timeZone, hourCycle: 'h23', year: 'numeric', month: '2-digit', day: '2-digit',
      hour: '2-digit', minute: '2-digit', second: '2-digit', weekday: 'short',
    });
    formatters.set(timeZone, f);
  }
  return f;
}

/** Partes da data/hora local num fuso (IANA). */
export function localParts(ms, timeZone) {
  const parts = {};
  for (const p of formatterFor(timeZone).formatToParts(new Date(ms))) parts[p.type] = p.value;
  const hour = Number(parts.hour);
  const minute = Number(parts.minute);
  const second = Number(parts.second);
  return {
    year: Number(parts.year), month: Number(parts.month), day: Number(parts.day),
    hour, minute, second, weekday: parts.weekday,
    minuteOfDay: hour * 60 + minute,
    hours: hour + minute / 60 + second / 3600,
    iso: `${parts.year}-${parts.month}-${parts.day}T${parts.hour}:${parts.minute}:${parts.second}`,
    hhmm: `${parts.hour}:${parts.minute}`,
  };
}

export class SimClock {
  /**
   * @param {{startMs?: number|null, timeScale?: number, realNow?: () => number}} opts
   */
  constructor({ startMs = null, timeScale = 1, realNow = Date.now } = {}) {
    this.realNow = realNow;
    this.timeScale = timeScale;
    this.baseWallMs = startMs ?? realNow();
    this.simMs = 0;
    /** Relógio real (true) ou deslocado/acelerado (false). */
    this.isReal = startMs === null && timeScale === 1;
  }

  advance(dtMs) {
    this.simMs += dtMs;
  }

  /** Instante do mundo (epoch ms). */
  wallMs() {
    if (this.isReal) return this.realNow();
    return this.baseWallMs + this.simMs;
  }
}
