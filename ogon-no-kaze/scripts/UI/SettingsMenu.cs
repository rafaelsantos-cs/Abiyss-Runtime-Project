using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using Godot;
using OgonNoKaze.Core;
using OgonNoKaze.Settings;

namespace OgonNoKaze.UI;

/// <summary>
/// Tela de configurações (Vídeo, Controles, Áudio). Serve tanto para o menu principal
/// quanto para o menu de pausa.
///
/// As linhas são geradas por código: cada opção é descrita uma vez (rótulo, valores,
/// como ler e como escrever no <see cref="GameSettings"/>) e os "refreshers" guardados
/// atualizam os widgets quando os dados mudam por fora (ex.: escolher um preset).
/// Mudanças valem na hora; o arquivo é gravado ao fechar a tela.
/// </summary>
public partial class SettingsMenu : Control
{
    [Signal] public delegate void ClosedEventHandler();

    [Export] public TabContainer Tabs { get; set; }
    [Export] public VBoxContainer VideoRows { get; set; }
    [Export] public VBoxContainer ControlRows { get; set; }
    [Export] public VBoxContainer AudioRows { get; set; }
    [Export] public Button BackButton { get; set; }
    [Export] public Button ResetButton { get; set; }
    /// <summary>Aba aberta ao chamar Open (0 Vídeo, 1 Controles, 2 Áudio).</summary>
    [Export] public int InitialTab { get; set; }
    /// <summary>Abre sozinho ao entrar na árvore (para rodar a cena direto ou capturar).</summary>
    [Export] public bool OpenOnReady { get; set; }
    /// <summary>Espera, em segundos, antes de aplicar mudanças gráficas pesadas (sliders).</summary>
    [Export] public float ApplyDebounceSeconds { get; set; } = 0.25f;
    [Export] public float LabelColumnWidth { get; set; } = 520f;
    [Export] public float ControlColumnWidth { get; set; } = 460f;
    /// <summary>Largura da coluna de remapeamento (dois botões: teclado/mouse e controle).</summary>
    [Export] public float BindingColumnWidth { get; set; } = 640f;
    [Export] public int BindingFontSize { get; set; } = 26;

    private static readonly CultureInfo PtBr = new("pt-BR");

    private readonly List<Action> _refreshers = new();
    private OptionButton _presetOption;
    private double _applyCountdown = -1;
    private bool _resetArmed;

    // Remapeamento em andamento: qual ação/dispositivo está esperando uma tecla.
    private StringName _listenAction;
    private InputBindings.Device _listenDevice;
    private Button _listenButton;

    private SettingsManager S => SettingsManager.Instance;

    public override void _Ready()
    {
        ProcessMode = ProcessModeEnum.Always;
        Visible = false;

        BuildVideoTab();
        BuildControlsTab();
        BuildAudioTab();

        BackButton.Pressed += Close;
        ResetButton.Pressed += OnResetPressed;
        Tabs.TabChanged += _ => CallDeferred(MethodName.FocusFirstInCurrentTab);

        if (OpenOnReady)
            Open();
    }

    public void Open()
    {
        RefreshAll();
        DisarmReset();
        Visible = true;
        Tabs.CurrentTab = Mathf.Clamp(InitialTab, 0, Tabs.GetTabCount() - 1);
        CallDeferred(MethodName.FocusFirstInCurrentTab);
    }

    public void Close()
    {
        StopListening(cancelled: true);
        FlushPendingApply();
        S.Save();
        Visible = false;
        EmitSignal(SignalName.Closed);
    }

    public override void _Process(double delta)
    {
        if (_applyCountdown < 0)
            return;
        _applyCountdown -= delta;
        if (_applyCountdown < 0)
            S.ApplyAll();
    }

    private void FlushPendingApply()
    {
        if (_applyCountdown < 0)
            return;
        _applyCountdown = -1;
        S.ApplyAll();
    }

    private void ScheduleApply() => _applyCountdown = ApplyDebounceSeconds;

    private void RefreshAll()
    {
        foreach (var refresh in _refreshers)
            refresh();
    }

    private void FocusFirstInCurrentTab()
    {
        var rows = Tabs.GetCurrentTabControl()?.FindChildren("*", "Control", true, false)
            .OfType<Control>()
            .FirstOrDefault(c => c.FocusMode == FocusModeEnum.All && c.IsVisibleInTree());
        rows?.GrabFocus();
    }

