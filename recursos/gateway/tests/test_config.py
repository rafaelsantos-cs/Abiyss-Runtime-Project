from pathlib import Path

import pytest

import config


def projeto(tmp_path: Path, gateway: str, dotenv: str | None = None) -> dict[str, str]:
    (tmp_path / "abiyss.toml").write_text(gateway)
    if dotenv is not None:
        (tmp_path / ".env").write_text(dotenv)
    return {"ABIYSS_CONFIG": str(tmp_path / "abiyss.toml")}


def test_socket_do_toml_e_token_do_ambiente(tmp_path):
    env = projeto(tmp_path, '[gateway]\nativo = true\nsocket = "data/x.sock"\n')
    env["ABIYSS_DISCORD_TOKEN"] = "abc.def"
    c = config.carregar(env)
    assert c.socket == tmp_path / "data/x.sock"
    assert c.token == "abc.def"
    assert "abc.def" not in repr(c), "o token nunca aparece num log"


def test_token_do_dotenv_sem_sobrescrever_o_ambiente(tmp_path):
    env = projeto(tmp_path, "[gateway]\nativo = true\n", 'OUTRA=1\nexport ABIYSS_DISCORD_TOKEN="do.dotenv"\n')
    assert config.carregar(env).token == "do.dotenv"
    assert config.carregar(env).socket == tmp_path / config.SOCKET_PADRAO
    env["ABIYSS_DISCORD_TOKEN"] = "do.ambiente"
    assert config.carregar(env).token == "do.ambiente"


@pytest.mark.parametrize(
    "toml, dotenv",
    [
        ("[gateway]\nativo = false\n", "ABIYSS_DISCORD_TOKEN=x"),
        ("[gateway]\nativo = true\n", None),
        ("[gateway]\nativo = true\n", "ABIYSS_DISCORD_TOKEN=COLOQUE-AQUI"),
    ],
)
def test_recusa_subir_sem_gateway_ou_sem_token(tmp_path, toml, dotenv):
    with pytest.raises(config.ErroConfig):
        config.carregar(projeto(tmp_path, toml, dotenv))
