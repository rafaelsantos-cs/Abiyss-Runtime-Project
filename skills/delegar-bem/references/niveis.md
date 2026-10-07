# Níveis de sub-agente

| Nível | Modelo | Bom para | Mãos | Custo |
|---|---|---|---|---|
| low | Nemotron 120b | extrair, formatar, resumir texto curto | web, ambiente (só lê) | baixo |
| medium | GLM 5.3 Flash | analisar, comparar, escrever rascunhos | terminal, ambiente (sem web) | médio |
| ultra | Kimi K3 | raciocínio difícil, planos longos | web, ambiente (sem terminal) | alto |

Web e terminal nunca no mesmo nível: pesquisa vai para `low` ou `ultra`,
comandos para `medium` (veja a skill `usar-as-maos`).

Comece pelo nível mais baixo que resolve; suba só se o relatório voltar
`parcial` por falta de capacidade (não por falta de contexto).
