using Godot;
using OgonNoKaze.Settings;

namespace OgonNoKaze.World;

/// <summary>
/// Câmera lenta do menu: desliza num vaivém ao longo de um Path3D olhando para um alvo,
/// com uma leve oscilação de "câmera na mão" para a cena não parecer congelada.
///
/// O vaivém usa (1 - cos(fase)) / 2, que vai de 0 a 1 e volta com velocidade zero nas
/// pontas — a câmera desacelera e inverte sem tranco, como um travelling de verdade.
/// </summary>
[GlobalClass]
public partial class CinematicCamera : Camera3D
{
    [Export] public Path3D Path { get; set; }
    [Export] public Node3D LookTarget { get; set; }
    /// <summary>Segundos para ir de uma ponta do caminho à outra.</summary>
    [Export(PropertyHint.Range, "5,600,1,suffix:s")] public float TravelSeconds { get; set; } = 70f;
    /// <summary>Amplitude da oscilação de mão, em graus.</summary>
    [Export(PropertyHint.Range, "0,3,0.05")] public float SwayDegrees { get; set; } = 0.35f;
    [Export(PropertyHint.Range, "0.01,2,0.01")] public float SwayFrequency { get; set; } = 0.12f;
    /// <summary>Fase inicial (0–1) do vaivém, para escolher onde o menu começa.</summary>
    [Export(PropertyHint.Range, "0,1,0.01")] public float StartPhase { get; set; } = 0.15f;
    /// <summary>Se ligado, usa o FOV das configurações; senão mantém o do inspetor.</summary>
    [Export] public bool UseSettingsFov { get; set; } = false;

    private double _time;
    private readonly FastNoiseLite _noise = new() { NoiseType = FastNoiseLite.NoiseTypeEnum.SimplexSmooth };

    public override void _Ready()
    {
        _time = StartPhase * TravelSeconds * 2.0;
        if (UseSettingsFov && SettingsManager.Instance != null)
        {
            SettingsManager.Instance.SettingsChanged += OnSettingsChanged;
            OnSettingsChanged();
        }
        UpdatePose();
    }

    public override void _ExitTree()
    {
        if (UseSettingsFov && SettingsManager.Instance != null)
            SettingsManager.Instance.SettingsChanged -= OnSettingsChanged;
    }

    private void OnSettingsChanged() => Fov = SettingsManager.Instance.Current.Video.Fov;

    public override void _Process(double delta)
    {
        _time += delta;
        UpdatePose();
    }

    private void UpdatePose()
    {
        var curve = Path?.Curve;
        if (curve == null || curve.PointCount < 2)
            return;

        var phase = (float)(_time / TravelSeconds * Mathf.Pi);
        var t = (1f - Mathf.Cos(phase)) * 0.5f;
        var local = curve.SampleBaked(t * curve.GetBakedLength(), cubic: true);
        GlobalPosition = Path.GlobalTransform * local;

        if (LookTarget != null)
            LookAt(LookTarget.GlobalPosition, Vector3.Up);

        // Ruído suave em yaw/pitch: dois canais do mesmo ruído com deslocamento no tempo.
        var s = (float)_time * SwayFrequency * 100f;
        var yaw = _noise.GetNoise1D(s) * SwayDegrees;
        var pitch = _noise.GetNoise1D(s + 1000f) * SwayDegrees * 0.6f;
        RotateObjectLocal(Vector3.Up, Mathf.DegToRad(yaw));
        RotateObjectLocal(Vector3.Right, Mathf.DegToRad(pitch));
    }
}
