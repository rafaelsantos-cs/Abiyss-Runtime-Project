// Clima de Contagem (MG).
//
// Fonte principal: Open-Meteo (https://open-meteo.com) — gratuita, sem
// chave, dados "current" com código de tempo da OMM (WMO 4677).
//
// Se a rede estiver bloqueada ou a API falhar, o escritório NÃO inventa que
// está chovendo de verdade: ele usa uma estimativa CLIMATOLÓGICA (média do
// mês para a região metropolitana de Belo Horizonte) e o HUD mostra
// claramente "estimado". Os valores médios abaixo são aproximações das
// Normais Climatológicas do INMET para Belo Horizonte (1991–2020); eles NÃO
// puderam ser conferidos neste ambiente (rede bloqueada) e servem apenas de
// reserva — confirme antes de usar para qualquer outra coisa.

import fs from 'node:fs';
import path from 'node:path';

import { createLogger } from '../log.js';
import { hashString, Rng } from '../world/rng.js';
import { localParts } from '../world/clock.js';

const log = createLogger('clima');

/** Códigos WMO → condição. */
export function describeWmo(code) {
  const c = Number(code);
  if (c === 0) return { condition: 'clear', label: 'céu limpo' };
  if (c === 1) return { condition: 'clear', label: 'predominantemente limpo' };
  if (c === 2) return { condition: 'cloudy', label: 'parcialmente nublado' };
  if (c === 3) return { condition: 'overcast', label: 'nublado' };
  if (c === 45 || c === 48) return { condition: 'fog', label: 'neblina' };
  if (c >= 51 && c <= 57) return { condition: 'drizzle', label: 'garoa' };
  if (c >= 61 && c <= 67) return { condition: 'rain', label: c >= 65 ? 'chuva forte' : c >= 63 ? 'chuva' : 'chuva fraca' };
  if (c >= 71 && c <= 77) return { condition: 'snow', label: 'neve' };
  if (c >= 80 && c <= 82) return { condition: 'rain', label: c === 82 ? 'pancadas fortes' : 'pancadas de chuva' };
  if (c === 85 || c === 86) return { condition: 'snow', label: 'pancadas de neve' };
  if (c >= 95) return { condition: 'storm', label: c > 95 ? 'tempestade com granizo' : 'tempestade' };
  return { condition: 'cloudy', label: `código ${c}` };
}

/** Intensidade visual da chuva (0..1) a partir do código e da precipitação. */
function rainIntensity(code, precipitationMm) {
  const c = Number(code);
  let base = 0;
  if (c >= 51 && c <= 57) base = 0.25;
  else if (c === 61 || c === 80 || c === 66) base = 0.45;
  else if (c === 63 || c === 81) base = 0.7;
  else if (c === 65 || c === 82 || c === 67) base = 0.95;
  else if (c >= 95) base = 1;
  if (precipitationMm > 0 && base === 0) base = Math.min(0.6, 0.2 + precipitationMm / 5);
  return base;
}

function normalize({ source, code, temperatureC, apparentC = null, humidity = null, cloudCover, precipitationMm = 0, windKmh = 0, windDirDeg = 0, isDay = null, observedAtMs, estimated }) {
  const d = describeWmo(code);
  return {
    source,
    estimated,
    observedAtMs,
    weatherCode: Number(code),
    condition: d.condition,
    label: d.label,
    temperatureC: Math.round(temperatureC * 10) / 10,
    apparentC: apparentC === null ? null : Math.round(apparentC * 10) / 10,
    humidity,
    cloudCover: Math.round(cloudCover),
    precipitationMm,
    windKmh: Math.round(windKmh * 10) / 10,
    windDirDeg: Math.round(windDirDeg),
    isDay,
    rain: rainIntensity(code, precipitationMm),
    thunder: Number(code) >= 95,
    fog: d.condition === 'fog' ? 1 : 0,
  };
}

// ---------------------------------------------------------------------------
// Open-Meteo
// ---------------------------------------------------------------------------

const CURRENT_FIELDS = [
  'temperature_2m', 'relative_humidity_2m', 'apparent_temperature', 'is_day', 'precipitation',
  'weather_code', 'cloud_cover', 'wind_speed_10m', 'wind_direction_10m',
].join(',');

