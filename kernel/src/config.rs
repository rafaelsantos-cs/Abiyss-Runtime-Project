//! Configuração do Abiyss.
//!
//! Duas fontes, com papéis bem separados:
//! - `abiyss.toml`: tudo que NÃO é segredo (URL do NIM, IDs de modelo, limites...).
//!   Pode ir para o git.
//! - `.env`: só segredos (as chaves do NIM). Nunca vai para o git.
//!
//! O `abiyss.toml` diz o NOME da variável de ambiente de cada chave
//! (`api_key_env`); o valor vem do `.env` ou do ambiente do processo.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::{Map, Value};

/// Configuração completa, já carregada e validada.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub nim: ConfigNim,
    pub modelos: ConfigModelos,
    pub pools: ConfigPools,

    /// Diretório onde está o `abiyss.toml`. Todos os caminhos relativos
    /// da configuração são resolvidos a partir daqui.
    /// Não vem do arquivo: é preenchido por `Config::carregar`.
    #[serde(skip)]
    pub raiz: PathBuf,
}

/// Onde e como falar com o NVIDIA NIM.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigNim {
    /// Ex.: "https://integrate.api.nvidia.com/v1" (sem barra no final).
    pub base_url: String,
    /// Tempo máximo SEM receber bytes do servidor antes de desistir.
    /// Modelos com raciocínio longo podem demorar para começar a responder.
    #[serde(default = "padrao_timeout_leitura")]
    pub timeout_leitura_segundos: u64,
    /// Tempo máximo para abrir a conexão TCP/TLS.
    #[serde(default = "padrao_timeout_conexao")]
    pub timeout_conexao_segundos: u64,
}

fn padrao_timeout_leitura() -> u64 {
    600
}

fn padrao_timeout_conexao() -> u64 {
    15
}

/// Os quatro "cérebros" que o Abiyss usa.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigModelos {
    /// Cérebro principal do Abiyss.
    pub cerebro: ConfigModelo,
    /// Sub-agente de nível Ultra.
    pub sub_ultra: ConfigModelo,
    /// Sub-agente de nível Medium.
    pub sub_medium: ConfigModelo,
    /// Sub-agente de nível Low.
    pub sub_low: ConfigModelo,
}

/// Um modelo do NIM e os parâmetros enviados junto com ele.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigModelo {
    /// ID do modelo no NIM, ex.: "z-ai/glm-5.3".
    pub id: String,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperatura: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    /// Campos extras copiados como estão para o corpo da requisição.
    /// É aqui que entram parâmetros específicos do modelo, como o modo de
    /// raciocínio (`chat_template_kwargs`), sem precisar recompilar o kernel.
    #[serde(default)]
    pub extra: Map<String, Value>,
}

/// Configuração dos dois pools de chamadas ao NIM.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigPools {
    pub cerebro: ConfigPoolCerebro,
    pub subagentes: ConfigPoolSubagentes,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigPoolCerebro {
    /// Nome da variável de ambiente que guarda a chave deste pool.
    pub api_key_env: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigPoolSubagentes {
    /// Nome da variável de ambiente que guarda a chave deste pool.
    pub api_key_env: String,
}

/// Valor de exemplo do `.env.example`. Se a chave ainda for esta,
/// o usuário esqueceu de preencher.
const CHAVE_DE_EXEMPLO: &str = "nvapi-COLOQUE-AQUI";

impl Config {
    /// Descobre qual `abiyss.toml` usar:
    /// 1. o caminho passado explicitamente (flag `--config` ou `ABIYSS_CONFIG`);
    /// 2. senão, `./abiyss.toml` no diretório atual.
    pub fn caminho_padrao(explicito: Option<&Path>) -> PathBuf {
        match explicito {
            Some(caminho) => caminho.to_path_buf(),
            None => PathBuf::from("abiyss.toml"),
        }
    }

    /// Lê o `abiyss.toml`, carrega o `.env` que estiver ao lado dele
    /// e valida o resultado.
    pub fn carregar(caminho: &Path) -> anyhow::Result<Config> {
        let texto = std::fs::read_to_string(caminho)
            .with_context(|| format!("não consegui ler {}", caminho.display()))?;

        let mut config = Config::de_texto(&texto)
            .with_context(|| format!("erro no arquivo {}", caminho.display()))?;

        // A raiz é a pasta do abiyss.toml (em forma absoluta).
        let pasta = caminho.parent().unwrap_or(Path::new("."));
        let pasta = if pasta.as_os_str().is_empty() {
            Path::new(".")
        } else {
            pasta
        };
        config.raiz = pasta
            .canonicalize()
            .with_context(|| format!("pasta inválida: {}", pasta.display()))?;

        // O `.env` é opcional: em produção as chaves podem vir do systemd.
        // `from_path` NÃO sobrescreve variáveis que já existem no ambiente.
        let env = config.raiz.join(".env");
        if env.exists() {
            dotenvy::from_path(&env)
                .with_context(|| format!("não consegui ler {}", env.display()))?;
        }

        Ok(config)
    }

    /// Interpreta o conteúdo TOML e valida. Separado de `carregar` para
    /// facilitar os testes (não precisa de arquivo).
    pub fn de_texto(texto: &str) -> anyhow::Result<Config> {
        let mut config: Config = toml::from_str(texto)?;
        config.raiz = PathBuf::from(".");
        config.validar()?;
        Ok(config)
    }

    /// Checagens que o TOML sozinho não garante.
    fn validar(&mut self) -> anyhow::Result<()> {
        // Normaliza a URL: sem barra no final, para montar "{base}/chat/completions".
        while self.nim.base_url.ends_with('/') {
            self.nim.base_url.pop();
        }
        if !self.nim.base_url.starts_with("http://") && !self.nim.base_url.starts_with("https://") {
            bail!("nim.base_url precisa começar com http:// ou https://");
        }
        for (nome, modelo) in [
            ("cerebro", &self.modelos.cerebro),
            ("sub_ultra", &self.modelos.sub_ultra),
            ("sub_medium", &self.modelos.sub_medium),
            ("sub_low", &self.modelos.sub_low),
        ] {
            if modelo.id.trim().is_empty() {
                bail!("modelos.{nome}.id está vazio");
            }
        }
        Ok(())
    }

    /// Resolve um caminho da configuração relativo à raiz do projeto.
    pub fn resolver(&self, caminho: &str) -> PathBuf {
        let p = Path::new(caminho);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.raiz.join(p)
        }
    }
}

