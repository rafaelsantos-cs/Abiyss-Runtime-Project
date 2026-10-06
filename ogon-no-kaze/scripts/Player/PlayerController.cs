using Godot;
using OgonNoKaze.Core;
using OgonNoKaze.Player.StateMachine;
using OgonNoKaze.Player.States;

namespace OgonNoKaze.Player;

/// <summary>
/// Corpo físico do jogador. Lê o input, guarda o buffer e delega a decisão do que fazer
/// à máquina de estados; os estados usam os métodos daqui (MoveOnGround, ApplyGravity...)
/// para mexer na velocidade. No fim do tick, MoveAndSlide resolve colisões.
///
/// O movimento é relativo à câmera: "frente" é para onde a câmera olha, projetado no
/// chão. É o padrão de jogos em terceira pessoa.
/// </summary>
public partial class PlayerController : CharacterBody3D
{
    [Export] public MovementTuning Tuning { get; set; }
    [Export] public PlayerActionSet Actions { get; set; }
    /// <summary>Raiz visual que gira para encarar o movimento (a cápsula não gira).</summary>
    [Export] public Node3D Visual { get; set; }
    [Export] public CollisionShape3D Collider { get; set; }
    /// <summary>Raio de cima que verifica espaço livre para levantar do agachado.</summary>
    [Export] public ShapeCast3D HeadroomProbe { get; set; }

    public PlayerStateMachine States { get; private set; }
    public InputBuffer Buffer { get; private set; }

    /// <summary>Input de movimento cru (comprimento 0–1).</summary>
    public Vector2 MoveInput { get; private set; }
    /// <summary>Direção desejada no mundo (já relativa à câmera), comprimento = intensidade.</summary>
    public Vector3 MoveDirection { get; private set; }
    public bool HasMoveInput => MoveInput.LengthSquared() > 0.01f;

    public bool IsCrouching { get; private set; }

    /// <summary>
    /// Quão visível o jogador está para a IA (1 = normal). Agachado já reduz; na Fase 3
    /// a altura do trigo (GrassQuery.GetHeightAt) entra nesta conta.
    /// </summary>
    public float VisibilityFactor => IsCrouching ? 0.5f : 1f;

    private ulong _lastGroundedTick;
    private bool _jumpedSinceGrounded;

    /// <summary>Ainda dá para pular logo depois de sair de uma borda (coyote time).</summary>
    public bool CanCoyoteJump =>
        !_jumpedSinceGrounded
        && Engine.GetPhysicsFrames() - _lastGroundedTick <= (ulong)Mathf.RoundToInt(Tuning.CoyoteTime * Engine.PhysicsTicksPerSecond);

    public override void _Ready()
    {
        Tuning ??= new MovementTuning();
        Buffer = new InputBuffer(Tuning.InputBufferTime,
            InputActions.Jump, InputActions.Crouch, InputActions.Grapple,
            InputActions.Attack, InputActions.Deflect);

        ConfigureCapsule(Tuning.StandingHeight);
        FloorSnapLength = 0.3f;
        FloorMaxAngle = Mathf.DegToRad(50f);

        States = new PlayerStateMachine(this);
        States.Add(new GroundedState());
        States.Add(new IdleState(), StateId.Grounded);
        States.Add(new WalkState(), StateId.Grounded);
        States.Add(new RunState(), StateId.Grounded);
        States.Add(new SprintState(), StateId.Grounded);
        States.Add(new CrouchState(), StateId.Grounded);
        States.Add(new AirborneState());
        States.Add(new JumpState(), StateId.Airborne);
        States.Add(new FallState(), StateId.Airborne);
        States.Add(new TraversalState());
        States.Add(new GrappleState(), StateId.Traversal);
        States.Add(new LedgeHangState(), StateId.Traversal);
        States.Add(new ClimbState(), StateId.Traversal);
        States.Add(new CombatState());
        States.Add(new AttackState(), StateId.Combat);
        States.Add(new DeflectState(), StateId.Combat);
        States.Add(new GuardState(), StateId.Combat);
        States.Add(new StaggerState(), StateId.Combat);
        States.Add(new PostureBrokenState(), StateId.Combat);
        States.Add(new DeathblowState(), StateId.Combat);
        States.Add(new DeathState(), StateId.Combat);
        States.Start(StateId.Fall);
    }

    public override void _PhysicsProcess(double delta)
    {
        var dt = (float)delta;
        Buffer.WindowSeconds = Tuning.InputBufferTime;
        ReadMoveInput();
        Buffer.Update();
        States.Tick(dt);
        MoveAndSlide();
        if (IsOnFloor())
            _lastGroundedTick = Engine.GetPhysicsFrames();
        FaceMovement(dt);
    }

