# wai — Plano mestre (a partir da 0.5.0)

> **Para agentes:** este é o plano de ondas. Cada onda ganha o próprio plano detalhado (superpowers:writing-plans) em `docs/superpowers/plans/AAAA-MM-DD-wai-onda-N.md` na hora de executar — não antes, porque outra sessão também versiona o wai e o código muda rápido. Executar com superpowers:subagent-driven-development, um worktree por item.

**Objetivo:** o Guilherme desligar a Central IA WayCarbon e passar a usar só o wai; depois deixar o wai pronto para colegas; depois fechar as pendências dos planos anteriores.

**Fontes (2026-11-11):**
- Comparação com a Central (`central-ia-waycarbon-claude`, main `e26affc`) feita por agentes de produto, decidida pelo log de uso real da Central (`macos-state/central.log`, 04–11/11): uso real = pular entre conversas (135), abrir o terminal (50), enviar/fechar/nova. Zero uso de ▶ Rodar, anexos, rotinas, modelos, PDF da daily, Spotify.
- Auditoria dos planos e specs de `docs/superpowers/` contra o código da 0.5.0 (`3bdc1bc`).
- Spec do rascunho de nova sessão, `docs/superpowers/specs/2026-11-11-wai-new-session-draft-design.md` (mockup aprovado pelo dono em 11/11).
- "Lacunas conhecidas" de `docs/RELATORIO-final.md` e as memórias do projeto (testes instáveis, desenho de grupos ainda não conferido com o dono).

**Estado de partida:** roadmap Fases 0–4, Aurora, Settings+agentes, subagentes (0.4.0), clique direito (0.4.1), colar anexos (0.4.2), reordenar sessões (0.4.3), mensagem durante o turno + menu `/` (0.5.0) — executados. Os checkboxes dos planos antigos não foram marcados; o status vale pelo código.

## Andamento (2026-11-11, fim da tarde)

Mergeado na `development` (`6427cfc`), sem versão nova ainda: **0.1** (worktrees limpos), **0.4** e **0.5** (validados com o Claude real; teste `#[ignore]` `permission_requests_in_default_mode`), **0.8** (testes instáveis; ainda falta a prova de 20 rodadas sob carga), **1.1**, **1.2**, **1.3**, **1.5**, **1.6**, **1.7**, **1.8**, **1.9**. Planos detalhados: `2026-11-11-wai-onda-1-{chat,sessions,settings}.md`; spec nova `2026-11-11-wai-settings-modal-design.md`. Suíte completa verde na frente sessões após o merge (595 passaram, 0 falharam). Decisões tomadas sem o dono e verificações no app rodando: `.superpowers/sdd/master-onda-1/` (`rulings.md`, `track-*-report.md`).

Ainda abertos na Onda 0/1: 0.2, 0.3 e 0.9 (dono), 0.6 e 0.7 (precisam do app na tela/parado), 1.4 (semana de uso), toggle de servidor MCP (gravar `~/.claude.json`, aguarda o dono) e a pergunta 4 (submodules; hoje "New worktree" fica desligado com dica).

## Restrições globais

- CPU parado < 0,5% (medido 0,12–0,20%). Nada animado ou com polling no núcleo.
- Núcleo pequeno; o que é vertical ou de nicho vai para extensão.
- O núcleo nunca toca em credencial do usuário e não abre porta (sem servidor HTTP).
- O código da Central não tem licença: o que for trazido é **reescrito**, nunca copiado.
- Outra sessão também versiona o wai: `git log` antes de criar branch ou versionar. `crates/wai/src/workspace.rs` (~4,9 mil linhas) é o ponto quente de conflito.
- Não rodar a Central e o wai ao mesmo tempo: retomar no wai uma conversa viva no tmux põe dois Claudes no mesmo `.jsonl`.
- Disco quase cheio: `cargo clean` ao terminar cada onda; nunca `cargo run` com HOME falso.

---

## Onda 0 — Validar e arrumar (sem funcionalidade nova, 2–3 dias)

