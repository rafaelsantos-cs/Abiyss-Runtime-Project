using Godot;

namespace OgonNoKaze.Combat;

/// <summary>
/// Janela (em frames de física, 60/s) em que a ação pode ser interrompida por outras.
/// Ex.: um golpe pode virar deflexão entre os frames 2 e 5 ("feint"), e virar
/// qualquer coisa nos últimos frames da recuperação.
/// </summary>
[GlobalClass]
public partial class CancelWindow : Resource
{
    [Export] public int FromFrame { get; set; }
    [Export] public int ToFrame { get; set; }
    [Export] public ActionCancelMask Into { get; set; } = ActionCancelMask.None;

    public bool Contains(int frame) => frame >= FromFrame && frame <= ToFrame;
}
