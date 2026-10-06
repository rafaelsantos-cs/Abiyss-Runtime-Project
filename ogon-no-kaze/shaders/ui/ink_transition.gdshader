// Transição de tela em "mancha de tinta": em vez de um fade uniforme, a tinta avança
// sobre a tela seguindo um ruído, como nanquim se espalhando no papel.
//
// Como funciona: cada pixel tem um "limiar" g (ruído misturado com um gradiente
// diagonal, para a tinta vir de um canto). O pixel fica coberto quando o progresso
// passa do seu limiar. Perto da borda a tinta é mais rala (cor de lavagem), no
// centro é preta — é o aspecto típico de sumi-e.
shader_type canvas_item;

// 0 = tela limpa, 1 = tela coberta.
uniform float progress : hint_range(0.0, 1.0) = 0.0;
uniform vec4 ink_color : source_color = vec4(0.055, 0.047, 0.043, 1.0);
// Cor da tinta rala nas bordas da mancha.
uniform vec4 wash_color : source_color = vec4(0.16, 0.13, 0.11, 1.0);
uniform sampler2D noise_tex : repeat_enable, filter_linear;
// Largura (em unidades de limiar) da borda difusa.
uniform float edge_width : hint_range(0.01, 0.4) = 0.12;
// Quanto o gradiente diagonal pesa contra o ruído (0 = só ruído).
uniform float sweep_amount : hint_range(0.0, 1.0) = 0.45;
uniform float noise_scale = 1.4;

void fragment() {
	float aspect = 1.0 / max(SCREEN_PIXEL_SIZE.x / SCREEN_PIXEL_SIZE.y, 0.0001);
	vec2 nuv = UV * vec2(aspect, 1.0) * noise_scale;
	float n = texture(noise_tex, nuv).r;
	// Gradiente diagonal: a tinta entra pelo canto superior esquerdo.
	float sweep = clamp(UV.x * 0.6 + UV.y * 0.4, 0.0, 1.0);
	float g = mix(n, sweep, sweep_amount);

	// Reescala o progresso para que 0 e 1 cubram mesmo a tela inteira, bordas incluídas.
	float t = progress * (1.0 + 2.0 * edge_width) - edge_width;
	float alpha = smoothstep(g - edge_width, g, t);
	float density = smoothstep(g - edge_width * 0.5, g + edge_width * 1.5, t);

	COLOR = vec4(mix(wash_color.rgb, ink_color.rgb, density), alpha * ink_color.a);
}