    // ------------------------------------------------------------------ entrada

    public override void _Input(InputEvent e)
    {
        if (!Visible || _listenAction == null)
            return;
        if (!e.IsPressed() || e.IsEcho())
            return;

        // Esc sempre cancela a escuta (e não pode ser atribuída a nenhuma ação).
        if (e is InputEventKey { PhysicalKeycode: Key.Escape })
        {
            StopListening(cancelled: true);
            GetViewport().SetInputAsHandled();
            return;
        }

        var device = InputBindings.DeviceOf(e);
        if (device != _listenDevice || e is InputEventMouseMotion)
            return;
        if (e is InputEventJoypadMotion m && Mathf.Abs(m.AxisValue) < 0.6f)
            return; // ignora analógico em repouso / encostado de leve

        var bound = Normalize(e);
        if (bound == null)
            return;
        AssignBinding(_listenAction, bound);
        StopListening(cancelled: false);
        GetViewport().SetInputAsHandled();
    }

    public override void _UnhandledInput(InputEvent e)
    {
        if (!Visible || _listenAction != null)
            return;
        if (e.IsActionPressed("ui_cancel"))
        {
            Close();
            GetViewport().SetInputAsHandled();
        }
        else if (e.IsActionPressed(InputActions.UiTabNext))
        {
            Tabs.CurrentTab = (Tabs.CurrentTab + 1) % Tabs.GetTabCount();
            GetViewport().SetInputAsHandled();
        }
        else if (e.IsActionPressed(InputActions.UiTabPrev))
        {
            Tabs.CurrentTab = (Tabs.CurrentTab + Tabs.GetTabCount() - 1) % Tabs.GetTabCount();
            GetViewport().SetInputAsHandled();
        }
    }

    /// <summary>Cria um evento "limpo" (sem modificadores/posição) a partir do que foi pressionado.</summary>
    private static InputEvent Normalize(InputEvent e) => e switch
    {
        InputEventKey k => new InputEventKey { PhysicalKeycode = k.PhysicalKeycode != Key.None ? k.PhysicalKeycode : k.Keycode },
        InputEventMouseButton mb => new InputEventMouseButton { ButtonIndex = mb.ButtonIndex },
        InputEventJoypadButton jb => new InputEventJoypadButton { ButtonIndex = jb.ButtonIndex },
        InputEventJoypadMotion jm => new InputEventJoypadMotion { Axis = jm.Axis, AxisValue = Mathf.Sign(jm.AxisValue) },
        _ => null,
    };

    // ------------------------------------------------------------------ aba Vídeo

