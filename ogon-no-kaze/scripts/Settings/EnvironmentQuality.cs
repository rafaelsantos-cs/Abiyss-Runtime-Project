using Godot;

namespace OgonNoKaze.Settings;

/// <summary>
/// Componente de cena: liga/desliga os efeitos do Environment e ajusta as sombras do sol
/// conforme a qualidade escolhida. Os parâmetros artísticos (cor e densidade da névoa,
/// intensidade do SSAO etc.) continuam sendo os do .tscn; aqui só entra o que custa GPU.
/// </summary>
[GlobalClass]
public partial class EnvironmentQuality : Node
{
    [Export] public WorldEnvironment WorldEnvironment { get; set; }
    [Export] public DirectionalLight3D Sun { get; set; }

    public override void _Ready()
    {
        var settings = SettingsManager.Instance;
        if (settings == null)
            return;
        settings.SettingsChanged += Apply;
        Apply();
    }

    public override void _ExitTree()
    {
        if (SettingsManager.Instance != null)
            SettingsManager.Instance.SettingsChanged -= Apply;
    }

    private void Apply()
    {
        var q = SettingsManager.Instance.Current.Video.Quality;
        var t = SettingsManager.Instance.Tuning;

        var env = WorldEnvironment?.Environment;
        if (env != null)
        {
            env.SsaoEnabled = q.Ssao != SsaoQuality.Off;
            env.VolumetricFogEnabled = q.VolumetricFog != FogQuality.Off;
            env.SsilEnabled = q.GlobalIllumination == GlobalIlluminationOption.Ssil;
            env.SdfgiEnabled = q.GlobalIllumination == GlobalIlluminationOption.Sdfgi;
        }

        if (Sun != null)
        {
            var s = (int)q.Shadows;
            Sun.DirectionalShadowMode = (DirectionalLight3D.ShadowMode)RenderTuning.Pick(t.ShadowSplitMode, s);
            Sun.DirectionalShadowMaxDistance = RenderTuning.Pick(t.ShadowMaxDistance, s);
        }
    }
}
