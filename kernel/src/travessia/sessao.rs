//! O cérebro jogando: um laço modelo → ferramentas parecido com o do chat,
//! mas sem dono do outro lado.
//!
//! - O contexto é o de sempre (regras do kernel, núcleo, índice de skills) e
//!   uma mensagem com a tarefa; nenhuma estratégia, nenhuma dica de fase.
//! - Os sub-agentes rodam no mesmo processo (o executor do harness). O
//!   relatório de cada um chega sozinho, como mensagem do kernel, antes da
//!   próxima chamada do cérebro; `aguardar` espera sem gastar chamada.
//! - Braço C: quando uma fase nova começa, conversa nova e workspace limpo.
//! - Travas contra laço (não são parte da dificuldade): chamadas por fase,
//!   tempo da execução e respostas seguidas sem ferramenta.

use std::io::{BufRead, Seek, SeekFrom};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::instancia::ArquivosExecucao;
use super::{Braco, LinhaDoTempo, registrar};
use crate::config::Config;
use crate::dados;
use crate::db::Banco;
use crate::esforco;
use crate::eventos;
use crate::ferramentas::{CaixaDeFerramentas, ContextoChamada, interpretar_argumentos};
use crate::historico;
use crate::identidade::{BlocosPrompt, Identidade};
use crate::interocepcao;
use crate::mcp::PonteMcp;
use crate::nim::{self, Ferramenta, Mensagem};
use crate::orquestrador::{Origem, Orquestrador};
use crate::subagentes;
use crate::tempo::agora_ms;

/// Ferramenta só do harness: espera sub-agentes sem chamar o modelo.
pub const AGUARDAR: &str = "aguardar";

/// O que o harness sabe do jogo, lendo o replay que o servidor grava.
#[derive(Debug, Default)]
pub struct EstadoJogo {
    deslocamento: u64,
    /// Fases carregadas, em ordem.
    pub fases: Vec<u32>,
    /// Fases que já terminaram.
    pub terminadas: usize,
    pub encerrada: bool,
}

impl EstadoJogo {
    /// Lê o que o servidor acrescentou ao replay desde a última vez.
    pub fn atualizar(&mut self, replay: &std::path::Path) -> anyhow::Result<()> {
        let Ok(mut arquivo) = std::fs::File::open(replay) else {
            return Ok(());
        };
        arquivo.seek(SeekFrom::Start(self.deslocamento))?;
        let mut leitor = std::io::BufReader::new(arquivo);
        let mut linha = String::new();
        loop {
            linha.clear();
            let n = leitor.read_line(&mut linha)?;
            if n == 0 || !linha.ends_with('\n') {
                break; // linha incompleta: lê na próxima vez
            }
            self.deslocamento += n as u64;
            if !(linha.contains("\"tipo\":\"fase\"")
                || linha.contains("\"tipo\":\"fim_fase\"")
                || linha.contains("\"tipo\":\"fim_execucao\""))
            {
                continue;
            }
            let Ok(d) = serde_json::from_str::<Value>(&linha) else {
                continue;
            };
            match d["tipo"].as_str() {
                Some("fase") => self.fases.push(d["numero"].as_u64().unwrap_or(0) as u32),
                Some("fim_fase") => self.terminadas += 1,
                Some("fim_execucao") => self.encerrada = true,
                _ => {}
            }
        }
        Ok(())
    }

    /// A fase atual está em jogo (carregada e sem fim)?
    pub fn jogando(&self) -> bool {
        !self.encerrada && self.fases.len() > self.terminadas
    }
}

pub struct SessaoJogo {
    config: Config,
    banco: Banco,
    orquestrador: Orquestrador,
    caixa: Arc<CaixaDeFerramentas>,
    mcp: Arc<PonteMcp>,
    braco: Braco,
    arquivos: ArquivosExecucao,
    linha: LinhaDoTempo,
    conversa: i64,
    pub jogo: EstadoJogo,
    /// Chamadas do cérebro na fase atual (trava contra laço).
    chamadas_na_fase: u32,
    /// Quantas fases já tinham sido carregadas na última olhada.
    fases_vistas: usize,
    /// O texto da última `jogo__proxima_fase` bem-sucedida (braço C).
    ultima_visao: Option<String>,
    pub chamadas_total: u32,
}

