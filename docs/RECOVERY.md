# ABIYSS Recovery Procedure

This document exists so an environment loss does not become a project loss.

## Recover the v0.1 source

```bash
git clone https://github.com/rafaelsantos-cs/Abiyss-Runtime-Project.git
cd Abiyss-Runtime-Project
git checkout v0.1.0
```

The `v0.1.0` branch is a frozen checkpoint for the validated v0.1 source state.

## Verify the Python source

```bash
python3 -m venv .venv
. .venv/bin/activate
python -m pip install --upgrade pip
python -m pip install -e '.[dev]'
python -m pytest -q
python -m compileall -q src
```

Network access is only required to install development/build dependencies. The runtime itself has no mandatory third-party dependency.

## Enable Gemini separately

```bash
python -m pip install '.[gemini]'
export GEMINI_API_KEY='...'
```

Never put the key into source control, configuration committed to Git, examples, logs or QuPs.

## Safe first run

```bash
python -m abiyss doctor
python -m abiyss status
```

Keep execution disabled until the deployment has an explicit tool and privilege policy.

## Restore from the source archive

`Abiyss-v0.1.0-source.zip` is the local release archive corresponding to the v0.1 source snapshot. Its SHA-256 is recorded in `docs/BUILD_REPORT.md` and `docs/RELEASE_CHECKSUMS.txt`.

## Recovery semantics

A Query that was durably `running` when an ABIYSS process died is intentionally converted to `recovery_required` on restart. Do not automatically requeue it unless an operator has independently established whether the external side effect happened.

## What is deliberately not in the v0.1 runtime

- War Pigs adversarial attack harness;
- unrestricted shell;
- arbitrary package-manager control;
- unrestricted filesystem mutation;
- hostile-code sandbox claims.
