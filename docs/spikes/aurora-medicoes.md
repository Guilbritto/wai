# Revisão visual "Aurora" — medições do wai

> 2026-11-09 · commit 0d1f43b (branch `feat/visual-aurora`), neste Mac. Para o `.app` (`CI=true cargo packager --release`, sem a feature `demo`), o método é o da Fase 4. Para o streaming (build release com `demo`), o das Fases 1 e 2. Agente falso (`wai-fake-agent --reply-file … --chunk-delay-ms 25`), extensão `wai-waycarbon` (binário Go) com consentimento gravado, `jira` falso (4 cards EC-9001..9004, inventados), tudo num `HOME` temporário. Nada de `~/.config/wai` foi tocado.

## Resultado

Só entram as execuções limpas (ver "Condições").

| Métrica | Meta | Aurora | Fase 4 (`.app`) | Fase 2 | Fase 1 |
|---|---|---|---|---|---|
| CPU parado, extensão ligada + 1 projeto + 3 sessões + terminal (30 s), aberto com `env -i`, janela visível | < 0,5% | **0,20%** (9 execuções, 0,17–0,23%) | 0,16% | 0,15% | 0,15% |
| CPU parado, mesmo cenário, aberto com `open -n` | < 0,5% | **0,12%** (5 execuções, 0,10–0,13%) | 0,12% | — | — |
| RAM do wai parado (`top`) | < 300 MB | **78–80 MB** (uma execução: 97 MB). `ps` RSS: 98–114 MB | 73–74 MB (RSS 113–114 MB) | 72 MB | 127–133 MB |
| RSS do processo da extensão (`wai-waycarbon`) | — | **5,5–5,8 MB** (duas execuções: ~3 MB) | 5,6–6,0 MB | n/a | n/a |
| CPU com streaming a 40 tokens/s, janela visível | < 10% | **5,1%** (6 execuções, médias 4,98–5,32%; amostras 3,6–7,4%) | não remedido | 7,5% | 6,4% |
| RAM durante o streaming | < 300 MB | 207–255 MB (`top`), RSS 80–95 MB | — | 241–244 MB (RSS 100 MB) | 247–249 MB |
| Janela visível, 1ª abertura (espera o shell de login) | < 1 s | **301–365 ms** (6 execuções; shell 26–39 ms) | 261–297 ms | 244–293 ms | 230–270 ms |
| Janela visível, abertura com cache (R69) | < 1 s | **270–352 ms** (6 execuções) | 251–264 ms | — | — |
| Janela visível por `open -n` | < 1 s | **325–375 ms** (5 execuções; 1ª abertura só uma, 365; com cache 325–375) | 326–362 ms | — | — |
| `wai.app` / DMG | — | **45 MB** (binário 46 296 384 bytes) / **14,2 MB** (14 843 347 bytes) | 44 MB / 14,0 MB | n/a | n/a |

Todas as metas foram batidas, e o fundo estático de reserva da Tarefa 2 (um PNG parado) não foi preciso.
- **Streaming:** caiu de 6,4–7,5% (Fases 1 e 2) para ~5%. A aurora e as sombras dos cartões são pintadas em todo quadro, mas pela GPU, então provavelmente não pesam na CPU. Isso não foi investigado, nem a queda em si.
- **Parado:** 0,20% com `env -i`, um pouco acima da Fase 4 (0,13–0,18%), e igual por `open -n`. A causa provável é que cada piscada do cursor redesenha a janela, agora com a aurora e as sombras.
- **RAM:** pelo `top`, ~5 MB a mais que na Fase 4 (78–80 contra 73–74 MB). Só o RSS do `ps` ficou no mesmo nível (98–114 contra 113–114 MB).
- **Janela:** aparece ~50 ms mais tarde (0,30–0,36 s contra 0,25–0,30 s). Não separei a causa: o tema Aurora e a fonte Nunito agora carregam na abertura, e este Mac estava ocupado (ver "Condições").

## Condições desta rodada

Ao contrário das fases anteriores, o Mac estava em uso durante as medições:
- o seu wai (`wai-dist/wai.app`) estava aberto e parado em segundo plano no começo, com o Claude de uma sessão dele, ao lado da Central, do Arc e do Teams. Ele cobriu janelas de teste em execuções descartadas e saiu em algum momento que não foi registrado;
- entre 22:11 e 22:13, uma cópia retomada desta sessão (aberta no seu wai) rodou medições ao mesmo tempo. Tudo o que foi medido até 22:13 foi descartado e refeito, e os arquivos dela foram apagados;
- antes de você parar de usar o Mac (22:48–22:58), janelas suas cobriram as de teste. Em algumas execuções, teclas ou cliques chegaram a elas: a memória pulou para ~250 MB, como num streaming; uma janela fechou sozinha aos 11 s; outra recarregou o painel.