impl SessaoJogo {
    #[allow(clippy::too_many_arguments)]
    pub fn nova(
        config: Config,
        banco: Banco,
        orquestrador: Orquestrador,
        caixa: Arc<CaixaDeFerramentas>,
        mcp: Arc<PonteMcp>,
        braco: Braco,
        arquivos: ArquivosExecucao,
        linha: LinhaDoTempo,
    ) -> anyhow::Result<SessaoJogo> {
        let conversa = historico::criar_conversa(&banco)?;
        let sessao = SessaoJogo {
            config,
            banco,
            orquestrador,
            caixa,
            mcp,
            braco,
            arquivos,
            linha,
            conversa,
            jogo: EstadoJogo::default(),
            chamadas_na_fase: 0,
            fases_vistas: 0,
            ultima_visao: None,
            chamadas_total: 0,
        };
        historico::adicionar(
            &sessao.banco,
            sessao.conversa,
            &Mensagem::usuario(sessao.tarefa()),
        )?;
        Ok(sessao)
    }

    /// A tarefa, sem estratégia nenhuma.
    fn tarefa(&self) -> String {
        let delega = if self.braco.delega() {
            format!(
                "Você pode delegar a sub-agentes (delegar, status, cancelar e {AGUARDAR}). Eles rodam em \
                 paralelo, pelo pool de chamadas deles, e têm estas ferramentas do jogo: {}. Só você \
                 executa jogadas. O relatório de cada sub-agente chega sozinho, numa mensagem do \
                 kernel; {AGUARDAR} espera por eles sem gastar uma chamada sua.",
                self.config.travessia.ferramentas_jogo_subagentes.join(", ")
            )
        } else {
            "Nesta execução não há sub-agentes: você joga sozinho.".to_string()
        };
        format!(
            "[kernel · benchmark Travessia] Esta é uma instância de teste, separada da sua vida \
             real: nada daqui entra na sua memória nem no seu diário.\n\n\
             Jogue o Travessia, um quebra-cabeça em grade, com as ferramentas jogo__* (comece por \
             jogo__regras e jogo__ver). As fases vêm em sequência: quando uma acabar (concluída, \
             falha ou desistência), chame jogo__proxima_fase. O jogo termina quando \
             jogo__proxima_fase disser que não há mais fases. O objetivo é concluir cada fase com o \
             menor número de jogadas que conseguir.\n\n{delega}"
        )
    }

    fn definicoes(&self) -> Vec<Ferramenta> {
        let mut lista = self.caixa.definicoes();
        if self.braco.delega() {
            lista.push(Ferramenta::nova(
                AGUARDAR,
                "Espera sub-agentes terminarem, sem gastar chamada ao modelo. Sem `ids`, espera o \
                 primeiro dos que estão rodando. Diz quem terminou; os relatórios chegam na mensagem \
                 seguinte do kernel.",
                json!({
                    "type": "object",
                    "properties": {
                        "ids": {"type": "array", "items": {"type": "integer"},
                                "description": "Sub-agentes a esperar (vazio = os que estão rodando)"},
                        "todos": {"type": "boolean",
                                  "description": "true = espera todos da lista; false = o primeiro"},
                        "max_segundos": {"type": "integer", "description": "Tempo máximo esperando"}
                    }
                }),
            ));
        }
        lista
    }

    fn mensagens(&self) -> anyhow::Result<Vec<Mensagem>> {
        let identidade = Identidade::carregar(&self.config.caminho_identidade());
        let contexto = format!(
            "Data e hora: {}\nVocê está jogando o benchmark Travessia numa instância isolada.",
            interocepcao::agora_formatado()
        );
        let blocos = BlocosPrompt {
            memoria_central: None,
            skills: self.caixa.indice_skills(),
            contexto: Some(contexto),
        };
        let mut mensagens = vec![Mensagem::sistema(identidade.prompt_sistema_com(&blocos))];
        mensagens.extend(historico::carregar(
            &self.banco,
            self.conversa,
            self.config.travessia.historico_max_mensagens,
        )?);
        Ok(mensagens)
    }

    fn marcar(&self, evento: Value) {
        registrar(&self.linha, evento);
    }