| Id | Item | Pronto quando |
|---|---|---|
| 0.1 | Limpar os 9 worktrees em `.claude/worktrees/` (todos com branch já mergeada e sem mudança pendente) | `git worktree list` mostra só o checkout principal |
| 0.2 | **Dono:** criar remoto privado e enviar branches e tags (hoje o wai só existe nesta máquina) | `git remote -v` aponta para o remoto; tags `v0.1.0…v0.5.0` lá |
| 0.3 | **Dono:** desligar a Central de forma reversível — fechar as conversas dela, `launchctl bootout gui/$UID/com.waycarbon.central-ia` (e o `.atualizar`), deixar instalada 2 semanas | `launchctl list` não mostra a Central; o clone continua lá |
| 0.4 | Cartão de permissão com o Claude real: pasta de rascunho, sessão em modo `default` (a máquina usa `bypassPermissions`), pedir um Edit, um Bash e sair do modo plano | os três cartões aparecem e a resposta chega ao agente |
| 0.5 | Steering com o Claude real: mandar mensagem durante o turno; confirmar que o `claude-agent-acp@0.103.0` anuncia `_meta.steering.supported` (`wai-agent/src/actor.rs:621`) | a mensagem entra no turno em andamento, não na fila |
| 0.6 | Conferência no app rodando do que só passou por testes (0.3–0.5): modos de processamento da conversa (Step by step, Grouped, Hidden e a linha de loading, contra o mockup `.superpowers/brainstorm/38118-1794303719/content/processamento.html`), painel Agents à direita, grupos e renomear, cartões de subagente, aviso do SO com a janela atrás (clicar abre a conversa), reabrir depois de ⌘Q, Revert, menu `/` | cada item visto funcionando ou anotado no Diário de tropeços |
| 0.7 | Medir CPU parado com 3 sessões abertas e as 3 extensões ligadas | < 0,5% |
| 0.8 | Estabilizar os testes instáveis: `new_session_appears_in_open_list` (depende do processo falso não responder antes da asserção) e `crash_loop_stops_after_three` do `wai-ext` (estourou 10 s uma vez sob carga) | 20 rodadas seguidas da suíte sob carga sem falha |
| 0.9 | **Dono:** conferir o desenho de grupos e renomear da 0.3.0 (lápis/duplo clique, grupos recolhíveis no Open, arrastar linha para o cabeçalho) — foi decisão minha, você nunca respondeu o exemplo pedido | aprovado ou ajustes anotados |

**Pronto quando:** um dia útil inteiro só no wai, com a Central parada, e os tropeços anotados no fim deste arquivo.

## Onda 1 — Desligar a Central (núcleo, ~1 semana)