Por isso cada execução passou a registrar quatro coisas, e só conta a que passa nas quatro:
- **visibilidade:** a cada 1 s, pelo `CGWindowListCopyWindowInfo`, quanto da janela as janelas na frente cobrem. Conta só a que fica descoberta durante as amostras;
- **prompts recebidos:** o comando do agente falso passa por um `tee` que conta só o nome do método (`session/prompt`), nunca o conteúdo. Parado tem de dar 0, e streaming, 1;
- **viva no fim;**
- **2 chamadas ao `jira`**, ou seja, o painel carregou uma vez só.

Coberta por uma janela opaca, a janela do wai não é pintada, porque o GPUI para de desenhar. O streaming então marca 1,0–1,7%, e o parado 0,0%. O parado também deu 0,0% com a janela visível mas, pelo jeito, sem foco (o cursor não pisca). A meta vale para a janela visível e com foco, o pior caso, e é esse o número da tabela.

## Método

- **Build:** `CI=true cargo packager --release` com o `HOME` real, do commit 0d1f43b. O hook compila só o `wai` release, sem `demo`, e o binário do `.app` não tem as strings do modo roteirizado. Também:
  - `cargo build -p wai --release --features demo`, para o streaming;
  - `cargo build --release -p wai-agent --bins`;
  - `go build` da `extensions/wai-waycarbon`, com a saída fora do repositório.

  A assinatura é só a ad-hoc do linker.
- **Cenário parado** (o da Fase 4, num `HOME` temporário):
  - `settings.toml` com `default_agent = "fake"`;
  - `extensions.json` com `wai-waycarbon` ligada (`process`, `panels`, `sessions:control`);
  - `bin/jira` falso e um `.zprofile` de uma linha, `export PATH="$HOME/bin:$PATH"`;
  - projeto `acme-api` (3 arquivos `.rs`, `Cargo.toml`, `README.md`);
  - `state.json` por `WAI_STATE_PATH`, copiado de novo antes de cada execução. Ele restaura 3 sessões do agente falso em 2 painéis, com o terminal visível.
- **Aberto como pelo Finder:** `cd / && env -i HOME=<tmp> PATH=/usr/bin:/bin:/usr/sbin:/sbin WAI_STATE_PATH=<tmp>/state.json <launcher> wai.app/Contents/MacOS/wai`, e também por `open -n`. O launcher do `open -n` ignora as janelas "wai" que já existiam (o seu wai tem o mesmo nome).
- **R68 de ponta a ponta:** toda execução que conta chamou o `jira` 2 vezes. O `jira` falso só está no `PATH` do `.zprofile`.
- **Janela visível:** o launcher em Swift da Fase 4. Ele consulta `CGWindowListCopyWindowInfo` a cada 5 ms, até existir uma janela do processo, na tela, com mais de 500 px.
- **CPU/RAM:** `top -l 7 -s 5 -pid <pid> -stats pid,cpu,mem` a partir de ~14 s após o lançamento, descartando a 1ª amostra (6 amostras = 30 s). Logo depois, o RSS do wai e dos filhos por `ps -o rss=`.
- **Streaming:** o método das Fases 1 e 2. Com `WAI_SHOT_PROMPT=oi`, o build com `demo` abre uma sessão e manda o prompt. A resposta tem 3000 palavras, a 25 ms cada (~40 tokens/s), sem estado salvo. O `top` começa ~6 s após o lançamento e faz as mesmas 7 amostras de 5 s.
- **Segurança:** cada script só sinaliza o processo que ele mesmo iniciou e os filhos desse processo. Antes de matar, confere que o executável é o do `.app` de teste. Antes de começar, espera se houver outro wai de teste rodando.

## Amostras brutas (`top`, CPU %; `ps` RSS em KB)

Parado, `.app`, execuções que contam (janela descoberta, 0 prompts, 2 chamadas ao `jira`, vivo no fim):

