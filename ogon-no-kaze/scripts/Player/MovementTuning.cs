using Godot;

namespace OgonNoKaze.Player;

/// <summary>
/// Todos os números da locomoção num Resource (data/player/movement_tuning.tres), para
/// ajustar a sensação de controle no inspetor com o jogo rodando. Valores iniciais;
/// a calibragem final é feita com o Rafael jogando.
/// </summary>
[GlobalClass]
public partial class MovementTuning : Resource
{
    [ExportGroup("Velocidades (m/s)")]
    [Export(PropertyHint.Range, "0.5,5,0.1")] public float WalkSpeed { get; set; } = 2.2f;
    [Export(PropertyHint.Range, "1,10,0.1")] public float RunSpeed { get; set; } = 5.2f;
    [Export(PropertyHint.Range, "2,15,0.1")] public float SprintSpeed { get; set; } = 7.6f;
    [Export(PropertyHint.Range, "0.5,5,0.1")] public float CrouchSpeed { get; set; } = 1.9f;
    /// <summary>Abaixo disto (0–1) o analógico anda; acima, corre.</summary>
    [Export(PropertyHint.Range, "0.1,0.9,0.05")] public float WalkInputThreshold { get; set; } = 0.55f;

    [ExportGroup("Resposta no chão")]
    /// <summary>m/s² ao acelerar. Alto = arranque imediato (Sekiro-like).</summary>
    [Export(PropertyHint.Range, "1,200,1")] public float GroundAcceleration { get; set; } = 60f;
    [Export(PropertyHint.Range, "1,200,1")] public float GroundDeceleration { get; set; } = 70f;
    /// <summary>Graus por segundo que o corpo gira para encarar a direção do movimento.</summary>
    [Export(PropertyHint.Range, "90,2000,10")] public float TurnSpeed { get; set; } = 900f;

    [ExportGroup("Pulo e ar")]
    [Export(PropertyHint.Range, "0.2,5,0.05,suffix:m")] public float JumpHeight { get; set; } = 1.35f;
    /// <summary>Gravidade na subida (m/s²). Maior que a real deixa o pulo "seco".</summary>
    [Export(PropertyHint.Range, "5,80,0.5")] public float Gravity { get; set; } = 26f;
    /// <summary>Multiplica a gravidade na descida: cair mais rápido que subir dá peso.</summary>
    [Export(PropertyHint.Range, "1,4,0.05")] public float FallGravityMultiplier { get; set; } = 1.6f;
    /// <summary>Soltar o pulo cedo multiplica a velocidade de subida por isto (pulo curto).</summary>
    [Export(PropertyHint.Range, "0,1,0.05")] public float JumpCutMultiplier { get; set; } = 0.45f;
    [Export(PropertyHint.Range, "5,80,1")] public float MaxFallSpeed { get; set; } = 40f;
    /// <summary>Aceleração horizontal no ar (controle aéreo).</summary>
    [Export(PropertyHint.Range, "0,100,1")] public float AirAcceleration { get; set; } = 18f;

    [ExportGroup("Tolerâncias (s)")]
    /// <summary>Tempo após sair de uma borda em que ainda dá para pular.</summary>
    [Export(PropertyHint.Range, "0,0.4,0.01")] public float CoyoteTime { get; set; } = 0.10f;
    /// <summary>Por quanto tempo um botão apertado fica guardado esperando poder agir.</summary>
    [Export(PropertyHint.Range, "0,0.5,0.01")] public float InputBufferTime { get; set; } = 0.15f;

    [ExportGroup("Cápsula (m)")]
    [Export] public float StandingHeight { get; set; } = 1.8f;
    [Export] public float CrouchingHeight { get; set; } = 1.05f;
    [Export] public float CapsuleRadius { get; set; } = 0.35f;

    /// <summary>Velocidade inicial de pulo para atingir JumpHeight: v = √(2·g·h).</summary>
    public float JumpVelocity => Mathf.Sqrt(2f * Gravity * JumpHeight);
}