| Id | Item | Onde | Tamanho |
|---|---|---|---|
| 1.1 | **Confirmar antes de encerrar sessão em trabalho.** Ao sair (⌘Q / fechar janela) e no × da linha do Open, se houver sessão `Working`, perguntar "N sessões ainda estão trabalhando — encerrar mesmo assim?". Sessão parada não pergunta. Reaproveitar o diálogo de abas não salvas. | `leave()` `workspace.rs:1734`, fechamento da janela `:299-306`, `end_session` `:1681`; ajustar o teste `quit_stops_agents` `:3746` | P |
| 1.2 | **Botões de copiar** na mensagem inteira e em cada bloco de código. | `chat/view.rs` (hoje só `selectable(true)` em `:550`); widget pronto em `gpui-component` `clipboard.rs` | P |
| 1.3 | **"Continuar no terminal"** no menu da sessão: encerra a sessão ACP e digita `claude --resume <id>` no terminal, sem Enter. Antes: confirmar que o id da sessão ACP é o id do Claude Code. | `Workspace::type_in_terminal` `workspace.rs:1645` (já usado pelo "Install in terminal"), `end_session` `:1681`, menu de clique direito em `sidebar.rs` | P |
| 1.4 | Uma semana usando só o wai, anotando cada vez que precisou do terminal do Claude ou da Central | Diário de tropeços | — |
| 1.5 | **Rascunho de nova sessão** ("What are we building?"): New session abre a tela sem iniciar nada; pasta, Local/New worktree, branch, agente, modo e modelo escolhidos antes; a sessão nasce no 1º envio. Cada New session abre um rascunho novo. Depois do envio, o cabeçalho da conversa ganha as pílulas pasta · branch · agente·modelo. Mockup aprovado: `.superpowers/brainstorm/44632-1794399351/content/new-session-draft-v2.html`. Falta a resposta dos submodules (pergunta 4) antes do plano detalhado. | spec `2026-11-11-wai-new-session-draft-design.md`; `start_session` `workspace.rs:1476`, `Pane`, `sidebar.rs` (Open), `chat/input.rs` | G |
| 1.6 | **Pedido de ação fixo** (pedido do dono, 11/11). Cartão de permissão e de pergunta vira um balão fixo logo acima da caixa de texto: a conversa continua rolando por trás dele e ele não aparece no meio da lista enquanto espera. Só depois de respondido ele entra na conversa (forma compacta, com a resposta escolhida) e o fluxo segue. Vários pedidos ao mesmo tempo empilham, o mais antigo em cima; os atalhos 1/2/3 respondem o de cima. Minhas escolhas, a conferir: posição na lista = onde estava a conversa no momento da resposta; o balão fixo usa o vidro do Aurora com sombra, para ler a conversa por trás. | `chat/permission.rs`, `chat/question.rs`, `chat/render.rs`, `chat/view.rs` | M |
| 1.7 | **Mensagem enviada durante o turno fica fixa** (pedido do dono, 11/11). Mesmo comportamento da 1.6: a mensagem fica como balão fixo acima da caixa de texto (no lugar das pílulas "Queued · …", mantendo o × que a devolve para a caixa) e só entra na lista de mensagens quando o agente de fato a lê. Fila (agentes sem steering): lida = quando sai como prompt no fim do turno. Steering (Claude): hoje entra na lista assim que o adaptador aceita (`push_steered`, `chat/input.rs:179`). **Risco:** o `claude-agent-acp@0.103.0` responde `injected` ao pôr a mensagem na entrada do CLI, antes de o Claude ler, e não avisa o cliente quando ela é consumida (só controla isso por dentro, `Turn.steeredEchoes`). O plano detalhado escolhe o sinal: 1º update do agente depois do `injected` (com prioridade `now` o adaptador interrompe a geração atual e o steer roda a seguir), ou depois da resposta quando há permissão pendente (prioridade `later`), ou pedir o aviso no adaptador. A 1.6 e a 1.7 dividem o mesmo "balão fixo"; fazer juntas. | `chat/input.rs:133-224,449-500`, `chat/model.rs:304`, `wai-agent/src/actor.rs:198` | M |
| 1.8 | **Processamento igual ao mockup** (pedido do dono, 11/11: "está bem diferente do que aparece na tela"). A 0.3.0 entregou o comportamento dos três modos, mas não o visual do mockup `.superpowers/brainstorm/38118-1794303719/content/processamento.html`. Diferenças lidas no código, a conferir com um print do app: (a) **linha de passo** — no mockup é baixa e discreta (texto apagado, cantos 8 px, ícone do tipo: ⌘ comando, 📄 leitura, ✓ verde à direita); no app é um cartão de vidro maior (cantos 12 px, mais espaço), texto claro em peso médio, sem ícone do tipo; (b) **saída aberta** — no mockup sai numa caixa escura de fonte mono colada abaixo da linha; no app fica dentro do mesmo cartão; (c) **bloco agrupado** — mockup resume todos os passos ("3 passos · listou arquivos, leu README.md, viu branches"); app mostra só o último ("3 steps · <último>"); (d) **linha de loading** — mockup em verde boreal com ✻ girando e tempo/"Esc para parar" à direita; app em cinza apagado, ◐◓◑◒ trocando 1×/s, tempo logo depois do texto; (e) **"Thinking…"** aparece como linha própria no app, o mockup não mostra; (f) espaçamento entre itens mais apertado no mockup (8 px). Restrição: girar o ✻ de verdade custa quadros durante o turno — manter 1 tique/s ou animar só enquanto o turno roda, medindo contra o alvo de streaming (< 10%). Fazer junto da 1.6/1.7 (mesma tela). | `chat/render.rs:168-465`, `chat/input.rs:420-446`, `chat/model.rs:690-708` | M |
| 1.9 | **Settings em modal, completo** (pedido do dono, 11/11, referência: modal "Customizations" do VS Code; mockup aprovado `.superpowers/brainstorm/91501-1794403714/content/settings-modal-v2.html`). Clicar em Settings (engrenagem na barra de topo, ⌘,) abre um modal grande sobre o wai no lugar do `Pane::Settings`: título com o contexto (`Settings (Claude · <projeto>)`), maximizar e fechar (Esc ou clique fora), navegação à esquerda com contadores em três grupos, conteúdo com título, descrição, busca e listas agrupadas e recolhíveis; cada linha com nome, selo, caminho, resumo, estado, toggle e `⋯`. **Decisão do dono: fazer completo; o que o wai ainda não faz leva o selo "Coming soon"; o que o Claude já fornece é mostrado e funciona.** Grupos: **wai** — AI, Appearance, Conversation, Terminal, Updates (as seções de hoje) e Extensions (sai da barra lateral). **Claude** — Plugins (`~/.claude/plugins/installed_plugins.json` + `enabledPlugins` do `~/.claude/settings.json`; toggle grava `enabledPlugins` e vale para as próximas sessões), MCP Servers (dos plugins e do `~/.claude.json`, com estado e toggle de ligar/desligar), Skills, Instructions (`CLAUDE.md` do usuário e do projeto e os arquivos que ele inclui), Agents e Hooks — listas com "Open" que abre o arquivo no editor do wai. **Coming soon:** Browse/instalar plugin, atualizar/desinstalar (`⋯` do plugin), Add server, criar skill/agent/hook/instrução (os `+`), conectores do claude.ai (vivem na conta, não em arquivo) e o grupo **Other agents** (Codex, Copilot, Antigravity). Antes do plano detalhado, confirmar no Claude Code: onde fica o liga/desliga de cada tipo de servidor MCP e de onde ler o estado da conexão (se não houver fonte confiável, o estado vira "Coming soon"); gravar `settings.json` preservando o resto e de forma atômica. | `settings_view.rs`, `open_settings` `workspace.rs:2073`, `extensions_view.rs`, `editor` (Open) | G |

