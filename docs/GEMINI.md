# Gemini Integration Notes

## API boundary

ABIYSS uses Google's GenAI Python SDK through the Gemini **Interactions API**. The current integration is deliberately manual: the model proposes a function call, ABIYSS turns that proposal into an Aquery, QQ schedules it, the local tool executes, and the result is returned to Gemini.

```text
Gemini interaction
      |
      | function_call(id, name, arguments)
      v
ABIYSS validation
      |
      v
Aquery -> QuPs -> QQ -> AST -> tool
                           |
                           v
                     Linux system
                           |
                           v
function_result(name, call_id, result)
      |
      v
previous_interaction_id -> next Gemini interaction
```

The model never receives local authority simply because it requested a tool. Tool authorization is local and remains in the registry/tool plane.

## Stateful interactions

The agent loop uses `previous_interaction_id` so Gemini can retain the conversation state on the provider side. The current Interactions API documentation says `previous_interaction_id` carries the history, while `tools`, `system_instruction` and `generation_config` are scoped to each interaction and therefore must be sent again on subsequent requests.

The provider's default state is stateful. Google documents that interactions are stored by default (`store=true`) and gives different retention periods by tier. This is a data-boundary decision, not merely a convenience setting. Future ABIYSS versions should expose an explicit retention/privacy policy.

## Function calling contract

A Gemini `function_call` step contains an identifier, function name and arguments. ABIYSS validates the identifier, decodes JSON arguments when necessary, rejects non-object or non-finite arguments, limits payload size, and rejects duplicate call IDs in the same interaction.

When a tool result is returned, ABIYSS uses:

- `type = "function_result"`;
- the original function `name`;
- `call_id = function_call.id`;
- a JSON text result;
- `previous_interaction_id = interaction.id`.

This mirrors the current Google function-calling contract.

## Thinking

`gemini-3.8-flash` is the v0.1 default. ABIYSS makes reasoning configuration explicit through `generation_config` and `thinking_level`, currently accepting `low`, `medium` or `high`. Google documents that `minimal` is not supported by Gemini 3.8 Flash.

The default is `medium`, chosen as the v0.1 balance between agentic reasoning quality and latency/cost. This is a configuration choice, not an architectural dependency.

## Structured output

Sleep Key Alignment can call Gemini for semantic grouping using JSON structured output. The returned value is always treated as untrusted data and revalidated locally. Unknown keys, duplicate membership and malformed group structures are discarded rather than trusted.

Because semantic alignment does not require conversation continuity, production deployments should prefer `store=false` for that auxiliary interaction. The v0.1 provider keeps the mechanism isolated so this policy can be made explicit without changing QQ or Memory.

## API key

The SDK may obtain the key from the `GEMINI_API_KEY` environment variable. Secrets must never be committed to the repository, written into examples, or copied into audit records.

## Live integration status

The v0.1 automated suite uses a deterministic provider and mocked SDK objects. No live Gemini request was made in the reconstruction environment because network access and an API credential were not guaranteed. Therefore “Gemini integration tested” means the adapter contract is covered, not that network transport was exercised end-to-end.

## References reviewed on 2026-09-08

- https://ai.google.dev/gemini-api/docs/interactions-overview
- https://ai.google.dev/gemini-api/docs/function-calling
- https://ai.google.dev/gemini-api/docs/latest-model
- https://ai.google.dev/gemini-api/docs/thinking
- https://ai.google.dev/gemini-api/docs/thought-signatures
- https://pypi.org/project/google-genai/2.22.0/