    private void BuildVideoTab()
    {
        var p = VideoRows;
        var v = () => S.Current.Video;
        var q = () => S.Current.Video.Quality;

        AddHeader(p, "Tela");
        AddOption(p, "Modo de janela", new[]
        {
            ("Janela", WindowModeOption.Windowed),
            ("Tela cheia (sem bordas)", WindowModeOption.Borderless),
            ("Tela cheia exclusiva", WindowModeOption.ExclusiveFullscreen),
        }, () => v().WindowMode, x => v().WindowMode = x);

        var resolutions = AvailableResolutions();
        var resOption = AddOption(p, "Resolução da janela",
            resolutions.Select(r => ($"{r.X} × {r.Y}", r)).ToArray(),
            () => v().Resolution, x => v().Resolution = x);
        _refreshers.Add(() => resOption.Disabled = v().WindowMode != WindowModeOption.Windowed);

        AddOption(p, "Sincronização vertical (VSync)", new[]
        {
            ("Desligada", VSyncOption.Off), ("Ligada", VSyncOption.On), ("Adaptativa", VSyncOption.Adaptive),
        }, () => v().VSync, x => v().VSync = x);
        AddOption(p, "Limite de FPS", new[]
        {
            ("Sem limite", 0), ("30", 30), ("60", 60), ("90", 90), ("120", 120), ("144", 144), ("165", 165), ("240", 240),
        }, () => v().MaxFps, x => v().MaxFps = x);
        AddSlider(p, "Campo de visão", 55, 100, 1, () => v().Fov, x => v().Fov = x, x => $"{x:0}°");

        AddHeader(p, "Qualidade gráfica");
        _presetOption = AddOption(p, "Predefinição", new[]
        {
            ("Baixa", QualityPresetId.Low), ("Média", QualityPresetId.Medium), ("Alta", QualityPresetId.High),
            ("Ultra", QualityPresetId.Ultra), ("Personalizada", QualityPresetId.Custom),
        }, () => v().Preset, SelectPreset);

        AddSlider(p, "Escala de renderização 3D", 0.5f, 1f, 0.01f, () => q().RenderScale, x => q().RenderScale = x,
            x => $"{x * 100:0}%", quality: true);
        AddOption(p, "Reconstrução (upscaler)", new[]
        {
            ("Bilinear", UpscalerOption.Bilinear), ("FSR 1.0", UpscalerOption.Fsr1), ("FSR 2.2", UpscalerOption.Fsr2),
        }, () => q().Upscaler, x => q().Upscaler = x, quality: true);
        AddOption(p, "Antisserrilhado", new[]
        {
            ("Desligado", AntiAliasingOption.Off), ("FXAA", AntiAliasingOption.Fxaa),
            ("SMAA", AntiAliasingOption.Smaa), ("TAA", AntiAliasingOption.Taa),
        }, () => q().AntiAliasing, x => q().AntiAliasing = x, quality: true);
        AddOption(p, "Sombras", new[]
        {
            ("Baixa", ShadowQuality.Low), ("Média", ShadowQuality.Medium), ("Alta", ShadowQuality.High), ("Ultra", ShadowQuality.Ultra),
        }, () => q().Shadows, x => q().Shadows = x, quality: true);
        AddOption(p, "Névoa volumétrica", new[]
        {
            ("Desligada", FogQuality.Off), ("Baixa", FogQuality.Low), ("Média", FogQuality.Medium), ("Alta", FogQuality.High),
        }, () => q().VolumetricFog, x => q().VolumetricFog = x, quality: true);
        AddOption(p, "Iluminação global", new[]
        {
            ("Desligada", GlobalIlluminationOption.Off), ("SSIL", GlobalIlluminationOption.Ssil), ("SDFGI", GlobalIlluminationOption.Sdfgi),
        }, () => q().GlobalIllumination, x => q().GlobalIllumination = x, quality: true);
        AddOption(p, "Oclusão de ambiente (SSAO)", new[]
        {
            ("Desligada", SsaoQuality.Off), ("Baixa", SsaoQuality.Low), ("Média", SsaoQuality.Medium), ("Alta", SsaoQuality.High),
        }, () => q().Ssao, x => q().Ssao = x, quality: true);
        AddSlider(p, "Densidade da grama", 0.1f, 1f, 0.05f, () => q().GrassDensity, x => q().GrassDensity = x,
            x => $"{x * 100:0}%", quality: true);
        AddSlider(p, "Distância da grama", 20f, 250f, 5f, () => q().GrassDistance, x => q().GrassDistance = x,
            x => $"{x:0} m", quality: true);
    }

    private void SelectPreset(QualityPresetId id)
    {
        S.SelectPreset(id);
        RefreshAll(); // os itens de qualidade passam a mostrar os valores do preset
    }

    private static Vector2I[] AvailableResolutions()
    {
        var screen = DisplayServer.ScreenGetSize();
        var common = new[]
        {
            new Vector2I(1280, 720), new Vector2I(1366, 768), new Vector2I(1600, 900), new Vector2I(1920, 1080),
            new Vector2I(2560, 1440), new Vector2I(3840, 2160),
        };
        var list = common.Where(r => r.X <= screen.X && r.Y <= screen.Y).ToList();
        if (!list.Contains(screen) && screen.X > 0)
            list.Add(screen);
        if (list.Count == 0)
            list.Add(new Vector2I(1280, 720));
        return list.OrderBy(r => r.X * r.Y).ToArray();
    }

    // ------------------------------------------------------------------ aba Controles

