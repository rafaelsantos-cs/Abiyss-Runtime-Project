// Modelo de iluminação: elevação do sol + clima → parâmetros de luz.
//
// Calculado no servidor (estado autoritativo do ambiente) e aplicado pelo
// cliente. Funções puras: fáceis de testar e iguais nos dois lados.

const clamp01 = (v) => Math.min(1, Math.max(0, v));

export function smoothstep(a, b, x) {
  const t = clamp01((x - a) / (b - a));
  return t * t * (3 - 2 * t);
}

function mixColor(c1, c2, t) {
  const r = ((c1 >> 16) & 255) + (((c2 >> 16) & 255) - ((c1 >> 16) & 255)) * t;
  const g = ((c1 >> 8) & 255) + (((c2 >> 8) & 255) - ((c1 >> 8) & 255)) * t;
  const b = (c1 & 255) + ((c2 & 255) - (c1 & 255)) * t;
  return (Math.round(r) << 16) | (Math.round(g) << 8) | Math.round(b);
}

const SKY = {
  nightTop: 0x070b1d, nightHorizon: 0x18213d,
  dayTop: 0x3f86d6, dayHorizon: 0xbfe2ff,
  greyTop: 0x7d8896, greyHorizon: 0xc3c9d0,
  duskTop: 0x3c4a86, duskHorizon: 0xff9b5c,
  stormTop: 0x3e4651, stormHorizon: 0x77808b,
};

/**
 * @param {number} elevation elevação do sol em graus
 * @param {{cloudCover:number, rain:number, thunder:boolean, fog:number}} weather
 */
export function computeLighting(elevation, weather) {
  const clouds = clamp01((weather?.cloudCover ?? 30) / 100);
  const rain = clamp01(weather?.rain ?? 0);
  const storm = weather?.thunder ? 1 : 0;
  const fog = clamp01(weather?.fog ?? 0);

  const day = smoothstep(-7, 10, elevation); // 0 noite → 1 dia
  const lowSun = elevation > -6 ? 1 - smoothstep(3, 22, elevation) : 0; // luz dourada
  const grey = clamp01(clouds * 0.85 + rain * 0.5 + fog * 0.6);

  let top = mixColor(SKY.nightTop, SKY.dayTop, day);
  let horizon = mixColor(SKY.nightHorizon, SKY.dayHorizon, day);
  const dusk = lowSun * day * (1 - grey * 0.8);
  top = mixColor(top, SKY.duskTop, dusk * 0.5);
  horizon = mixColor(horizon, SKY.duskHorizon, dusk);
  top = mixColor(top, mixColor(SKY.nightTop, SKY.greyTop, day), grey * 0.85);
  horizon = mixColor(horizon, mixColor(SKY.nightHorizon, SKY.greyHorizon, day), grey * 0.85);
  if (storm) {
    top = mixColor(top, mixColor(SKY.nightTop, SKY.stormTop, day), 0.7);
    horizon = mixColor(horizon, mixColor(SKY.nightHorizon, SKY.stormHorizon, day), 0.7);
  }

  const sunVisible = smoothstep(-1.5, 6, elevation);
  const sunIntensity = 2.4 * sunVisible * (1 - 0.82 * clouds) * (1 - 0.6 * rain) * (1 - 0.7 * fog);
  const sunColor = mixColor(0xfff3df, 0xffa560, lowSun);
  const ambient = 0.22 + 0.85 * day * (1 - 0.2 * clouds);
  const ambientSky = mixColor(0x2a3560, mixColor(0xcfe4ff, 0xd4d8de, grey), day);
  const ambientGround = mixColor(0x14161d, 0x6b6050, day);
  const lampsOn = elevation < 7 || clouds > 0.9 || rain > 0.6 || storm === 1;
  const lampLevel = lampsOn ? (elevation < 0 ? 1 : 0.65) : 0;
  const phase = elevation < -6 ? 'noite' : elevation < 0 ? 'crepúsculo' : elevation < 15 ? 'sol baixo' : 'dia';

  return {
    phase,
    day: Math.round(day * 1000) / 1000,
    skyTop: top,
    skyHorizon: horizon,
    sunIntensity: Math.round(sunIntensity * 1000) / 1000,
    sunColor,
    ambient: Math.round(ambient * 1000) / 1000,
    ambientSky,
    ambientGround,
    lampsOn,
    lampLevel,
    fogDensity: Math.round((0.004 + fog * 0.05 + rain * 0.012) * 10000) / 10000,
    fogColor: horizon,
  };
}
