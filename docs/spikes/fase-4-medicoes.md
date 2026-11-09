# Fase 4 — medições do wai

> 2026-11-09 · `.app` empacotado (`CI=true cargo packager --release`), commit 23ef679, sem a feature `demo` e sem nenhuma mudança de código, neste Mac. Tudo medido de fora, com o executável do pacote aberto como pelo Finder (`env -i`, sem `TERM`, a partir de `/`) e também por `open -n`. Agente falso (`wai-fake-agent --reply-file … --chunk-delay-ms 25`), extensão `wai-waycarbon` (binário Go) instalada com consentimento gravado em `extensions.json`, `jira` falso (4 cards EC-9001..9004, inventados), tudo num `HOME` temporário; nada de `~/.config/wai` foi tocado.

## Resultado

| Métrica | Meta | Fase 4 (`.app`) | Fase 3 | Fase 2 | Fase 1 | Central (S3) |
|---|---|---|---|---|---|---|
| CPU parado, extensão ligada + 1 projeto + 3 sessões + terminal (30 s), aberto com `env -i` | < 0,5% | **0,16%** (6 execuções; 1ª abertura: 0,15 / 0,13 / 0,15; com cache: 0,17 / 0,17 / 0,18) | 0,16% | 0,15% | 0,15% | ~8–9% |
| CPU parado, mesmo cenário, aberto com `open -n` | < 0,5% | **0,12%** (0,13 / 0,12 / 0,10) | — | — | — | — |
| RAM do wai (`top`) | < 300 MB | **73–74 MB** (uma execução: 92 MB); `open -n`: 72–73 MB. `ps` RSS: 113–114 MB (`open -n`: 109–113 MB) | 72 MB (RSS 96–97 MB) | 72 MB | 127–133 MB | 33–95 MB só no servidor |
| RSS do processo da extensão (`wai-waycarbon`, Go) | — | **5,6–6,0 MB** | 5,7–6,2 MB | n/a | n/a | n/a |
| Janela visível, 1ª abertura (sem cache do `PATH`, espera o shell de login) | < 1 s | **261–297 ms** (262, 297, 261; shell de login: 26 ms) | não remedida | 244–293 ms | 230–270 ms quente; 690 ms fria | n/a |
| Janela visível, abertura com cache (R69) | < 1 s | **251–264 ms** (251, 264, 252) | não remedida | 244–293 ms | 230–270 ms | n/a |
| Janela visível por `open -n` (inclui o LaunchServices) | < 1 s | **326–362 ms** (1ª abertura 333; com cache 326, 362) | — | — | — | n/a |
| Janela visível com um rc lento (`sleep 1` no `.zprofile`) | < 1 s | 1ª abertura **1268–1306 ms** (shell 1033–1050 ms); com cache **240–470 ms** (470, 240, 240) | — | — | — | n/a |
| CPU com streaming a 40 tokens/s | < 10% | não remedido (inalterado desde a Fase 2) | — | 7,5% | 6,4% | texto com atraso de até 2 s |
| `wai.app` | — | **44 MB** (binário 45 806 688 bytes, `wai.icns` 650 702 bytes) | n/a | n/a | n/a | n/a |
| DMG (`wai_0.1.0_aarch64.dmg`) | — | **14,0 MB** (14 633 427 bytes) | n/a | n/a | n/a | n/a |

O pacote se comporta como o binário das fases anteriores: CPU parado e RAM iguais à Fase 3, janela em ~0,25–0,30 s. O `open -n` soma ~70–100 ms do LaunchServices. Neste `HOME` o shell de login leva 17–42 ms, então a R69 quase não aparece. Com um rc de 1 s, a R69 tira o tempo inteiro do shell da abertura: de 1,3 s na 1ª para 0,24 s nas seguintes. Todas as metas foram batidas.

## Método

- **Build:** `CI=true cargo packager --release` com o `HOME` real; o hook compila só o `wai` release, sem `demo`. Também `cargo build --release -p wai-agent --bins` e `go build` da `extensions/wai-waycarbon`, com a saída fora do repositório. `Info.plist`: `CFBundleIdentifier` `app.wai.ide`, `CFBundleShortVersionString` `0.1.0`, `CFBundleVersion` `20261109.153943` (carimbo do build), `CFBundleExecutable` `wai`. A assinatura é só a ad-hoc do linker, sem notarização.
- **Cenário** (`HOME` temporário, `<tmp>`):
  - `.config/wai/settings.toml` com `default_agent = "fake"` e `[agents.fake] command = "<repo>/target/release/wai-fake-agent --reply-file <tmp>/reply.txt --chunk-delay-ms 25"`;
  - `extensions.json` com `wai-waycarbon` ligada e `granted = [process, panels, sessions:control]`, e a pasta da extensão com o binário Go em `.config/wai/extensions/wai-waycarbon/`;
  - `bin/jira` falso, que registra cada chamada num arquivo;
  - um `.zprofile` de uma linha, `export PATH="$HOME/bin:$PATH"`, único rc do `HOME`;
  - 1 projeto temporário com 3 arquivos `.rs`;
  - `state.json` por `WAI_STATE_PATH`, copiado de novo antes de cada execução. Ele restaura 3 sessões do agente falso em 2 painéis, com o terminal visível.