| Execução | 6 amostras de CPU | Média | `top` mem | RSS wai | Janela |
|---|---|---|---|---|---|
| `env -i`, 1ª abertura 1 | 0,2 / 0,1 / 0,2 / 0,2 / 0,2 / 0,2 | 0,18% | 79M | 112 352 | 351,6 ms (shell 27 ms) |
| `env -i`, 1ª abertura 2 | 0,2 / 0,2 / 0,1 / 0,2 / 0,2 / 0,2 | 0,18% | 78M | 116 144 | 301,1 ms (shell 26 ms) |
| `env -i`, 1ª abertura 3 | 0,2 / 0,2 / 0,2 / 0,2 / 0,1 / 0,1 | 0,17% | 78M | 115 936 | 347,6 ms (shell 39 ms) |
| `env -i`, 1ª abertura 4 | 0,1 / 0,2 / 0,2 / 0,3 / 0,2 / 0,2 | 0,20% | 79M | 117 152 | 324,4 ms (shell 26 ms) |
| `env -i`, 1ª abertura 5 | 0,3 / 0,2 / 0,1 / 0,2 / 0,2 / 0,2 | 0,20% | 78M | 116 208 | 303,7 ms (shell 26 ms) |
| `env -i`, 1ª abertura 6 | 0,2 / 0,2 / 0,4 / 0,3 / 0,2 / 0,1 | 0,23% | 79M | 100 528 | 364,7 ms (shell 28 ms) |
| `env -i`, com cache 1 | 0,2 / 0,1 / 0,3 / 0,3 / 0,1 / 0,2 | 0,20% | 79M | 112 224 | 284,2 ms |
| `env -i`, com cache 2 | 0,2 / 0,2 / 0,2 / 0,1 / 0,2 / 0,2 | 0,18% | 97–98M | 111 408 | 351,8 ms |
| `env -i`, com cache 3 | 0,3 / 0,1 / 0,2 / 0,3 / 0,2 / 0,2 | 0,22% | 79–80M | 112 592 | 270,1 ms |
| `open -n`, 1ª abertura | 0,1 / 0,1 / 0,1 / 0,1 / 0,2 / 0,2 | 0,13% | 78M | 111 024 | 365,2 ms (shell 40 ms) |
| `open -n`, com cache 1 | 0,2 / 0,1 / 0,1 / 0,1 / 0,1 / 0,1 | 0,12% | 78–79M | 111 264 | 357,2 ms |
| `open -n`, com cache 2 | 0,1 / 0,1 / 0,2 / 0,1 / 0,1 / 0,2 | 0,13% | 79M | 100 416 | 324,5 ms |
| `open -n`, com cache 3 | 0,1 / 0,1 / 0,1 / 0,1 / 0,1 / 0,1 | 0,10% | 78–79M | 111 584 | 338,6 ms |
| `open -n`, com cache 4 | 0,1 / 0,1 / 0,1 / 0,1 / 0,1 / 0,1 | 0,10% | 79M | 111 728 | 375,4 ms |

RSS dos filhos nessas execuções (`ps`, KB):
- `wai-waycarbon`: 5616–5952 (duas execuções: 3152 e 3024);
- `wai-fake-agent`, um processo para as 3 sessões: 3632–3728 (uma execução: 2880). Some ~1,5 MB do `tee` que conta os prompts;
- shell do terminal: `/bin/sh` 2112–2128 com `env -i`, `/bin/zsh` 2816 com `open -n`.

Streaming a 40 tokens/s, build com `demo`, execuções que contam (janela descoberta nas 6 amostras; as duas últimas também com 1 prompt contado):

| Execução | 6 amostras de CPU | Média | `top` mem | RSS wai |
|---|---|---|---|---|
| 1 | 3,6 / 4,3 / 6,4 / 4,9 / 5,1 / 5,6 | 4,98% | 207–249M | 92 400 |
| 2 | 3,7 / 4,3 / 6,7 / 4,7 / 5,2 / 5,3 | 4,98% | 207–249M | 91 792 |
| 3 | 3,6 / 4,4 / 6,9 / 5,2 / 5,1 / 5,4 | 5,10% | 226–248M | 81 632 |
| 4 | 3,7 / 4,3 / 6,3 / 4,9 / 5,2 / 5,5 | 4,98% | 227–249M | 92 208 |
| 5 | 3,8 / 4,7 / 6,3 / 5,1 / 5,3 / 5,9 | 5,18% | 207–249M | 92 240 |
| 6 | 3,9 / 4,4 / 7,4 / 5,0 / 5,6 / 5,6 | 5,32% | 214–255M | 97 088 |

A CPU sobe com o tamanho da resposta: ~3,7% no começo e ~5,5% depois de ~1300 palavras. O pico de 6,3–7,4% vem sempre na 3ª amostra, quando a memória salta de ~208 para ~248 MB.

Execuções descartadas (depois de 22:13):
- **Parado, antes do contador de prompts** (9 execuções):
  - 3 tiveram a memória pulando para ~250 MB no meio, um streaming que ninguém roteirizou; uma delas também teve 4 chamadas ao `jira`;
  - 1 fechou aos 11 s;
  - 2 tiveram a janela coberta (pelo seu wai e pela Central), com 0,0%;
  - 2 tiveram a janela visível e 0,0%, provavelmente sem foco;
  - 1 deu 0,17%, mas foi descartada por ser da rodada sem contador.
- **Parado, com o contador** (4 execuções):
  - 2 com a janela coberta pelo Arc, com 0,0%;
  - 1 com a janela fora da tela a partir dos 17 s;
  - 1 em que a Central passou na frente na última amostra (0,8%; as outras 5 deram 0,1–0,3%).