**Pronto quando:** a Central fica 5 dias úteis desligada sem você precisar dela, e o Diário de tropeços vira o backlog da Onda 2/3.

## Onda 2 — Pronto para colegas

| Id | Item | Onde | Tamanho |
|---|---|---|---|
| 2.1 | **Aviso de conversa aberta em outro lugar.** Detectar pelo `.jsonl` mexido nos últimos ~2 min e/ou um processo `claude --resume <id>`; avisar ao retomar e oferecer fork; marca na History. Fazer depois da 1.5 (as duas mexem na abertura de sessão). | `workspace.rs` (abrir/retomar), `sidebar.rs` | P |
| 2.2 | **Copiar diagnóstico**: versões (wai, adaptador, agentes), SO, settings sem segredos, fim do log. Pré-requisito: o wai passa a gravar log em arquivo (hoje só `eprintln!`). | `main.rs`, nova seção em `settings_view.rs` | P–M |
| 2.3 | **Versão do adaptador em Settings > AI** e aviso quando sair outra, junto da checagem de atualização (opt-in). | versão já chega em `AgentEvent::Ready` (`actor.rs:609`) e é descartada em `agents.rs:214`; fixada em `wai-settings/src/lib.rs:25` | P |
| 2.4 | **Custo da conversa** ao lado do contexto. | `cost_usd` lido em `chat/model.rs:222`; pill em `chat/render.rs:32` | P |
| 2.5 | **API de extensão 1.1: pasta de dados** (`WAI_EXT_DATA`, sobrevive à reinstalação) **e atualizar sem desinstalar.** | `wai-ext/src/host.rs:400`, `store.rs:84-97`, `install.rs:62`, `docs/extensions.md` | P |
| 2.6 | **Corrigir R84** (evento vai para a 1ª sessão com mesmo agente e id) antes de testar outros agentes. | `Workspace::session` `workspace.rs:840` | P |
| 2.7 | **Outros agentes de verdade:** instalar e testar Codex, Antigravity e Copilot. | — | P cada |
| 2.8 | **Windows de verdade** (depende de 0.2 para a CI rodar): primeira execução do `.exe`, `wai-rotinas` no Windows (hoje recusa todo script, R67). Só se os primeiros colegas usarem Windows. | `.github/workflows/ci.yml`, `extensions/wai-rotinas` | M |