    /// Relatórios de sub-agentes que terminaram viram UMA mensagem do kernel.
    fn entregar_relatorios(&self) -> anyhow::Result<()> {
        let lista: Vec<_> = eventos::pendentes(&self.banco, 50)?
            .into_iter()
            .filter(|e| e.tipo == eventos::TIPO_SUBAGENTE)
            .collect();
        if lista.is_empty() {
            return Ok(());
        }
        let mut texto = String::from("[kernel] Sub-agentes terminaram:");
        for e in &lista {
            texto.push('\n');
            texto.push_str(&dados::rotular(
                &format!("subagente:{}", e.origem),
                &e.conteudo,
            ));
        }
        historico::adicionar(&self.banco, self.conversa, &Mensagem::usuario(texto))?;
        let ids: Vec<i64> = lista.iter().map(|e| e.id).collect();
        eventos::marcar_consumidos(&self.banco, &ids)
    }

    /// Fase nova: zera a trava da fase e, no braço C, apaga a memória.
    fn olhar_fase(&mut self) -> anyhow::Result<()> {
        if self.jogo.fases.len() == self.fases_vistas {
            return Ok(());
        }
        let primeira = self.fases_vistas == 0;
        self.fases_vistas = self.jogo.fases.len();
        self.chamadas_na_fase = 0;
        let numero = *self.jogo.fases.last().unwrap_or(&0);
        self.marcar(json!({"tipo": "fase", "t": agora_ms(), "numero": numero, "evento": "inicio"}));
        if self.braco.apaga_memoria() && !primeira {
            limpar_pasta(&self.config.caminho_workspace())?;
            self.conversa = historico::criar_conversa(&self.banco)?;
            let mut texto = self.tarefa();
            texto.push_str(
                "\n\n[kernel] Uma fase nova começou. A memória desta execução foi apagada (conversa e \
                 workspace).",
            );
            if let Some(visao) = &self.ultima_visao {
                texto.push('\n');
                texto.push_str(&dados::rotular("mcp:jogo__proxima_fase", visao));
            }
            historico::adicionar(&self.banco, self.conversa, &Mensagem::usuario(texto))?;
        }
        Ok(())
    }

    /// Trava contra laço: o harness encerra a fase (desistência) e passa
    /// para a próxima, avisando o cérebro.
    async fn encerrar_fase_pela_trava(&mut self, motivo: &str) -> anyhow::Result<()> {
        let numero = *self.jogo.fases.last().unwrap_or(&0);
        tracing::warn!("travessia: fase {numero} encerrada pelo harness ({motivo})");
        self.marcar(
            json!({"tipo": "fase", "t": agora_ms(), "numero": numero, "evento": "trava_do_harness",
                           "motivo": motivo}),
        );
        if self.jogo.jogando() {
            let _ = self
                .mcp
                .chamar_no_servidor("jogo", "desistir_fase", json!({}))
                .await;
        }
        let texto = match self
            .mcp
            .chamar_no_servidor("jogo", "proxima_fase", json!({}))
            .await
        {
            Ok(r) => r.texto,
            Err(e) => format!("{e:#}"),
        };
        self.ultima_visao = Some(texto.clone());
        let aviso = format!(
            "[kernel] A fase {numero} foi encerrada pelo harness ({motivo}) e conta como desistência. \
             Resultado de jogo__proxima_fase:\n{}",
            dados::rotular("mcp:jogo__proxima_fase", &texto)
        );
        historico::adicionar(&self.banco, self.conversa, &Mensagem::usuario(aviso))?;
        self.chamadas_na_fase = 0;
        Ok(())
    }

