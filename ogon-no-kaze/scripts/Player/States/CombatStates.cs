using OgonNoKaze.Combat;
using OgonNoKaze.Player.StateMachine;

namespace OgonNoKaze.Player.States;

// Combate: só a estrutura (a jogabilidade depende da decisão pendente no CLAUDE.md).
// Nenhum input leva a estes estados ainda.

/// <summary>Pai dos estados de combate.</summary>
public sealed class CombatState : PlayerState
{
    public override StateId Id => StateId.Combat;
    public override StateId? DefaultChild => StateId.Guard;
}

/// <summary>
/// Base para estados dirigidos por <see cref="ActionData"/>: conta frames de física,
/// expõe a fase (início/ativo/recuperação) e as janelas de cancelamento.
/// Quando a ação termina, volta para o chão.
/// </summary>
public abstract class ActionState : PlayerState
{
    protected abstract ActionData Data { get; }

    public int Frame { get; private set; }
    public ActionPhase Phase => Data?.PhaseAt(Frame) ?? ActionPhase.Finished;

    public bool CanCancelInto(ActionCancelMask what) => Data != null && Data.CanCancelInto(Frame, what);

    public override void Enter(StateId previous) => Frame = 0;

    public override StateId? CheckTransition() => Phase == ActionPhase.Finished ? StateId.Grounded : null;

    public override void PhysicsUpdate(float delta)
    {
        Frame++;
        var speed = Data == null ? 0f : Player.Tuning.RunSpeed * Data.MoveSpeedFactor;
        Player.MoveOnGround(Player.HasMoveInput ? speed : 0f, delta);
        Player.ApplyGravity(1f, delta);
    }
}

public sealed class AttackState : ActionState
{
    public override StateId Id => StateId.Attack;
    protected override ActionData Data => Player.Actions?.Attack;
}

public sealed class DeflectState : ActionState
{
    public override StateId Id => StateId.Deflect;
    protected override ActionData Data => Player.Actions?.Deflect;
}

/// <summary>Guarda mantida (segurando defender depois da janela de deflexão).</summary>
public sealed class GuardState : PlayerState
{
    public override StateId Id => StateId.Guard;
    public override StateId? CheckTransition() => StateId.Grounded;
}

/// <summary>Atordoado após receber golpe.</summary>
public sealed class StaggerState : ActionState
{
    public override StateId Id => StateId.Stagger;
    protected override ActionData Data => Player.Actions?.Stagger;
}

/// <summary>Postura quebrada: vulnerável à execução.</summary>
public sealed class PostureBrokenState : PlayerState
{
    public override StateId Id => StateId.PostureBroken;
    public override StateId? CheckTransition() => StateId.Grounded;
}

/// <summary>Execução (golpe de misericórdia) aplicada num inimigo de postura quebrada.</summary>
public sealed class DeathblowState : PlayerState
{
    public override StateId Id => StateId.Deathblow;
    public override StateId? CheckTransition() => StateId.Grounded;
}

public sealed class DeathState : PlayerState
{
    public override StateId Id => StateId.Death;
}
