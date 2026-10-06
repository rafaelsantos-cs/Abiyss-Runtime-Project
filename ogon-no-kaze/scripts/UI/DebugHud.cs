using Godot;
using OgonNoKaze.Player;

namespace OgonNoKaze.UI;

/// <summary>
/// Texto de depuração no canto da tela: estado atual da máquina de estados, velocidade,
/// chão/ar e FPS. F3 liga/desliga. Só existe em builds de depuração.
/// </summary>
public partial class DebugHud : Label
{
    [Export] public PlayerController Player { get; set; }
    [Export] public bool StartVisible { get; set; } = true;

    public override void _Ready()
    {
        if (!OS.IsDebugBuild())
        {
            QueueFree();
            return;
        }
        Visible = StartVisible;
    }

    public override void _UnhandledInput(InputEvent e)
    {
        if (e is InputEventKey { Pressed: true, Echo: false, PhysicalKeycode: Key.F3 })
            Visible = !Visible;
    }

    public override void _Process(double delta)
    {
        if (!Visible || Player?.States == null)
            return;
        var v = Player.Velocity;
        var horizontal = new Vector2(v.X, v.Z).Length();
        Text = $"estado: {Player.States.Path}\n" +
               $"vel. horizontal: {horizontal:0.00} m/s   vertical: {v.Y:0.00} m/s\n" +
               $"no chão: {(Player.IsOnFloor() ? "sim" : "não")}   visibilidade: {Player.VisibilityFactor:0.00}\n" +
               $"FPS: {Engine.GetFramesPerSecond():0}   (F3 esconde)";
    }
}