/// Configuração pronta para testes e para o mock do NIM: aponta para
/// `base_url` e usa `raiz` como pasta do projeto. Nenhuma chave real é usada.
pub fn config_de_teste(base_url: &str, raiz: &Path) -> Config {
    let texto = format!(
        r#"
        [nim]
        base_url = "{base_url}"
        timeout_leitura_segundos = 30
        timeout_conexao_segundos = 5

        [modelos.cerebro]
        id = "teste/cerebro"
        [modelos.sub_ultra]
        id = "teste/ultra"
        [modelos.sub_medium]
        id = "teste/medium"
        [modelos.sub_low]
        id = "teste/low"

        [pools.cerebro]
        api_key_env = "ABIYSS_TESTE_CHAVE_CEREBRO"
        [pools.subagentes]
        api_key_env = "ABIYSS_TESTE_CHAVE_SUBAGENTES"
        "#
    );
    let mut config = Config::de_texto(&texto).expect("config de teste inválida");
    config.raiz = raiz.to_path_buf();
    config
}

/// Lê uma chave do NIM da variável de ambiente indicada.
/// A mensagem de erro explica o que fazer, mas nunca mostra o valor.
pub fn ler_chave(nome_variavel: &str) -> anyhow::Result<String> {
    validar_chave(nome_variavel, std::env::var(nome_variavel).ok())
}

/// Parte "pura" de `ler_chave`, separada para poder ser testada
/// sem mexer nas variáveis de ambiente do processo.
fn validar_chave(nome_variavel: &str, valor: Option<String>) -> anyhow::Result<String> {
    let Some(valor) = valor else {
        bail!("variável {nome_variavel} não definida: copie .env.example para .env e preencha");
    };
    let valor = valor.trim().to_string();
    if valor.is_empty() || valor == CHAVE_DE_EXEMPLO {
        bail!("variável {nome_variavel} ainda está vazia ou com o valor de exemplo");
    }
    Ok(valor)
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Uma configuração mínima válida.
    const MINIMA: &str = r#"
        [nim]
        base_url = "http://127.0.0.1:9/v1/"

        [modelos.cerebro]
        id = "teste/cerebro"
        extra = { chat_template_kwargs = { enable_thinking = true } }
        [modelos.sub_ultra]
        id = "teste/ultra"
        [modelos.sub_medium]
        id = "teste/medium"
        [modelos.sub_low]
        id = "teste/low"

        [pools.cerebro]
        api_key_env = "TESTE_CHAVE_CEREBRO"
        [pools.subagentes]
        api_key_env = "TESTE_CHAVE_SUB"
    "#;

    #[test]
    fn carrega_config_minima_e_normaliza_url() {
        let config = Config::de_texto(MINIMA).unwrap();
        assert_eq!(config.nim.base_url, "http://127.0.0.1:9/v1");
        assert_eq!(config.nim.timeout_leitura_segundos, 600);
        assert_eq!(
            config.modelos.cerebro.extra["chat_template_kwargs"]["enable_thinking"],
            Value::Bool(true)
        );
    }

    #[test]
    fn rejeita_url_sem_esquema() {
        let texto = MINIMA.replace("http://127.0.0.1:9/v1/", "integrate.api.nvidia.com/v1");
        assert!(Config::de_texto(&texto).is_err());
    }

    #[test]
    fn rejeita_modelo_vazio() {
        let texto = MINIMA.replace("teste/low", " ");
        assert!(Config::de_texto(&texto).is_err());
    }

    #[test]
    fn chave_ausente_vazia_ou_de_exemplo_e_recusada() {
        assert!(validar_chave("X", None).is_err());
        assert!(validar_chave("X", Some("   ".into())).is_err());
        assert!(validar_chave("X", Some(CHAVE_DE_EXEMPLO.into())).is_err());
        assert_eq!(
            validar_chave("X", Some(" nvapi-abc \n".into())).unwrap(),
            "nvapi-abc"
        );
    }
}
