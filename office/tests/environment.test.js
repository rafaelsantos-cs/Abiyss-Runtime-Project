import { test } from 'node:test';
import assert from 'node:assert/strict';

import { solarPosition, sunTimes } from '../server/environment/sun.js';
import { describeWmo, parseOpenMeteo, climatologyWeather, WeatherService, openMeteoUrl } from '../server/environment/weather.js';
import { Environment } from '../server/environment/environment.js';
import { computeLighting } from '../shared/lighting.js';
import { localParts } from '../server/world/clock.js';
import { configureLog } from '../server/log.js';

configureLog({ level: 'silent' });

const LOC = { name: 'Contagem, MG', latitude: -19.9317, longitude: -44.0536, timezone: 'America/Sao_Paulo' };
const at = (iso) => Date.parse(iso);

test('sol em Contagem: nascer/pôr plausíveis e sol ao norte no inverno', () => {
  const t = sunTimes(at('2026-10-05T12:00:00-03:00'), LOC.latitude, LOC.longitude);
  const rise = localParts(t.sunriseMs, LOC.timezone);
  const set = localParts(t.sunsetMs, LOC.timezone);
  assert.ok(rise.hours > 5.3 && rise.hours < 6.0, `nascer ${rise.hhmm}`);
  assert.ok(set.hours > 17.6 && set.hours < 18.3, `pôr ${set.hhmm}`);
  const winterNoon = solarPosition(at('2026-06-21T11:50:00-03:00'), LOC.latitude, LOC.longitude);
  assert.ok(winterNoon.elevation > 44 && winterNoon.elevation < 49, `elevação ${winterNoon.elevation}`);
  assert.ok(winterNoon.azimuth > 340 || winterNoon.azimuth < 20, 'ao meio-dia de junho o sol fica ao norte');
  const summerNoon = solarPosition(at('2026-12-21T12:00:00-03:00'), LOC.latitude, LOC.longitude);
  assert.ok(summerNoon.elevation > 84, 'perto do zênite no solstício de verão (trópico)');
  const night = solarPosition(at('2026-10-05T23:00:00-03:00'), LOC.latitude, LOC.longitude);
  assert.ok(night.elevation < -30);
  const morning = solarPosition(at('2026-10-05T07:00:00-03:00'), LOC.latitude, LOC.longitude);
  assert.ok(morning.azimuth > 70 && morning.azimuth < 110, 'de manhã o sol está a leste');
});

test('códigos WMO viram condições legíveis', () => {
  assert.equal(describeWmo(0).condition, 'clear');
  assert.equal(describeWmo(3).condition, 'overcast');
  assert.equal(describeWmo(45).condition, 'fog');
  assert.equal(describeWmo(53).condition, 'drizzle');
  assert.equal(describeWmo(63).label, 'chuva');
  assert.equal(describeWmo(81).condition, 'rain');
  assert.equal(describeWmo(95).condition, 'storm');
});

test('parser da Open-Meteo (formato "current")', () => {
  const json = {
    utc_offset_seconds: -10800,
    current: {
      time: '2026-10-05T15:00', interval: 900, temperature_2m: 27.4, relative_humidity_2m: 48, apparent_temperature: 28.1,
      is_day: 1, precipitation: 0.8, weather_code: 80, cloud_cover: 76, wind_speed_10m: 11.2, wind_direction_10m: 95,
    },
  };
  const w = parseOpenMeteo(json);
  assert.equal(w.source, 'open-meteo');
  assert.equal(w.estimated, false);
  assert.equal(w.condition, 'rain');
  assert.equal(w.temperatureC, 27.4);
  assert.equal(w.observedAtMs, at('2026-10-05T15:00:00-03:00'));
  assert.ok(w.rain > 0.3);
  assert.throws(() => parseOpenMeteo({}));
  const url = new URL(openMeteoUrl(LOC));
  assert.equal(url.hostname, 'api.open-meteo.com');
  assert.equal(url.searchParams.get('latitude'), '-19.9317');
  assert.match(url.searchParams.get('current'), /weather_code/);
});