    private void BuildControlsTab()
    {
        var p = ControlRows;
        var c = () => S.Current.Controls;

        AddHeader(p, "Câmera");
        AddSlider(p, "Sensibilidade do mouse", 0.02f, 0.5f, 0.01f, () => c().MouseSensitivity, x => c().MouseSensitivity = x,
            x => x.ToString("0.00", PtBr));
        AddSlider(p, "Sensibilidade do controle", 60f, 400f, 10f, () => c().GamepadSensitivity, x => c().GamepadSensitivity = x,
            x => $"{x:0}°/s");
        AddCheck(p, "Inverter eixo vertical", () => c().InvertY, x => c().InvertY = x);

        var hint = new Label
        {
            Text = "Escolha um botão e pressione a nova tecla. Esc cancela.",
            ThemeTypeVariation = "HintLabel",
            AutowrapMode = TextServer.AutowrapMode.WordSmart,
        };

        string group = null;
        foreach (var r in InputActions.RemappableActions)
        {
            if (r.Group != group)
            {
                group = r.Group;
                AddHeader(p, group);
                if (p.GetChildCount() > 0 && hint.GetParent() == null)
                    p.AddChild(hint);
            }
            AddBindingRow(p, r);
        }
    }

    private void AddBindingRow(VBoxContainer parent, InputActions.Remappable r)
    {
        var box = new HBoxContainer();
        box.AddThemeConstantOverride("separation", 12);
        foreach (var device in new[] { InputBindings.Device.KeyboardMouse, InputBindings.Device.Gamepad })
        {
            var button = new Button
            {
                SizeFlagsHorizontal = SizeFlags.ExpandFill,
                SizeFlagsStretchRatio = 1f,
                ClipText = true,
                TooltipText = device == InputBindings.Device.KeyboardMouse ? "Teclado / mouse" : "Controle",
            };
            button.AddThemeFontSizeOverride("font_size", BindingFontSize);
            var dev = device;
            button.Pressed += () => StartListening(r.Action, dev, button);
            _refreshers.Add(() => button.Text = InputBindings.Describe(InputBindings.GetBinding(r.Action, dev)));
            box.AddChild(button);
        }
        AddRow(parent, r.Label, box, BindingColumnWidth);
    }

    private void StartListening(StringName action, InputBindings.Device device, Button button)
    {
        StopListening(cancelled: true);
        _listenAction = action;
        _listenDevice = device;
        _listenButton = button;
        button.Text = device == InputBindings.Device.KeyboardMouse ? "Pressione uma tecla…" : "Pressione um botão…";
    }

    private void StopListening(bool cancelled)
    {
        if (_listenAction == null)
            return;
        var button = _listenButton;
        _listenAction = null;
        _listenButton = null;
        RefreshAll();
        if (!cancelled)
            button?.GrabFocus();
    }

    /// <summary>
    /// Atribui o evento à ação. Se outra ação já usava esse evento, as duas trocam
    /// (a outra recebe o evento antigo desta), para nenhuma ficar sem tecla.
    /// </summary>
    private void AssignBinding(StringName action, InputEvent newEvent)
    {
        var code = InputBindings.Encode(newEvent);
        var previous = InputBindings.GetBinding(action, InputBindings.DeviceOf(newEvent));
        foreach (var other in InputActions.RemappableActions.Where(o => o.Action != action))
        {
            var otherEvent = InputBindings.GetBinding(other.Action, InputBindings.DeviceOf(newEvent));
            if (otherEvent != null && InputBindings.Encode(otherEvent) == code && previous != null)
                InputBindings.SetBinding(other.Action, (InputEvent)previous.Duplicate());
        }
        InputBindings.SetBinding(action, newEvent);
    }

    // ------------------------------------------------------------------ aba Áudio

    private void BuildAudioTab()
    {
        var p = AudioRows;
        var a = () => S.Current.Audio;
        AddHeader(p, "Volume");
        AddVolume(p, "Geral", () => a().Master, x => a().Master = x);
        AddVolume(p, "Música", () => a().Music, x => a().Music = x);
        AddVolume(p, "Efeitos", () => a().Sfx, x => a().Sfx = x);
        AddVolume(p, "Ambiente", () => a().Ambient, x => a().Ambient = x);
    }

    private void AddVolume(VBoxContainer p, string label, Func<float> get, Action<float> set)
    {
        // Áudio é barato de aplicar: aplica na hora para o jogador ouvir a diferença.
        AddSlider(p, label, 0f, 1f, 0.01f, get, x => { set(x); S.ApplyAudioSettings(); }, x => $"{x * 100:0}%",
            applyAll: false);
    }

    // ------------------------------------------------------------------ restaurar

