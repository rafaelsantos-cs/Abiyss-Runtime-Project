using Godot;

namespace OgonNoKaze.UI;

/// <summary>
/// Tela de carregamento: pincelada de progresso, porcentagem e uma dica. O progresso
/// mostrado persegue o real suavemente, para a barra não "pular".
/// </summary>
public partial class LoadingScreen : CanvasLayer
{
    private static readonly StringName ProgressParam = "progress";

    [Export] public ColorRect Stroke { get; set; }
    [Export] public Label PercentLabel { get; set; }
    [Export] public Label HintLabel { get; set; }
    /// <summary>Velocidade com que a barra alcança o progresso real (por segundo).</summary>
    [Export] public float FollowSpeed { get; set; } = 3f;
    [Export] public string[] Hints { get; set; } =
    {
        "Agachado no trigo alto, você some da vista de quem vigia o campo.",
        "O vento dobra o trigo em ondas: preste atenção ao rumo das rajadas.",
        "Galhos fortes e o topo do torii servem de apoio para o gancho.",
        "Segure a corrida para atravessar o campo depressa — mas o trigo farfalha.",
    };

    /// <summary>Progresso real (0–1) informado pelo carregador.</summary>
    public float TargetProgress { get; set; }
    public float DisplayedProgress { get; private set; }

    public override void _Ready()
    {
        ProcessMode = ProcessModeEnum.Always;
        Visible = false;
    }

    public void Begin()
    {
        TargetProgress = 0f;
        DisplayedProgress = 0f;
        if (HintLabel != null && Hints.Length > 0)
            HintLabel.Text = Hints[GD.RandRange(0, Hints.Length - 1)];
        UpdateVisuals();
        Visible = true;
    }

    public void End() => Visible = false;

    /// <summary>Fixa a barra num progresso, sem animação (prévias e capturas).</summary>
    public void ShowStatic(float progress)
    {
        TargetProgress = DisplayedProgress = Mathf.Clamp(progress, 0f, 1f);
        UpdateVisuals();
        Visible = true;
    }

    /// <summary>True quando a barra já chegou visualmente ao fim.</summary>
    public bool VisuallyComplete => DisplayedProgress >= 0.999f;

    public override void _Process(double delta)
    {
        if (!Visible)
            return;
        // Aproximação exponencial: rápida quando longe, suave perto do alvo.
        DisplayedProgress = Mathf.MoveToward(
            DisplayedProgress, TargetProgress,
            (float)delta * Mathf.Max(FollowSpeed * (TargetProgress - DisplayedProgress), 0.35f));
        UpdateVisuals();
    }

    private void UpdateVisuals()
    {
        (Stroke?.Material as ShaderMaterial)?.SetShaderParameter(ProgressParam, DisplayedProgress);
        if (PercentLabel != null)
            PercentLabel.Text = $"{Mathf.RoundToInt(DisplayedProgress * 100f)}%";
    }
}
