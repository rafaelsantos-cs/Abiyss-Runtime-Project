using Godot;

namespace OgonNoKaze.Settings;

/// <summary>
/// Conjunto de opções gráficas que um preset controla. Os presets Baixo/Médio/Alto/Ultra
/// são instâncias deste Resource em data/settings/preset_*.tres (editáveis no inspetor);
/// a configuração atual do jogador é uma cópia (Duplicate) que ele pode alterar à vontade.
/// </summary>
[GlobalClass]
public partial class QualitySettings : Resource
{
    [ExportGroup("Resolução interna")]
    /// <summary>Fração da resolução da janela usada para renderizar o 3D (o upscaler completa).</summary>
    [Export(PropertyHint.Range, "0.5,1.0,0.01")] public float RenderScale { get; set; } = 1.0f;
    [Export] public UpscalerOption Upscaler { get; set; } = UpscalerOption.Fsr1;
    [Export] public AntiAliasingOption AntiAliasing { get; set; } = AntiAliasingOption.Fxaa;

    [ExportGroup("Iluminação")]
    [Export] public ShadowQuality Shadows { get; set; } = ShadowQuality.Medium;
    [Export] public FogQuality VolumetricFog { get; set; } = FogQuality.Medium;
    [Export] public GlobalIlluminationOption GlobalIllumination { get; set; } = GlobalIlluminationOption.Off;
    [Export] public SsaoQuality Ssao { get; set; } = SsaoQuality.Low;

    [ExportGroup("Grama")]
    /// <summary>Multiplicador da densidade de hastes (1 = densidade máxima do mapa).</summary>
    [Export(PropertyHint.Range, "0.1,1.0,0.05")] public float GrassDensity { get; set; } = 0.6f;
    /// <summary>Distância, em metros, até onde a grama é desenhada.</summary>
    [Export(PropertyHint.Range, "20,250,5,suffix:m")] public float GrassDistance { get; set; } = 80f;

    public QualitySettings Clone() => (QualitySettings)Duplicate();

    public bool SameAs(QualitySettings other) =>
        other != null
        && Mathf.IsEqualApprox(RenderScale, other.RenderScale)
        && Upscaler == other.Upscaler
        && AntiAliasing == other.AntiAliasing
        && Shadows == other.Shadows
        && VolumetricFog == other.VolumetricFog
        && GlobalIllumination == other.GlobalIllumination
        && Ssao == other.Ssao
        && Mathf.IsEqualApprox(GrassDensity, other.GrassDensity)
        && Mathf.IsEqualApprox(GrassDistance, other.GrassDistance);

    public void SaveTo(ConfigFile cfg, string section)
    {
        cfg.SetValue(section, "render_scale", RenderScale);
        cfg.SetValue(section, "upscaler", (int)Upscaler);
        cfg.SetValue(section, "anti_aliasing", (int)AntiAliasing);
        cfg.SetValue(section, "shadows", (int)Shadows);
        cfg.SetValue(section, "volumetric_fog", (int)VolumetricFog);
        cfg.SetValue(section, "global_illumination", (int)GlobalIllumination);
        cfg.SetValue(section, "ssao", (int)Ssao);
        cfg.SetValue(section, "grass_density", GrassDensity);
        cfg.SetValue(section, "grass_distance", GrassDistance);
    }

    /// <summary>Lê os valores do arquivo; chaves ausentes mantêm o valor atual.</summary>
    public void LoadFrom(ConfigFile cfg, string section)
    {
        RenderScale = Mathf.Clamp((float)cfg.GetValue(section, "render_scale", RenderScale), 0.5f, 1f);
        Upscaler = (UpscalerOption)(int)cfg.GetValue(section, "upscaler", (int)Upscaler);
        AntiAliasing = (AntiAliasingOption)(int)cfg.GetValue(section, "anti_aliasing", (int)AntiAliasing);
        Shadows = (ShadowQuality)(int)cfg.GetValue(section, "shadows", (int)Shadows);
        VolumetricFog = (FogQuality)(int)cfg.GetValue(section, "volumetric_fog", (int)VolumetricFog);
        GlobalIllumination = (GlobalIlluminationOption)(int)cfg.GetValue(section, "global_illumination", (int)GlobalIllumination);
        Ssao = (SsaoQuality)(int)cfg.GetValue(section, "ssao", (int)Ssao);
        GrassDensity = Mathf.Clamp((float)cfg.GetValue(section, "grass_density", GrassDensity), 0.1f, 1f);
        GrassDistance = Mathf.Clamp((float)cfg.GetValue(section, "grass_distance", GrassDistance), 20f, 250f);
    }
}
