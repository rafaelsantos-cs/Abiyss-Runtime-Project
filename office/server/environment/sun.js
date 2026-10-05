// Posição do sol (algoritmo da NOAA — "Solar Calculator", com as equações
// de Jean Meeus). Precisão de cerca de 1 minuto para nascer/pôr do sol,
// mais que suficiente para iluminação.
//
// Azimute: graus a partir do NORTE, sentido horário (leste = 90°).

const RAD = Math.PI / 180;
const DEG = 180 / Math.PI;

function julianCentury(ms) {
  const jd = ms / 86400000 + 2440587.5;
  return (jd - 2451545) / 36525;
}

function solarParams(jc) {
  const L0 = (280.46646 + jc * (36000.76983 + jc * 0.0003032)) % 360;
  const M = 357.52911 + jc * (35999.05029 - 0.0001537 * jc);
  const e = 0.016708634 - jc * (0.000042037 + 0.0000001267 * jc);
  const C = Math.sin(M * RAD) * (1.914602 - jc * (0.004817 + 0.000014 * jc))
    + Math.sin(2 * M * RAD) * (0.019993 - 0.000101 * jc)
    + Math.sin(3 * M * RAD) * 0.000289;
  const trueLong = L0 + C;
  const omega = 125.04 - 1934.136 * jc;
  const appLong = trueLong - 0.00569 - 0.00478 * Math.sin(omega * RAD);
  const meanObliq = 23 + (26 + (21.448 - jc * (46.815 + jc * (0.00059 - jc * 0.001813))) / 60) / 60;
  const obliq = meanObliq + 0.00256 * Math.cos(omega * RAD);
  const decl = Math.asin(Math.sin(obliq * RAD) * Math.sin(appLong * RAD)) * DEG;
  const y = Math.tan((obliq / 2) * RAD) ** 2;
  const eqTime = 4 * DEG * (y * Math.sin(2 * L0 * RAD) - 2 * e * Math.sin(M * RAD)
    + 4 * e * y * Math.sin(M * RAD) * Math.cos(2 * L0 * RAD)
    - 0.5 * y * y * Math.sin(4 * L0 * RAD) - 1.25 * e * e * Math.sin(2 * M * RAD));
  return { decl, eqTime };
}

/** Elevação e azimute do sol (graus) num instante. */
export function solarPosition(ms, lat, lon) {
  const { decl, eqTime } = solarParams(julianCentury(ms));
  const d = new Date(ms);
  const utcMin = d.getUTCHours() * 60 + d.getUTCMinutes() + d.getUTCSeconds() / 60;
  let tst = (utcMin + eqTime + 4 * lon) % 1440;
  if (tst < 0) tst += 1440;
  let ha = tst / 4 < 0 ? tst / 4 + 180 : tst / 4 - 180;
  const cosZ = Math.sin(lat * RAD) * Math.sin(decl * RAD) + Math.cos(lat * RAD) * Math.cos(decl * RAD) * Math.cos(ha * RAD);
  const zenith = Math.acos(Math.min(1, Math.max(-1, cosZ))) * DEG;
  let elevation = 90 - zenith;
  // Refração atmosférica (aproximação da NOAA).
  if (elevation > -0.575 && elevation <= 85) {
    const te = Math.tan(elevation * RAD);
    let r;
    if (elevation > 5) r = 58.1 / te - 0.07 / te ** 3 + 0.000086 / te ** 5;
    else r = 1735 + elevation * (-518.2 + elevation * (103.4 + elevation * (-12.79 + elevation * 0.711)));
    elevation += r / 3600;
  } else if (elevation <= -0.575) {
    elevation += (-20.774 / Math.tan(elevation * RAD)) / 3600;
  }
  const den = Math.cos(lat * RAD) * Math.sin(zenith * RAD);
  let azimuth;
  if (Math.abs(den) < 1e-9) azimuth = 0;
  else {
    const a = Math.acos(Math.min(1, Math.max(-1, (Math.sin(lat * RAD) * Math.cos(zenith * RAD) - Math.sin(decl * RAD)) / den))) * DEG;
    azimuth = ha > 0 ? (a + 180) % 360 : (540 - a) % 360;
  }
  return { elevation, azimuth, declination: decl };
}

/**
 * Nascer, meio-dia solar e pôr do sol (epoch ms) para o dia UTC que contém `ms`
 * (para o Brasil, UTC−3, é o mesmo dia local durante o período claro).
 */
export function sunTimes(ms, lat, lon) {
  const d = new Date(ms);
  const dayStart = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
  const { decl, eqTime } = solarParams(julianCentury(dayStart + 12 * 3600000));
  const cosHa = Math.cos(90.833 * RAD) / (Math.cos(lat * RAD) * Math.cos(decl * RAD)) - Math.tan(lat * RAD) * Math.tan(decl * RAD);
  const noonMin = 720 - 4 * lon - eqTime;
  if (cosHa > 1 || cosHa < -1) return { sunriseMs: null, sunsetMs: null, noonMs: dayStart + noonMin * 60000 };
  const ha = Math.acos(cosHa) * DEG;
  return {
    sunriseMs: dayStart + (noonMin - ha * 4) * 60000,
    noonMs: dayStart + noonMin * 60000,
    sunsetMs: dayStart + (noonMin + ha * 4) * 60000,
  };
}
