using System;
using System.Linq;
using Godot;
using OgonNoKaze.Core;
using OgonNoKaze.Settings;

namespace OgonNoKaze.Tools;

/// <summary>
/// Base das ferramentas de linha de comando (captura e desempenho): lê argumentos,
/// aplica um preset gráfico só em memória, carrega a cena-alvo e posiciona a câmera.
///
/// Argumentos comuns:
///   --scene=res://...           cena a carregar (obrigatório, exceto em --loading)
///   --graphics=low|medium|high|ultra   preset gráfico (padrão: o salvo pelo jogador)
///   --size=1920x1080            tamanho da janela
///   --camera=nome               Marker3D/Camera3D do grupo "capture_point" na cena
///   --set=caminho:Prop=valor    altera uma propriedade antes de a cena entrar na árvore
///                               (caminho relativo à raiz da cena; "." = a raiz)
/// </summary>
public abstract partial class ToolRunner : Node
{
    public const string CapturePointGroup = "capture_point";

    protected CmdArgs Args { get; private set; }
    protected Node Target { get; private set; }
    protected string GraphicsLabel { get; private set; } = "saved";

    public override void _Ready()
    {
        Args = CmdArgs.FromOs();

        var settings = SettingsManager.Instance;
        settings.PersistenceEnabled = false;
        if (SceneLoader.Instance != null)
        {
            SceneLoader.Instance.SkipTransitions = true;
            SceneLoader.Instance.Transition.Progress = 0f;
        }

        var video = settings.Current.Video;
        if (Args.Get("graphics") is { } g)
        {
            if (Enum.TryParse<QualityPresetId>(g, ignoreCase: true, out var preset) && preset != QualityPresetId.Custom)
            {
                settings.SelectPreset(preset);
                GraphicsLabel = preset.ToString().ToLowerInvariant();
            }
            else
            {
                Fail($"--graphics inválido: {g} (use low, medium, high ou ultra)");
                return;
            }
        }
        if (Args.GetSize("size") is { } size)
        {
            video.WindowMode = WindowModeOption.Windowed;
            video.Resolution = size;
        }
        else
        {
            // Ferramentas sempre em janela, para não brigar com tela cheia do jogador.
            video.WindowMode = WindowModeOption.Windowed;
        }
        ConfigureSettings(settings);
        settings.ApplyAll();

        // Adia a montagem para o próximo frame: o autoload e a janela terminam de se ajustar.
        CallDeferred(MethodName.Begin);
    }

    /// <summary>Último ajuste nas configurações antes de aplicar (ex.: desligar VSync).</summary>
    protected virtual void ConfigureSettings(SettingsManager settings) { }

    /// <summary>Chamado com a cena-alvo já na árvore e a câmera posicionada.</summary>
    protected abstract void Run();

    private void Begin()
    {
        if (!Args.Has("scene") && AllowsNoScene)
        {
            Run();
            return;
        }

        var path = Args.Get("scene");
        if (string.IsNullOrEmpty(path))
        {
            Fail("Falta --scene=res://caminho/da/cena.tscn");
            return;
        }
        var packed = GD.Load<PackedScene>(path);
        if (packed == null)
        {
            Fail($"Não foi possível carregar {path}");
            return;
        }

        Target = packed.Instantiate();
        foreach (var assignment in Args.GetAll("set"))
        {
            if (!ApplySet(Target, assignment))
                return;
        }
        GetTree().Root.AddChild(Target);
        GetTree().CurrentScene = Target;

        if (Args.Get("camera") is { } cameraName && !PlaceCamera(cameraName))
            return;

        Run();
    }

    /// <summary>Se true, a ferramenta funciona sem --scene (ex.: prévia da tela de carregamento).</summary>
    protected virtual bool AllowsNoScene => false;

    private bool ApplySet(Node root, string assignment)
    {
        // Formato: caminho:Propriedade=valor
        var colon = assignment.IndexOf(':');
        var eq = assignment.IndexOf('=', Math.Max(colon, 0));
        if (colon < 0 || eq < 0)
        {
            Fail($"--set mal formado: {assignment} (use caminho:Propriedade=valor)");
            return false;
        }
        var nodePath = assignment[..colon];
        var property = assignment[(colon + 1)..eq];
        var rawValue = assignment[(eq + 1)..];
        var node = nodePath == "." ? root : root.GetNodeOrNull(nodePath);
        if (node == null)
        {
            Fail($"--set: nó '{nodePath}' não encontrado");
            return false;
        }
        // Tenta interpretar como Variant do Godot (número, bool, Vector3(...)); senão, texto.
        var parsed = GD.StrToVar(rawValue);
        node.Set(property, parsed.VariantType == Variant.Type.Nil ? rawValue : parsed);
        return true;
    }

    private bool PlaceCamera(string name)
    {
        var point = GetTree().GetNodesInGroup(CapturePointGroup)
            .OfType<Node3D>()
            .FirstOrDefault(n => n.Name == name);
        if (point == null)
        {
            var available = string.Join(", ", GetTree().GetNodesInGroup(CapturePointGroup).Select(n => n.Name.ToString()));
            Fail($"Marcador de câmera '{name}' não existe. Disponíveis: {available}");
            return false;
        }

        if (point is Camera3D existing)
        {
            existing.MakeCurrent();
            return true;
        }
        var camera = new Camera3D
        {
            Name = "ToolCamera",
            Fov = SettingsManager.Instance.Current.Video.Fov,
        };
        Target.AddChild(camera);
        camera.GlobalTransform = point.GlobalTransform;
        camera.MakeCurrent();
        return true;
    }

    /// <summary>Caminho de saída: absoluto, res://, user:// ou relativo à pasta do projeto.</summary>
    protected static string ResolveOutputPath(string path)
    {
        if (path.StartsWith("res://") || path.StartsWith("user://"))
            return ProjectSettings.GlobalizePath(path);
        if (System.IO.Path.IsPathRooted(path))
            return path;
        return System.IO.Path.Combine(ProjectSettings.GlobalizePath("res://"), path);
    }

    protected void Fail(string message)
    {
        GD.PrintErr($"[{GetType().Name}] {message}");
        GetTree().Quit(1);
    }

    protected void Succeed(string message)
    {
        GD.Print($"[{GetType().Name}] {message}");
        GetTree().Quit(0);
    }
}
