// Barra de progresso desenhada como uma pincelada de tinta que avança da esquerda
// para a direita.
//
// A forma é uma faixa cuja meia-espessura varia ao longo do traço:
//   - ruído de baixa frequência: o pincel não aperta sempre igual;
//   - começo arredondado (o pincel pousa) e fim afinando (o pincel levanta);
//   - na ponta do progresso a espessura vai a zero, então ela termina em ponta
//     redonda em vez de "desbotar".
// Por cima, ruído esticado em x vira os fios do pincel seco (kasure), mais fortes
// perto das bordas do traço, onde a tinta acaba primeiro.
shader_type canvas_item;

uniform float progress : hint_range(0.0, 1.0) = 0.0;
uniform vec4 stroke_color : source_color = vec4(0.89, 0.69, 0.33, 1.0);
// Traço "fantasma" que mostra o caminho inteiro, bem fraco.
uniform vec4 track_color : source_color = vec4(0.925, 0.894, 0.831, 0.08);
uniform sampler2D noise_tex : repeat_enable, filter_linear;
// Fração da altura usada pelo traço no ponto mais grosso.
uniform float thickness : hint_range(0.1, 1.0) = 0.7;
// Comprimento da ponta que afina (em fração do comprimento total).
uniform float tip_length : hint_range(0.0, 0.3) = 0.04;
uniform float dry_brush : hint_range(0.0, 1.0) = 0.6;

float half_thickness(float x) {
	float wobble = texture(noise_tex, vec2(x * 2.0, 0.17)).r;
	float t = thickness * 0.5 * mix(0.7, 1.15, wobble);
	float press = sqrt(smoothstep(0.0, 0.03, x));       // pousa arredondado
	float lift = mix(0.45, 1.0, 1.0 - smoothstep(0.85, 1.0, x)); // afina no fim
	return t * press * lift;
}

float stroke_mask(vec2 uv, float half_t) {
	float center = 0.5 + (texture(noise_tex, vec2(uv.x * 0.7, 0.61)).r - 0.5) * 0.15;
	float d = abs(uv.y - center);
	// Contorno irregular: ruído fino desloca a borda.
	float ragged = (texture(noise_tex, uv * vec2(6.0, 3.0)).r - 0.5) * 0.12;
	float body = 1.0 - smoothstep(half_t - 0.03, half_t + 0.02, d + ragged);
	float streak = texture(noise_tex, vec2(uv.x * 0.25, uv.y * 9.0)).r;
	float dry = dry_brush * smoothstep(0.15 * half_t, half_t, d);
	return body * mix(1.0, smoothstep(0.3, 0.6, streak), dry);
}

void fragment() {
	float ht = half_thickness(UV.x);
	float track = stroke_mask(UV, ht);
	// 1 antes da ponta, cai a 0 exatamente no progresso; sqrt deixa a ponta redonda.
	float head = 1.0 - smoothstep(progress - tip_length, progress, UV.x);
	float filled = stroke_mask(UV, ht * sqrt(head)) * step(UV.x, progress) * step(0.001, progress);
	vec4 base = vec4(track_color.rgb, track_color.a * track);
	COLOR = mix(base, stroke_color, filled);
}
