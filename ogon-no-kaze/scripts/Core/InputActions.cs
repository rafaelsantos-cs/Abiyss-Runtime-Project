using Godot;

namespace OgonNoKaze.Core;

/// <summary>
/// Nomes das ações do InputMap (definidas em project.godot) e metadados para a tela de
/// remapeamento. Usar estas constantes em vez de strings soltas evita erro de digitação.
/// </summary>
public static class InputActions
{
    public static readonly StringName MoveForward = "move_forward";
    public static readonly StringName MoveBack = "move_back";
    public static readonly StringName MoveLeft = "move_left";
    public static readonly StringName MoveRight = "move_right";
    public static readonly StringName LookLeft = "look_left";
    public static readonly StringName LookRight = "look_right";
    public static readonly StringName LookUp = "look_up";
    public static readonly StringName LookDown = "look_down";
    public static readonly StringName Jump = "jump";
    public static readonly StringName Sprint = "sprint";
    public static readonly StringName Crouch = "crouch";
    public static readonly StringName Grapple = "grapple";
    public static readonly StringName Interact = "interact";
    public static readonly StringName Attack = "attack";
    public static readonly StringName Deflect = "deflect";
    public static readonly StringName LockOn = "lock_on";
    public static readonly StringName Pause = "pause";
    public static readonly StringName UiTabPrev = "ui_tab_prev";
    public static readonly StringName UiTabNext = "ui_tab_next";

    /// <summary>Uma ação que o jogador pode remapear, com o rótulo mostrado na tela.</summary>
    public readonly record struct Remappable(StringName Action, string Label, string Group);

    /// <summary>Ordem em que as ações aparecem na aba Controles.</summary>
    public static readonly Remappable[] RemappableActions =
    {
        new(MoveForward, "Andar para frente", "Movimento"),
        new(MoveBack, "Andar para trás", "Movimento"),
        new(MoveLeft, "Andar para a esquerda", "Movimento"),
        new(MoveRight, "Andar para a direita", "Movimento"),
        new(Jump, "Pular", "Movimento"),
        new(Sprint, "Correr (segurar)", "Movimento"),
        new(Crouch, "Agachar", "Movimento"),
        new(Grapple, "Gancho", "Movimento"),
        new(Interact, "Interagir", "Movimento"),
        new(LookLeft, "Câmera: esquerda", "Câmera"),
        new(LookRight, "Câmera: direita", "Câmera"),
        new(LookUp, "Câmera: cima", "Câmera"),
        new(LookDown, "Câmera: baixo", "Câmera"),
        new(Attack, "Atacar", "Combate (em breve)"),
        new(Deflect, "Defender / Desviar", "Combate (em breve)"),
        new(LockOn, "Travar alvo", "Combate (em breve)"),
        new(Pause, "Pausar", "Sistema"),
    };
}