- **Streaming** (4 execuções): a janela foi coberta pelo seu wai ou pela Central em parte do tempo ou o tempo todo. As amostras cobertas marcam 1,0–1,7%. As que pegam a janela voltando para a frente marcam 7,5–7,8%, porque o quadro inteiro é repintado.

Só a janela, todas as execuções depois das 22:13 (o tempo até a janela aparecer vem antes de qualquer interferência):

| Série | Janela (ms) |
|---|---|
| 1ª execução do binário logo após o build (1ª abertura) | 1668,5 |
| 1ª abertura seguinte, sem medir CPU | 558,0 |
| `env -i`, 1ª abertura | 381,4 / 347,0 / 309,1 / 351,6 / 301,1 / 347,6 / 324,4 / 303,7 / 364,7 |
| `env -i`, com cache | 498,1 / 302,1 / 291,5 / 303,3 / 300,3 / 308,9 / 284,2 / 351,8 / 270,1 |
| `open -n`, 1ª abertura | 389,7 / 412,5 / 365,2 |
| `open -n`, com cache | 344,5 / 356,6 / 357,2 / 324,5 / 338,6 / 375,4 |

As 13 execuções descartadas do parado (9 + 4, acima) são as primeiras de cada série: 3 na 1ª abertura e 6 com cache no `env -i`, e 2 em cada série do `open -n` (389,7 e 412,5 ms na 1ª abertura; 344,5 e 356,6 ms com cache). As que contam são as seguintes. O 498 ms é dessa rodada descartada.

## Barra de título no macOS (passo 4 do plano)

- **Você conferiu à mão**, no preview do commit 7099b86 (`wai-dist/wai.app`): arrastar pela barra do topo move a janela, e o duplo clique maximiza. Depois de 7099b86, nenhum commit mexeu em `topbar.rs`, `app.rs` ou `workspace.rs`; os seguintes só tocam o chat, `aurora.rs` e o painel de extensão. Então o resultado vale para 0d1f43b.
- **Eu conferi**, sem permissão de Acessibilidade nem de Gravação de Tela:
  - pelo `CGWindowListCopyWindowInfo`, o `.app` abre uma janela só, de 1400 × 883 em (56, 33). O topo fica colado na barra de menus, sem uma barra de título nativa por cima;
  - pelo código e pela captura fora da tela, a `TitleBar` deixa 80 pt livres à esquerda, e a marca começa em x = 82 pt (medido em `aurora-light.png`). O GPUI põe o botão de fechar em x = 9 pt (`traffic_light_position`). Com o tamanho padrão do AppKit (botões de 14 pt, um a cada 20 pt), os três acabam por volta de x = 63 pt e sobram ~19 pt até a marca. Isso é conta, não pixel da tela real;
  - em tela cheia, o GPUI devolve os semáforos ao lugar nativo. A `TitleBar` mantém os seus 80 pt e soma mais 12 pt (`pl_3`) por dentro, o que deixava 92 pt vazios à esquerda. A barra do topo agora zera os 80 pt só em tela cheia (`pl_0`, que vale porque a `TitleBar` aplica o estilo de quem a usa depois do dela), e sobram os 12 pt. Na janela normal nada muda.
- **Não deu para conferir daqui:**
  - a foto da janela real: `screencapture -l` responde "could not create image from window", porque falta a permissão de Gravação de Tela;
  - a tela cheia: o `osascript` com `AXFullScreen` foi recusado ("osascript is not allowed assistive access", -1719);
  - arrastar e o duplo clique no build atual, que pedem Acessibilidade.
- **Falta você conferir à mão:**
  - que os semáforos não encostam na marca;
  - que a tela cheia (botão verde) não deixa faixa vazia no topo.

## Não medido / ressalvas

- **Mac em uso:** os números que contam vêm de execuções limpas. Delas, 8 das 14 do parado e 2 das 6 do streaming rodaram com você longe do Mac (22:48–22:58). Mesmo assim o Arc, o Teams e a Central rodavam ao lado, e talvez o seu wai, que saiu em algum momento não registrado. Isso pode explicar parte dos ~50 ms a mais na janela.
- **Foco:** o parado só passa de 0,0% com a janela visível e com foco. A tabela traz esse caso. Que a janela estava com foco é inferência, porque o monitor de "app da frente" (`NSWorkspace.frontmostApplication` num processo sem run loop) não é confiável e não foi usado.
- **Sem captura da janela real:** falta a permissão de Gravação de Tela. As telas `docs/screens/aurora-*.png` vêm da captura fora da tela do próprio wai (feature `screenshot`).
- **Lançamento a frio** e o `.zshrc` real não foram medidos: sem `sudo purge`, e o `HOME` real não é usado.
- **Streaming com o Claude real:** não medido (cota), como nas fases anteriores.
