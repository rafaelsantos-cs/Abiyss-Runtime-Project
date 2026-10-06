using Godot;

namespace OgonNoKaze.Settings;

/// <summary>Estado completo das configurações do jogador (o que vai para o settings.cfg).</summary>
public sealed class GameSettings
{
    public VideoSettings Video { get; } = new();
    public AudioSettings Audio { get; } = new();
    public ControlSettings Controls { get; } = new();
}

public sealed class VideoSettings
{
    public Vector2I Resolution { get; set; } = new(1600, 900);
    public WindowModeOption WindowMode { get; set; } = WindowModeOption.Borderless;
    public VSyncOption VSync { get; set; } = VSyncOption.On;
    /// <summary>0 = sem limite.</summary>
    public int MaxFps { get; set; } = 0;
    /// <summary>Campo de visão vertical da câmera, em graus.</summary>
    public float Fov { get; set; } = 70f;
    public QualityPresetId Preset { get; set; } = QualityPresetId.Medium;
    /// <summary>Opções gráficas atuais; uma cópia do preset ou valores personalizados.</summary>
    public QualitySettings Quality { get; set; } = new();
}

public sealed class AudioSettings
{
    // Volumes lineares de 0 a 1 (convertidos para dB ao aplicar).
    public float Master { get; set; } = 0.8f;
    public float Music { get; set; } = 0.7f;
    public float Sfx { get; set; } = 0.8f;
    public float Ambient { get; set; } = 0.8f;
}

public sealed class ControlSettings
{
    /// <summary>Graus de rotação da câmera por pixel de mouse.</summary>
    public float MouseSensitivity { get; set; } = 0.12f;
    /// <summary>Graus por segundo com o analógico no máximo.</summary>
    public float GamepadSensitivity { get; set; } = 180f;
    public bool InvertY { get; set; } = false;
}
