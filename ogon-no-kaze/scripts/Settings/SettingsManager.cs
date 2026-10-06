using System;
using Godot;

namespace OgonNoKaze.Settings;

/// <summary>
/// Autoload que carrega, aplica e salva as configurações do jogador em user://settings.cfg.
///
/// O que é global (janela, VSync, FPS, escala 3D, antisserrilhado, tamanho dos atlas de
/// sombra, qualidade de SSAO/névoa no servidor, volumes) é aplicado aqui mesmo. O que
/// depende de nós da cena (Environment, sol, câmera, grama) é aplicado por componentes da
/// cena que escutam o sinal <see cref="SettingsChanged"/> — ex.: <see cref="EnvironmentQuality"/>.
/// </summary>
public partial class SettingsManager : Node
{
    public const string SettingsPath = "user://settings.cfg";
    private const int FileVersion = 1;

    private static readonly string[] PresetPaths =
    {
        "res://data/settings/preset_low.tres",
        "res://data/settings/preset_medium.tres",
        "res://data/settings/preset_high.tres",
        "res://data/settings/preset_ultra.tres",
    };
    private const string RenderTuningPath = "res://data/settings/render_tuning.tres";

    public static SettingsManager Instance { get; private set; }

    /// <summary>Emitido depois que qualquer configuração foi aplicada.</summary>
    [Signal] public delegate void SettingsChangedEventHandler();

    public GameSettings Current { get; private set; } = new();
    public RenderTuning Tuning { get; private set; }

    /// <summary>
    /// Quando falso, nada é gravado em disco. As ferramentas de captura e desempenho
    /// desligam isso para não estragar as configurações do jogador.
    /// </summary>
    public bool PersistenceEnabled { get; set; } = true;

    private QualitySettings[] _presets;

    public override void _EnterTree()
    {
        Instance = this;
        // Precisa continuar funcionando com o jogo pausado (menu de pausa).
        ProcessMode = ProcessModeEnum.Always;
    }

    public override void _Ready()
    {
        _presets = new QualitySettings[PresetPaths.Length];
        for (var i = 0; i < PresetPaths.Length; i++)
            _presets[i] = GD.Load<QualitySettings>(PresetPaths[i]);
        Tuning = GD.Load<RenderTuning>(RenderTuningPath) ?? new RenderTuning();

        Current.Video.Quality = GetPreset(Current.Video.Preset).Clone();
        Load();
        ApplyAll();
    }

    /// <summary>Se a grama deve projetar sombra (só nos níveis altos; nos demais apenas recebe).</summary>
    public bool GrassCastsShadows => Current.Video.Quality.Shadows >= Tuning.GrassCastsShadowsFrom;

    public QualitySettings GetPreset(QualityPresetId id) =>
        id == QualityPresetId.Custom ? Current.Video.Quality : _presets[(int)id];

    /// <summary>Copia os valores de um preset para a configuração atual.</summary>
    public void SelectPreset(QualityPresetId id)
    {
        Current.Video.Preset = id;
        if (id != QualityPresetId.Custom)
            Current.Video.Quality = _presets[(int)id].Clone();
    }

    /// <summary>
    /// Chamar depois de alterar qualquer item de Current.Video.Quality: se os valores
    /// coincidirem com um preset, ele é selecionado; senão vira Personalizado.
    /// </summary>
    public void RefreshPresetFromQuality()
    {
        Current.Video.Preset = QualityPresetId.Custom;
        for (var i = 0; i < _presets.Length; i++)
        {
            if (_presets[i].SameAs(Current.Video.Quality))
            {
                Current.Video.Preset = (QualityPresetId)i;
                break;
            }
        }
    }

    public void ApplyAll()
    {
        ApplyDisplay();
        ApplyRendering();
        ApplyAudio();
        EmitSignal(SignalName.SettingsChanged);
    }

    /// <summary>Aplica e salva (se permitido). É o que a tela de configurações chama.</summary>
    public void ApplyAndSave()
    {
        ApplyAll();
        Save();
    }

    /// <summary>Aplica só os volumes (barato; usado enquanto o jogador arrasta o slider).</summary>
    public void ApplyAudioSettings() => ApplyAudio();

    // ---------------------------------------------------------------- aplicação