- **Aberto como pelo Finder:** `cd / && env -i HOME=<tmp> PATH=/usr/bin:/bin:/usr/sbin:/sbin WAI_STATE_PATH=<tmp>/state.json <launcher> wai.app/Contents/MacOS/wai`, sem argumento (o projeto vem do estado). Com `ps eww`, o ambiente do wai tem só `HOME`, `PATH`, `WAI_STATE_PATH` e `__CF_USER_TEXT_ENCODING`, sem `TERM` nem `SHELL`. O `stderr` traz a linha que o wai já imprime fora do `demo`: `wai: PATH from login shell (14 entries, 26 ms)` ou `wai: PATH from cache (14 entries)`. Na "1ª abertura" o `shell-path` foi apagado antes; "com cache" usa o que o lançamento anterior gravou.
- **R68 de ponta a ponta:** o `jira` falso não está no `PATH` de lançamento, só no do shell de login (pelo `.zprofile`). Toda execução com o `.zprofile` chamou o `jira` 2 vezes (`jira me`, depois `jira issue list …`). Sem rc, foram 0 chamadas.
- **`open -n`:** `cd / && env -i HOME=<tmp> PATH=… WAI_STATE_PATH=… open -n --stderr <log> wai.app`. Ao contrário do esperado, dá para apontar o `open` para o `HOME` temporário, porque ele repassa o ambiente de quem chama (`man open`). Antes de abrir o wai, isso foi conferido com um app de teste que só grava o `env`. No wai, o `ps eww` confirmou `HOME` temporário e nenhum `TERM`, mais `SHELL`, `USER`, `TMPDIR` e `XPC_*` do launchd, como numa abertura pelo Finder. Assim o terminal abre em `/bin/zsh`; com `env -i`, sem `SHELL`, abre em `/bin/sh`. O tempo conta da chamada do `open`, e a janela é encontrada pelo nome do dono, `wai`.
- **Janela visível:** um launcher em Swift (`Process`) inicia o executável e consulta `CGWindowListCopyWindowInfo([.optionOnScreenOnly])` a cada 5 ms, até existir uma janela na tela, do pid, com largura > 500 px. O tempo conta de antes do `Process.run()`. É o método das Fases 1 e 2, mas exigindo que a janela esteja na tela.
- **CPU/RAM:** como nas fases anteriores, `top -l 7 -s 5 -pid <pid> -stats pid,cpu,mem` a partir de ~14 s após o lançamento, descartando a 1ª amostra (6 amostras = 30 s). Logo depois, o RSS do wai e de todos os descendentes por `ps -o rss=`. Os MB aqui são KB/1024.
- **rc lento:** um `.zprofile` com `sleep 1` antes do `export`, só para medir quanto a R69 economiza.
- **Binário novo:** duas cópias do `.app` (`cp -R`) numa pasta temporária, cada uma aberta 2 vezes com `env -i` e com cache.

## Amostras brutas (`top`, CPU % / mem; `ps` RSS em KB)

| Execução | 6 amostras de CPU | Média | mem | RSS wai | Janela |
|---|---|---|---|---|---|
| `env -i`, 1ª abertura 1 | 0,2 / 0,1 / 0,1 / 0,2 / 0,1 / 0,2 | 0,15% | 73–74M | 116 400 | 262,5 ms (shell 26 ms) |
| `env -i`, 1ª abertura 2 | 0,1 / 0,1 / 0,1 / 0,2 / 0,1 / 0,2 | 0,13% | 73M | 115 968 | 296,6 ms (shell 26 ms) |
| `env -i`, 1ª abertura 3 | 0,1 / 0,1 / 0,2 / 0,2 / 0,2 / 0,1 | 0,15% | 74M | 116 704 | 261,3 ms (shell 26 ms) |
| `env -i`, com cache 1 | 0,2 / 0,1 / 0,1 / 0,3 / 0,1 / 0,2 | 0,17% | 73M | 116 272 | 250,5 ms |
| `env -i`, com cache 2 | 0,1 / 0,1 / 0,2 / 0,3 / 0,1 / 0,2 | 0,17% | 92M | 116 336 | 263,7 ms |
| `env -i`, com cache 3 | 0,1 / 0,2 / 0,1 / 0,3 / 0,3 / 0,1 | 0,18% | 73M | 115 696 | 251,6 ms |
| `open -n`, 1ª abertura | 0,2 / 0,1 / 0,1 / 0,2 / 0,1 / 0,1 | 0,13% | 73M | 115 360 | 333,1 ms (shell 36 ms) |
| `open -n`, com cache 1 | 0,2 / 0,1 / 0,1 / 0,1 / 0,1 / 0,1 | 0,12% | 72M | 115 360 | 326,4 ms |
| `open -n`, com cache 2 | 0,1 / 0,1 / 0,1 / 0,1 / 0,1 / 0,1 | 0,10% | 73M | 111 184 | 362,4 ms |