**Pronto quando:** um colega instala o `.app` e o `wai-waycarbon`, usa um dia e manda o diagnóstico quando algo falha.

## Onda 3 — Pendências dos planos anteriores (núcleo)

| Id | Item | Origem | Tamanho |
|---|---|---|---|
| 3.1 | Parar um subagente individual | plano `2026-11-10-wai-acp-v2-and-subagents.md` ("Not in this plan") | P–M |
| 3.2 | Transcrição do subagente num painel (hoje o texto é descartado em `subagents.rs:46`); incluir subagentes terminados na aba Agents | mesmo plano | M |
| 3.3 | Vários terminais (abas) — sobe para a Onda 1 se a 1.4 mostrar muito uso do terminal | `RELATORIO-final.md`; `workspace.rs:174` guarda um só | M |
| 3.4 | Abas do editor voltam ao reabrir | `RELATORIO-final.md`; `AppState` em `wai-settings/src/lib.rs:466` | P |
| 3.5 | Terminal: roda do mouse dentro de `less`/`vim` | `terminal_view.rs:17` | P |
| 3.6 | Revert aceitar arquivos com tab/CRLF | `chat/edits.rs` | P |
| 3.7 | Checagem de atualização com proxy e redirecionamento | `update.rs:224,237` | P |
| 3.8 | Atalhos customizáveis (ESTUDO §6) | keymap fixo em `app.rs:61-87` | M |
| 3.9 | **Projeto 3:** servidor MCP `wai` + "Configure with AI" (a IA mexe em configuração, painéis, arquivos e sessões; o que muda o que roda pede confirmação) — precisa de spec própria antes | spec `2026-11-10-wai-settings-and-agents-design.md` | G |

### Lacunas menores do relatório final (núcleo e extensões próprias)

| Id | Item | Origem | Tamanho |
|---|---|---|---|
| 3.10 | Ao trocar de pasta (⌘O), reler também tema e `default_agent` do `.wai/settings.toml` do projeto novo (hoje só modelos e sugestões) | `RELATORIO-final.md` "Uso" | P |
| 3.11 | Árvore e busca respeitarem `.gitignore` fora de repositório git | idem | P |
| 3.12 | Hot-exit: `fsync` da cópia e limpeza das cópias que não voltam (tamanho, erro de leitura) | idem | P |
| 3.13 | Um wai por vez: impedir (ou avisar) a 2ª instância, que hoje divide `state.json` e `recovery/` | idem | P |
| 3.14 | `wai-rotinas`: matar o que o script iniciou quando ele estoura o tempo; horário que cai durante o sono (R66) | idem, "Extensões próprias" | P |
| 3.15 | Windows (junto da 2.8): ícone no `wai.exe` (R64), filhos do `npx` sobrevivendo ao agente, `--prefix` do `wai-navegador` num prefixo do usuário (R71) | idem | P–M |
| 3.16 | Publicação (junto do 1º release assinado): DMG notarizado (R65), passo do Finder que arruma a janela do DMG, aviso do SO a partir do `.app` de ponta a ponta | idem, "Publicação" | P |

## Depois, se fizer falta (núcleo)

