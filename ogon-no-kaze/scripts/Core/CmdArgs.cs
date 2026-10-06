using System.Collections.Generic;
using Godot;

namespace OgonNoKaze.Core;

/// <summary>
/// Argumentos de linha de comando do usuário (tudo depois de "--" na chamada do Godot),
/// no formato --chave=valor ou --flag. Chaves repetidas acumulam valores (ex.: vários --set).
/// </summary>
public sealed class CmdArgs
{
    private readonly Dictionary<string, List<string>> _values = new();

    public static CmdArgs FromOs() => new(OS.GetCmdlineUserArgs());

    public CmdArgs(IEnumerable<string> args)
    {
        foreach (var raw in args)
        {
            if (!raw.StartsWith("--"))
                continue;
            var body = raw[2..];
            var eq = body.IndexOf('=');
            var key = eq < 0 ? body : body[..eq];
            var value = eq < 0 ? "true" : body[(eq + 1)..];
            if (!_values.TryGetValue(key, out var list))
                _values[key] = list = new List<string>();
            list.Add(value);
        }
    }

    public bool Has(string key) => _values.ContainsKey(key);

    public string Get(string key, string fallback = null) =>
        _values.TryGetValue(key, out var list) ? list[^1] : fallback;

    public IReadOnlyList<string> GetAll(string key) =>
        _values.TryGetValue(key, out var list) ? list : new List<string>();

    public int GetInt(string key, int fallback) =>
        int.TryParse(Get(key), out var v) ? v : fallback;

    public float GetFloat(string key, float fallback) =>
        float.TryParse(Get(key), System.Globalization.NumberStyles.Float,
            System.Globalization.CultureInfo.InvariantCulture, out var v) ? v : fallback;

    /// <summary>Lê "1920x1080".</summary>
    public Vector2I? GetSize(string key)
    {
        var parts = Get(key)?.Split('x');
        if (parts is { Length: 2 } && int.TryParse(parts[0], out var w) && int.TryParse(parts[1], out var h))
            return new Vector2I(w, h);
        return null;
    }
}