RSS dos filhos (`ps`, KB):
- `wai-waycarbon`: 5856 / 5712 / 5856 / 5792 / 5984 / 6192 (`env -i`) e 5776 / 5952 / 5968 (`open -n`);
- `wai-fake-agent`, um processo para as 3 sessões: 3616–3712;
- shell do terminal: `/bin/sh` 2096–2128 com `env -i`, `/bin/zsh` 2816 com `open -n`.

Só a janela (`env -i`, sem `top`):

| Série | Janela (ms) | Shell de login |
|---|---|---|
| 1ª abertura, `HOME` sem nenhum rc | 297,4 / 268,4 / 260,1 | 42 / 31 / 17 ms (13 entradas; 0 chamadas ao `jira`) |
| 1ª abertura, `.zprofile` com `sleep 1` | 1268,2 / 1304,1 / 1305,5 | 1033 / 1044 / 1050 ms |
| Com cache, `.zprofile` com `sleep 1` | 469,6 / 239,7 / 240,4 | — (refresh em segundo plano) |
| 1ª execução do binário logo após o build (1ª abertura) | 1614,2 | 25 ms |
| Cópia nova A do `.app`, 1ª e 2ª execução (com cache) | 1270,5 / 232,5 | — |
| Cópia nova B do `.app`, 1ª e 2ª execução (com cache) | 693,3 / 228,8 | — |
| 1º `open -n` do `.app` recém-construído (com cache) | 721,7 | — |

## Não medido / ressalvas

- **Streaming a 40 tokens/s:** não remedido (inalterado desde a Fase 2). Sem `demo` não há como enviar um prompt de fora. Não existe CLI para isso, o `sessions/open` da extensão chega com `send: false` e depende de um clique, e esta sessão não tem permissão de Acessibilidade para digitar.
- **1ª execução de um binário novo:** custa de 0,5 a 1,4 s a mais, antes do código do wai: 1614 ms logo após o build, com 25 ms de shell, e 1270 e 693 ms nas cópias novas, contra 229–233 ms na 2ª execução. A causa provável é a verificação que o macOS faz na 1ª execução de um arquivo novo, então isso vale para a 1ª abertura depois de copiar o app do DMG. O DMG baixado da internet, com quarentena e Gatekeeper, não foi medido. O "~1,6 s só na 1ª abertura" que o relatório final atribui ao `.zshrc` (R68) é da mesma ordem que esse custo. Daqui não dá para separar quanto daquele número era do `.zshrc`.
- **`.zshrc` real:** não medido, porque o `HOME` real não é usado. No `HOME` temporário o shell de login leva 17–42 ms. O ganho da R69 é o tempo do rc de cada um, simulado aqui com `sleep 1`.
- **`env -i` não é igual ao Finder:** faltam `SHELL`, `USER`, `TMPDIR` etc. Sem `SHELL`, o terminal do wai abre `/bin/sh` e o shell de login cai no padrão `/bin/zsh`. O `open -n` cobre essa diferença e deu CPU e RAM iguais.
- **Painel "My cards":** a aba não foi aberta, porque o `WAI_SHOT_PANEL` da Fase 3 só existe sob `demo`. A extensão roda e chama o `jira`, mas o painel não fica à frente. Esta é a diferença de cenário em relação à Fase 3.
- **RSS do wai:** 113–114 MB, contra 96–97 MB na Fase 3, com o `top` igual (72–74 MB). Não foi investigado. O binário cresceu (a checagem de atualização, desligada por padrão, trouxe a pilha HTTPS), e o RSS conta páginas mapeadas do binário. Um `top` de 92 MB numa execução, com o RSS igual às outras, é ruído de memória comprimida, como na Fase 3.
- **Janela visível** significa uma janela na tela com mais de 500 px, não "1º quadro pintado". Não houve captura de tela (sem permissão de Gravação de Tela). Lançamento a frio também não foi medido: sem `sudo purge`, não dá para limpar o cache de disco.
- **`jira` real:** não usado; foi usado o falso.

## Smoke test com o Claude real

`cargo test -p wai-agent --test real_claude -- --ignored --nocapture` (`@agentclientprotocol/claude-agent-acp@0.101.0`, login existente do Claude Code; o wai não lê credenciais): **2 testes passaram** em 17,3 s (22 s com o build).

| Etapa | Resultado |
|---|---|
| `write_a_file_then_resume`: spawn → Ready, `new_session` | 1,3 s, 6,0 s |
| Prompt (Write de `hello.txt`) | turno de 3,3 s (1º texto em 3,2 s); `Diff` visto; sem pedido de permissão; `hello.txt` contém `wai` |
| `load_session` + replay | 2,7 s; replay contém o prompt do usuário |
| Fork anunciado | sim; `fork_session` + `load_session`: 2,3 s, replay contém o prompt |
| `list_sessions` | 101 sessões |
| `answer_a_question`: `AskUserQuestion` | `QuestionAsked` após 2,0 s: "Pick a color: red or blue?" (`question_0` escolha única Red/Blue, `question_0_custom` texto) |
| Resposta "Red" | turno de 3,1 s, resposta final `Red` |

## Suíte

`cargo test --workspace`: **283 passaram, 0 falharam, 3 ignorados**.
