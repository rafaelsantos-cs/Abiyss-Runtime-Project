using System.Threading.Tasks;
using Godot;
using OgonNoKaze.UI;

namespace OgonNoKaze.Core;

/// <summary>
/// Autoload (scenes/ui/SceneLoader.tscn) que troca de cena com transição de tinta e,
/// opcionalmente, tela de carregamento.
///
/// O carregamento usa ResourceLoader.LoadThreadedRequest: o .tscn e suas dependências
/// são lidos numa thread separada enquanto o jogo continua desenhando a tela de
/// carregamento, então a janela nunca "trava". Só a instanciação final acontece na
/// thread principal (a árvore de cena não é thread-safe).
/// </summary>
public partial class SceneLoader : Node
{
    public static SceneLoader Instance { get; private set; }

    [Export] public InkTransition Transition { get; set; }
    [Export] public LoadingScreen Loading { get; set; }
    /// <summary>Tempo mínimo na tela de carregamento, para ela não piscar em cargas rápidas.</summary>
    [Export(PropertyHint.Range, "0,5,0.1,suffix:s")] public float MinLoadingSeconds { get; set; } = 1.2f;

    /// <summary>Desliga as animações de transição (usado pelas ferramentas de captura).</summary>
    public bool SkipTransitions { get; set; }
    public bool IsBusy { get; private set; }

    public override void _EnterTree()
    {
        Instance = this;
        ProcessMode = ProcessModeEnum.Always;
    }

    public override void _Ready()
    {
        // O jogo abre com a tela coberta e a cena inicial é revelada logo depois.
        Transition.Progress = 1f;
        CallDeferred(MethodName.RevealInitialScene);
    }

    private async void RevealInitialScene()
    {
        // Dois frames para a cena inicial montar shaders e layout antes de aparecer.
        await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
        await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
        if (!IsBusy)
            await Transition.RevealAsync(SkipTransitions);
    }

    /// <summary>Troca para a cena em <paramref name="path"/>.</summary>
    public async void ChangeScene(string path, bool showLoadingScreen = true)
    {
        if (IsBusy)
            return;
        IsBusy = true;
        var tree = GetTree();

        await Transition.CoverAsync(SkipTransitions);
        tree.Paused = false;
        Input.MouseMode = Input.MouseModeEnum.Visible;

        // Libera a cena antiga antes de carregar a nova, para não ter as duas na memória.
        tree.CurrentScene?.QueueFree();
        await ToSignal(tree, SceneTree.SignalName.ProcessFrame);

        if (showLoadingScreen)
        {
            Loading.Begin();
            await Transition.RevealAsync(SkipTransitions);
        }

        var scene = await LoadThreaded(path, showLoadingScreen);
        if (scene == null)
        {
            GD.PushError($"Falha ao carregar a cena {path}.");
            Loading.End();
            await Transition.RevealAsync(SkipTransitions);
            IsBusy = false;
            return;
        }

        if (showLoadingScreen)
        {
            await Transition.CoverAsync(SkipTransitions);
            Loading.End();
        }

        var instance = scene.Instantiate();
        tree.Root.AddChild(instance);
        tree.CurrentScene = instance;
        await ToSignal(tree, SceneTree.SignalName.ProcessFrame);

        IsBusy = false;
        await Transition.RevealAsync(SkipTransitions);
    }

    private async Task<PackedScene> LoadThreaded(string path, bool reportProgress)
    {
        var err = ResourceLoader.LoadThreadedRequest(path, "PackedScene", useSubThreads: true);
        if (err != Error.Ok)
            return null;

        var tree = GetTree();
        var progress = new Godot.Collections.Array();
        var elapsed = 0.0;
        var startMs = Time.GetTicksMsec();

        while (true)
        {
            await ToSignal(tree, SceneTree.SignalName.ProcessFrame);
            elapsed = (Time.GetTicksMsec() - startMs) / 1000.0;
            var status = ResourceLoader.LoadThreadedGetStatus(path, progress);

            if (status is ResourceLoader.ThreadLoadStatus.Failed or ResourceLoader.ThreadLoadStatus.InvalidResource)
                return null;

            var loaded = status == ResourceLoader.ThreadLoadStatus.Loaded;
            if (reportProgress)
            {
                // Mostra no máximo a fração do tempo mínimo já passada, para a barra
                // não chegar a 100% instantaneamente em cargas rápidas.
                var real = loaded ? 1f : (progress.Count > 0 ? (float)progress[0] : 0f);
                var timeCap = MinLoadingSeconds > 0 ? (float)(elapsed / MinLoadingSeconds) : 1f;
                Loading.TargetProgress = Mathf.Min(real, timeCap);
            }

            var done = loaded && (!reportProgress || (elapsed >= MinLoadingSeconds && Loading.VisuallyComplete));
            if (done || (loaded && SkipTransitions))
                return ResourceLoader.LoadThreadedGet(path) as PackedScene;
        }
    }

    /// <summary>Cobre a tela com tinta e fecha o jogo.</summary>
    public async void QuitGame()
    {
        if (IsBusy)
            return;
        IsBusy = true;
        await Transition.CoverAsync(SkipTransitions);
        GetTree().Quit();
    }
}
