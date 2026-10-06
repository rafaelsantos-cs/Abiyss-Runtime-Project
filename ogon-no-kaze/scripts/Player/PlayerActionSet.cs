using Godot;
using OgonNoKaze.Combat;

namespace OgonNoKaze.Player;

/// <summary>As ações de combate do personagem (data/player/player_actions.tres).</summary>
[GlobalClass]
public partial class PlayerActionSet : Resource
{
    [Export] public ActionData Attack { get; set; }
    [Export] public ActionData Deflect { get; set; }
    [Export] public ActionData Stagger { get; set; }
}
