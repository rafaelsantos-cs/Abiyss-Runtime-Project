using System;
using System.Threading.Tasks;
using Godot;
using OgonNoKaze.Core;
using OgonNoKaze.Player;
using OgonNoKaze.Player.StateMachine;
using OgonNoKaze.Settings;

namespace OgonNoKaze.Tools;

/// <summary>
/// Teste de fumaça do controlador: carrega o mundo, simula input e confere a máquina de
/// estados. Não mede "sensação" (isso é o Rafael jogando), mas pega regressões de lógica.
///
///   godot --headless --path . res://tools/tests/PlayerSmokeTest.tscn
///
/// Sai com código 0 se tudo passou, 1 se algo falhou.
/// </summary>
public partial class PlayerSmokeTest : Node
{
    private const string WorldPath = "res://scenes/world/World.tscn";
    /// <summary>
    /// Continuações de await do C# rodam fora do passo de física, então um input simulado
    /// pode só ser visto pelo jogador no tick seguinte. Esperar 2 ticks absorve isso.
    /// </summary>
    private const int InputLatencyTicks = 2;

    private PlayerController _player;
    private int _failures;

    public override void _Ready()
    {
        SettingsManager.Instance.PersistenceEnabled = false;
        if (SceneLoader.Instance != null)
        {
            SceneLoader.Instance.SkipTransitions = true;
            SceneLoader.Instance.Transition.Progress = 0f;
        }
        // Roda antes do jogador em cada tick, para o "acabou de apertar" valer no mesmo tick.
        ProcessPhysicsPriority = -1000;
        _ = Run();
    }

    private async Task Run()
    {
        try
        {
            // A raiz está ocupada montando os filhos durante _Ready: espera um tick.
            await Ticks(1);
            var world = GD.Load<PackedScene>(WorldPath).Instantiate();
            GetTree().Root.AddChild(world);
            _player = world.GetNode<PlayerController>("Player");
            var t = _player.Tuning;

            await Ticks(45);
            Check("começa parado no chão", _player.States.IsIn(StateId.Idle));

            // Correr para a frente (câmera olhando para -Z).
            var z0 = _player.GlobalPosition.Z;
            Input.ActionPress(InputActions.MoveForward);
            await Ticks(60);
            Check("segurar frente → Run", _player.States.IsIn(StateId.Run), _player.States.Path);
            Check("andou para -Z", _player.GlobalPosition.Z < z0 - 3f, $"Δz = {_player.GlobalPosition.Z - z0:0.00}");
            Check("velocidade de corrida", Mathf.Abs(HorizontalSpeed() - t.RunSpeed) < 0.15f, $"{HorizontalSpeed():0.00} m/s");

            Input.ActionPress(InputActions.Sprint);
            await Ticks(30);
            Check("segurar sprint → Sprint", _player.States.IsIn(StateId.Sprint), _player.States.Path);
            Check("velocidade de sprint", Mathf.Abs(HorizontalSpeed() - t.SprintSpeed) < 0.15f, $"{HorizontalSpeed():0.00} m/s");
            Input.ActionRelease(InputActions.Sprint);
            Input.ActionRelease(InputActions.MoveForward);
            await Ticks(20);
            Check("soltar → Idle", _player.States.IsIn(StateId.Idle), _player.States.Path);
            Check("parou", HorizontalSpeed() < 0.05f, $"{HorizontalSpeed():0.00} m/s");

            // Pulo segurado: altura ≈ JumpHeight.
            var y0 = _player.GlobalPosition.Y;
            Input.ActionPress(InputActions.Jump);
            await Ticks(InputLatencyTicks);
            Check("apertar pulo → Jump", _player.States.IsIn(StateId.Jump), _player.States.Path);
            var apex = await TrackApexUntilGrounded(releaseJumpAfterTicks: 60);
            Check("altura do pulo segurado", Mathf.Abs(apex - y0 - t.JumpHeight) < 0.12f, $"{apex - y0:0.00} m (alvo {t.JumpHeight:0.00})");

            // Pulo tocado: o corte de subida deixa o pulo bem mais baixo.
            await Ticks(10);
            Input.ActionPress(InputActions.Jump);
            var tapApex = await TrackApexUntilGrounded(releaseJumpAfterTicks: 3);
            Check("pulo curto é mais baixo", tapApex - y0 < t.JumpHeight * 0.6f, $"{tapApex - y0:0.00} m");

            // Buffer: apertar pulo pouco antes de tocar o chão pula de novo ao aterrissar.
            await Ticks(10);
            Input.ActionPress(InputActions.Jump);
            await Ticks(2);
            Input.ActionRelease(InputActions.Jump);
            await WaitUntil(() => _player.Velocity.Y < 0f && _player.GlobalPosition.Y < y0 + 0.25f, 120);
            Tap(InputActions.Jump);
            var rejumped = await WaitUntil(() => _player.States.IsIn(StateId.Jump) && _player.Velocity.Y > 0f, 20);
            Check("pulo no buffer sai ao aterrissar", rejumped, _player.States.Path);
            await WaitUntil(() => _player.States.IsIn(StateId.Grounded), 120);

            // Agachar alterna e encolhe a cápsula.
            await Ticks(5);
            Tap(InputActions.Crouch);
            await Ticks(2);
            Check("agachar → Crouch", _player.States.IsIn(StateId.Crouch), _player.States.Path);
            Check("visibilidade reduzida agachado", _player.VisibilityFactor < 1f);
            await Ticks(5);
            Tap(InputActions.Crouch);
            await Ticks(2);
            Check("agachar de novo → em pé", _player.States.IsIn(StateId.Idle), _player.States.Path);

            await CoyoteTest(t);
        }
        catch (Exception ex)
        {
            GD.PrintErr($"[PlayerSmokeTest] exceção: {ex}");
            _failures++;
        }
        finally
        {
            ReleaseAll();
        }

        GD.Print(_failures == 0 ? "[PlayerSmokeTest] OK" : $"[PlayerSmokeTest] {_failures} falha(s)");
        GetTree().Quit(_failures == 0 ? 0 : 1);
    }

