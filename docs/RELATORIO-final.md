# wai — Relatório final (Fases 0 a 4)

> 2026-11-09 · branch `feat/fase-4-release` · 283 testes automáticos em Rust + 15 em Go (8 em `wai-rotinas`, 7 em `wai-waycarbon`) passando depois da revisão final (+ 2 opt-in com o Claude real)

O wai é uma IDE nativa (Rust + GPUI), IA primeiro, para macOS e Windows.
- Conversa com agentes pelo ACP. O Claude Code entra pelo `claude-agent-acp`, com o login que você já tem.
- O wai nunca toca em credencial.
- Tem extensões em qualquer linguagem (processo + JSON-RPC por stdio), com painéis nativos.

Está pronto para publicar, **menos o que só você pode fazer**: licença, remoto, certificados, confirmação com a Anthropic e as escolhas de identidade. Isso está na [lista do fim](#o-que-só-você-pode-fazer).

Documentação nova (em inglês):
- `README.md`;
- `docs/user-guide.md`;
- `docs/extensions.md` (guia de autor de extensão, com exemplos mínimos em Go e Rust);
- `docs/building.md`.

## O que foi entregue

### Fase 0: spikes

| Spike | Resultado |
|---|---|
| S1 · ACP com o Claude | **GO**. Funciona com a sua assinatura, sem API key. `session/list` e `load_session` substituem ler `~/.claude/projects` |
| S2 · GPUI | **GO com 4 condições**, que viraram regra: editor sem soft wrap; cursor que para de piscar; no máximo um redesenho por quadro; arquivo > 20 MB só leitura |
| S3 · Central atual | Linha de base: ~8–9% de um núcleo parada; texto com até 2 s de atraso |

### Fase 1: núcleo (macOS)

| Área | O que dá para fazer |
|---|---|
| Sessões | Criar (⌘N), retomar do histórico, fechar (⌘W), até 4 painéis lado a lado |
| Chat | Markdown em tempo real, pensamento recolhido, ferramentas com saída limitada, diffs |
| Permissões | Cartão com o diff e as opções do agente; Esc interrompe o turno |
| Anexos | Arrastar imagem (≤ 5 MB) ou arquivo |
| Barra lateral | Status por sessão (Working / Needs you / Error) e histórico do projeto ou de todos |
| Terminal | Shell de verdade no painel de baixo (ctrl-`) |
| Avisos | Notificação do sistema quando uma sessão precisa de você ou termina com a janela em segundo plano |
| Retomar | Projeto, painéis, sessões e terminal voltam ao reabrir |

### Fase 2: IDE

| Área | O que dá para fazer |
|---|---|
| Segurança | O agente roda a partir da pasta de configuração, nunca de dentro do repositório |
| Perguntas e fork | AskUserQuestion vira formulário. Fork de sessão aberta ou do histórico |
| Arquivos e editor | Árvore com `.gitignore`, abas, salvar atômico, só leitura > 20 MB, "Reload / Keep mine" |
| Revisão | Bloco "Edits (N files)" com Open e Revert seguro |
| Busca, agentes, modelos | Busca com regex em streaming, aba Agents, modelos de conversa em `templates.toml` |

### Fase 3: extensões + Windows

| Área | O que dá para fazer |
|---|---|
| Manifesto e instalação | `wai-extension.toml` validado. Instala de pasta ou `git`, com consentimento que lista cada permissão |
| Host | Processos em qualquer linguagem, JSON-RPC por stdio, cada chamada conferida contra o que você aprovou |
| Painéis e paleta | Painéis declarativos nativos. Paleta ⌘⇧P com ações, modelos, comandos e agentes |
| wai-waycarbon | Modelos da Way, plugin `beehive-waycarbon` sugerido, painel "My cards" via o seu `jira` |
| Windows | Código pronto (caminhos, CRLF, ConPTY, atalhos do terminal) e CI montada |

### Fase 4: publicação

| Tarefa | O que entrou |
|---|---|
| 4.0 Recuperar edições | Buffers sujos guardados ~1 s depois de parar de digitar e em toda saída (inclusive Dock e logout). Voltam como abas com "Recovered unsaved changes". Também resolveu as sobras da R57 |
| 4.1 Empacotar | `wai.app` + DMG pelo `cargo-packager`, configuração do instalador NSIS, ícone próprio. Notificações com a identidade do próprio app |
| 4.2 Release | Workflow por tag `v*`: confere tag = versão, gera os pacotes das duas plataformas e um **rascunho** de release com `SHA256SUMS.txt`. Assina e notariza só se os segredos existirem |
| 4.3 Atualização | Checagem **desligada por padrão**, uma vez por abertura, só avisa (`wai <versão> is available`). Nunca baixa nada |
| 4.4 Extensões próprias | `wai-navegador` (MCP do Playwright via `npx`), `wai-rotinas` (scripts agendados de uma lista sua) e `wai-remoto` (só o desenho) |
| 4.5 Documentação | README, guia do usuário, guia de extensões, este relatório |
| 4.5.1 PATH pelo Finder | Aberto pelo Finder ou Dock, o wai pega o `PATH` do seu shell de login, com cache (R68, R69). Sem isso, o agente padrão (`npx`) não abria no 1º uso do DMG |
| 4.6 Revisão final | Revisão do branch inteiro, uma leva de correções (R70), medições do `.app` e teste com o Claude real |

## Números ao longo das fases

Build release, neste Mac.

| Métrica | Meta | Central | S2 (Fase 0) | Fase 1 | Fase 2 | Fase 3 | Fase 4 (`.app`) |
|---|---|---|---|---|---|---|---|
| CPU parado | < 0,5% | ~8–9% | 0,0% | **0,15%** | **0,15%** (0,18% com 5 arquivos) | **0,16%** (extensão ligada) | **0,16%** (0,12% por `open -n`) |
| Memória (1 projeto, 3 sessões, terminal) | < 300 MB | servidor 33–95 MB + aba do Chrome | 153 MB (spike, sem sessões) | 127–133 MB | **72 MB** (119 MB com 5 arquivos) | **72 MB** | **73–74 MB** |
| Janela visível | < 1 s | aba do navegador | ~180 ms | 0,23–0,27 s (0,69 s a frio) | 0,24–0,29 s | painel "My cards" em 0,22–0,29 s | **0,25–0,30 s** (0,33–0,36 s por `open -n`) |
| CPU com streaming a 40 tokens/s | < 10% | texto com até 2 s de atraso | 14–17% | 6,4% | 7,5% | — | — |
| Arquivo de 100 MB | sem travar | não existe | rolagem 0,6–2,6 ms/quadro | — | abre em 436 ms, só leitura | — | — |
| Listar conversas | — | varre 1,2 GB de jsonl | 74 ms (94 sessões) | pelo protocolo | — | 93 sessões | 101 sessões |
| Busca (`fn ` no front da Way) | — | — | — | — | 1º resultado em 6 ms, total 55 ms | — | — |
| Processo de uma extensão | — | — | — | — | — | ~6 MB | ~6 MB |
| Testes automáticos | — | — | — | 71 | 159 | 250 | 283 + 15 em Go |

Na Fase 4, os 15 testes em Go são 8 do `wai-rotinas` e 7 do `wai-waycarbon`. O detalhe do `.app` fica na seção abaixo.

## Fase 4: medições do .app e revisão final

### Medições do `.app`

Detalhe e amostras brutas em `docs/spikes/fase-4-medicoes.md`. O `.app` foi gerado pelo `cargo-packager` do commit 23ef679, sem a feature `demo`, e aberto como pelo Finder: com `env -i`, sem `TERM`, a partir de `/`, e também por `open -n`. Tudo rodou num `HOME` temporário, com agente falso, `wai-waycarbon` ligada e um `jira` falso.

| Métrica | Meta | Resultado |
|---|---|---|
| CPU parado (30 s) | < 0,5% | **0,16%** (`open -n`: 0,12%) |
| RAM do wai (`top`) | < 300 MB | **73–74 MB** (RSS do `ps`: 113–114 MB) |
| Janela visível, 1ª abertura (espera o shell de login) | < 1 s | **261–297 ms** |
| Janela visível, com o `PATH` em cache (R69) | < 1 s | **251–264 ms** |
| Janela visível por `open -n` | < 1 s | **326–362 ms** |
| Com um rc de 1 s (`sleep 1` no `.zprofile`) | — | 1ª abertura 1,27–1,31 s; com cache 0,24–0,47 s |
| `wai.app` / DMG | — | **44 MB** / **14,0 MB** |

O que isso mostra:
- **A R68 funciona de ponta a ponta.** O `jira` falso só estava no `PATH` do `.zprofile`, e toda abertura com ele chamou o `jira`. Sem o rc, nenhuma chamada.
- **A R69 tira o shell da abertura.** Com um rc de 1 s, só a 1ª abertura espera; as seguintes ficam em 0,24 s.
- **Novo: a 1ª execução de uma cópia nova do app custa 0,5–1,4 s a mais**, antes de qualquer código do wai. A 2ª execução da mesma cópia leva ~0,23 s. A causa provável é a verificação que o macOS faz num executável novo, então isso aparece na 1ª abertura depois de copiar o app do DMG. Um DMG baixado, com quarentena e Gatekeeper, não foi medido.
- **O RSS subiu ~17 MB desde a Fase 3** (96–97 para 113–114 MB), com o `top` igual. Não investiguei. A causa provável é o binário maior: a checagem de atualização trouxe a pilha HTTPS, e o RSS conta páginas mapeadas do binário.

Não medido: CPU com streaming a 40 tokens/s (sem `demo` não há como mandar um prompt de fora; o caminho não mudou desde a Fase 2), o painel "My cards" à frente, lançamento a frio e o `.zshrc` real.

### Teste com o Claude real

`cargo test -p wai-agent --test real_claude -- --ignored --nocapture`, com o `claude-agent-acp@0.101.0` e o seu login (o wai não lê credenciais): **2 de 2 passaram** em 17,3 s.
- Escrita de `hello.txt` num turno de 3,3 s, com `Diff` e sem pedido de permissão.
- `load_session` com replay em 2,7 s; fork anunciado e recarregado em 2,3 s.
- `list_sessions` achou 101 sessões.
- `AskUserQuestion` virou pergunta no wai em 2,0 s; a resposta "Red" voltou como resposta final.

### Revisão final

Revisão do branch inteiro da Fase 4 (`48033ac..1642055`): **nenhum problema crítico**. Saíram 2 importantes e 6 menores, todos corrigidos numa leva só (23ef679, R70):
- **`wai-navegador`:** o `npx` rodava na pasta do projeto, então um repositório podia plantar `node_modules` ou `.npmrc`. Agora roda com `--prefix=/` (no Windows, ver R71).
- **Corrida de pipes:** processos iniciados ao mesmo tempo podiam herdar o pipe um do outro, e a queda de uma extensão demorava a ser notada. Era a causa do teste instável `crash_loop_stops_after_three`. Agora os inícios de extensão passam por uma trava.
- Menores: um limite duplicado, a versão do `cargo-packager` sem pin na doc, a folga de um teste, contagens deste relatório, uma nota sobre o Windows e um caminho pessoal num benchmark.

A re-revisão de 23ef679 deu **pronto para merge e tag `v0.1.0`**. Verificações:
- 283 testes em Rust passaram duas vezes, mais os 15 em Go;
- `go vet` com `GOOS=windows` limpo;
- o pacote gerado de um checkout limpo;
- o teste de estresse da trava passou 720 de 720 vezes, e mais 48 de 48 na re-revisão.

## Revisão visual: "Aurora"

> 2026-11-09 · branch `feat/visual-aurora` (de `development`, a9e4d54) · desenho em `docs/superpowers/specs/2026-11-09-wai-visual-aurora-design.md`

Você pediu uma revisão do visual da IDE com a Central de IA como base ("esse design é bonito") e pulou as perguntas de design. Então as escolhas foram minhas: estão como R73–R86 na tabela de decisões.

### O que mudou

- **Tema "wai Aurora"** (`crates/wai/assets/theme/aurora.json`): a paleta da Central (noite, boreal, lilás, magenta, céu, brasa, âmbar) virou o tema escuro padrão, e toda tela que já lia o tema herdou as cores. O modo claro (`theme = "light"`) continua o "Default Light" do gpui-component, sem aurora e com cartões simples (R73).
- **Fundo:** uma aurora parada atrás de tudo, feita de um gradiente escuro e três manchas (boreal, lilás e magenta) pintadas só com sombra borrada. Não há animação (R76).
- **Cartões:** a barra lateral, cada painel e o terminal flutuam como cartões de vidro escuro, com raio de 16 px, brilho lilás e 8 px entre eles. A alça de redimensionar não aparece em repouso e fica lilás com o mouse em cima ou arrastando.
- **Barra do topo:** marca (ícone + "wai" em Nunito), projeto, uma pílula de status ("No agent working", "1 agent working", "N agents working", mais " · 1 needs you" em âmbar) e as ações globais (modelos, extensões, paleta, terminal, nova sessão). Essas ações saíram do cabeçalho da barra lateral e mantêm os mesmos ids. No macOS a barra do topo é a própria barra de título, com os semáforos na mesma linha (R74).
- **Barra lateral e editor:** abas segmentadas numa linha só (R81), alcançáveis pelo teclado (R82). Linhas e pontos de status seguem o estilo da Central, e as abas do editor viraram pílulas. O painel vazio mostra "Start a session" e o atalho (R80).
- **Chat:** balão lilás para você e balão de vidro para o agente, com o cabeçalho "✦ <agente>". Os avisos viraram pílulas. Os cartões de permissão e de pergunta seguem o estilo da Central ("PERMISSION REQUESTED", "QUESTION", opções numeradas), e o prompt é uma pílula com botão redondo e borda boreal no foco (R83).
- **Fonte de títulos:** Nunito Bold embutida (R75, R86). O terminal e o realce de sintaxe usam a paleta.
- **Varredura final:** passei pela paleta, consentimento, painel de extensão, Agents, Search, Files, menu de modelos e notificação, nos dois temas. Nada ignorava o tema. Só as linhas do painel de extensão destoavam, porque o hover usava o lilás da seleção, e agora usam a mesma linha da barra lateral (0d1f43b).

Commits:
- 479a589: tema, fonte e terminal;
- 2b12ea5 e 71ed5da: fundo, cartões e barra do topo;
- adb15a2 e f20ba91: barra lateral, abas e painel vazio;
- 7099b86 e 43ac310: chat;
- 0d1f43b: varredura;
- mais o commit destas telas e medições.

Depois da revisão: 293 testes em Rust passando (eram 283), mais os 15 em Go (8 em `wai-rotinas`, 7 em `wai-waycarbon`), com `clippy -D warnings` e `cargo fmt --check` limpos.

**Dois pedidos a mais seus**, feitos depois das telas (as regras estão como R87–R92 na tabela):
- **Fechar sessões e recolher o histórico** (64f26d6): cada painel de sessão ganhou um **×** no cabeçalho, que faz o mesmo que o ⌘W, e um painel vazio num split ganhou o seu **×**. O **History** da barra lateral virou uma seção recolhível, "HISTORY · N", que começa fechada.
- **Arrastar sessões** (eab2aa8 e a leva de correções final): dá para arrastar uma sessão pelo título do painel, ou uma linha do Open ou do History, e soltar num painel. Os quartos da esquerda e da direita abrem um painel novo daquele lado; o meio mostra a sessão ali. O layout é salvo.

### Telas

Em `docs/screens/`, com um projeto de demonstração (`acme-api`), o agente falso e cards EC-9001..9004 inventados. Não aparece nome de usuário, nome da máquina nem caminho pessoal.

| Arquivo | O que mostra |
|---|---|
| `aurora-chat.png` | Uma sessão pedindo permissão, com "1 needs you" na barra do topo. É a imagem principal do README |
| `aurora-editor.png` | Dois arquivos no editor e o terminal |
| `aurora-panel.png` | O painel "My cards" da `wai-waycarbon` |
| `aurora-palette.png` | A paleta de comandos, com os modelos da extensão |
| `aurora-light.png` | O mesmo chat com `theme = "light"` |

### Medições

Detalhe, método e amostras brutas em `docs/spikes/aurora-medicoes.md`. Para o parado e a janela, o `.app` do commit 0d1f43b (`CI=true cargo packager --release`) foi aberto como pela Fase 4. Para o streaming, o build com `demo`, como nas Fases 1 e 2. O Mac estava em uso, então cada execução registrou se a janela ficou visível, quantos prompts o agente recebeu e se o processo chegou vivo ao fim. Só contam as execuções limpas, e as outras estão listadas no arquivo.

| Métrica | Meta | Aurora | Fase 4 (`.app`) | Fase 2 |
|---|---|---|---|---|
| CPU parado (30 s), janela visível | < 0,5% | **0,20%** com `env -i` (0,17–0,23%); **0,12%** por `open -n` | 0,16% / 0,12% | 0,15% |
| RAM do wai (`top`) | < 300 MB | **78–80 MB** (RSS 98–114 MB) | 73–74 MB | 72 MB |
| CPU com streaming a 40 tokens/s | < 10% | **5,1%** (médias 4,98–5,32%) | não remedido | 7,5% |
| Janela visível, 1ª abertura / com cache | < 1 s | **301–365 ms** / **270–352 ms** | 261–297 / 251–264 ms | 244–293 ms |
| Janela visível por `open -n` | < 1 s | **325–375 ms** | 326–362 ms | — |
| `wai.app` / DMG | — | 45 MB / 14,2 MB | 44 MB / 14,0 MB | — |

Todas as metas foram batidas, e o fundo estático de reserva da Tarefa 2 (um PNG parado) não foi preciso.
- O streaming não ficou mais caro (5,1% contra 6,4–7,5% nas Fases 1 e 2), provavelmente porque a aurora e as sombras são trabalho da GPU. Não investiguei isso nem por que caiu.
- O parado subiu um pouco (0,20% contra 0,16%), provavelmente porque cada piscada do cursor repinta a aurora.
- A janela aparece ~50 ms mais tarde. A causa não foi separada: o tema e a fonte carregam na abertura, e o Mac estava ocupado.
- Coberta por outra janela, a janela do wai não é pintada: o parado dá 0,0% e o streaming ~1,5%.

### Barra de título no macOS

- **Você conferiu à mão** no preview de 7099b86: arrastar pela barra do topo move a janela, e o duplo clique maximiza. A barra não mudou desde então (`topbar.rs`, `app.rs` e `workspace.rs` estão iguais em 0d1f43b).
- **Eu conferi**, sem permissão de Acessibilidade nem de Gravação de Tela:
  - o `.app` abre uma janela só (1400 × 883, colada na barra de menus);
  - a marca começa em x = 82 pt, depois dos 80 pt que a `TitleBar` reserva. Os semáforos começam em x = 9 pt e, com o tamanho padrão, acabam em ~63 pt. Isso é conta, não pixel da tela.
- **Não deu para conferir daqui:** a foto da janela real (`screencapture` recusado) e a tela cheia (`osascript` sem acesso de Acessibilidade, -1719).
- **Falta você conferir à mão:** que os semáforos não encostam na marca e que a tela cheia não deixa faixa vazia no topo.

## Decisões que eu tomei sem você (confira)

| # | Decisão | Custo se estiver errada |
|---|---|---|
| R1–R4 | Ajustes de ordem e detalhes do plano da Fase 1 | Nenhum relevante |
| R5 | O cartão de permissão mostra o diff do pedido ou, se faltar, o da ferramenta | Uma busca a mais |
| R6 | "Não logado" também é detectado no envio do prompt | Nenhum |
| R7 | A resposta de permissão vai para todas as sessões, e cada uma ignora o que não é dela | O(sessões) por evento |
| R8 | Capturas de tela saem do app real, atrás da feature `screenshot` | Nenhum |
| R9 | O status volta a "Ready" quando uma sessão funciona depois do login | Nenhum |
| R10 | Pequenas mudanças de interface interna na 1.6 | Nenhum |
| R11 | O agente padrão inicia junto com o app | ~100 MB do Node parado |
| R12 | Teste extra para o ⌘W, que cancela o turno | Um teste |
| R13 | "Terminou" só em fim de turno normal | Nenhum aviso em turno cancelado (de propósito) |
| R14 | Sessão restaurada pega o título do histórico | Nenhum |
| R15 | **Segurança:** o `.wai/` do projeto não define agentes, comandos, shell nem ambiente | Times não fixam o comando do agente por repositório |
| R16 | Extras na leva final da Fase 1 | Diff maior |
| R17 | Ao sair, o grupo de processos do agente leva SIGKILL na hora | A última linha do histórico pode ficar incompleta |
| R18 | O agente herdava a pasta de onde o wai foi aberto | Resolvida na 2.0 |
| R19 | Merge local da Fase 1 (`feat/fase-1-core` → `development` → `main`) e tag `fase-1` sem perguntar: repositório só local, sem remoto | Um `git reset` |
| R20 | ⌘W é contextual: no editor fecha a aba, no chat fecha a sessão | Um atalho |
| R21 | "Open full output" grava em `~/.config/wai/scratch/`, limpo ao abrir | Arquivos temporários |
| R22–R24 | Perguntas: mostrar descrições; recusar formulários desconhecidos; "Needs you" até responder | Formulários exóticos ficam sem resposta |
| R25 | Fork só aparece se o agente recarrega sessões | Nenhum |
| R26–R27 | Árvore com os ignores globais do git; o watcher limita lotes enormes | Uma releitura completa de vez em quando |
| R28–R30 | Sair ou trocar de projeto com edição não salva pergunta antes; > 20 MB mostra aviso | Um clique a mais |
| R31 | Recarga que falha marca a aba; permissões definidas antes de gravar | Nenhum |
| R32–R35 | Revert por trecho único, nunca apaga arquivo pré-existente, desabilitado durante o turno | Arquivos com tab ou CRLF não são revertidos |
| R36–R37 | Plano com até 8 linhas; "Done" só para turno terminado | Nenhum |
| R38 | Ajustes de segurança e texto incorporados na 2.9 | Tarefa maior |
| R39–R41 | Salvar seguro (nunca por cima do que você não viu), arquivo temporário sem seguir symlink, `.wai/` até 1 MB | Um clique a mais após "Keep mine" |
| R42 | Dois caminhos de sobrescrita | Resolvida na 3.0 |
| R43 | Merge local da Fase 2 e tag `fase-2` (mesmo motivo da R19) | Um `git reset` |
| R44, R46 | Divisão de tarefas da Fase 3; a API de sessão ganhou servidores MCP | Nenhum |
| R45 | WayCarbon chama `jira me` e depois `jira issue list`, sem shell | Um processo a mais por atualização |
| R47 | O aviso de "mudou no disco" depende de a aba estar editada | Nenhum |
| R48–R49 | Agentes e MCP exigem `process`; modelos com campo errado e NUL são recusados | Extensões só de agente precisam declarar `process` |
| R50 | O consentimento mostra a origem; `git` endurecido; desinstalar não trava | Nenhum |
| R51–R52 | Reinícios zeram ao reabilitar; Job Object no Windows | Mais 3 tentativas por reabilitação |
| R53 | Lista que ocupa o painel e valores iniciais nos campos | Esquema um pouco maior |
| R54 | WayCarbon: "Loading…" na hora, `jira` em segundo plano | Nenhum |
| R55 | Licenças: `[licenses.private] ignore = true` até você escolher a licença | Nenhum |
| R56 | Extensão abre no máximo 1 sessão a cada 2 s; ⌘N tem prioridade; `ssh -o BatchMode=yes`; comando sem caminho só pelo PATH absoluto | Config de ssh customizada não vale ao instalar extensões |
| R57 | Sobras da Fase 3 (painéis extras, largura de lista, nota do PATH do MCP) | Resolvida na 4.0 e na 4.5 |
| R58 | Merge local da Fase 3 e tag `fase-3` (mesmo motivo da R19) | Um `git reset` |
| R59 | Timer que dorme até a próxima rotina é agendamento por evento, não polling | Nenhum |
| R60 | Hot-exit grava uma cópia ~1 s depois da última edição **e** ao sair, então crash e Dock também ficam cobertos | Pequenas escritas na pasta de config enquanto você edita |
| R61 | Cópia > 50 MB não é gravada (com aviso); aviso único quando a cópia falha; sessão de extensão nunca toma o painel focado | Nenhum |
| R62 | A 1ª sessão de uma extensão pode abrir um painel novo (se houver < 4); arquivo apagado exige "Keep mine" antes de salvar | Um painel a mais por extensão |
| R63 | O app empacotado chama `set_application` com o próprio id (sem isso as notificações sairiam como Finder) | Nenhum |
| R64 | Aceitos: o create-dmg baixado pelo cargo-packager sem checksum (commit fixo, HTTPS); `wai.exe` sem ícone embutido | Confiança no pin do cargo-packager; cosmético |
| R65 | Adiados: o DMG em si não é notarizado (o app dentro é); rodar de novo pode criar 2º rascunho; certificado Windows em hardware precisa de `sign-command` | Um passo de notarização a mais depois |
| R66 | `wai-rotinas` pode limitar cada espera a 1 min, porque o relógio do Go para durante o sono do Mac | ~60 despertares/h numa extensão opcional |
| R67 | `wai-rotinas` recusa todo script no Windows (não há checagem de ACL na stdlib) | Sem rotinas no Windows até existir essa checagem |
| R68 | No macOS, aberto fora do terminal (sem `TERM`), o wai pega só o `PATH` do shell de login (`-i -l`, 3 s, uma vez, antes de qualquer thread) | O `.zshrc` atrasa a abertura pelo Finder. Nesta máquina o `-i -l` leva 1,4–1,7 s, e desde a R69 isso só pesa na 1ª abertura |
| R69 | Cache do `PATH` do shell: o 1º launch espera o shell uma vez, os seguintes usam o cache e atualizam em segundo plano | Um `PATH` novo só vale no launch seguinte |
| R70 | Revisão final: corrigidos o `npx` do `wai-navegador` (agora `--prefix=/`, para um repositório não plantar `node_modules`/`.npmrc`) e a corrida de pipes entre processos iniciados ao mesmo tempo (trava só nos inícios de extensão). Uma trava única para todo processo que o wai inicia fica para depois da v0.1.0 | Raramente, o fim de uma extensão que caiu demora a ser notado |
| R71 | No Windows, `--prefix=/` é a raiz do disco, que outros usuários da máquina costumam poder escrever. Aceito na v0.1.0 (ainda melhor que a pasta do projeto) | Em máquina Windows compartilhada, outro usuário poderia plantar um pacote MCP |
| R72 | Merge local da Fase 4 e tags `fase-4` e `v0.1.0`, só locais (a `v0.1.0` só é empurrada por você no passo 8) | Um `git reset` / `git tag -d` |
| R73 | Revisão visual: o Aurora substitui o tema escuro padrão, sem seletor de tema; o modo claro continua o "Default Light" do gpui-component | Trocar um ajuste ou um JSON |
| R74 | No macOS, a barra do topo é a `TitleBar` do gpui-component (barra de título transparente, semáforos na mesma linha); Windows e Linux mantêm a barra de título nativa, porque o Windows nunca rodou e assim não há botões de janela desenhados à mão | No Windows, a barra do topo fica embaixo de uma barra de título nativa |
| R75 | Fonte de títulos: Nunito Bold (OFL 1.1) embutida; o texto continua com a fonte do sistema e o mono não muda | ~130 KB e uma linha de licença |
| R76 | Nenhuma animação: a aurora é parada, para manter a CPU parada < 0,5% | Fica menos viva que a Central |
| R77 | O GPUI de teste não carrega fontes, então o teste da fonte confere o arquivo embutido (família "Nunito") em vez da lista de fontes; o app de verdade lista a Nunito | Um teste mais fraco |
| R78 | O `OFL.txt` fica no repositório, ao lado do TTF, e não vai para o `.app`; o TTF embutido traz o aviso da OFL na tabela `name`, o que a OFL 1.1 §2 aceita. **Substituída pela R86** | Pôr o `OFL.txt` no pacote depois |
| R79 | O teste da barra do topo numa janela estreita ganhou uma checagem de verdade (o status termina antes das ações; o nome do projeto fica mais estreito a 900 px do que a 1400 px). A do plano nunca falhava | Um teste a mais |
| R80 | O painel vazio mostra o título "Start a session" e só o atalho numa pílula (⌘N), em vez de repetir a frase da dica | Dica um pouco mais curta |
| R81 | As 4 abas fixas da barra lateral cabem numa linha na largura padrão; as abas de painel de extensão podem quebrar a linha | Texto das abas menor |
| R82 | As abas da barra lateral voltaram a ser alcançáveis pelo teclado (Tab, depois Enter ou Espaço) | Algumas linhas |
| R83 | A leva de correções do chat incluiu o foco visível no prompt (borda boreal; clicar na pílula foca o texto; sem `InputGroup`, que anima) e uma função única para preencher botões | Algumas linhas |
| R84 | Ids de sessão repetidos (os eventos vão para a 1ª sessão com o mesmo agente e id) ficaram fora da revisão visual, como lacuna conhecida. O problema é anterior; só afeta agentes que repetem ids entre execuções, e o `claude-agent-acp` usa ids únicos | Um agente real que repita ids misturaria uma sessão retomada com uma nova |
| R85 | A redução das PNGs (`sips -Z 1400`) e as correções do doc de medições entraram no próprio commit 632560a (`--amend`, local, sem remoto), para que os 6,8 MB nunca entrem no histórico | Nenhum além de um SHA local reescrito |
| R86 | O `OFL.txt` passa a ir dentro do `.app`, pelos `resources` do packager. Substitui a R78 | Um arquivo a mais no app |
| R87 | O **×** do painel de sessão faz o mesmo que o ⌘W: para a sessão, que sai do Open e continua no History, pronta para retomar | Uma sessão que você queria deixar rodando é parada (dá para retomar) |
| R88 | O History começa recolhido a cada abertura; aberto ou fechado fica só na memória, não é salvo | Um clique para abrir |
| R89 | Zonas de soltar: os 25% da esquerda e os 25% da direita abrem um painel novo ao lado; o meio mostra a sessão ali. No meio, uma sessão vinda de outro painel troca de lugar com a do alvo; qualquer outra substitui, e a sessão que saiu continua aberta no Open. Os painéis ficam numa linha só. Um editor com edições não salvas nunca é substituído: a sessão abre ao lado | Um arranjo diferente do que você esperava; ajuste numa função pura com testes |
| R90 | Soltar na borda de um painel vazio preenche o painel, em vez de abrir outro ao lado | Quem queria um painel novo ao lado do vazio precisa fechar o vazio antes |
| R91 | Soltar no meio de um painel vazio uma sessão vinda de outro painel move a sessão para lá e fecha o painel antigo, em vez de trocar e deixar um vazio para trás | Se você preferia a troca, o painel antigo some; é uma linha em `layout::place` |
| R92 | Com 4 painéis, um painel ainda pode ser movido para o lado (o número de painéis não muda); só as linhas da barra lateral viram um soltar no meio. O realce mostra só as zonas que agem: quando todo soltar naquele painel seria no meio, acende o painel inteiro | Se você esperava a troca de antes, o painel vai para o lado em vez de trocar; é uma linha em `layout::only_center` |

## Lacunas conhecidas (atuais)

**Publicação**
- **App sem assinatura e sem notarização** até você ter os certificados. O macOS bloqueia a 1ª abertura de uma cópia baixada; o README ensina o "Open Anyway".
- **O DMG em si não é notarizado**, só o app dentro dele (R65). Depois do 1º release assinado, rode as checagens de `docs/building.md`.
- **Windows nunca rodou de verdade.** O app completo, o instalador NSIS e a assinatura só serão verificados pela CI, que depende do remoto.
  - `wai.exe` não tem ícone embutido (R64).
  - No Windows, o que um agente `npx` inicia sobrevive a ele até o stdin fechar.
- **O DMG só foi gerado com `CI=true`.** O passo que arruma a janela do DMG pelo Finder nunca rodou.
- **A notificação a partir do `.app` não foi testada de ponta a ponta.** Precisa de um turno real terminando com a janela em segundo plano.
- **A 1ª abertura de uma cópia nova do app leva 0,5–1,4 s a mais**, pela verificação do macOS, fora do controle do wai. A abertura de um DMG baixado da internet (com quarentena) não foi medida.

**Uso**
- **Aberto pelo Finder, Dock ou Launchpad, o wai usa o PATH do shell de login (R68), guardado em cache (R69).**
  - O macOS dá a esses apps o PATH `/usr/bin:/bin:/usr/sbin:/sbin`, sem `npx` nem `node`. Sem `TERM`, o wai lê o PATH de `~/.config/wai/shell-path` e, em segundo plano, pergunta de novo ao `$SHELL -i -l` para a próxima abertura. Pelo terminal, continua com o PATH do terminal.
  - **Um PATH novo (outra versão do Node no nvm, por exemplo) só vale no launch seguinte.** A 1ª abertura depois da mudança ainda usa o cache.
  - Só a 1ª abertura (sem cache) espera o shell, com limite de 3 s; passou disso, segue com o PATH do launchd. O `.zshrc` desta máquina leva ~1,5–1,8 s. Nas outras aberturas esse tempo corre em segundo plano.
  - Testei com HOMEs de rascunho e um `.zshrc` de 1,5 s. A 1ª abertura esperou 1543 ms; as seguintes leram o cache (a linha do log sai ~60 ms após o exec, só o início do processo). Um agente `command = "npx"` achou o `npx` do `.zprofile`, e a troca de v1 para v2 valeu na abertura seguinte.
- **A v1 não isola extensões.** Elas rodam com os seus poderes e herdam o ambiente do wai; `network` e `fs:` são só declarativos.
- **Um wai por vez.** Dois processos dividem `state.json` e `recovery/`.
- **Hot-exit:**
  - um arquivo aberto antes de a restauração terminar fica para a próxima abertura.
- **Checagem de atualização:**
  - não usa proxy (`HTTPS_PROXY`) nem segue redirecionamento (repositório renomeado dá `HTTP 301` no log);
  - sem `updates.repo` definido, não checa nada, nem ligada (ver a lista abaixo).
- **Editor e busca:**
  - o Revert recusa arquivos com tab e CRLF.
- **Permissão com o Claude real** nunca apareceu nesta máquina, porque as suas configurações usam `bypassPermissions`. O fluxo foi testado com o agente falso.
- **Ids de sessão repetidos (R84).** `Workspace::session` (`crates/wai/src/workspace.rs`, ~linha 427) entrega cada evento à 1ª sessão aberta com o mesmo agente e id. Um agente que repete ids entre execuções mistura uma sessão retomada com uma nova: o texto do turno novo cola na sessão antiga e o prompt novo não aparece. Só o agente falso faz isso (um contador `fake-{n}` por processo); o `claude-agent-acp` usa ids únicos.

**Extensões próprias**
- **"My cards"** só foi testado com um `jira` falso.
- **`wai-rotinas`:**
  - no Windows recusa todo script (R67);
  - um horário que cai durante o sono do Mac roda até 1 min depois de acordar (R66);
  - um script que estoura o tempo morre, mas o que ele iniciou só morre quando o wai para a extensão;
  - scripts em `/Applications` são recusados (a pasta é gravável pelo grupo `admin`).
- **`wai-navegador` no Windows:** o `--prefix=/` aponta para a raiz do disco (R71).
- **`wai-remoto`** é só desenho: precisa da permissão `listen` e de eventos de sessão que o host ainda não tem.
- **`wai-waycarbon`** descreve repositórios e convenções internas da Way. Antes de publicar, decida se ele fica no repositório público (ver o item 1 da lista).

## O que só você pode fazer

Na ordem. Os comandos rodam na raiz do repositório e foram testados aqui. Os de `git` foram testados numa cópia do repositório, empurrando para um remoto local.

### 1. Licença e dono do código

1. **Confirme com a WayCarbon, por escrito, de quem é o wai.**
   - O wai é uma reescrita do zero: nenhuma linha da Central entrou (a Central não tem licença, então não poderia).
   - Mas foi feito por um funcionário. Confira a cláusula de propriedade intelectual do seu contrato.
2. **Decida o destino de `extensions/wai-waycarbon`.** Ele cita repositórios, branches e o fluxo do Jira da Way. Ou a WayCarbon autoriza que fique público, ou ele vai para um repositório privado da Way.
3. **Escolha a licença.** A sugestão do estudo (§11) é Apache-2.0, a mesma do GPUI. Depois:
   - crie o arquivo `LICENSE` na raiz;
   - em `Cargo.toml`, na seção `[workspace.package]`, acrescente `license = "Apache-2.0"`;
   - em cada `crates/*/Cargo.toml`, acrescente `license.workspace = true`;
   - troque a seção **License** do `README.md`.

   O `cargo deny` na CI já aceita Apache-2.0. A R55 (`[licenses.private] ignore = true`) pode ficar.

### 2. Remoto e push

1. **Decida o histórico antes do 1º push.**
   - Todos os commits têm como autor o e-mail do `git config` aplicado a esta pasta, que não é o da WayCarbon. Se quiser outro autor, ajuste `git config user.email` antes do commit novo.
   - O commit `e39062f` tem um caminho absoluto desta máquina num teste (removido depois, na `23ef679`).
   - **Para publicar limpo**, crie um único commit inicial com o conteúdo do `main` e empurre só ele; as outras branches e as tags `fase-*` ficam locais, e no passo 8 a `v0.1.0` é criada em `publico` (troque `main` por `publico` no comando):

     ```sh
     git checkout --orphan publico main
     git commit -m "feat: wai 0.1.0"
     git remote add origin git@github.com:<dono>/wai.git
     git push -u origin publico:main
     ```

   - **Para manter o histórico**, siga os passos 2 e 3.
2. **Crie o repositório** no GitHub (por exemplo `<dono>/wai`), vazio: sem README, sem licença e sem `.gitignore`.
3. **Empurre todas as branches e as tags de fase:**

   ```sh
   git remote add origin git@github.com:<dono>/wai.git
   git push -u origin --all
   git push origin 'refs/tags/fase-*:refs/tags/fase-*'
   ```

   Empurre a `v0.1.0` **só no passo 8**: ela dispara o workflow de release.

### 3. CI

1. **Settings → Actions → General:** confira que o GitHub Actions está permitido (em repositório novo já vem ligado). Se usar a lista restrita, libere estas ações:
   - `actions/checkout`, `actions/setup-go`, `actions/setup-node`;
   - `actions/upload-artifact`, `actions/download-artifact`;
   - `Swatinem/rust-cache`;
   - `EmbarkStudios/cargo-deny-action`.
2. **O push do passo 2 já dispara o `ci.yml`** em `main` e `development`. Ele roda:
   - fmt, clippy, testes e Go no `macos-latest` e no `windows-latest`;
   - `cargo deny check licenses`.
3. **Confira na aba Actions que tudo ficou verde.** É a 1ª verificação real do Windows. Se algo falhar lá, é a primeira coisa a corrigir.

### 4. Apple Developer ID e notarização

1. **Entre no Apple Developer Program.** Se o dono for a WayCarbon, a conta precisa ser de organização.
2. **Crie o certificado:**
   - no Mac, Acesso às Chaves → Assistente de Certificado → **Solicitar um Certificado de uma Autoridade de Certificação**, e salve o CSR;
   - em developer.apple.com → Certificates → **+** → **Developer ID Application**, envie o CSR, baixe o `.cer` e abra-o para instalar.
3. **Exporte o `.p12`:** em Acesso às Chaves → Meus Certificados, clique com o botão direito em "Developer ID Application: …" → Exportar. Use uma senha **sem `"` nem `\`**: o motivo está em `docs/building.md`.
4. **Converta para base64:**

   ```sh
   base64 -i Certificates.p12 -o Certificates.p12.b64
   ```

5. **Crie uma senha de app** em account.apple.com → Iniciar Sessão e Segurança → Senhas de app.
6. **Cadastre os segredos** em GitHub → Settings → Secrets and variables → Actions → New repository secret:

   | Segredo | Valor |
   |---|---|
   | `APPLE_CERTIFICATE` | O conteúdo de `Certificates.p12.b64` |
   | `APPLE_CERTIFICATE_PASSWORD` | A senha do `.p12` |
   | `APPLE_ID` | O e-mail da sua Apple ID |
   | `APPLE_TEAM_ID` | O Team ID (10 caracteres, em Membership details) |
   | `APPLE_APP_PASSWORD` | A senha de app do passo 5 (não a senha da conta) |

   Só com os dois primeiros, o app sai assinado. Com os três últimos também, sai notarizado.
7. **Apague** `Certificates.p12` e `Certificates.p12.b64` do disco.

### 5. Certificado do Windows

- **Certificados de assinatura de código emitidos desde junho de 2023 ficam em hardware** (token ou HSM) e não exportam `.pfx`. O caminho provável é um assinador na nuvem (Azure Trusted Signing, DigiCert KeyLocker, SSL.com eSigner). Nesse caso:
  1. em `crates/wai/Cargo.toml`, `[package.metadata.packager]`, acrescente `windows = { sign-command = "<ferramenta> … %1" }`, conforme a documentação do assinador;
  2. em `.github/workflows/release.yml`, apague o passo **Import signing certificate**.
- **Se você conseguir um `.pfx` com a chave privada:**

  ```sh
  base64 -i cert.pfx -o cert.pfx.b64
  ```

  Cadastre `WINDOWS_CERTIFICATE` (o conteúdo do `.b64`) e `WINDOWS_CERTIFICATE_PASSWORD`.
- **Sem nada disso,** o instalador sai sem assinatura e o SmartScreen avisa.

### 6. Anthropic

**Antes de anunciar publicamente**, confirme com a Anthropic, pelo contato oficial, a pergunta do estudo §5: um editor de terceiros publicado pode deixar cada usuário usar a **própria assinatura** do Claude via `claude-agent-acp` (que roda sobre o Agent SDK)?

O que contar a eles:
- o wai nunca mostra tela de login, nunca lê, guarda nem repassa token;
- quem autentica é o Claude Code do próprio usuário, como no Zed.

Enquanto a resposta não vier, o caminho garantido é API key (ou Bedrock/Vertex) configurada no próprio agente.

### 7. Bundle id e repositório de atualização

**Faça antes da 1ª tag:** trocar o id depois muda a identidade do app, e as permissões de notificação do macOS recomeçam.

1. **Bundle id.**
   - Troque `identifier = "app.wai.ide"` em `crates/wai/Cargo.toml` (`[package.metadata.packager]`) por um id reverso de um domínio seu, por exemplo `com.<seu-domínio>.wai`. É o único lugar: as notificações leem o id do próprio app.
   - Confira:

     ```sh
     CI=true cargo packager --release
     plutil -extract CFBundleIdentifier raw target/release/wai.app/Contents/Info.plist
     ```

2. **Repositório de atualização.**
   - Hoje o padrão de `updates.repo` é vazio, então ninguém recebe aviso, nem com `updates.check = true`, a menos que configure o repositório à mão.
   - Para o app publicado já saber onde olhar, mude o padrão em `crates/wai-settings/src/lib.rs`: troque o `Default` derivado de `UpdateSettings` por um `impl Default` com `check: false` e `repo: Some("<dono>/wai".into())`.
   - Ajuste o teste `project_cannot_set_updates`, que hoje exige `repo` vazio.
   - A checagem continua desligada até o usuário ligar.

### 8. Primeira tag: `v0.1.0`

O workflow de release só segue se a tag for igual à `version` do `Cargo.toml` da raiz (hoje `0.1.0`).

1. **Se a tag ainda não existe,** crie-a. Se você mexeu no passo 7 depois que ela foi criada, apague-a com `git tag -d v0.1.0` e crie de novo, no `main` atualizado:

   ```sh
   git tag -a v0.1.0 -m "wai 0.1.0" main
   ```

2. **Confira que a versão bate:**

   ```sh
   git show v0.1.0:Cargo.toml | grep -xF 'version = "0.1.0"'
   ```

3. **Empurre a tag:**

   ```sh
   git push origin v0.1.0
   ```

4. **Revise o rascunho.** Em Actions → release, espere os jobs `macos`, `windows` e `release`. O rascunho em Releases traz:
   - `wai_0.1.0_aarch64.dmg`;
   - `wai_0.1.0_x64-setup.exe`;
   - `SHA256SUMS.txt`.

   Baixe, teste e clique em **Publish**. Se precisar rodar de novo, apague o rascunho antes (R65).
5. **Depois do 1º release assinado,** rode as checagens `spctl` e `stapler` de `docs/building.md`.

**Próximas versões:** mude `version` em `[workspace.package]` no `Cargo.toml` da raiz, faça o commit, crie a tag `v<mesma versão>` e empurre a tag.
