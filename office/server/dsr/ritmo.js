// Porte fiel de kernel/src/ritmo.rs (runtime do Abiyss).
//
// O runtime NÃO grava a fase do dia no banco: ele a calcula por código a
// partir de `[ritmo] horas_ativas` e do relógio local. O escritório repete
// exatamente a mesma regra (os testes reproduzem os casos do kernel) para
// mostrar a mesma fase que o daemon está usando.
//
// - vigília: dentro das horas ativas;
// - descanso: fora delas;
// - sono: a consolidação noturna está rodando (quem sabe disso é o daemon;
//   aqui ela é detectada pelos sinais no banco — ver runtime-sqlite.js).

export const MINUTOS_NO_DIA = 24 * 60;

export class RitmoError extends Error {}

/** "HH:MM" → minutos desde a meia-noite. */
export function minutoDeTexto(texto) {
  const partes = String(texto).trim().split(':');
  if (partes.length !== 2) throw new RitmoError(`horário '${texto}' fora do formato HH:MM`);
  const [hs, ms] = partes.map((p) => p.trim());
  if (!/^\d+$/.test(hs)) throw new RitmoError(`hora inválida em '${texto}'`);
  if (!/^\d+$/.test(ms)) throw new RitmoError(`minuto inválido em '${texto}'`);
  const h = Number(hs);
  const m = Number(ms);
  if (h > 23 || m > 59) throw new RitmoError(`horário '${texto}' fora do relógio`);
  return h * 60 + m;
}

/** Janela "HH:MM-HH:MM" (pode cruzar a meia-noite; início == fim = dia inteiro). */
export class Janela {
  constructor(inicio, fim) {
    this.inicio = inicio;
    this.fim = fim;
  }

  static deTexto(texto) {
    const i = String(texto).indexOf('-');
    if (i < 0) throw new RitmoError(`use HH:MM-HH:MM, recebi '${texto}'`);
    return new Janela(minutoDeTexto(texto.slice(0, i)), minutoDeTexto(texto.slice(i + 1)));
  }

  contem(minuto) {
    const m = minuto % MINUTOS_NO_DIA;
    if (this.inicio === this.fim) return true;
    if (this.inicio < this.fim) return m >= this.inicio && m < this.fim;
    return m >= this.inicio || m < this.fim;
  }
}

export const FASE = Object.freeze({ VIGILIA: 'vigília', DESCANSO: 'descanso', SONO: 'sono' });

/** Fase pelo relógio (o sono, quem sabe é o daemon). */
export function fasePeloRelogio(horasAtivas, minutoDoDia) {
  return Janela.deTexto(horasAtivas).contem(minutoDoDia) ? FASE.VIGILIA : FASE.DESCANSO;
}
