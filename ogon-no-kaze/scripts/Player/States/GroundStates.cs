using OgonNoKaze.Core;
using OgonNoKaze.Player.StateMachine;
using Godot;

namespace OgonNoKaze.Player.States;

/// <summary>Pai de tudo que acontece com os pés no chão.</summary>
public sealed class GroundedState : PlayerState
{
    public override StateId Id => StateId.Grounded;
    public override StateId? DefaultChild => StateId.Idle;

    public override void Enter(StateId previous) => Player.NotifyGrounded();

    public override StateId? CheckTransition()
    {
        // Saiu do chão sem pular (borda, degrau): cai. O coyote time é tratado no Fall.
        if (!Player.IsOnFloor())
            return StateId.Fall;
        if (Player.Buffer.IsBuffered(InputActions.Jump) && (!Player.IsCrouching || Player.CanStandUp()))
        {
            Player.Buffer.Consume(InputActions.Jump);
            return StateId.Jump;
        }
        return null;
    }

    public override void PhysicsUpdate(float delta) => Player.ApplyGravity(1f, delta);
}

/// <summary>Escolhe a folha de locomoção a partir do input atual (usado por Idle/Walk/Run/Sprint).</summary>
internal static class Locomotion
{
    public static StateId Pick(PlayerController p)
    {
        if (!p.HasMoveInput)
            return StateId.Idle;
        if (Input.IsActionPressed(InputActions.Sprint))
            return StateId.Sprint;
        return p.MoveInput.Length() < p.Tuning.WalkInputThreshold ? StateId.Walk : StateId.Run;
    }

    public static StateId? Next(PlayerController p, StateId current)
    {
        if (p.Buffer.Consume(InputActions.Crouch))
            return StateId.Crouch;
        var pick = Pick(p);
        return pick == current ? null : pick;
    }
}

public sealed class IdleState : PlayerState
{
    public override StateId Id => StateId.Idle;
    public override StateId? CheckTransition() => Locomotion.Next(Player, Id);
    public override void PhysicsUpdate(float delta) => Player.MoveOnGround(0f, delta);
}

public sealed class WalkState : PlayerState
{
    public override StateId Id => StateId.Walk;
    public override StateId? CheckTransition() => Locomotion.Next(Player, Id);
    public override void PhysicsUpdate(float delta) => Player.MoveOnGround(Player.Tuning.WalkSpeed, delta);
}

public sealed class RunState : PlayerState
{
    public override StateId Id => StateId.Run;
    public override StateId? CheckTransition() => Locomotion.Next(Player, Id);
    public override void PhysicsUpdate(float delta) => Player.MoveOnGround(Player.Tuning.RunSpeed, delta);
}

public sealed class SprintState : PlayerState
{
    public override StateId Id => StateId.Sprint;
    public override StateId? CheckTransition() => Locomotion.Next(Player, Id);
    public override void PhysicsUpdate(float delta) => Player.MoveOnGround(Player.Tuning.SprintSpeed, delta);
}

/// <summary>
/// Agachado (furtividade). Alterna com o botão de agachar; correr também levanta.
/// A cápsula encolhe, e só volta a crescer se houver espaço acima da cabeça.
/// </summary>
public sealed class CrouchState : PlayerState
{
    public override StateId Id => StateId.Crouch;

    public override void Enter(StateId previous) => Player.SetCrouching(true);

    public override void Exit(StateId next) => Player.SetCrouching(false);

    public override StateId? CheckTransition()
    {
        var wantsUp = Player.Buffer.IsBuffered(InputActions.Crouch) || Input.IsActionPressed(InputActions.Sprint);
        if (!wantsUp || !Player.CanStandUp())
            return null;
        Player.Buffer.Consume(InputActions.Crouch);
        return Locomotion.Pick(Player);
    }

    public override void PhysicsUpdate(float delta) =>
        Player.MoveOnGround(Player.HasMoveInput ? Player.Tuning.CrouchSpeed : 0f, delta);
}