    /// <summary>Plataforma isolada: sai pela borda e testa o pulo atrasado dentro e fora da janela.</summary>
    private async Task CoyoteTest(MovementTuning t)
    {
        var platform = new StaticBody3D { Position = new Vector3(60f, 2f, 0f) };
        platform.AddChild(new CollisionShape3D { Shape = new BoxShape3D { Size = new Vector3(4f, 0.4f, 4f) } });
        GetTree().Root.AddChild(platform);

        foreach (var (delayTicks, shouldJump) in new[] { (0, true), (2, true), (Mathf.RoundToInt(t.CoyoteTime * 60f) + 4, false) })
        {
            _player.GlobalPosition = new Vector3(60f, 2.25f, 0f);
            _player.Velocity = Vector3.Zero;
            await WaitUntil(() => _player.States.IsIn(StateId.Grounded), 60);
            Input.ActionPress(InputActions.MoveRight);
            var left = await WaitUntil(() => _player.States.IsIn(StateId.Fall), 120);
            Input.ActionRelease(InputActions.MoveRight);
            var leftAt = Engine.GetPhysicsFrames();
            await Ticks(delayTicks);
            var pressedAt = Engine.GetPhysicsFrames();
            Tap(InputActions.Jump);
            await Ticks(InputLatencyTicks);
            var jumped = _player.States.IsIn(StateId.Jump);
            Check(shouldJump ? $"coyote: pulo {delayTicks} ticks após a borda" : $"coyote: sem pulo {delayTicks} ticks após a borda",
                left && jumped == shouldJump, $"{_player.States.Path}, apertou {pressedAt - leftAt} ticks após detectar a queda");
            await WaitUntil(() => _player.States.IsIn(StateId.Grounded), 180);
        }
        platform.QueueFree();
    }

    // ------------------------------------------------------------ utilidades

    private float HorizontalSpeed() => new Vector2(_player.Velocity.X, _player.Velocity.Z).Length();

    private async Task<float> TrackApexUntilGrounded(int releaseJumpAfterTicks)
    {
        var apex = _player.GlobalPosition.Y;
        for (var i = 0; i < 240; i++)
        {
            await Ticks(1);
            if (i == releaseJumpAfterTicks)
                Input.ActionRelease(InputActions.Jump);
            apex = Mathf.Max(apex, _player.GlobalPosition.Y);
            if (i > 2 && _player.States.IsIn(StateId.Grounded))
                break;
        }
        Input.ActionRelease(InputActions.Jump);
        return apex;
    }

    private async void Tap(StringName action)
    {
        Input.ActionPress(action);
        await Ticks(1);
        Input.ActionRelease(action);
    }

    private async Task<bool> WaitUntil(Func<bool> condition, int maxTicks)
    {
        for (var i = 0; i < maxTicks; i++)
        {
            if (condition())
                return true;
            await Ticks(1);
        }
        return condition();
    }

    private async Task Ticks(int count)
    {
        for (var i = 0; i < count; i++)
            await ToSignal(GetTree(), SceneTree.SignalName.PhysicsFrame);
    }

    private void Check(string name, bool ok, string detail = "")
    {
        GD.Print($"  [{(ok ? "ok" : "FALHOU")}] {name}{(detail.Length > 0 ? $" ({detail})" : "")}");
        if (!ok)
            _failures++;
    }

    private static void ReleaseAll()
    {
        foreach (var action in new[] { InputActions.MoveForward, InputActions.MoveRight, InputActions.Sprint, InputActions.Jump, InputActions.Crouch })
            Input.ActionRelease(action);
    }
}
