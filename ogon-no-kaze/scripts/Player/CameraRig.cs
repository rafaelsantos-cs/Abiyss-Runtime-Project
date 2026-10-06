using Godot;
using OgonNoKaze.Combat;
using OgonNoKaze.Core;
using OgonNoKaze.Settings;

namespace OgonNoKaze.Player;

/// <summary>
/// Câmera em terceira pessoa: um pivô (yaw/pitch) que segue o alvo com suavização, um
/// SpringArm3D que encurta quando bate em algo (a câmera não atravessa paredes) e a
/// Camera3D na ponta, levemente deslocada para o lado (por cima do ombro).
///
/// O pivô é "top level": não herda a rotação do jogador, então girar o personagem não
/// gira a câmera. Isso é o que permite andar em direção à câmera.
/// </summary>
public partial class CameraRig : Node3D, ICameraShake
{
    [Export] public Node3D FollowTarget { get; set; }
    [Export] public SpringArm3D Arm { get; set; }
    [Export] public Camera3D Camera { get; set; }

    [ExportGroup("Enquadramento")]
    [Export] public float FollowHeight { get; set; } = 1.55f;
    /// <summary>Quão rápido o pivô alcança o alvo (maior = mais grudado).</summary>
    [Export(PropertyHint.Range, "1,40,0.5")] public float FollowSharpness { get; set; } = 14f;
    [Export(PropertyHint.Range, "1,10,0.1,suffix:m")] public float ArmLength { get; set; } = 3.6f;
    [Export(PropertyHint.Range, "-1,1,0.05,suffix:m")] public float ShoulderOffset { get; set; } = 0.35f;
    [Export(PropertyHint.Range, "-89,0,1")] public float PitchMin { get; set; } = -65f;
    [Export(PropertyHint.Range, "0,89,1")] public float PitchMax { get; set; } = 45f;

    [ExportGroup("Tremor")]
    [Export(PropertyHint.Range, "0,10,0.1")] public float MaxShakeDegrees { get; set; } = 2.5f;
    /// <summary>Trauma perdido por segundo.</summary>
    [Export(PropertyHint.Range, "0.1,5,0.1")] public float TraumaDecay { get; set; } = 1.6f;

    /// <summary>Alvo travado (lock-on). Preparado para o combate; ninguém define ainda.</summary>
    public ILockOnTarget LockTarget { get; set; }

    private float _yaw;
    private float _pitch = -12f;
    private float _trauma;
    private readonly FastNoiseLite _shakeNoise = new() { Frequency = 0.08f };
    private double _time;

    public override void _Ready()
    {
        TopLevel = true;
        if (Arm != null)
        {
            Arm.SpringLength = ArmLength;
            // O braço não deve colidir com o próprio jogador.
            if (FollowTarget is CollisionObject3D body)
                Arm.AddExcludedObject(body.GetRid());
        }
        if (FollowTarget != null)
            GlobalPosition = TargetPoint();
        _yaw = GlobalRotationDegrees.Y;

        if (SettingsManager.Instance != null)
        {
            SettingsManager.Instance.SettingsChanged += ApplySettings;
            ApplySettings();
        }
        Input.MouseMode = Input.MouseModeEnum.Captured;
    }

    public override void _ExitTree()
    {
        if (SettingsManager.Instance != null)
            SettingsManager.Instance.SettingsChanged -= ApplySettings;
    }

    private void ApplySettings()
    {
        if (Camera != null)
            Camera.Fov = SettingsManager.Instance.Current.Video.Fov;
    }

    private ControlSettings Controls => SettingsManager.Instance?.Current.Controls ?? new ControlSettings();

    public override void _UnhandledInput(InputEvent e)
    {
        if (e is InputEventMouseMotion motion && Input.MouseMode == Input.MouseModeEnum.Captured)
        {
            var c = Controls;
            _yaw -= motion.Relative.X * c.MouseSensitivity;
            _pitch -= motion.Relative.Y * c.MouseSensitivity * (c.InvertY ? -1f : 1f);
        }
        else if (e is InputEventMouseButton { Pressed: true } && Input.MouseMode != Input.MouseModeEnum.Captured
                 && !GetTree().Paused)
        {
            // Voltou para a janela (ex.: depois de alt-tab): recaptura o mouse.
            Input.MouseMode = Input.MouseModeEnum.Captured;
        }
    }

    public override void _Process(double delta)
    {
        var dt = (float)delta;
        _time += delta;

        // Analógico direito: graus por segundo, escalado pela intensidade.
        var c = Controls;
        var look = Input.GetVector(InputActions.LookLeft, InputActions.LookRight, InputActions.LookUp, InputActions.LookDown);
        _yaw -= look.X * c.GamepadSensitivity * dt;
        _pitch -= look.Y * c.GamepadSensitivity * 0.7f * dt * (c.InvertY ? -1f : 1f);
        _pitch = Mathf.Clamp(_pitch, PitchMin, PitchMax);

        if (FollowTarget != null)
        {
            // Suavização exponencial independente de FPS: 1 - e^(-k·dt).
            var t = 1f - Mathf.Exp(-FollowSharpness * dt);
            GlobalPosition = GlobalPosition.Lerp(TargetPoint(), t);
        }

        _trauma = Mathf.Max(0f, _trauma - TraumaDecay * dt);
        // Tremor proporcional a trauma²: pancadas pequenas quase não tremem, grandes tremem muito.
        var shake = _trauma * _trauma * MaxShakeDegrees;
        var s = (float)_time * 100f;
        GlobalRotationDegrees = new Vector3(
            _pitch + shake * _shakeNoise.GetNoise1D(s),
            _yaw + shake * _shakeNoise.GetNoise1D(s + 500f),
            shake * 0.5f * _shakeNoise.GetNoise1D(s + 1000f));

        // O deslocamento de ombro vai no braço (o SpringArm reposiciona a câmera filha).
        if (Arm != null)
            Arm.Position = new Vector3(ShoulderOffset, 0f, 0f);
    }

    private Vector3 TargetPoint() => FollowTarget.GlobalPosition + Vector3.Up * FollowHeight;

    public void AddTrauma(float amount) => _trauma = Mathf.Clamp(_trauma + amount, 0f, 1f);
}
