using System;
using System.Collections.Generic;
using System.Linq;
using Godot;

namespace OgonNoKaze.Player.StateMachine;

public enum StateId
{
    // Estados-pai (compostos): nunca ficam ativos sozinhos, sempre com um filho.
    Grounded, Airborne, Traversal, Combat,

    // Folhas
    Idle, Walk, Run, Sprint, Crouch,
    Jump, Fall,
    Grapple, LedgeHang, Climb,
    Attack, Deflect, Guard, Stagger, PostureBroken, Deathblow, Death,
}

/// <summary>
/// Um estado do personagem. Estados formam uma árvore: um estado-pai concentra a lógica
/// comum aos filhos (ex.: Grounded detecta "saí do chão" para Idle, Walk, Run...).
/// </summary>
public abstract class PlayerState
{
    public abstract StateId Id { get; }
    /// <summary>Filho ativado quando alguém pede transição para este estado-pai.</summary>
    public virtual StateId? DefaultChild => null;

    public PlayerState Parent { get; internal set; }
    protected PlayerController Player { get; private set; }
    protected PlayerStateMachine Machine { get; private set; }
    /// <summary>Segundos desde que este estado foi ativado.</summary>
    public float TimeInState { get; internal set; }

    internal void Bind(PlayerController player, PlayerStateMachine machine)
    {
        Player = player;
        Machine = machine;
    }

    public virtual void Enter(StateId previous) { }
    public virtual void Exit(StateId next) { }

    /// <summary>
    /// Retorna o próximo estado ou null para ficar. Pais são consultados antes dos filhos,
    /// então regras gerais ("caí de um penhasco") vencem as específicas.
    /// </summary>
    public virtual StateId? CheckTransition() => null;

    public virtual void PhysicsUpdate(float delta) { }
}

/// <summary>
/// Máquina de estados hierárquica (HSM).
///
/// A cada tick de física: (1) pergunta a cada estado da cadeia raiz→folha se quer
/// transicionar; o primeiro "sim" vence; (2) aplica a transição saindo dos estados
/// até o ancestral comum e entrando do ancestral até o novo alvo; (3) roda o
/// PhysicsUpdate da cadeia raiz→folha. Ir de Walk para Run, por exemplo, não sai
/// nem entra de novo em Grounded — só troca a folha.
/// </summary>
public sealed class PlayerStateMachine
{
    private readonly Dictionary<StateId, PlayerState> _states = new();
    private readonly List<PlayerState> _chain = new();
    private readonly PlayerController _player;

    public PlayerState Current { get; private set; }
    /// <summary>(anterior, novo) depois de cada troca de folha.</summary>
    public event Action<StateId, StateId> StateChanged;

    /// <summary>Limite de transições encadeadas no mesmo tick (proteção contra laços).</summary>
    public int MaxTransitionsPerTick { get; set; } = 4;

    public PlayerStateMachine(PlayerController player) => _player = player;

    public void Add(PlayerState state, StateId? parent = null)
    {
        state.Bind(_player, this);
        if (parent.HasValue)
            state.Parent = _states[parent.Value];
        _states.Add(state.Id, state);
    }

    public T Get<T>(StateId id) where T : PlayerState => (T)_states[id];

    public bool IsIn(StateId id)
    {
        for (var s = Current; s != null; s = s.Parent)
            if (s.Id == id)
                return true;
        return false;
    }

    /// <summary>Caminho atual, ex.: "Grounded/Run" (para depuração).</summary>
    public string Path => string.Join("/", Ancestry(Current).Select(s => s.Id));

    public void Start(StateId initial)
    {
        Current = null;
        TransitionTo(initial);
    }

    public void Tick(float delta)
    {
        for (var i = 0; i < MaxTransitionsPerTick; i++)
        {
            var next = FirstRequestedTransition();
            if (next == null || ResolveLeaf(next.Value) == Current)
                break;
            TransitionTo(next.Value);
        }

        RebuildChain();
        foreach (var state in _chain)
        {
            state.TimeInState += delta;
            state.PhysicsUpdate(delta);
        }
    }

    public void TransitionTo(StateId id)
    {
        var target = ResolveLeaf(id);
        var previous = Current;
        var previousId = previous?.Id ?? id;

        var oldPath = Ancestry(previous);
        var newPath = Ancestry(target);
        var common = 0;
        while (common < oldPath.Count && common < newPath.Count && oldPath[common] == newPath[common])
            common++;

        // Sai da folha para cima até o ancestral comum, depois entra de cima para baixo.
        for (var i = oldPath.Count - 1; i >= common; i--)
            oldPath[i].Exit(target.Id);
        for (var i = common; i < newPath.Count; i++)
        {
            newPath[i].TimeInState = 0f;
            newPath[i].Enter(previousId);
        }

        Current = target;
        RebuildChain();
        if (previous != target)
            StateChanged?.Invoke(previousId, target.Id);
    }

    private StateId? FirstRequestedTransition()
    {
        RebuildChain();
        foreach (var state in _chain)
        {
            var next = state.CheckTransition();
            if (next.HasValue)
                return next;
        }
        return null;
    }

    private PlayerState ResolveLeaf(StateId id)
    {
        var state = _states[id];
        while (state.DefaultChild.HasValue)
            state = _states[state.DefaultChild.Value];
        return state;
    }

    private void RebuildChain()
    {
        _chain.Clear();
        _chain.AddRange(Ancestry(Current));
    }

    /// <summary>Lista raiz→folha.</summary>
    private static List<PlayerState> Ancestry(PlayerState leaf)
    {
        var list = new List<PlayerState>();
        for (var s = leaf; s != null; s = s.Parent)
            list.Insert(0, s);
        return list;
    }
}
