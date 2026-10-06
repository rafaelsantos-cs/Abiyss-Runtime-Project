using OgonNoKaze.Player.StateMachine;

namespace OgonNoKaze.Player.States;

// Estrutura da travessia vertical. A lógica entra na Fase 4 (gancho com GrapplePoint,
// detecção de borda, escalada com root motion). Por enquanto, se algo ativar um
// destes estados, ele devolve o controle para a queda no tick seguinte.

/// <summary>Pai de gancho, pendurar e escalar: movimento guiado, sem gravidade comum.</summary>
public sealed class TraversalState : PlayerState
{
    public override StateId Id => StateId.Traversal;
    public override StateId? DefaultChild => StateId.Grapple;
}

/// <summary>Kaginawa: puxão em curva até o ponto de agarre e aterrissagem em root motion.</summary>
public sealed class GrappleState : PlayerState
{
    public override StateId Id => StateId.Grapple;
    public override StateId? CheckTransition() => StateId.Fall;
}

/// <summary>Pendurado numa borda: desloca lateralmente, sobe (Climb) ou solta (Fall).</summary>
public sealed class LedgeHangState : PlayerState
{
    public override StateId Id => StateId.LedgeHang;
    public override StateId? CheckTransition() => StateId.Fall;
}

/// <summary>Subida da borda para cima da plataforma, guiada por root motion.</summary>
public sealed class ClimbState : PlayerState
{
    public override StateId Id => StateId.Climb;
    public override StateId? CheckTransition() => StateId.Fall;
}
