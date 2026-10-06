using Godot;

namespace OgonNoKaze.Settings;

/// <summary>
/// Tabela que traduz cada nível de qualidade (Baixo, Médio...) em valores concretos do
/// renderizador. Fica em data/settings/render_tuning.tres para ser calibrada com o medidor
/// de desempenho sem mexer em código. Cada array tem uma posição por valor do enum
/// correspondente (ex.: Shadow* tem 4 posições: Low, Medium, High, Ultra).
/// </summary>
[GlobalClass]
public partial class RenderTuning : Resource
{
    [ExportGroup("Sombras (Low, Medium, High, Ultra)")]
    [Export] public int[] ShadowDirectionalAtlasSize { get; set; } = { 1024, 2048, 4096, 4096 };
    [Export] public int[] ShadowPositionalAtlasSize { get; set; } = { 1024, 2048, 4096, 4096 };
    /// <summary>RenderingServer.ShadowQuality: 0 Hard, 1 SoftVeryLow, 2 SoftLow, 3 SoftMedium, 4 SoftHigh, 5 SoftUltra.</summary>
    [Export] public int[] ShadowFilterQuality { get; set; } = { 1, 2, 3, 4 };
    /// <summary>DirectionalLight3D.ShadowMode: 0 Orthogonal, 1 Parallel2Splits, 2 Parallel4Splits.</summary>
    [Export] public int[] ShadowSplitMode { get; set; } = { 1, 1, 2, 2 };
    [Export] public float[] ShadowMaxDistance { get; set; } = { 60f, 100f, 160f, 250f };
    /// <summary>A partir deste nível a grama projeta sombra (abaixo, só recebe).</summary>
    [Export] public ShadowQuality GrassCastsShadowsFrom { get; set; } = ShadowQuality.High;

    [ExportGroup("Névoa volumétrica (Off, Low, Medium, High)")]
    [Export] public int[] FogVolumeSize { get; set; } = { 0, 48, 64, 96 };
    [Export] public int[] FogVolumeDepth { get; set; } = { 0, 48, 64, 96 };
    /// <summary>A partir deste nível o filtro temporal da névoa fica ligado (menos ruído, custa mais).</summary>
    [Export] public FogQuality FogFilterFrom { get; set; } = FogQuality.Medium;

    [ExportGroup("SSAO (Off, Low, Medium, High)")]
    /// <summary>RenderingServer.EnvironmentSsaoQuality: 0 VeryLow, 1 Low, 2 Medium, 3 High, 4 Ultra.</summary>
    [Export] public int[] SsaoServerQuality { get; set; } = { 0, 0, 2, 3 };
    [Export] public SsaoQuality SsaoFullResolutionFrom { get; set; } = SsaoQuality.High;

    [ExportGroup("Upscaler")]
    /// <summary>Nitidez do FSR (0 = máxima, 2 = mínima), igual ao parâmetro do Godot.</summary>
    [Export(PropertyHint.Range, "0,2,0.05")] public float FsrSharpness { get; set; } = 0.2f;

    /// <summary>Lê o array na posição do nível, prendendo o índice nas bordas.</summary>
    public static T Pick<T>(T[] values, int level) =>
        values[Mathf.Clamp(level, 0, values.Length - 1)];
}
