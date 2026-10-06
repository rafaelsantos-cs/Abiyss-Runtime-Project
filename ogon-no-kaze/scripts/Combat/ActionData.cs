using System;
using Godot;

namespace OgonNoKaze.Combat;

/// <summary>Em que tipo de ação uma ação em andamento pode ser cancelada.</summary>
[Flags]
public enum ActionCancelMask
{
    None = 0,
    Move = 1 << 0,
    Jump = 1 << 1,
    Attack = 1 << 2,
    Deflect = 1 << 3,
    Dodge = 1 << 4,
    Grapple = 1 << 5,
    Crouch = 1 << 6,
}

public enum ActionPhase { Startup, Active, Recovery, Finished }

/// <summary>
/// Descrição de uma ação (golpe, deflexão, aterrissagem do gancho...) por dados: os
/// valores ficam em .tres e são ajustados no inspetor, sem recompilar.
///
/// Tempo em frames de física (o jogo roda a física a 60 ticks), como em jogos de luta:
/// "6 frames de início" é sempre 100 ms, independente do FPS de renderização.
/// </summary>
[GlobalClass]
public partial class ActionData : Resource
{
    [Export] public StringName Id { get; set; } = "action";
    /// <summary>Animação tocada pela AnimationTree (Fase 4).</summary>
    [Export] public StringName AnimationName { get; set; }

    [ExportGroup("Frames")]
    [Export(PropertyHint.Range, "0,120,1")] public int StartupFrames { get; set; } = 6;
    [Export(PropertyHint.Range, "0,120,1")] public int ActiveFrames { get; set; } = 4;
    [Export(PropertyHint.Range, "0,240,1")] public int RecoveryFrames { get; set; } = 14;

    [ExportGroup("Movimento")]
    /// <summary>Usa root motion da animação em vez da velocidade do controlador.</summary>
    [Export] public bool UseRootMotion { get; set; } = true;
    /// <summary>Fração da velocidade de locomoção permitida durante a ação (0 = parado).</summary>
    [Export(PropertyHint.Range, "0,1,0.05")] public float MoveSpeedFactor { get; set; }
    /// <summary>Quanto o personagem pode girar durante o início (graus por segundo).</summary>
    [Export(PropertyHint.Range, "0,1440,10")] public float StartupTurnSpeed { get; set; } = 360f;

    [ExportGroup("Cancelamento")]
    [Export] public Godot.Collections.Array<CancelWindow> CancelWindows { get; set; } = new();
    /// <summary>Para onde a ação pode ir quando termina naturalmente (sem input = Idle).</summary>
    [Export] public ActionCancelMask AllowedAfterFinish { get; set; } = (ActionCancelMask)~0;

    public int TotalFrames => StartupFrames + ActiveFrames + RecoveryFrames;

    public ActionPhase PhaseAt(int frame)
    {
        if (frame < StartupFrames) return ActionPhase.Startup;
        if (frame < StartupFrames + ActiveFrames) return ActionPhase.Active;
        if (frame < TotalFrames) return ActionPhase.Recovery;
        return ActionPhase.Finished;
    }

    public bool CanCancelInto(int frame, ActionCancelMask what)
    {
        if (PhaseAt(frame) == ActionPhase.Finished)
            return (AllowedAfterFinish & what) != 0;
        foreach (var window in CancelWindows)
        {
            if (window != null && window.Contains(frame) && (window.Into & what) != 0)
                return true;
        }
        return false;
    }
}
