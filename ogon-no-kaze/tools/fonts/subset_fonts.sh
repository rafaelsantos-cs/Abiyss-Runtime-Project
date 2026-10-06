#!/usr/bin/env bash
# Baixa as fontes OFL do repositório do Google Fonts e recorta só os glifos usados,
# para não versionar arquivos de 8 MB. Requer: curl e fonttools (pip install fonttools).
#
# Uso (a partir de ogon-no-kaze/):  tools/fonts/subset_fonts.sh
# Se a interface passar a usar um caractere novo (ex.: outro kanji), acrescente-o em
# TITLE_TEXT ou LATIN_RANGES e rode de novo.
set -euo pipefail

OUT="assets/fonts"
BASE="https://raw.githubusercontent.com/google/fonts/main/ofl"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Kanji/kana desenhados com a fonte de pincel (título e detalhes caligráficos).
TITLE_TEXT="黄金の風"

# Latim básico + Latin-1 + Latin Extended-A (Ō), pontuação, setas e formas geométricas
# (usadas nos nomes dos botões do controle).
LATIN_RANGES="U+0020-007E,U+00A0-00FF,U+0100-017F,U+2010-2027,U+2030-203A,U+2190-2193,U+25A0-25CF"

curl -sSfL -o "$TMP/cormorant.ttf" "$BASE/cormorantgaramond/CormorantGaramond%5Bwght%5D.ttf"
curl -sSfL -o "$OUT/OFL-CormorantGaramond.txt" "$BASE/cormorantgaramond/OFL.txt"
curl -sSfL -o "$TMP/yujiboku.ttf" "$BASE/yujiboku/YujiBoku-Regular.ttf"
curl -sSfL -o "$OUT/OFL-YujiBoku.txt" "$BASE/yujiboku/OFL.txt"

# A fonte é variável (eixo wght). Geramos instâncias estáticas nos pesos usados: assim o
# Godot não precisa interpolar contornos e acentos compostos (ê, õ, Ō) saem no lugar.
for spec in "Medium:500" "SemiBold:650"; do
    name="${spec%%:*}"; weight="${spec##*:}"
    fonttools varLib.instancer "$TMP/cormorant.ttf" wght="$weight" --static \
        --output "$TMP/cormorant-$name.ttf"
    pyftsubset "$TMP/cormorant-$name.ttf" --unicodes="$LATIN_RANGES" \
        --layout-features='*' --output-file="$OUT/CormorantGaramond-$name.ttf"
done
pyftsubset "$TMP/yujiboku.ttf" --text="$TITLE_TEXT" \
    --layout-features='*' --output-file="$OUT/YujiBoku-Title.ttf"

ls -la "$OUT"
