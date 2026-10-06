using Godot;
using OgonNoKaze.Core;

namespace OgonNoKaze.UI;

/// <summary>
/// Menu de pausa do mundo (Esc / Start). Pausa a árvore de cena — tudo com
/// ProcessMode "Pausable" congela — e libera o mouse enquanto aberto.
/// </summary>
public partial class PauseMenu : CanvasLayer
{
    [Export(PropertyHint.File, "*.tscn")] public string MainMenuScenePath { get; set; } = "res://scenes/menu/MainMenu.tscn";

    [Export] public Control Root { get; set; }
    [Export] public Control MenuPanel { get; set; }
    [Export] public Button ResumeButton { get; set; }
    [Export] public Button SettingsButton { get; set; }
    [Export] public Button MainMenuButton { get; set; }
    [Export] public Button QuitButton { get; set; }
    [Export] public SettingsMenu Settings { get; set; }

    public bool IsOpen => Root.Visible;

    public override void _Ready()
    {
        ProcessMode = ProcessModeEnum.Always;
        Root.Visible = false;
        ResumeButton.Pressed += Close;
        SettingsButton.Pressed += OpenSettings;
        MainMenuButton.Pressed += () => SceneLoader.Instance.ChangeScene(MainMenuScenePath, showLoadingScreen: false);
        QuitButton.Pressed += () => SceneLoader.Instance.QuitGame();
        Settings.Closed += OnSettingsClosed;
    }

    public override void _UnhandledInput(InputEvent e)
    {
        if (SceneLoader.Instance?.IsBusy == true || Settings.Visible)
            return;
        if (e.IsActionPressed(InputActions.Pause) || (IsOpen && e.IsActionPressed("ui_cancel")))
        {
            if (IsOpen) Close(); else Open();
            GetViewport().SetInputAsHandled();
        }
    }

    public void Open()
    {
        GetTree().Paused = true;
        Input.MouseMode = Input.MouseModeEnum.Visible;
        Root.Visible = true;
        MenuPanel.Visible = true;
        ResumeButton.GrabFocus();
    }

    public void Close()
    {
        Root.Visible = false;
        GetTree().Paused = false;
        Input.MouseMode = Input.MouseModeEnum.Captured;
    }

    private void OpenSettings()
    {
        MenuPanel.Visible = false;
        Settings.Open();
    }

    private void OnSettingsClosed()
    {
        MenuPanel.Visible = true;
        SettingsButton.GrabFocus();
    }
}