    /// Joga até o jogo acabar (ou uma trava). Devolve o motivo do fim.
    pub async fn jogar(&mut self) -> anyhow::Result<String> {
        let t = self.config.travessia.clone();
        let inicio = Instant::now();
        let limite_execucao = Duration::from_secs(t.max_minutos_por_execucao.max(1) * 60);
        let modelo = esforco::modelo_no_nivel(
            &self.config.modelos,
            "cerebro",
            self.config.chat.esforco_padrao,
        );
        let mut cutucadas = 0;
        loop {
            self.jogo.atualizar(&self.arquivos.replay)?;
            if self.jogo.encerrada {
                return Ok("jogo encerrado".into());
            }
            self.olhar_fase()?;
            if inicio.elapsed() >= limite_execucao {
                return Ok(format!(
                    "trava do harness: execução passou de {} min",
                    t.max_minutos_por_execucao
                ));
            }
            if self.chamadas_na_fase >= t.max_chamadas_por_fase {
                self.encerrar_fase_pela_trava(&format!(
                    "{} chamadas do cérebro na fase",
                    t.max_chamadas_por_fase
                ))
                .await?;
                continue;
            }
            self.entregar_relatorios()?;

            let pedido = nim::montar_pedido(&modelo, self.mensagens()?, self.definicoes());
            self.chamadas_na_fase += 1;
            self.chamadas_total += 1;
            let resposta = self
                .orquestrador
                .cerebro
                .chamar(Origem::Conversa, &pedido, None)
                .await?;
            historico::adicionar(&self.banco, self.conversa, &resposta.mensagem)?;

            let pedidas = resposta.mensagem.chamadas().to_vec();
            if pedidas.is_empty() {
                cutucadas += 1;
                if cutucadas > t.max_cutucadas {
                    return Ok(format!(
                        "o cérebro parou de jogar ({cutucadas} respostas seguidas sem ferramenta)"
                    ));
                }
                historico::adicionar(
                    &self.banco,
                    self.conversa,
                    &Mensagem::usuario(
                        "[kernel] O jogo ainda não terminou: continue jogando com as ferramentas jogo__*.",
                    ),
                )?;
                continue;
            }
            cutucadas = 0;
            for chamada in &pedidas {
                let nome = chamada.function.name.as_str();
                let resultado = if nome == AGUARDAR && self.braco.delega() {
                    let texto = self.aguardar(&chamada.function.arguments).await;
                    crate::ferramentas::ResultadoFerramenta {
                        texto: dados::rotular(AGUARDAR, &texto),
                        erro: false,
                        origem_externa: None,
                    }
                } else {
                    self.caixa
                        .executar_com(chamada, &ContextoChamada::sem_rastreio())
                        .await
                };
                self.marcar(
                    json!({"tipo": "ferramenta", "t": agora_ms(), "quem": "cerebro",
                                   "nome": nome, "erro": resultado.erro}),
                );
                if nome == "jogo__proxima_fase" && !resultado.erro {
                    self.ultima_visao = Some(resultado.texto.clone());
                }
                historico::adicionar(
                    &self.banco,
                    self.conversa,
                    &Mensagem::resultado_ferramenta(&chamada.id, nome, resultado.texto),
                )?;
            }
        }
    }

    /// `aguardar`: espera sub-agentes terminarem (consultando o banco),
    /// sem chamar o modelo.
    async fn aguardar(&self, argumentos: &str) -> String {
        let args = interpretar_argumentos(argumentos).unwrap_or_else(|_| json!({}));
        let teto = self.config.travessia.aguardar_max_segundos.max(1);
        let max = args["max_segundos"].as_u64().unwrap_or(teto).clamp(1, teto);
        let todos = args["todos"].as_bool().unwrap_or(false);
        let mut ids: Vec<i64> = args["ids"]
            .as_array()
            .map(|l| l.iter().filter_map(Value::as_i64).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            ids = subagentes::ativos(&self.banco)
                .map(|l| l.into_iter().map(|i| i.id).collect())
                .unwrap_or_default();
        }
        if ids.is_empty() {
            return "nenhum sub-agente rodando".into();
        }
        let comeco = Instant::now();
        loop {
            let mut terminados = Vec::new();
            let mut rodando = Vec::new();
            for id in &ids {
                match subagentes::obter(&self.banco, *id) {
                    Ok(Some(info)) if info.estado.eh_final() => terminados.push(*id),
                    Ok(Some(_)) => rodando.push(*id),
                    _ => terminados.push(*id), // não existe: não há o que esperar
                }
            }
            let pronto = if todos {
                rodando.is_empty()
            } else {
                !terminados.is_empty()
            };
            if pronto || comeco.elapsed() >= Duration::from_secs(max) {
                return json!({
                    "terminaram": terminados,
                    "ainda_rodando": rodando,
                    "esperou_segundos": comeco.elapsed().as_secs(),
                    "observacao": "os relatórios de quem terminou chegam na próxima mensagem do kernel"
                })
                .to_string();
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

/// Apaga o conteúdo de uma pasta (não a pasta). Não segue links.
fn limpar_pasta(pasta: &std::path::Path) -> anyhow::Result<()> {
    let Ok(entradas) = std::fs::read_dir(pasta) else {
        return Ok(());
    };
    for entrada in entradas.flatten() {
        let caminho = entrada.path();
        let tipo = entrada.file_type()?;
        if tipo.is_dir() {
            std::fs::remove_dir_all(&caminho)?;
        } else {
            std::fs::remove_file(&caminho)?;
        }
    }
    Ok(())
}