Modelo de conversa com pasta; busca na History; 5º painel; LSP; interface de git; catálogo de extensões com sandbox WASM (pós-publicação); ACP v2 (quando sair do rascunho); nós `tree`/`table` em painéis de extensão; filtro do menu `/` fora do frame (`chat/commands.rs:22`, só se medir custo).

---

## Mapa de extensões — o que fica fora do núcleo

O que a Central tem e não vem para o núcleo, com a extensão candidata e o que o host precisa ganhar antes. Uso real = log da Central, 04–11/11.

| Funcionalidade da Central | Uso real | Extensão candidata | O host precisa antes | Estado |
|---|---|---|---|---|
| Acesso pelo celular | não existe no Mac | `wai-remoto` | os 5 pré-requisitos de `extensions/wai-remoto/DESIGN.md:105-115` (permissão `listen`; `sessions/read` + `event/session_update`; `event/permission_requested` + `permission/answer`; `sessions/cancel`; pasta de dados = 2.5) + revisão de segurança do dono | só desenho |
| PDF da daily (Jira) | nunca configurado | `wai-waycarbon` (reescrito) ou script do usuário via `wai-rotinas` | pasta de dados (2.5); token fica no Keychain e é da extensão, não do núcleo | não iniciado |
| Rotinas silenciosas | 0 ligadas | `wai-rotinas` (já existe) | nada; falta dias úteis e Windows | existe, com lacunas |
| Prévia do navegador / "assumir controle" | 4 aberturas | `wai-navegador` (hoje só sobe o MCP do Playwright; a janela do Chrome é a prévia) | nó de imagem em painel, se um dia quiser prévia dentro do wai | só MCP |
| ▶ Rodar / modo "!" | 0 | extensão de comandos | ação em bloco de código contribuída por extensão + `terminal/type` exposto à extensão | — |
| Escritório 2D/3D, "Vem aqui!" | quente no código, não medido | extensão de painel | nó de canvas/webview; animar só com o painel visível (CPU parado) | — |
| Notch de uso do plano (janela de 5 h) | desligado no Mac | extensão própria — ela usa a credencial do usuário, o núcleo continua sem tocar | só painel | — |
| % dos workflows (ultracode) | não medido | extensão que lê `~/.claude/projects` | pasta de dados (2.5); fonte instável | — |
| Monitor da máquina | não medido | extensão de painel | nada | baixo valor |
| Spotify | ~60 mil consultas sem conta ligada | extensão de painel com OAuth próprio | pasta de dados (2.5) | baixo valor |
| Barra de teclas, layout mobile, 5 painéis, instalador/LaunchAgent, SSO/TLS/login | — | nenhuma (coisa de tmux, celular ou servidor) | — | não trazer |

## Itens só do dono

Licença e dono do código; remoto e CI (0.2); publicar ou não o `wai-waycarbon`; certificados Apple/Windows; confirmação com a Anthropic sobre login por assinatura; bundle id final; `updates.repo`; revisão de segurança do `wai-remoto`.

## Perguntas em aberto (mudam a ordem)

1. No terminal da Central (50 aberturas em 7 dias), o que você faz: conversa com o Claude no TUI, roda `/mcp`/`/plugin`, ou comandos seus? → decide se 3.3 sobe para a Onda 1.
2. Primeiros colegas usam Mac ou Windows? → decide se 2.8 entra.
3. Vai continuar em `bypassPermissions`? → se sim, 0.4 importa mais para colegas e pode ir para a Onda 2.
4. Rascunho de nova sessão: em pasta com submodules (o `waycarbon-ecosystem-local` tem 36), New worktree deve baixar os submodules, deixá-los vazios ou sumir (você escolhe a pasta do submodule)? → fecha a spec da 1.5.
5. ~~Settings em modal (1.9): só formato ou completo?~~ Respondida em 11/11: completo, com "Coming soon" no que o wai ainda não faz.

## Diário de tropeços

(preencher na Onda 0 e 1: data — o que tentou fazer — o que faltou)
