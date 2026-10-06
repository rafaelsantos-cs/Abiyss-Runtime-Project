using System.Collections.Generic;
using System.Linq;
using Godot;
using OgonNoKaze.Core;

namespace OgonNoKaze.Settings;

/// <summary>
/// Remapeamento de controles: troca eventos no InputMap e converte eventos de/para um
/// texto curto e legível no settings.cfg (ex.: "key:87", "joy_axis:1:-").
/// Cada ação tem no máximo um evento de teclado/mouse e um de controle.
/// </summary>
public static class InputBindings
{
    public enum Device { KeyboardMouse, Gamepad }

    public static Device DeviceOf(InputEvent e) =>
        e is InputEventJoypadButton or InputEventJoypadMotion ? Device.Gamepad : Device.KeyboardMouse;

    public static InputEvent GetBinding(StringName action, Device device) =>
        InputMap.ActionGetEvents(action).FirstOrDefault(e => DeviceOf(e) == device);

    /// <summary>Substitui o evento do dispositivo correspondente, mantendo o do outro.</summary>
    public static void SetBinding(StringName action, InputEvent newEvent)
    {
        var device = DeviceOf(newEvent);
        foreach (var e in InputMap.ActionGetEvents(action).Where(e => DeviceOf(e) == device).ToList())
            InputMap.ActionEraseEvent(action, e);
        InputMap.ActionAddEvent(action, newEvent);
    }

    /// <summary>Volta todas as ações para o que está definido em project.godot.</summary>
    public static void ResetToDefaults() => InputMap.LoadFromProjectSettings();

    public static void SaveTo(ConfigFile cfg, string section)
    {
        if (cfg.HasSection(section))
            cfg.EraseSection(section);
        foreach (var r in InputActions.RemappableActions)
        {
            var codes = InputMap.ActionGetEvents(r.Action).Select(Encode).Where(c => c != null).ToArray();
            cfg.SetValue(section, r.Action, codes);
        }
    }

    public static void LoadFrom(ConfigFile cfg, string section)
    {
        if (!cfg.HasSection(section))
            return;
        foreach (var r in InputActions.RemappableActions)
        {
            if (!cfg.HasSectionKey(section, r.Action) || !InputMap.HasAction(r.Action))
                continue;
            var events = cfg.GetValue(section, r.Action).AsStringArray().Select(Decode).Where(e => e != null).ToList();
            if (events.Count == 0)
                continue; // arquivo corrompido: mantém o padrão em vez de deixar a ação sem tecla
            InputMap.ActionEraseEvents(r.Action);
            foreach (var e in events)
                InputMap.ActionAddEvent(r.Action, e);
        }
    }

    public static string Encode(InputEvent e) => e switch
    {
        InputEventKey k when k.PhysicalKeycode != Key.None => $"key:{(long)k.PhysicalKeycode}",
        InputEventKey k => $"keycode:{(long)k.Keycode}",
        InputEventMouseButton m => $"mouse:{(int)m.ButtonIndex}",
        InputEventJoypadButton b => $"joy_button:{(int)b.ButtonIndex}",
        InputEventJoypadMotion a => $"joy_axis:{(int)a.Axis}:{(a.AxisValue < 0 ? "-" : "+")}",
        _ => null,
    };

    public static InputEvent Decode(string code)
    {
        var p = code.Split(':');
        if (p.Length < 2 || !long.TryParse(p[1], out var n))
            return null;
        return p[0] switch
        {
            "key" => new InputEventKey { PhysicalKeycode = (Key)n },
            "keycode" => new InputEventKey { Keycode = (Key)n },
            "mouse" => new InputEventMouseButton { ButtonIndex = (MouseButton)n },
            "joy_button" => new InputEventJoypadButton { ButtonIndex = (JoyButton)n },
            "joy_axis" when p.Length == 3 => new InputEventJoypadMotion { Axis = (JoyAxis)n, AxisValue = p[2] == "-" ? -1f : 1f },
            _ => null,
        };
    }

