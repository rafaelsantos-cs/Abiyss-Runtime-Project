using System.IO;
using Godot;
using OgonNoKaze.Core;

namespace OgonNoKaze.Tools;

/// <summary>
/// Captura de tela automatizada. Além dos argumentos de <see cref="ToolRunner"/>:
///   --out=docs/captures/x.png   arquivo de saída (obrigatório)
///   --frames=N                  frames mínimos de espera antes de capturar (padrão 60)
///   --seconds=S                 tempo mínimo de espera, para animações (padrão 0)
///   --loading=0.6               sem --scene: mostra a tela de carregamento nesse progresso
///
/// Exemplo:
///   godot --path . res://tools/capture/CaptureRunner.tscn -- \
///     --scene=res://scenes/menu/MainMenu.tscn --out=docs/captures/menu.png --seconds=4
/// </summary>
public partial class CaptureRunner : ToolRunner
{
    private string _outPath;
    private int _framesLeft;
    private double _secondsLeft;
    private bool _capturing;

    protected override bool AllowsNoScene => Args.Has("loading");

    protected override void Run()
    {
        var output = Args.Get("out");
        if (string.IsNullOrEmpty(output))
        {
            Fail("Falta --out=caminho/da/captura.png");
            return;
        }
        _outPath = ResolveOutputPath(output);
        _framesLeft = Args.GetInt("frames", 60);
        _secondsLeft = Args.GetFloat("seconds", 0f);

        if (Args.Has("loading") && SceneLoader.Instance != null)
        {
            var loading = SceneLoader.Instance.Loading;
            loading.Begin();
            loading.ShowStatic(Args.GetFloat("loading", 0.6f));
        }

        SetProcess(true);
    }

    public override void _EnterTree() => SetProcess(false);

    public override void _Process(double delta)
    {
        if (_capturing)
            return;
        _framesLeft--;
        _secondsLeft -= delta;
        if (_framesLeft > 0 || _secondsLeft > 0)
            return;
        _capturing = true;
        Capture();
    }

    private async void Capture()
    {
        // Espera o frame atual terminar de ser desenhado antes de ler a imagem.
        await ToSignal(RenderingServer.Singleton, RenderingServer.SignalName.FramePostDraw);
        var image = GetViewport().GetTexture().GetImage();
        Directory.CreateDirectory(Path.GetDirectoryName(_outPath)!);
        var err = image.SavePng(_outPath);
        if (err != Error.Ok)
        {
            Fail($"Falha ao salvar {_outPath}: {err}");
            return;
        }
        Succeed($"Captura salva em {_outPath} ({image.GetWidth()}x{image.GetHeight()}, " +
                $"{RenderingServer.GetVideoAdapterName()})");
    }
}
