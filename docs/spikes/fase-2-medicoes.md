# Fase 2 — medições do wai

> 2026-11-08 · build release (`cargo build --release -p wai --features demo`), commit dcc8c11, neste Mac. Agente falso (`wai-fake-agent --reply-file … --chunk-delay-ms 25`, ~40 tokens/s) em `HOME` temporário (`settings.toml` e `WAI_STATE_PATH` próprios, projeto temporário com 5 arquivos `.rs` pequenos e um `big.txt` de 100 MB); nada de `~/.config/wai` foi tocado.

## Resultado

| Métrica | Meta | Fase 2 | Fase 1 | Central (S3) |
|---|---|---|---|---|
| CPU parado, 1 projeto + 3 sessões + terminal (30 s) | < 0,5% | **0,15%** (0,2 / 0,2 / 0,1 / 0,1 / 0,1 / 0,1) | 0,15% | ~8–9% |
| CPU parado, + 5 arquivos abertos no editor (30 s) | < 0,5% | **0,18%** (0,1 / 0,2 / 0,2 / 0,2 / 0,2 / 0,1) | — | — |
| RAM parado, sem arquivos (`top`) | < 300 MB | **72 MB** (RSS `ps`: 94 MB) | 127–133 MB | 33–95 MB só no servidor |
| RAM parado, 5 arquivos abertos (`top`) | < 300 MB | **119 MB** (RSS `ps`: 103 MB) | — | — |
| CPU com streaming a 40 tokens/s | < 10% | **7,5%** em média (6,8 / 6,0 / 7,5 / 6,7 / 9,4 / 8,3) | 6,4% | texto com atraso de até 2 s |
| RAM durante o streaming | < 300 MB | 241–244 MB (`top`), RSS 100 MB | 247–249 MB | — |
| Abrir arquivo de 100 MB (somente leitura) | sem travar além do `set_value` conhecido (~330 ms) | **436 ms** na thread principal (log do app); resto em segundo plano | — (S2: 340–380 ms) | n/a |
| RAM depois de abrir os 100 MB (+ 5 arquivos) | < 300 MB para o caso base | **637 MB** (`top`), RSS 886 MB; CPU parado 0,1% | — | n/a |
| Busca `fn ` neste repo (661 matches, 38 arquivos) | — | 1º lote 4,2 ms / total 13 ms (fria); 0,9 ms / 3,5 ms (quente) | — | n/a |
| Busca `fn ` em waycarbon-ecosystem-front (166 matches, 44 arquivos) | — | 1º lote 40 ms / total 358 ms (fria); 6,3 ms / 55 ms (quente) | — | n/a |
| Janela visível após abrir | < 1 s | **244–293 ms** (4 execuções: 262, 254, 244, 293) | 230–270 ms quente; 690 ms fria | n/a |

Metas: todas batidas, com ressalvas abaixo. Fora do orçamento por natureza: os 637 MB com o arquivo de 100 MB aberto (o orçamento de 300 MB é do caso base; o arquivo de 100 MB vira ~100 MB de texto mais o buffer e os índices do editor). O `set_value` de 436 ms ficou acima dos ~330 ms conhecidos (variação de execução única; não houve repetição).

## Método

- **CPU/RAM:** `top -l 7 -s 5 -pid <pid> -stats pid,cpu,mem`, descartando a 1ª amostra (6 amostras = 30 s), com ~12 s de espera após o lançamento. RSS por `ps -o rss=` logo depois. O estado (`state.json`) restaura 3 sessões em 2 painéis e o terminal visível.
- **5 arquivos:** `WAI_SHOT_OPEN=a.rs,b.rs,c.rs,d.rs,e.rs` (modo `demo` já existente; 1,1 ms a 98 µs cada na thread principal, de 107 a 523 linhas).
- **Streaming:** `WAI_SHOT_PROMPT=oi`, resposta de 3000 palavras a 25 ms por chunk; amostragem começa ~6 s depois do lançamento.
- **100 MB:** mesmos 5 arquivos mais `big.txt` (100 MB de linhas de ~60 bytes) em `WAI_SHOT_OPEN`; o tempo vem da linha `took … on the main thread` do app (`demo`).
- **Busca:** `crates/wai-project/tests/search_bench.rs` (`#[ignore]`, `cargo test --release -p wai-project --test search_bench -- --ignored --nocapture`), API `wai_project::search`, padrão `fn ` literal, sem diferenciar maiúsculas, respeitando `.gitignore`. "Fria" é a 1ª execução do processo; "quente" a 2ª, logo depois (cache de arquivos do SO; não foi possível limpar o cache sem `sudo purge`).
- **Janela:** `CGWindowListCopyWindowInfo` em laço de 5 ms (Swift) até existir janela do processo com largura > 500 px, contado do início do processo.

## Não medido / ressalvas

- **Lançamento a frio:** sem `sudo purge` não dá para esvaziar o cache de disco; as 4 execuções são quentes (a 1ª, 262 ms, é a mais próxima de fria). O número frio da Fase 1 (690 ms) não foi reproduzido.
- **RAM parado menor que na Fase 1** (72 contra ~130 MB): o `top` varia com memória comprimida e o estado restaurado aqui usa o agente falso (sem o Node do Claude). Não foi possível confirmar visualmente (sem acesso de acessibilidade) que as 3 sessões ficaram montadas; o app não registrou erro na restauração.
- A mensagem `could not list fake sessions: the agent process has exited` no log do streaming vem do encerramento do agente pelo script, ao final da medição.
- Dois números de memória do `top` e do `ps` divergem (o `top` inclui memória comprimida/compartilhada); a tabela traz os dois.
- Streaming com o Claude real não foi medido (cota). Medição feita com a tela possivelmente bloqueada.
- Busca: não há medição com a UI (só a API); a latência de pintar os resultados não foi medida.

## Smoke test com o Claude real

`cargo test -p wai-agent --test real_claude -- --ignored --nocapture` (`@agentclientprotocol/claude-agent-acp@0.101.0`, login existente): **2 testes passaram** em 16,5 s.

| Etapa | Resultado |
|---|---|
| `write_a_file_then_resume`: spawn → Ready, `new_session` | 1,8 s, 3,5 s |
| Prompt (Write de `hello.txt`) | turno de 5,2 s; `Diff` visto; arquivo contém `wai` |
| `load_session` + replay | 2,5 s; replay contém o prompt do usuário |
| Fork anunciado (`caps.fork`) | sim; `fork_session` + `load_session` da cópia: 2,5 s, replay contém o prompt |
| `list_sessions` | 90 sessões, contém o id |
| `answer_a_question`: `AskUserQuestion` | **`QuestionAsked` chegou** após 4,2 s: "Pick a color: red or blue?", campo `question_0` (escolha única: Red / Blue) e `question_0_custom` (texto opcional) |
| Resposta (1ª opção, "Red") | turno de 6,2 s, resposta final `Red`; `EndTurn` |
