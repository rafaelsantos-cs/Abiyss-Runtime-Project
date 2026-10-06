using System.Collections.Generic;
using Godot;

namespace OgonNoKaze.Player;

/// <summary>
/// Buffer de input: lembra por alguns milissegundos que um botão foi apertado.
///
/// Sem buffer, apertar "pular" 3 frames antes de tocar o chão simplesmente se perde e o
/// jogo parece não responder. Com buffer, o pedido fica guardado e é executado assim
/// que for possível. O estado que executa a ação chama Consume para não repetir.
///
/// O tempo é contado em ticks de física (não em relógio), então pausar o jogo não
/// "gasta" a janela e o comportamento é idêntico com qualquer FPS de renderização.
/// </summary>
public sealed class InputBuffer
{
    private readonly Dictionary<StringName, ulong> _pressedAtTick = new();
    private readonly StringName[] _actions;

    public float WindowSeconds { get; set; }

    public InputBuffer(float windowSeconds, params StringName[] actions)
    {
        WindowSeconds = windowSeconds;
        _actions = actions;
    }

    /// <summary>Chamar uma vez por tick de física, antes da máquina de estados.</summary>
    public void Update()
    {
        var now = Engine.GetPhysicsFrames();
        foreach (var action in _actions)
        {
            if (Input.IsActionJustPressed(action))
                _pressedAtTick[action] = now;
        }
    }

    /// <summary>True se a ação foi apertada dentro da janela (sem consumir).</summary>
    public bool IsBuffered(StringName action) =>
        _pressedAtTick.TryGetValue(action, out var at)
        && Engine.GetPhysicsFrames() - at <= (ulong)Mathf.RoundToInt(WindowSeconds * Engine.PhysicsTicksPerSecond);

    /// <summary>True e limpa, se a ação está no buffer.</summary>
    public bool Consume(StringName action)
    {
        if (!IsBuffered(action))
            return false;
        _pressedAtTick.Remove(action);
        return true;
    }

    public void Clear() => _pressedAtTick.Clear();
}
