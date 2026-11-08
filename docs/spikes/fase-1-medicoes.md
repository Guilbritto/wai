# Fase 1 — medições do wai

> 2026-11-08 · build release (`cargo build --release -p wai --features demo`), commit c1575f1, neste Mac. Agente falso (`wai-fake-agent --reply-file … --chunk-delay-ms 25`, ~40 tokens/s) em `HOME` temporário (`settings.toml` + `WAI_STATE_PATH` próprios), nada de `~/.config/wai` foi tocado.

## Resultado

| Métrica | Meta | Medido | Central (S3) |
|---|---|---|---|
| CPU parado (média de 30 s) | < 0,5% | **0,15%** (amostras 0,3 / 0,2 / 0,1 / 0,1 / 0,1 / 0,2) | ~8–9% |
| RAM, 1 projeto + 3 sessões + 1 terminal | < 300 MB | **127–133 MB** (coluna `mem` do `top`) | 33–95 MB só no servidor, sem a aba do Chrome |
| Janela visível após abrir | < 1 s | **230–270 ms** (quente); 690 ms na 1ª execução (fria) | n/a (aba do navegador) |
| CPU com streaming a 40 tokens/s | < 10% de um núcleo | **6,4%** em média (4,6 / 7,8 / 5,7 / 6,4 / 6,6 / 7,4) | texto com atraso de até 2 s |
| RAM durante o streaming | < 300 MB | 247–249 MB (1 sessão, resposta de 3000 palavras) | — |

## Método

- **CPU/RAM:** `top -l 7 -s 5 -pid <pid> -stats pid,cpu,mem`, descartando a 1ª amostra (6 amostras = 30 s). Parado: estado com projeto, 3 sessões abertas (2 painéis) e terminal visível, após ~10 s de uso.
- **Streaming:** `WAI_SHOT_PROMPT=oi`; o app abre uma sessão e envia o prompt, a resposta vem em 3000 chunks de 25 ms. Amostragem começa ~6 s após o lançamento e cobre 30 s do stream.
- **Janela:** `CGWindowListCopyWindowInfo` em laço de 5 ms (Swift) até existir janela do processo com largura > 500 px, contado do `fork`; 3 execuções: 690, 269, 230 ms.
- Processos filhos do agente falso: ~3–4 MB RSS cada (agente e shell do terminal).

## Não medido / ressalvas

- `footprint` travou neste ambiente, então a RAM é o valor do `top` (que inclui memória comprimida/compartilhada; `ps rss` deu ~100 MB no streaming, contra 248 MB do `top`). O valor do `top` é o maior, por isso o usado na tabela.
- Não há log de tempo de lançamento no app; o tempo é "janela existe", e não "primeiro quadro pintado".
- Sem acesso de acessibilidade, não deu para confirmar visualmente que as 3 sessões restauradas estavam todas montadas; o agente falso mostrou um processo (um agente por app, sessões multiplexadas).
- Streaming com o Claude real não foi medido (cota); o teste real abaixo cobre só o protocolo.
- Execução com a tela possivelmente bloqueada: o GPUI ainda desenha, mas não validamos a diferença para tela ativa.

## Smoke test com o Claude real

`cargo test -p wai-agent --test real_claude -- --ignored --nocapture` (`@agentclientprotocol/claude-agent-acp@0.101.0`, login existente do Claude Code): **passou** em 13,4 s.

| Etapa | Resultado |
|---|---|
| spawn → Ready | 1,6 s |
| `new_session` | 4,3 s |
| `set_mode("default")` | ok |
| Prompt (Write de `hello.txt`) | turno de 3,9 s, 1º texto após 3,8 s; `TurnEnded(EndTurn)` |
| Eventos | AgentText, ToolCall "Preparing file…" (kind edit) com Diff; **sem `PermissionRequested`** (as configurações do usuário usam `bypassPermissions`) |
| Arquivo | `hello.txt` contém `wai` |
| Shutdown → spawn → `load_session` | 2,4 s; replay contém o prompt do usuário |
| `list_sessions` | 94 sessões, contém o id |