    private void OnResetPressed()
    {
        if (!_resetArmed)
        {
            _resetArmed = true;
            ResetButton.Text = "Confirmar restauração?";
            return;
        }
        DisarmReset();
        S.ResetAll();
        RefreshAll();
    }

    private void DisarmReset()
    {
        _resetArmed = false;
        ResetButton.Text = "Restaurar padrões";
    }

    // ------------------------------------------------------------------ construção de linhas

    private void AddHeader(VBoxContainer parent, string text)
    {
        var label = new Label { Text = text, ThemeTypeVariation = "HeaderLabel" };
        if (parent.GetChildCount() > 0)
        {
            var spacer = new Control { CustomMinimumSize = new Vector2(0, 14) };
            parent.AddChild(spacer);
        }
        parent.AddChild(label);
    }

    private void AddRow(VBoxContainer parent, string label, Control control, float controlWidth = -1f)
    {
        var row = new HBoxContainer();
        row.AddThemeConstantOverride("separation", 24);
        row.AddChild(new Label
        {
            Text = label,
            CustomMinimumSize = new Vector2(controlWidth > ControlColumnWidth ? LabelColumnWidth - (controlWidth - ControlColumnWidth) : LabelColumnWidth, 0),
            SizeFlagsHorizontal = SizeFlags.ExpandFill,
            VerticalAlignment = VerticalAlignment.Center,
        });
        var width = controlWidth > 0 ? controlWidth : ControlColumnWidth;
        control.CustomMinimumSize = new Vector2(width, control.CustomMinimumSize.Y);
        control.SizeFlagsVertical = SizeFlags.ShrinkCenter;
        row.AddChild(control);
        parent.AddChild(row);
    }

    private OptionButton AddOption<T>(VBoxContainer parent, string label, (string Text, T Value)[] items,
        Func<T> get, Action<T> set, bool quality = false)
    {
        var option = new OptionButton { FitToLongestItem = false };
        foreach (var item in items)
            option.AddItem(item.Text);
        option.ItemSelected += index =>
        {
            set(items[index].Value);
            OnValueChanged(quality, applyNow: true);
        };
        _refreshers.Add(() =>
        {
            var current = get();
            option.Selected = Array.FindIndex(items, it => EqualityComparer<T>.Default.Equals(it.Value, current));
        });
        AddRow(parent, label, option);
        return option;
    }

    private void AddSlider(VBoxContainer parent, string label, float min, float max, float step,
        Func<float> get, Action<float> set, Func<float, string> format, bool quality = false, bool applyAll = true)
    {
        var box = new HBoxContainer();
        box.AddThemeConstantOverride("separation", 18);
        var slider = new HSlider
        {
            MinValue = min, MaxValue = max, Step = step,
            SizeFlagsHorizontal = SizeFlags.ExpandFill,
            SizeFlagsVertical = SizeFlags.ShrinkCenter,
            FocusMode = FocusModeEnum.All,
        };
        var valueLabel = new Label
        {
            CustomMinimumSize = new Vector2(110, 0),
            HorizontalAlignment = HorizontalAlignment.Right,
        };
        slider.ValueChanged += value =>
        {
            set((float)value);
            valueLabel.Text = format((float)value);
            if (applyAll)
                OnValueChanged(quality, applyNow: false);
        };
        _refreshers.Add(() =>
        {
            slider.SetValueNoSignal(get());
            valueLabel.Text = format(get());
        });
        box.AddChild(slider);
        box.AddChild(valueLabel);
        AddRow(parent, label, box);
    }

    private void AddCheck(VBoxContainer parent, string label, Func<bool> get, Action<bool> set)
    {
        var check = new CheckButton();
        check.Toggled += on =>
        {
            set(on);
            OnValueChanged(quality: false, applyNow: true);
        };
        _refreshers.Add(() => check.SetPressedNoSignal(get()));
        AddRow(parent, label, check);
    }

    private void OnValueChanged(bool quality, bool applyNow)
    {
        if (quality)
            S.RefreshPresetFromQuality();
        if (applyNow)
        {
            _applyCountdown = -1;
            S.ApplyAll();
        }
        else
        {
            ScheduleApply();
        }
        // Atualiza dependências visuais (preset mostrado, resolução habilitada etc.).
        RefreshAll();
    }
}