    private void ReadMoveInput()
    {
        // GetVector aplica a zona morta de cada ação e normaliza a diagonal do teclado.
        MoveInput = Input.GetVector(InputActions.MoveLeft, InputActions.MoveRight, InputActions.MoveForward, InputActions.MoveBack);

        var camera = GetViewport().GetCamera3D();
        var basis = camera?.GlobalBasis ?? GlobalBasis;
        // Projeta os eixos da câmera no plano do chão para "frente" não apontar para o céu.
        var forward = new Vector3(-basis.Z.X, 0f, -basis.Z.Z).Normalized();
        var right = new Vector3(basis.X.X, 0f, basis.X.Z).Normalized();
        MoveDirection = right * MoveInput.X + forward * -MoveInput.Y;
    }

    // ------------------------------------------------------------ usados pelos estados

    /// <summary>Acelera a velocidade horizontal em direção a MoveDirection·speed.</summary>
    public void MoveOnGround(float speed, float delta)
    {
        var target = MoveDirection.LengthSquared() > 0.0001f ? MoveDirection.Normalized() * speed : Vector3.Zero;
        var horizontal = new Vector3(Velocity.X, 0f, Velocity.Z);
        var rate = target.LengthSquared() > horizontal.LengthSquared() ? Tuning.GroundAcceleration : Tuning.GroundDeceleration;
        horizontal = horizontal.MoveToward(target, rate * delta);
        Velocity = new Vector3(horizontal.X, Velocity.Y, horizontal.Z);
    }

    /// <summary>Controle aéreo: corrige a direção sem ultrapassar a velocidade de corrida.</summary>
    public void MoveInAir(float delta)
    {
        if (!HasMoveInput)
            return;
        var horizontal = new Vector3(Velocity.X, 0f, Velocity.Z);
        var maxSpeed = Mathf.Max(horizontal.Length(), Tuning.RunSpeed);
        horizontal = (horizontal + MoveDirection * Tuning.AirAcceleration * delta).LimitLength(maxSpeed);
        Velocity = new Vector3(horizontal.X, Velocity.Y, horizontal.Z);
    }

    public void ApplyGravity(float multiplier, float delta)
    {
        var y = Mathf.Max(Velocity.Y - Tuning.Gravity * multiplier * delta, -Tuning.MaxFallSpeed);
        Velocity = Velocity with { Y = y };
    }

    public void StartJump()
    {
        _jumpedSinceGrounded = true;
        Velocity = Velocity with { Y = Tuning.JumpVelocity };
    }

    public void NotifyGrounded() => _jumpedSinceGrounded = false;

    public void SetCrouching(bool crouch)
    {
        if (IsCrouching == crouch)
            return;
        IsCrouching = crouch;
        ConfigureCapsule(crouch ? Tuning.CrouchingHeight : Tuning.StandingHeight);
        // Placeholder visual: achata o boneco. Na Fase 4 a animação de agachar cuida disso.
        if (Visual != null)
            Visual.Scale = new Vector3(1f, crouch ? Tuning.CrouchingHeight / Tuning.StandingHeight : 1f, 1f);
    }

    /// <summary>Há espaço acima da cabeça para ficar de pé?</summary>
    public bool CanStandUp()
    {
        if (!IsCrouching || HeadroomProbe == null)
            return true;
        HeadroomProbe.TargetPosition = Vector3.Up * (Tuning.StandingHeight - Tuning.CrouchingHeight);
        HeadroomProbe.ForceShapecastUpdate();
        return !HeadroomProbe.IsColliding();
    }

    /// <summary>Cápsula com a base sempre em y = 0 do corpo (os pés no chão).</summary>
    private void ConfigureCapsule(float height)
    {
        if (Collider?.Shape is not CapsuleShape3D capsule)
            return;
        capsule.Radius = Tuning.CapsuleRadius;
        capsule.Height = height;
        Collider.Position = new Vector3(0f, height * 0.5f, 0f);
    }

    private void FaceMovement(float delta)
    {
        if (Visual == null)
            return;
        var horizontal = new Vector3(Velocity.X, 0f, Velocity.Z);
        if (horizontal.LengthSquared() < 0.05f)
            return;
        // Ângulo alvo (yaw) e giro limitado a TurnSpeed graus/s: vira rápido, mas não teleporta.
        var targetYaw = Mathf.Atan2(-horizontal.X, -horizontal.Z);
        var current = Visual.Rotation.Y;
        var diff = Mathf.Wrap(targetYaw - current, -Mathf.Pi, Mathf.Pi);
        var step = Mathf.DegToRad(Tuning.TurnSpeed) * delta;
        Visual.Rotation = Visual.Rotation with { Y = current + Mathf.Clamp(diff, -step, step) };
    }
}
