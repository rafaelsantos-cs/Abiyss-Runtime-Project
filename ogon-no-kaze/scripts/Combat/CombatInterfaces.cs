using Godot;

namespace OgonNoKaze.Combat;

// Contratos que o combate vai usar. Ficam prontos desde a Fase 1 para que câmera,
// personagem e (futuros) inimigos já nasçam conversando pela mesma interface.

/// <summary>Congela brevemente a animação/física de quem golpeia e de quem é atingido.</summary>
public interface IHitStop
{
    /// <param name="seconds">Duração do congelamento (tipicamente 0,04–0,12 s).</param>
    /// <param name="timeScale">Velocidade durante o congelamento (0 = parado).</param>
    void RequestHitStop(float seconds, float timeScale = 0f);
}

/// <summary>Tremor de câmera por "trauma" (0–1) que decai com o tempo.</summary>
public interface ICameraShake
{
    void AddTrauma(float amount);
}

/// <summary>Algo em que a câmera pode travar a mira (inimigo, alvo de treino).</summary>
public interface ILockOnTarget
{
    /// <summary>Ponto (em espaço de mundo) para onde a câmera olha, geralmente o peito.</summary>
    Vector3 LockOnPoint { get; }
    bool CanBeLockedOn { get; }
}

public enum HitOutcome { Hit, Guarded, Deflected, PostureBroken, Killed, Ignored }

/// <summary>Um golpe recebido.</summary>
public readonly record struct HitInfo(
    Node3D Attacker,
    float Damage,
    float PostureDamage,
    Vector3 Direction,
    bool Unblockable = false);

/// <summary>Vida e postura no estilo "quebrar a guarda": dano à postura abre a execução.</summary>
public interface IPostureHealth
{
    float Health { get; }
    float MaxHealth { get; }
    float Posture { get; }
    float MaxPosture { get; }
    HitOutcome ReceiveHit(in HitInfo hit);
}
