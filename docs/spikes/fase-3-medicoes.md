# Fase 3 — medições do wai

> 2026-11-09 · build release (`cargo build -p wai --release --features demo`), commit dcad89f mais um log de tempo e um modo roteirizado só sob `demo` (ver Método), neste Mac. Agente falso (`wai-fake-agent --reply-file … --chunk-delay-ms 25`), `jira` falso no `PATH` (4 cards EC-9001..9004, inventados), extensão `wai-waycarbon` (binário Go de `go build`) instalada em `HOME` temporário com consentimento gravado em `extensions.json`; nada de `~/.config/wai` foi tocado.

## Resultado

| Métrica | Meta | Fase 3 | Fase 2 | Fase 1 | Central (S3) |
|---|---|---|---|---|---|
| CPU parado, extensão ligada + painel "My cards" + 1 projeto + 3 sessões + terminal (30 s) | < 0,5% | **0,16%** (média de 3 execuções: 0,18 / 0,17 / 0,12; ver amostras) | 0,15% | 0,15% | ~8–9% |
| CPU parado, extensão desligada (30 s) | < 0,5% | **0,17%** (0,16 / 0,18) | — | — | — |
| RAM do wai, extensão ligada (`top`) | < 300 MB | **72 MB** (`ps` RSS: 96–97 MB) | 72 MB | 127–133 MB | 33–95 MB só no servidor |
| RAM do wai, extensão desligada (`top`) | < 300 MB | **71 MB** e **90 MB** (`ps` RSS: 95 MB) | — | — | — |
| RSS do processo da extensão (`wai-waycarbon`, Go) | — | **5,7–6,2 MB** (`ps`) | n/a | n/a | n/a |
| Início da extensão: do lançamento do processo ao 1º render sem "Loading…" | — | **217–286 ms** (286, 217, 240) | n/a | n/a | n/a |
| Janela visível após abrir | < 1 s | não remedido (ver abaixo); o painel renderiza ~240 ms após o `exec` | 244–293 ms | 230–270 ms | n/a |

Custo da extensão: ~0 de CPU e ~6 MB de RSS no processo dela; a diferença de RAM do wai (72 contra 71/90 MB) está dentro do ruído do `top`. Todas as metas batidas.

## Método

- **Build:** `cargo build -p wai --release --features demo`, `cargo build --release -p wai-agent --bins` e `go build -o bin/wai-waycarbon .` em `extensions/wai-waycarbon`, com o `HOME` real. O binário pronto roda com `HOME=<tmp>`, `PATH=<tmp>/bin:/usr/bin:…` (só o `jira` falso), `WAI_STATE_PATH=<tmp>/state.json`.
- **Cenário:** 1 projeto temporário (3 arquivos `.rs`); `state.json` restaura 3 sessões do agente falso em 2 painéis e o terminal visível; `extensions.json` com `granted = [process, panels, sessions:control]` e `enabled` true/false; a pasta da extensão copiada para `<tmp>/.config/wai/extensions/wai-waycarbon/` (com `bin/wai-waycarbon`).
- **Painel visível:** `WAI_SHOT_PANEL=1` sob `demo` (novo, sem `screenshot`): 3 s depois do lançamento seleciona a 1ª aba de painel de extensão, como o modo de captura já fazia. Não foi possível confirmar visualmente (sem captura nesta execução) que o painel estava pintado; o app registrou o render da árvore.
- **CPU/RAM:** igual às Fases 1 e 2: `top -l 7 -s 5 -pid <pid> -stats pid,cpu,mem`, descartando a 1ª amostra (6 amostras = 30 s), ~14 s após o lançamento. RSS por `ps -o rss=` (wai e filhos) na mesma hora.
- **Início da extensão:** log novo sob `demo` em `crates/wai/src/ext_host.rs`: `wai: panel render loading=<bool> at <ms epoch>` a cada `panel/render` recebido. O tempo é esse instante menos o epoch em ms tomado imediatamente antes do `exec` do wai. Nas 3 execuções o "Loading…" e os cards chegaram no mesmo milissegundo (o `jira` falso responde na hora); com o `jira` real o intervalo seria o tempo de `jira me` + `jira issue list`. Um 1º ensaio (fora das 3 execuções, sem o epoch de lançamento) deu 272 ms entre o "Loading…" e os cards.

