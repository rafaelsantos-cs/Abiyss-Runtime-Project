using System.Threading.Tasks;
using Godot;

namespace OgonNoKaze.UI;

/// <summary>
/// Retângulo de tela cheia com o shader ink_transition: cobre e revela a tela com
/// tinta. Cover/Reveal são aguardáveis (await) para encadear com o carregamento.
/// </summary>
public partial class InkTransition : ColorRect
{
    private static readonly StringName ProgressParam = "progress";

    [Export(PropertyHint.Range, "0.1,3,0.05,suffix:s")] public float CoverSeconds { get; set; } = 0.7f;
    [Export(PropertyHint.Range, "0.1,3,0.05,suffix:s")] public float RevealSeconds { get; set; } = 0.9f;

    private Tween _tween;

    private ShaderMaterial Mat => (ShaderMaterial)Material;

    public float Progress
    {
        get => (float)Mat.GetShaderParameter(ProgressParam);
        set
        {
            Mat.SetShaderParameter(ProgressParam, value);
            // Bloqueia cliques enquanto há tinta na tela; invisível quando limpo.
            Visible = value > 0.001f;
            MouseFilter = value > 0.001f ? MouseFilterEnum.Stop : MouseFilterEnum.Ignore;
        }
    }

    public Task CoverAsync(bool instant = false) => AnimateTo(1f, instant ? 0f : CoverSeconds, Tween.EaseType.In);

    public Task RevealAsync(bool instant = false) => AnimateTo(0f, instant ? 0f : RevealSeconds, Tween.EaseType.Out);

    private async Task AnimateTo(float target, float seconds, Tween.EaseType ease)
    {
        _tween?.Kill();
        if (seconds <= 0f)
        {
            Progress = target;
            return;
        }
        _tween = CreateTween().SetPauseMode(Tween.TweenPauseMode.Process);
        _tween.TweenMethod(Callable.From<float>(v => Progress = v), Progress, target, seconds)
            .SetTrans(Tween.TransitionType.Sine).SetEase(ease);
        await ToSignal(_tween, Tween.SignalName.Finished);
    }
}