test('climatologia de reserva: determinística, marcada como estimada e com valores plausíveis', () => {
  const a = climatologyWeather(at('2026-10-05T15:10:00-03:00'), LOC.timezone);
  const b = climatologyWeather(at('2026-10-05T15:10:00-03:00'), LOC.timezone);
  assert.deepEqual(a, b);
  assert.equal(a.estimated, true);
  assert.equal(a.source, 'climatologia');
  for (let d = 1; d <= 28; d++) {
    for (const h of [3, 9, 15, 21]) {
      const w = climatologyWeather(at(`2026-07-${String(d).padStart(2, '0')}T${String(h).padStart(2, '0')}:00:00-03:00`), LOC.timezone);
      assert.ok(w.temperatureC > 8 && w.temperatureC < 33, `temperatura implausível ${w.temperatureC}`);
    }
  }
  // Julho é seco: quase nunca chove; dezembro chove bastante à tarde.
  const rainyHours = (month) => {
    let n = 0;
    for (let d = 1; d <= 28; d++) {
      for (let h = 12; h <= 20; h++) {
        const w = climatologyWeather(at(`2026-${month}-${String(d).padStart(2, '0')}T${String(h).padStart(2, '0')}:30:00-03:00`), LOC.timezone);
        if (w.rain > 0) n++;
      }
    }
    return n;
  };
  assert.ok(rainyHours('12') > rainyHours('07') * 3);
  assert.equal(climatologyWeather(at('2026-10-05T12:00:00-03:00'), LOC.timezone, 'storm').thunder, true);
});

test('serviço: usa a observação real quando há, e a estimativa quando a API falha', async () => {
  const ok = async () => ({ ok: true, json: async () => ({ utc_offset_seconds: -10800, current: { time: '2026-10-05T15:00', temperature_2m: 19, weather_code: 3, cloud_cover: 100 } }) });
  const svc = new WeatherService({ location: LOC, config: { provider: 'open-meteo', refreshMinutes: 10, timeoutMs: 1000, cacheFile: null, override: null }, fetchImpl: ok });
  await svc.refresh();
  const w = svc.current(at('2026-10-05T15:20:00-03:00'));
  assert.equal(w.source, 'open-meteo');
  assert.equal(w.temperatureC, 19);
  // Observação velha demais → estimativa.
  assert.equal(svc.current(at('2026-10-05T23:00:00-03:00')).source, 'climatologia');

  const fail = async () => { throw new Error('rede bloqueada'); };
  const svc2 = new WeatherService({ location: LOC, config: { provider: 'open-meteo', refreshMinutes: 10, timeoutMs: 1000, cacheFile: null, override: null }, fetchImpl: fail });
  await svc2.refresh();
  const w2 = svc2.current(at('2026-10-05T15:20:00-03:00'));
  assert.equal(w2.source, 'climatologia');
  assert.equal(w2.error, 'rede bloqueada');
});

test('iluminação: noite liga as lâmpadas, dia nublado escurece o sol, crepúsculo é quente', () => {
  const night = computeLighting(-20, { cloudCover: 10, rain: 0 });
  assert.equal(night.lampsOn, true);
  assert.equal(night.sunIntensity, 0);
  const clear = computeLighting(60, { cloudCover: 0, rain: 0 });
  const grey = computeLighting(60, { cloudCover: 100, rain: 0.7 });
  assert.equal(clear.lampsOn, false);
  assert.ok(grey.sunIntensity < clear.sunIntensity * 0.3);
  assert.ok(grey.lampsOn);
  const dusk = computeLighting(3, { cloudCover: 10, rain: 0 });
  const r = (dusk.sunColor >> 16) & 255;
  const b = dusk.sunColor & 255;
  assert.ok(r - b > 80, 'sol baixo é alaranjado');
});

test('ambiente completo com override de clima', () => {
  const env = new Environment({ location: LOC, weatherConfig: { provider: 'open-meteo', refreshMinutes: 10, timeoutMs: 1000, cacheFile: null, override: 'rain' } });
  const e = env.current(at('2026-10-05T21:30:00-03:00'));
  assert.equal(e.weather.condition, 'rain');
  assert.equal(e.weather.source, 'manual');
  assert.equal(e.light.lampsOn, true);
  assert.equal(e.sun.isDay, false);
  assert.equal(e.local.hhmm, '21:30');
  assert.equal(e.location.name, 'Contagem, MG');
});