    private void ApplyDisplay()
    {
        var v = Current.Video;
        var window = GetWindow();

        switch (v.WindowMode)
        {
            case WindowModeOption.Windowed:
                window.Mode = Window.ModeEnum.Windowed;
                var screenSize = DisplayServer.ScreenGetSize(window.CurrentScreen);
                // Nunca maior que a tela.
                var size = new Vector2I(
                    Math.Min(v.Resolution.X, screenSize.X),
                    Math.Min(v.Resolution.Y, screenSize.Y));
                if (window.Size != size)
                {
                    window.Size = size;
                    window.MoveToCenter();
                }
                break;
            case WindowModeOption.Borderless:
                window.Mode = Window.ModeEnum.Fullscreen;
                break;
            case WindowModeOption.ExclusiveFullscreen:
                window.Mode = Window.ModeEnum.ExclusiveFullscreen;
                break;
        }

        DisplayServer.WindowSetVsyncMode(v.VSync switch
        {
            VSyncOption.Off => DisplayServer.VSyncMode.Disabled,
            VSyncOption.Adaptive => DisplayServer.VSyncMode.Adaptive,
            _ => DisplayServer.VSyncMode.Enabled,
        });
        Engine.MaxFps = Math.Max(0, v.MaxFps);
    }

    private void ApplyRendering()
    {
        var q = Current.Video.Quality;
        var t = Tuning;
        var vp = GetTree().Root;

        // Escala 3D: o mundo é renderizado em resolução menor e o upscaler reconstrói.
        // A interface (CanvasItem) continua na resolução nativa.
        vp.Scaling3DMode = q.Upscaler switch
        {
            UpscalerOption.Fsr1 => Viewport.Scaling3DModeEnum.Fsr,
            UpscalerOption.Fsr2 => Viewport.Scaling3DModeEnum.Fsr2,
            _ => Viewport.Scaling3DModeEnum.Bilinear,
        };
        vp.Scaling3DScale = q.RenderScale;
        vp.FsrSharpness = t.FsrSharpness;

        // O FSR 2 já faz a própria acumulação temporal; TAA junto seria redundante.
        var fsr2 = q.Upscaler == UpscalerOption.Fsr2;
        vp.UseTaa = !fsr2 && q.AntiAliasing == AntiAliasingOption.Taa;
        vp.ScreenSpaceAA = fsr2 ? Viewport.ScreenSpaceAAEnum.Disabled : q.AntiAliasing switch
        {
            AntiAliasingOption.Fxaa => Viewport.ScreenSpaceAAEnum.Fxaa,
            AntiAliasingOption.Smaa => Viewport.ScreenSpaceAAEnum.Smaa,
            _ => Viewport.ScreenSpaceAAEnum.Disabled,
        };

        var s = (int)q.Shadows;
        RenderingServer.DirectionalShadowAtlasSetSize(RenderTuning.Pick(t.ShadowDirectionalAtlasSize, s), true);
        vp.PositionalShadowAtlasSize = RenderTuning.Pick(t.ShadowPositionalAtlasSize, s);
        var filter = (RenderingServer.ShadowQuality)RenderTuning.Pick(t.ShadowFilterQuality, s);
        RenderingServer.DirectionalSoftShadowFilterSetQuality(filter);
        RenderingServer.PositionalSoftShadowFilterSetQuality(filter);

        var f = (int)q.VolumetricFog;
        if (q.VolumetricFog != FogQuality.Off)
        {
            RenderingServer.EnvironmentSetVolumetricFogVolumeSize(
                RenderTuning.Pick(t.FogVolumeSize, f), RenderTuning.Pick(t.FogVolumeDepth, f));
            RenderingServer.EnvironmentSetVolumetricFogFilterActive(q.VolumetricFog >= t.FogFilterFrom);
        }

        if (q.Ssao != SsaoQuality.Off)
        {
            var a = (int)q.Ssao;
            RenderingServer.EnvironmentSetSsaoQuality(
                (RenderingServer.EnvironmentSsaoQuality)RenderTuning.Pick(t.SsaoServerQuality, a),
                q.Ssao < t.SsaoFullResolutionFrom, 0.5f, 2, 50f, 300f);
        }
    }

    private void ApplyAudio()
    {
        var a = Current.Audio;
        SetBusVolume("Master", a.Master);
        SetBusVolume("Music", a.Music);
        SetBusVolume("SFX", a.Sfx);
        SetBusVolume("Ambient", a.Ambient);
    }