## Amostras brutas (`top`, CPU % / mem)

| Execução | 6 amostras de CPU | Média | mem |
|---|---|---|---|
| ligada 1 | 0,2 / 0,2 / 0,2 / 0,2 / 0,1 / 0,2 | 0,18% | 72M |
| ligada 2 | 0,1 / 0,1 / 0,1 / 0,2 / 0,3 / 0,2 | 0,17% | 72–73M |
| ligada 3 | 0,1 / 0,1 / 0,1 / 0,2 / 0,1 / 0,1 | 0,12% | 72M |
| ligada (ensaio) | 0,2 / 0,2 / 0,1 / 0,1 / 0,1 / 0,2 | 0,15% | 72M |
| desligada 1 | 0,1 / 0,2 / 0,1 / 0,2 / 0,2 / 0,1 | 0,15% | 71M |
| desligada 2 | 0,3 / 0,2 / 0,2 / 0,2 / 0,1 / 0,2 | 0,20% | 90M |

(A média da tabela principal para "ligada" é a das 3 execuções numeradas: 0,16%; "desligada": 0,17%.) RSS (`ps`): wai 98,5 / 98,9 / 98,6 MB ligada, 97,4 / 97,6 MB desligada; `wai-fake-agent` 3,6 MB; shell do terminal 2,9 MB; `wai-waycarbon` 5,9 / 5,7 / 5,8 MB (6,2 no ensaio).

## Não medido / ressalvas

- **Janela visível (< 1 s):** não refeita nesta fase (o laço `CGWindowListCopyWindowInfo` não foi reexecutado); nada na Fase 3 mexe no caminho de abertura além de iniciar a extensão em segundo plano, e o painel já renderiza ~240 ms após o `exec`.
- **`jira` real:** usado um `jira` falso; a latência do 1º render com cards depende do `jira` real (rede), não medida.
- **Painel pintado:** o log prova o render da árvore, não o desenho na tela (ver Método).
- **RAM com a extensão desligada:** 71 e 90 MB; o `top` varia com memória comprimida (90 MB parece ruído, o RSS do `ps` ficou igual: ~97 MB). Não há conclusão de que desligar economize RAM.
- Streaming e arquivo de 100 MB não foram repetidos (nada novo na Fase 3 neles). Streaming com o Claude real segue sem medição (cota).

## Smoke test com o Claude real

`cargo test -p wai-agent --test real_claude -- --ignored --nocapture` (`@agentclientprotocol/claude-agent-acp@0.101.0`, login existente): **2 testes passaram** em 13,8 s (16,6 s com o build).

| Etapa | Resultado |
|---|---|
| `write_a_file_then_resume`: spawn → Ready | 1,2 s (`new_session` 2,7 s no outro teste) |
| Prompt (Write de `hello.txt`) | turno de 4,4 s (1º texto em 4,3 s); `Diff` visto; sem pedido de permissão |
| `load_session` + replay | 2,3 s; replay contém o prompt do usuário |
| Fork anunciado | sim; `fork_session` + `load_session`: 2,1 s, replay contém o prompt |
| `list_sessions` | 93 sessões |
| `answer_a_question`: `AskUserQuestion` | `QuestionAsked` após 2,2 s: "Pick a color: red or blue?" (`question_0` escolha única Red/Blue, `question_0_custom` texto) |
| Resposta "Red" | turno de 3,0 s, resposta final `Red` |

## Suíte

`cargo test --workspace`: **250 passaram, 0 falharam, 3 ignorados**.
