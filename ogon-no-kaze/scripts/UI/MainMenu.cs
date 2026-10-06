using Godot;
using OgonNoKaze.Core;

namespace OgonNoKaze.UI;

/// <summary>
/// Menu principal. A abertura é uma pequena sequência: a pincelada de tinta é "pintada"
/// de cima para baixo, o título aparece, o carimbo bate e por fim os botões surgem.
/// Qualquer tecla/clique durante a abertura pula direto para o estado final.
/// </summary>
public partial class MainMenu : Node
{
    private static readonly StringName ProgressParam = "progress";

    [Export(PropertyHint.File, "*.tscn")] public string WorldScenePath { get; set; } = "res://scenes/world/World.tscn";

    [ExportGroup("Nós")]
    [Export] public Control MenuRoot { get; set; }
    [Export] public ColorRect TitleStroke { get; set; }
    [Export] public Control TitleKanji { get; set; }
    [Export] public Control TitleLatin { get; set; }
    [Export] public Control Seal { get; set; }
    [Export] public Control Buttons { get; set; }
    [Export] public Button PlayButton { get; set; }
    [Export] public Button SettingsButton { get; set; }
    [Export] public Button QuitButton { get; set; }
    [Export] public SettingsMenu Settings { get; set; }

    [ExportGroup("Abertura (segundos)")]
    [Export] public float StrokeDelay { get; set; } = 0.9f;
    [Export] public float StrokeDuration { get; set; } = 1.4f;
    [Export] public float TitleDuration { get; set; } = 1.2f;
    [Export] public float SealDuration { get; set; } = 0.35f;
    [Export] public float ButtonsDuration { get; set; } = 0.8f;
    /// <summary>Pula a abertura (capturas de tela).</summary>
    [Export] public bool SkipIntro { get; set; }

    private Tween _intro;

    public override void _Ready()
    {
        Input.MouseMode = Input.MouseModeEnum.Visible;

        PlayButton.Pressed += OnPlayPressed;
        SettingsButton.Pressed += OpenSettings;
        QuitButton.Pressed += () => SceneLoader.Instance.QuitGame();
        Settings.Closed += CloseSettings;

        // Sem botão "Sair" em plataformas onde o jogo não deve se fechar sozinho.
        QuitButton.Visible = !OS.HasFeature("web");

        PlayIntro();
    }

    private void PlayIntro()
    {
        SetStroke(0f);
        TitleKanji.Modulate = Colors.Transparent;
        TitleLatin.Modulate = Colors.Transparent;
        Seal.Modulate = Colors.Transparent;
        Buttons.Modulate = Colors.Transparent;

        _intro = CreateTween();
        _intro.TweenInterval(StrokeDelay);
        _intro.TweenMethod(Callable.From<float>(SetStroke), 0f, 1f, StrokeDuration)
            .SetTrans(Tween.TransitionType.Sine).SetEase(Tween.EaseType.InOut);
        _intro.TweenProperty(TitleKanji, "modulate", Colors.White, TitleDuration)
            .SetTrans(Tween.TransitionType.Sine);
        _intro.Parallel().TweenProperty(TitleLatin, "modulate", Colors.White, TitleDuration)
            .SetDelay(TitleDuration * 0.4f);
        // O carimbo "bate": aparece já um pouco maior e encolhe para o tamanho certo.
        _intro.TweenCallback(Callable.From(() => Seal.Scale = Vector2.One * 1.35f));
        _intro.TweenProperty(Seal, "modulate", Colors.White, SealDuration * 0.5f);
        _intro.Parallel().TweenProperty(Seal, "scale", Vector2.One, SealDuration)
            .SetTrans(Tween.TransitionType.Back).SetEase(Tween.EaseType.Out);
        _intro.TweenProperty(Buttons, "modulate", Colors.White, ButtonsDuration);
        _intro.TweenCallback(Callable.From(FocusDefault));

        if (SkipIntro)
            FinishIntro();
    }

    private void FinishIntro()
    {
        if (_intro == null || !_intro.IsValid())
            return;
        // CustomStep com um valor enorme avança a sequência inteira até o fim.
        _intro.CustomStep(1000);
        _intro = null;
    }

    public override void _UnhandledInput(InputEvent e)
    {
        if (_intro == null || !_intro.IsRunning())
            return;
        if (e is InputEventKey { Pressed: true } or InputEventMouseButton { Pressed: true } or InputEventJoypadButton { Pressed: true })
        {
            FinishIntro();
            GetViewport().SetInputAsHandled();
        }
    }

    private void SetStroke(float value) =>
        (TitleStroke.Material as ShaderMaterial)?.SetShaderParameter(ProgressParam, value);

    private void FocusDefault() => PlayButton.GrabFocus();

    private void OnPlayPressed()
    {
        SetButtonsEnabled(false);
        SceneLoader.Instance.ChangeScene(WorldScenePath);
    }

    private void OpenSettings()
    {
        FinishIntro();
        var t = CreateTween();
        t.TweenProperty(MenuRoot, "modulate:a", 0f, 0.25f);
        t.TweenCallback(Callable.From(() =>
        {
            MenuRoot.Visible = false;
            Settings.Open();
        }));
    }

    private void CloseSettings()
    {
        MenuRoot.Visible = true;
        CreateTween().TweenProperty(MenuRoot, "modulate:a", 1f, 0.3f);
        SettingsButton.GrabFocus();
    }

    private void SetButtonsEnabled(bool enabled)
    {
        PlayButton.Disabled = !enabled;
        SettingsButton.Disabled = !enabled;
        QuitButton.Disabled = !enabled;
    }
}