    private static void SetBusVolume(string bus, float linear)
    {
        var index = AudioServer.GetBusIndex(bus);
        if (index < 0)
        {
            GD.PushWarning($"Barramento de áudio '{bus}' não existe em default_bus_layout.tres.");
            return;
        }
        linear = Mathf.Clamp(linear, 0f, 1f);
        AudioServer.SetBusMute(index, linear <= 0.001f);
        AudioServer.SetBusVolumeDb(index, Mathf.LinearToDb(Mathf.Max(linear, 0.001f)));
    }

    // ---------------------------------------------------------------- persistência

    public void Save()
    {
        if (!PersistenceEnabled)
            return;

        var cfg = new ConfigFile();
        cfg.SetValue("meta", "version", FileVersion);

        var v = Current.Video;
        cfg.SetValue("video", "resolution", v.Resolution);
        cfg.SetValue("video", "window_mode", (int)v.WindowMode);
        cfg.SetValue("video", "vsync", (int)v.VSync);
        cfg.SetValue("video", "max_fps", v.MaxFps);
        cfg.SetValue("video", "fov", v.Fov);
        cfg.SetValue("video", "preset", (int)v.Preset);
        v.Quality.SaveTo(cfg, "quality");

        var a = Current.Audio;
        cfg.SetValue("audio", "master", a.Master);
        cfg.SetValue("audio", "music", a.Music);
        cfg.SetValue("audio", "sfx", a.Sfx);
        cfg.SetValue("audio", "ambient", a.Ambient);

        var c = Current.Controls;
        cfg.SetValue("controls", "mouse_sensitivity", c.MouseSensitivity);
        cfg.SetValue("controls", "gamepad_sensitivity", c.GamepadSensitivity);
        cfg.SetValue("controls", "invert_y", c.InvertY);

        InputBindings.SaveTo(cfg, "input");

        var err = cfg.Save(SettingsPath);
        if (err != Error.Ok)
            GD.PushError($"Falha ao salvar {SettingsPath}: {err}");
    }

    private void Load()
    {
        var cfg = new ConfigFile();
        var err = cfg.Load(SettingsPath);
        if (err == Error.FileNotFound)
            return; // primeira execução: fica com os padrões
        if (err != Error.Ok)
        {
            GD.PushWarning($"Não foi possível ler {SettingsPath} ({err}); usando padrões.");
            return;
        }

        var v = Current.Video;
        v.Resolution = (Vector2I)cfg.GetValue("video", "resolution", v.Resolution);
        v.WindowMode = (WindowModeOption)(int)cfg.GetValue("video", "window_mode", (int)v.WindowMode);
        v.VSync = (VSyncOption)(int)cfg.GetValue("video", "vsync", (int)v.VSync);
        v.MaxFps = (int)cfg.GetValue("video", "max_fps", v.MaxFps);
        v.Fov = Mathf.Clamp((float)cfg.GetValue("video", "fov", v.Fov), 50f, 110f);
        var preset = Mathf.Clamp((int)cfg.GetValue("video", "preset", (int)v.Preset), 0, (int)QualityPresetId.Custom);
        SelectPreset((QualityPresetId)preset);
        if (v.Preset == QualityPresetId.Custom)
            v.Quality.LoadFrom(cfg, "quality");

        var a = Current.Audio;
        a.Master = (float)cfg.GetValue("audio", "master", a.Master);
        a.Music = (float)cfg.GetValue("audio", "music", a.Music);
        a.Sfx = (float)cfg.GetValue("audio", "sfx", a.Sfx);
        a.Ambient = (float)cfg.GetValue("audio", "ambient", a.Ambient);

        var c = Current.Controls;
        c.MouseSensitivity = (float)cfg.GetValue("controls", "mouse_sensitivity", c.MouseSensitivity);
        c.GamepadSensitivity = (float)cfg.GetValue("controls", "gamepad_sensitivity", c.GamepadSensitivity);
        c.InvertY = (bool)cfg.GetValue("controls", "invert_y", c.InvertY);

        InputBindings.LoadFrom(cfg, "input");
    }

    /// <summary>Apaga as configurações salvas e volta tudo ao padrão.</summary>
    public void ResetAll()
    {
        Current = new GameSettings();
        SelectPreset(Current.Video.Preset);
        InputBindings.ResetToDefaults();
        ApplyAndSave();
    }
}