export function openMeteoUrl({ latitude, longitude, timezone }) {
  const u = new URL('https://api.open-meteo.com/v1/forecast');
  u.searchParams.set('latitude', String(latitude));
  u.searchParams.set('longitude', String(longitude));
  u.searchParams.set('current', CURRENT_FIELDS);
  u.searchParams.set('timezone', timezone);
  return u.toString();
}

/** Converte a resposta da Open-Meteo. Lança se faltar algo essencial. */
export function parseOpenMeteo(json, nowMs = Date.now()) {
  const c = json?.current;
  if (!c || typeof c.temperature_2m !== 'number' || c.weather_code === undefined) {
    throw new Error('resposta da Open-Meteo sem os campos "current" esperados');
  }
  // `time` vem no fuso pedido, sem offset; o horário da observação é aproximado.
  const offsetS = Number(json.utc_offset_seconds ?? 0);
  const observed = c.time ? Date.parse(`${c.time}:00Z`) - offsetS * 1000 : nowMs;
  return normalize({
    source: 'open-meteo',
    estimated: false,
    code: c.weather_code,
    temperatureC: c.temperature_2m,
    apparentC: c.apparent_temperature ?? null,
    humidity: c.relative_humidity_2m ?? null,
    cloudCover: c.cloud_cover ?? 50,
    precipitationMm: c.precipitation ?? 0,
    windKmh: c.wind_speed_10m ?? 0,
    windDirDeg: c.wind_direction_10m ?? 0,
    isDay: c.is_day === undefined ? null : c.is_day === 1,
    observedAtMs: Number.isFinite(observed) ? observed : nowMs,
  });
}

// ---------------------------------------------------------------------------
// Climatologia de reserva (região de Belo Horizonte / Contagem)
// ---------------------------------------------------------------------------

/** Aproximações mensais (jan..dez). Ver o aviso no topo do arquivo. */
export const CLIMATOLOGIA_BH = Object.freeze({
  temperaturaMedia: [23.6, 24.0, 23.5, 22.4, 20.6, 19.4, 19.2, 20.3, 21.9, 22.8, 22.6, 23.1],
  amplitudeDiaria: [8.5, 9.0, 8.5, 9.0, 10.0, 10.5, 11.0, 12.0, 11.0, 9.5, 8.5, 8.0],
  diasDeChuva: [15, 11, 11, 6, 3, 1, 1, 1, 4, 8, 14, 17],
  nebulosidade: [70, 62, 62, 50, 40, 32, 28, 28, 38, 55, 70, 74],
});

/**
 * Tempo "plausível" para a hora local, determinístico (a mesma hora dá
 * sempre o mesmo resultado). Não é previsão nem observação.
 */
export function climatologyWeather(ms, timeZone, override = null) {
  const lp = localParts(ms, timeZone);
  const m = lp.month - 1;
  const C = CLIMATOLOGIA_BH;
  const daysInMonth = new Date(lp.year, lp.month, 0).getDate();
  const dayRng = new Rng(hashString(`${lp.year}-${lp.month}-${lp.day}`));
  const rainyDay = dayRng.next() < C.diasDeChuva[m] / daysInMonth;
  const stormStart = 13.5 + dayRng.next() * 4; // pancadas de verão à tarde
  const stormLen = 0.8 + dayRng.next() * 2.2;
  const dayCloud = Math.min(100, Math.max(0, C.nebulosidade[m] + (dayRng.next() - 0.5) * 40 + (rainyDay ? 25 : 0)));
  const h = lp.hours;
  const temp = C.temperaturaMedia[m] + (C.amplitudeDiaria[m] / 2) * Math.sin((2 * Math.PI * (h - 9)) / 24)
    - (rainyDay && h > stormStart ? 2.5 : 0) + (dayRng.next() - 0.5) * 2;
  let code = dayCloud < 20 ? 0 : dayCloud < 45 ? 1 : dayCloud < 75 ? 2 : 3;
  let precip = 0;
  if (rainyDay && h >= stormStart && h <= stormStart + stormLen) {
    const strong = dayRng.next();
    code = C.diasDeChuva[m] >= 10 && strong > 0.55 ? 95 : strong > 0.3 ? 81 : 80;
    precip = code === 95 ? 6 : code === 81 ? 3 : 1;
  } else if (rainyDay && h > stormStart + stormLen && h < stormStart + stormLen + 1.5) {
    code = 51;
    precip = 0.3;
  }
  if (override) {
    code = { clear: 0, cloudy: 2, overcast: 3, fog: 45, drizzle: 53, rain: 63, storm: 95 }[override];
    precip = { drizzle: 0.4, rain: 3, storm: 8 }[override] ?? 0;
  }
  const cloud = override === 'clear' ? 5 : code >= 51 ? 95 : code === 3 ? 90 : code === 45 ? 100 : dayCloud;
  return normalize({
    source: override ? 'manual' : 'climatologia',
    estimated: true,
    code,
    temperatureC: temp,
    cloudCover: cloud,
    precipitationMm: precip,
    windKmh: 6 + dayRng.next() * 10 + (code >= 80 ? 15 : 0),
    windDirDeg: 70 + dayRng.next() * 60, // predominância de leste/nordeste
    observedAtMs: ms,
  });
}

