using OgonNoKaze.Core;
using OgonNoKaze.Player.StateMachine;
using Godot;

namespace OgonNoKaze.Player.States;

/// <summary>Pai dos estados no ar: controle aéreo e aterrissagem.</summary>
public sealed class AirborneState : PlayerState
{
    public override StateId Id => StateId.Airborne;
    public override StateId? DefaultChild => StateId.Fall;

    public override StateId? CheckTransition()
    {
        if (Player.IsOnFloor() && Player.Velocity.Y <= 0.01f)
            return StateId.Grounded;
        // Fase 4: gancho (Grapple) e agarrar bordas (LedgeHang) entram aqui.
        return null;
    }

    public override void PhysicsUpdate(float delta) => Player.MoveInAir(delta);
}

/// <summary>
/// Subida do pulo. Soltar o botão cedo corta a velocidade de subida (pulo curto):
/// o jogador controla a altura pelo tempo que segura.
/// </summary>
public sealed class JumpState : PlayerState
{
    private bool _cut;

    public override StateId Id => StateId.Jump;

    public override void Enter(StateId previous)
    {
        _cut = false;
        Player.StartJump();
    }

    public override StateId? CheckTransition() => Player.Velocity.Y <= 0f ? StateId.Fall : null;

    public override void PhysicsUpdate(float delta)
    {
        if (!_cut && !Input.IsActionPressed(InputActions.Jump) && Player.Velocity.Y > 0f)
        {
            _cut = true;
            Player.Velocity = Player.Velocity with { Y = Player.Velocity.Y * Player.Tuning.JumpCutMultiplier };
        }
        Player.ApplyGravity(1f, delta);
    }
}

/// <summary>Queda. Também aceita o pulo atrasado (coyote time) logo após sair de uma borda.</summary>
public sealed class FallState : PlayerState
{
    public override StateId Id => StateId.Fall;

    public override StateId? CheckTransition()
    {
        if (Player.CanCoyoteJump && Player.Buffer.Consume(InputActions.Jump))
            return StateId.Jump;
        return null;
    }

    public override void PhysicsUpdate(float delta) =>
        Player.ApplyGravity(Player.Tuning.FallGravityMultiplier, delta);
}