    /// <summary>Nome curto e legível do evento, em português, para a interface.</summary>
    public static string Describe(InputEvent e)
    {
        switch (e)
        {
            case null:
                return "—";
            case InputEventKey k:
                // Tecla física → rótulo no layout do teclado do jogador (ABNT2, AZERTY...).
                // Sem janela (headless) não há layout: usa o código físico.
                var key = k.PhysicalKeycode == Key.None ? k.Keycode
                    : DisplayServer.GetName() == "headless" ? k.PhysicalKeycode
                    : DisplayServer.KeyboardGetKeycodeFromPhysical(k.PhysicalKeycode);
                return KeyNames.TryGetValue(key, out var kn) ? kn : OS.GetKeycodeString(key);
            case InputEventMouseButton m:
                return MouseNames.TryGetValue(m.ButtonIndex, out var mn) ? mn : $"Mouse {(int)m.ButtonIndex}";
            case InputEventJoypadButton b:
                return JoyButtonNames.TryGetValue(b.ButtonIndex, out var bn) ? bn : $"Botão {(int)b.ButtonIndex}";
            case InputEventJoypadMotion a:
                var neg = a.AxisValue < 0;
                return a.Axis switch
                {
                    JoyAxis.LeftX => neg ? "Analógico esq. ←" : "Analógico esq. →",
                    JoyAxis.LeftY => neg ? "Analógico esq. ↑" : "Analógico esq. ↓",
                    JoyAxis.RightX => neg ? "Analógico dir. ←" : "Analógico dir. →",
                    JoyAxis.RightY => neg ? "Analógico dir. ↑" : "Analógico dir. ↓",
                    JoyAxis.TriggerLeft => "LT / L2",
                    JoyAxis.TriggerRight => "RT / R2",
                    _ => $"Eixo {(int)a.Axis}{(neg ? "-" : "+")}",
                };
            default:
                return e.AsText();
        }
    }

    private static readonly Dictionary<Key, string> KeyNames = new()
    {
        { Key.Space, "Espaço" }, { Key.Shift, "Shift" }, { Key.Ctrl, "Ctrl" }, { Key.Alt, "Alt" },
        { Key.Escape, "Esc" }, { Key.Tab, "Tab" }, { Key.Enter, "Enter" }, { Key.Backspace, "Backspace" },
        { Key.Up, "Seta ↑" }, { Key.Down, "Seta ↓" }, { Key.Left, "Seta ←" }, { Key.Right, "Seta →" },
    };

    private static readonly Dictionary<MouseButton, string> MouseNames = new()
    {
        { MouseButton.Left, "Mouse esquerdo" }, { MouseButton.Right, "Mouse direito" },
        { MouseButton.Middle, "Botão do meio" }, { MouseButton.Xbutton1, "Mouse 4" },
        { MouseButton.Xbutton2, "Mouse 5" }, { MouseButton.WheelUp, "Roda ↑" },
        { MouseButton.WheelDown, "Roda ↓" },
    };

    // Nomes neutros com as duas convenções mais comuns (Xbox / PlayStation).
    private static readonly Dictionary<JoyButton, string> JoyButtonNames = new()
    {
        { JoyButton.A, "A / ×" }, { JoyButton.B, "B / ○" }, { JoyButton.X, "X / □" }, { JoyButton.Y, "Y / △" },
        { JoyButton.LeftShoulder, "LB / L1" }, { JoyButton.RightShoulder, "RB / R1" },
        { JoyButton.LeftStick, "L3" }, { JoyButton.RightStick, "R3" },
        { JoyButton.Back, "Select" }, { JoyButton.Start, "Start" }, { JoyButton.Guide, "Guia" },
        { JoyButton.DpadUp, "Direcional ↑" }, { JoyButton.DpadDown, "Direcional ↓" },
        { JoyButton.DpadLeft, "Direcional ←" }, { JoyButton.DpadRight, "Direcional →" },
    };
}