// ---------------------------------------------------------------------------
// Serviço
// ---------------------------------------------------------------------------

export class WeatherService {
  /**
   * @param {object} opts
   * @param {{latitude:number, longitude:number, timezone:string}} opts.location
   * @param {{provider:string, refreshMinutes:number, timeoutMs:number, cacheFile:string|null, override:string|null}} opts.config
   * @param {typeof fetch} [opts.fetchImpl]
   */
  constructor({ location, config, fetchImpl = globalThis.fetch }) {
    this.location = location;
    this.config = config;
    this.fetchImpl = fetchImpl;
    this.last = null; // última observação real
    this.lastError = null;
    this.fetchedAtMs = null;
    this.timer = null;
    this.failures = 0;
    this.#loadCache();
  }

  #loadCache() {
    if (!this.config.cacheFile) return;
    try {
      const data = JSON.parse(fs.readFileSync(this.config.cacheFile, 'utf8'));
      if (data && data.source === 'open-meteo' && Number.isFinite(data.observedAtMs)) this.last = data;
    } catch {
      // sem cache
    }
  }

  #saveCache() {
    if (!this.config.cacheFile || !this.last) return;
    try {
      fs.mkdirSync(path.dirname(this.config.cacheFile), { recursive: true });
      fs.writeFileSync(this.config.cacheFile, JSON.stringify(this.last));
    } catch (e) {
      log.debug('não consegui gravar o cache do clima', { erro: e.message });
    }
  }

  async refresh() {
    if (this.config.override || this.config.provider !== 'open-meteo') return;
    const url = openMeteoUrl(this.location);
    try {
      const res = await this.fetchImpl(url, { signal: AbortSignal.timeout(this.config.timeoutMs), headers: { 'user-agent': 'abiyss-office/0.1' } });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      this.last = parseOpenMeteo(await res.json());
      this.fetchedAtMs = Date.now();
      if (this.lastError || this.failures === 0) log.info('clima real atualizado (Open-Meteo)', { condicao: this.last.label, temperatura: this.last.temperatureC });
      this.lastError = null;
      this.failures = 0;
      this.#saveCache();
    } catch (e) {
      const msg = e?.cause?.message ?? e?.message ?? String(e);
      this.failures++;
      if (this.lastError !== msg) log.warn('Open-Meteo indisponível; usando estimativa climatológica', { erro: msg });
      this.lastError = msg;
    }
  }

  start() {
    if (this.config.override || this.config.provider !== 'open-meteo') return;
    this.refresh();
    this.timer = setInterval(() => this.refresh(), this.config.refreshMinutes * 60_000);
    this.timer.unref?.();
  }

  stop() {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }

  /** Tempo para o instante (real se houver observação recente; senão estimado). */
  current(ms) {
    const fresh = this.last && Math.abs(ms - this.last.observedAtMs) < 3 * 3600_000;
    if (!this.config.override && fresh) return { ...this.last, stale: Math.abs(ms - this.last.observedAtMs) > 45 * 60_000 };
    const w = climatologyWeather(ms, this.location.timezone, this.config.override);
    return { ...w, stale: false, error: this.config.override ? null : this.lastError };
  }

  status() {
    return { provider: this.config.provider, override: this.config.override, lastError: this.lastError, failures: this.failures, fetchedAtMs: this.fetchedAtMs, hasReal: !!this.last };
  }
}
