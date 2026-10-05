// Ambiente: hora local, sol e clima de Contagem (MG) → estado de iluminação.
//
// O ambiente é parte do estado AUTORITATIVO do servidor: todos os clientes
// veem o mesmo céu, a mesma chuva e as mesmas lâmpadas acesas.

import { solarPosition, sunTimes } from './sun.js';
import { WeatherService } from './weather.js';
import { computeLighting } from '../../shared/lighting.js';
import { localParts } from '../world/clock.js';

export class Environment {
  /**
   * @param {object} opts
   * @param {{name:string, latitude:number, longitude:number, timezone:string}} opts.location
   * @param {object} opts.weatherConfig
   * @param {typeof fetch} [opts.fetchImpl]
   */
  constructor({ location, weatherConfig, fetchImpl }) {
    this.location = location;
    this.weather = new WeatherService({ location, config: weatherConfig, fetchImpl });
    this.cache = { key: -1, value: null };
  }

  start() {
    this.weather.start();
  }

  stop() {
    this.weather.stop();
  }

  /** Estado do ambiente no instante (recalculado no máximo 1×/s). */
  current(ms) {
    const key = Math.floor(ms / 1000);
    if (this.cache.key === key) return this.cache.value;
    const { latitude, longitude, timezone, name } = this.location;
    const sun = solarPosition(ms, latitude, longitude);
    const times = sunTimes(ms, latitude, longitude);
    const w = this.weather.current(ms);
    const light = computeLighting(sun.elevation, w);
    const lp = localParts(ms, timezone);
    const hhmm = (t) => (t === null ? null : localParts(t, timezone).hhmm);
    const value = {
      location: { name, latitude, longitude, timezone },
      local: { hhmm: lp.hhmm, hours: Math.round(lp.hours * 1000) / 1000, date: `${lp.day}/${lp.month}/${lp.year}` },
      sun: {
        elevation: Math.round(sun.elevation * 100) / 100,
        azimuth: Math.round(sun.azimuth * 100) / 100,
        sunrise: hhmm(times.sunriseMs),
        sunset: hhmm(times.sunsetMs),
        isDay: sun.elevation > -0.833,
      },
      weather: w,
      light,
    };
    this.cache = { key, value };
    return value;
  }

  status() {
    return this.weather.status();
  }
}
