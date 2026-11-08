# wai — estudo

> 2026-11-08 · Status: **estudo, aguardando o ok para a Fase 0** (spikes). Nenhum código escrito.
> **wai** = "UAI" (a gíria mineira) + WayCarbon.

## 0. Resumo

- A ideia: transformar a **Central IA WayCarbon** (página web que controla o Claude Code por tmux) numa
  **IDE nativa, IA primeiro**, compilada para macOS e Windows, **publicada para todos** e **extensível**. A
  WayCarbon vira só mais uma extensão.
- **A lentidão da Central não vem de ela ser web, vem do jeito que ela fala com o Claude.** Ela raspa a tela
  do tmux com regex e faz polling de 0,7 a 15 s, então o texto chega com até 2 s de atraso. Com a aba aberta,
  ela gasta **~9% de um núcleo o tempo todo** (medido). **O ganho principal vem da troca de protocolo; a
  linguagem vem em segundo lugar.**
- **Reescrever × evoluir (§12):**
  - só pelo desempenho, evoluir a Central bastaria;
  - reescrever se justifica pelos outros objetivos: nativo, Windows, IDE, publicação e extensões;
  - e o código da Central **não tem licença** que permita publicá-lo.
- **Integração:** o wai é **cliente ACP** (Agent Client Protocol, o "LSP dos agentes"). O Claude entra pelo
  adaptador `claude-agent-acp`; Gemini, Codex e outros, pelo mesmo código. **O wai nunca toca em credencial:
  cada agente cuida do próprio login**, como o Zed faz.
- **Stack:** **Rust + GPUI** (o framework de UI do Zed) **+ gpui-component**, com `alacritty_terminal` e
  `portable-pty` para o terminal. Plano B: Tauri 2 + React.
- **Extensões:**
  - um manifesto declarativo (modelos, comandos, rotinas, agentes, servidores MCP, plugins do Claude Code,
    temas);
  - **um processo opcional, em qualquer linguagem**, falando JSON-RPC por stdio (o mesmo encanamento do ACP);
  - painéis descritos em JSON e desenhados nativamente pelo wai;
  - a extensão `wai-waycarbon` é a primeira cliente.
- **Corte de escopo:** sai do núcleo o que o SO já faz ou que é enfeite. Acesso pelo celular, rotinas e
  navegador viram **extensões**.
- **Próximo passo:** a Fase 0, com três spikes descartáveis (protocolo, UI e medição da Central atual) (§10).

## 1. Decisões tomadas (2026-11-08)

| # | Decisão | Consequência no projeto |
|---|---|---|
| 1 | Publicado para todos; uso pessoal e dentro da WayCarbon; **customizável, com extensões**, e uma extensão da Way | O login segue as regras de **produto distribuído** (§5). O sistema de extensões é parte do núcleo (§6). Precisa de assinatura de código e auto-update (§10, Fase 4) |
| 2 | Perder o celular é aceitável; pode voltar **como extensão** | O núcleo não tem servidor HTTP. A API de extensões precisa permitir que uma extensão escute numa porta e controle sessões, com permissão do usuário (§6) |
| 3 | A linguagem não é problema (o código será escrito pela IA) | Rust + GPUI vira o plano A, sem ressalva |
| 4 | Construir o nosso, sem fork do Zed | Nada de código GPL do Zed; o Zed serve só como referência de leitura |
| 5 | Nome: **wai** | `wai` já existe no crates.io (crate abandonado de 2017), no npm e como biblioteca Haskell conhecida. O produto pode se chamar wai; os pacotes internos usam `wai-*` (`wai-ide` está livre no crates.io) |

## 2. O que a Central é hoje

Repositório: `Project/central-ia-waycarbon-claude`.

| Peça | Detalhe |
|---|---|
| Servidor | `central/central.py`, Python com só a biblioteca padrão: 11.729 linhas, das quais ~4,4 mil são autoteste |
| Tela | `central/www/`, JS puro: `app.js` 5.231 linhas, `app.css` 3.981, `cena3d.js` 2.693 com three.js |
| Claude Code | roda dentro de um `tmux` |
| Terminal | `ttyd` |
| Mac | `macos/` com LaunchAgent, TLS próprio e Chrome com Playwright |

**Núcleo:**
- ponte com o tmux: texto, teclas, leitura da tela, menus como botões, modo "!";
- mapeamento janela ↔ sessão;
- conversas: continuar, cópia (fork), fechar;
- anexos;
- terminal;
- painéis lado a lado;
- reabrir após reboot.

**Vitrine:** o Escritório 2D/3D e o progresso de workflows.

**Extras:**
- painel de uso, já **desligado no Mac** (`USO=0` em `macos/gerenciar.py:451`);
- monitor da máquina;
- rotinas e PDF da daily;
- navegador integrado;
- Spotify;
- ritmo/festa/DJ: o caminho de áudio é **código morto**, `/api/ritmo` sempre responde 503.

## 3. Por que é lenta (diagnóstico com evidência)

| Gargalo | Evidência |
|---|---|
| Um processo por operação | Toda chamada ao tmux é um `subprocess.run` (`central.py:3778`). No Mac, cada uma passa por um script de shell (`macos/tmux-central`). Enviar uma mensagem custa ≥5 spawns; ler a tela, ≥3; escolher num menu, uma captura a cada 120 ms por até 1,5 s. **Medido: ~9 ms por chamada** (~50 ms por envio). Custa pouco |
| Raspagem de tela | Menus, permissões e o modo "!" são detectados por regex sobre `capture-pane` (`central.py:924-1213`). As regras dependem de títulos em inglês e foram validadas no Claude Code **2.1.x**. Erros do parser são engolidos |
| Polling em tudo | O navegador só faz polling (`app.js:167`): cena a cada 2 s, conversa a cada 2 s, tela de 0,7 a 4 s, lista de 1 a 10 s. As respostas são snapshots completos, sem delta, sem ETag e sem gzip |
| Releitura de arquivo | `/api/conversa` relê **até 4 MB do fim do jsonl** a cada mudança de tamanho (`central.py:4148-4159`), ou seja, a cada 2 s enquanto o Claude trabalha. **Medido: 6 ms por releitura** (no maior jsonl, de 72 MB). Custa pouco |
| Varredura de disco | `indice()` lista `~/.claude/projects` inteiro e faz `stat` em cada jsonl, com cache de 1 s. Nesta máquina são **1,2 GB** |
| CLI em laço | `claude agents --json --all`, um processo Node, roda a cada 5 s. **Medido: 0,43 s por chamada e 0,17 s de CPU**, ~3,4% de um núcleo só nisso |
| **Total medido** | Servidor com a aba aberta e parado: **~5% de CPU** e 95 MB de memória. Somando o `claude agents`, **~9% de um núcleo o tempo todo**. A aba do Chrome não foi medida |
| Trava global | Uma trava cobre cena, lista e conversa, leitura de disco incluída. Fechar uma conversa segura as ações por ~7 s |
| Painéis lado a lado | Cada painel é um iframe com **uma cópia inteira do app**, então o polling multiplica |

Conclusão, corrigida depois das medições:
- spawn e releitura custam pouco;
- o que pesa é o **atraso do polling** (até 2 s), o **consumo constante** (~9% de um núcleo) e a **fragilidade**
  da raspagem;
- o que resolve é parar de raspar tela e de fazer polling.

## 4. Princípios

1. **Nada de polling.** Tudo é evento: stream do agente, watcher de arquivos, saída do PTY. Parado, gasta ~0% de
   CPU.
2. **Protocolo estruturado, nunca tela.** Permissão, pergunta, subagente e fim de turno chegam como mensagens
   tipadas.
3. **Um processo nativo.** Sem servidor HTTP, navegador ou ttyd no núcleo. Agentes e extensões são processos filhos.
4. **IA primeiro.** A tela principal é a conversa e o que os agentes estão fazendo. O editor existe para revisar e
   ajustar o que o agente fez.
5. **Núcleo pequeno, resto em extensão.** O núcleo tem só o que todo usuário precisa. O resto, Way incluída, é
   extensão.
6. **O wai nunca toca em credencial.** Cada agente cuida do próprio login.

## 5. Integração com o Claude e login

| Opção | Prós | Contras |
|---|---|---|
| tmux + raspagem (hoje) | Pega qualquer coisa do CLI interativo | Frágil, spawn por ação, polling |
| CLI direto: `claude -p --input-format stream-json --output-format stream-json` | Usa o `claude` instalado; sem dependência extra | Sem schema publicado. Permissão só por `--permission-prompt-tool`, que exige que o host exponha uma ferramenta MCP. `AskUserQuestion` não documentado |
| Agent SDK (TS/Python) como processo auxiliar | API completa: `canUseTool`, hooks, `interrupt`, resume/fork | Leva Node ou Python. Só serve para o Claude |
| **ACP + `claude-agent-acp`** | Protocolo padrão com **SDK oficial em Rust**; permissões fazem parte dele; serve 50 agentes; usado por Zed, JetBrains, Neovim e Emacs | O adaptador roda sobre o Agent SDK (Node) |

**Decisão: ACP.** Plano B para o Claude, se o adaptador falhar no spike: `stream-json` direto. Porta de saída
sempre aberta: o `claude` interativo no terminal embutido.

**Login num produto público:**
- **O que as regras atuais dizem** (ver Fontes):
  - terceiros não podem oferecer login claude.ai no próprio produto, incluindo agentes feitos com o Agent SDK, sem
    aprovação da Anthropic;
  - não podem guardar nem intermediar essas credenciais;
  - o que é permitido: o usuário entrar com a própria assinatura no **Claude Code sem modificação**, "inclusive
    quando uma plataforma hospeda o Claude Code".
- **Como o wai fica do lado certo dessa linha:**
  - o wai nunca mostra tela de login de nenhum provedor e nunca lê, guarda ou repassa token;
  - quem autentica é o agente: o adaptador e o Claude Code do próprio usuário, igual ao modelo do Zed, que é um
    editor distribuído e faz exatamente isso;
  - **caminho documentado:** API key (ou Bedrock/Vertex) configurada no agente;
  - **assinatura:** cada pessoa usa o próprio Claude Code, logado por ela.
- **Antes do lançamento público, confirmar com a Anthropic** que o uso por assinatura via `claude-agent-acp` num
  editor de terceiros é aceito. O ponto de atenção: o adaptador usa o Agent SDK, e a página do SDK fala em
  aprovação.
- **O painel de uso da Central sai.** Ele usa um endpoint não documentado (`api/oauth/usage`) com o token do usuário.
  A fonte possível é o JSON oficial da statusline (`rate_limits`); falta verificar se ele aparece via ACP.

**Hooks oficiais do Claude Code** que alimentam o painel de agentes sem heurística:
- `PermissionRequest`, `Notification`, `Stop`, `SubagentStart`/`SubagentStop`, `TaskCreated`/`TaskCompleted`,
  `SessionStart`/`SessionEnd`.

## 6. Extensões (novo)

**Objetivo:** a WayCarbon, ou qualquer empresa ou pessoa, adapta o wai sem mexer no núcleo.

**Antes, o que já existe:**
- o lado "agente" da customização já tem dono: **plugins do Claude Code**, com agentes, skills, comandos, hooks e
  MCP. A Way já usa isso (ex.: `beehive-waycarbon`);
- **as extensões do wai não reinventam isso.** Elas declaram quais plugins do Claude Code e quais servidores MCP
  querem, e o wai os instala e liga.

**Três camadas, da mais simples para a mais poderosa:**

| Camada | O que é | Exemplos |
|---|---|---|
| **1. Configuração** | `~/.config/wai/` para o usuário e `.wai/` dentro do repositório para o time, versionada junto com o código | Tema, atalhos, modelos de conversa do projeto, agente padrão |
| **2. Manifesto** (`wai-extension.toml`) | Extensão sem código: só declara contribuições | Modelos, comandos que viram prompt, agentes ACP, servidores MCP, plugins do Claude Code, temas, ícones |
| **3. Processo** (opcional) | Um executável em **qualquer linguagem** (Go, TS/Node, Python, Rust) que fala JSON-RPC por stdio com o wai, o mesmo encanamento do ACP | Painel do Jira, rotina da daily, acesso remoto pelo celular |

**API da camada 3:**
- **o wai avisa a extensão** de eventos (sessão aberta, permissão pedida, turno terminado, arquivo salvo) e de
  comandos e cliques de painel;
- **a extensão pode:**
  - abrir sessão a partir de um modelo;
  - mandar mensagem;
  - listar sessões e estados;
  - notificar;
  - mostrar diff;
  - **desenhar um painel**.

**Painéis declarativos:**
- a extensão manda uma árvore em JSON com componentes fixos (lista, árvore, tabela, markdown, formulário, botão,
  selo, barra de progresso), e o wai desenha nativamente;
- extensão nunca desenha pixel, o que mantém o desempenho e a cara do app;
- é o modelo do Raycast.

**Permissões:**
- o manifesto declara o que a extensão pede: rodar processos, rede, escutar porta, pastas fora do projeto, controlar
  sessões;
- o usuário aprova na instalação;
- **v1:**
  - instala por link git ou pasta local;
  - o processo roda com os poderes do usuário, como extensões do VS Code e servidores MCP;
- **depois:**
  - para um catálogo aberto a qualquer autor, sandbox em WASM (modelo do Zed, com `wasmtime`);
  - fica para quando houver catálogo.

**Primeiras extensões:**

| Extensão | Camada | O que faz |
|---|---|---|
| `wai-waycarbon` | 2 + 3 | Liga o plugin `beehive-waycarbon` e o MCP do Jira. Traz modelos por repositório (ecosystem, bff, front, Go) e um painel "meus cards" do projeto EC. Inclui a rotina da daily, a partir do `daily-pdf` da Central |
| `wai-remoto` | 3 | Acesso pelo celular: servidor local que expõe sessões e permissões; precisa da permissão "escutar porta" + "controlar sessões". Reaproveita o modelo de segurança da Central (cookie assinado, `Origin` exato, PBKDF2) |
| `wai-rotinas` | 3 | Rotinas silenciosas da Central (script na lista, horário, só avisa quando tem novidade) |
| `wai-navegador` | 2 | Liga o MCP do Playwright; a pré-visualização fica para depois |

## 7. Stack da interface

| Stack | Nativo | Editor de código | Terminal | Maturidade | Licença |
|---|---|---|---|---|---|
| **Rust + GPUI + gpui-component** | Sim, GPU (Metal / DirectX 11) | Rope + tree-sitter + LSP, "estável com 200 mil linhas" | `alacritty_terminal` + `portable-pty` | Zed 1.0 (2026-04), Windows estável desde 2025-10; GPUI ainda pré-1.0 | Apache-2.0 |
| Tauri 2 + React (plano B) | Casca nativa, UI em webview | Monaco / CodeMirror 6 | xterm.js + `portable-pty` | Madura (2.12) | MIT/Apache |
| Wails v3 (Go) | Casca nativa, UI em webview | Monaco / CodeMirror 6 | PTY no Windows fraco | v3.0 GA há 2 semanas | MIT |
| Avalonia 12 (C#) | Sim, Skia | AvaloniaEdit | Sem terminal maduro | Madura | MIT |
| Flutter | Sim | `re_editor` | Pacote `xterm` parado desde 2024 | Multi-janela fora do estável | BSD |
| Qt 6 (C++) | Sim | QScintilla | QTermWidget é GPL | Muito madura | LGPL/comercial |
| iced / egui / Slint / Makepad / Xilem / Floem | Sim | Fraco | Fraco | Experimental ou parado | Várias |

**Rust + GPUI + gpui-component** é a única stack com uma IDE rápida já em produção no macOS e no Windows (o Zed).
Ela já entrega editor, dock com abas e divisões, markdown e listas virtuais, e o ACP tem SDK oficial em Rust.

**Riscos que continuam:**
1. O crate `gpui` não sai no crates.io desde 2025-10. O gpui-component usa `gpui-pre`, uma cópia publicada por
   **um** mantenedor da Longbridge (risco de pessoa única).
   - Mitigação: fixar a versão.
   - Plano: aceitar ter que acompanhar o Zed direto pelo git.
2. A documentação, na prática, é o código do Zed.
3. O suporte a leitor de tela ainda é imaturo, e num produto público isso pesa mais.
4. Editor, rope e terminal do Zed são GPL: só leitura.

## 8. Arquitetura

```
┌──────────────────────── wai · 1 processo nativo (Rust + GPUI) ─────────────────────────┐
│ UI na GPU: [Agentes/Sessões] [Chat em streaming] [Editor + diff] [Terminal] [Painéis de extensão] │
│        ▲ redesenha só quando chega evento                                              │
│ Núcleo assíncrono                                                                      │
│  agentes ── cliente ACP ─stdio─▶ claude-agent-acp ─▶ Claude Code (login do próprio usuário) │
│                           └────▶ gemini --acp, codex-acp…                              │
│  extensões ─ JSON-RPC ─stdio─▶ wai-waycarbon, wai-remoto… (Go/TS/Python/Rust)          │
│  terminal ── portable-pty + alacritty_terminal                                         │
│  workspace ─ watcher (FSEvents / ReadDirectoryChangesW), git, .wai/                    │
│  histórico ─ ~/.claude/projects/*.jsonl lido incrementalmente, por evento              │
│  estado ──── layout e sessões abertas salvos localmente                                │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

- **Uma sessão = um processo de agente.** Uma extensão = um processo (ou nenhum, se for só manifesto). Agentes e
  extensões usam o mesmo tipo de canal: JSON-RPC por stdio.
- **Histórico:** o watcher avisa a mudança e o wai lê só os bytes novos.
- **Aprovar edições:** as edições do agente chegam como diff, com **aceitar/rejeitar por trecho**.

## 9. Funcionalidades: núcleo, extensão ou fora

| Central hoje | No wai | Como |
|---|---|---|
| Chat | **Núcleo** | Stream token a token; anexo por arrastar e soltar |
| Menus do Claude como botões | **Núcleo** | Permissão e pergunta chegam como mensagem |
| Conversas: continuar, cópia, fechar | **Núcleo** | resume/fork pelo protocolo |
| Lado a lado | **Núcleo** | Painéis nativos |
| Terminal | **Núcleo** | PTY nativo |
| "Vem aqui!" e avisos | **Núcleo** | Selo na sessão + notificação nativa |
| Escritório 2D/3D | **Núcleo**, como "painel de agentes" 2D | Sessões, subagentes e tarefas ao vivo pelos hooks. O 3D sai |
| Reabrir após reboot | **Núcleo** | Layout e sessões salvos; retoma com resume |
| Modelos de conversa | **Núcleo** (formato) + extensões (conteúdo) | Configuração e manifesto |
| Acesso pelo celular (PWA) | **Extensão** `wai-remoto` | §6 |
| Rotinas, PDF da daily | **Extensão** `wai-rotinas` / `wai-waycarbon` | §6 |
| Navegador integrado | **Extensão** `wai-navegador` | §6 |
| Painel de uso | Depois | Só com fonte oficial (§5) |
| Progresso de workflows | Depois | Só com fonte estável |
| Barra de teclas, modo "!" | Fora | Sem tela de terminal; existe o terminal ao lado |
| Monitor da máquina, Spotify, festa/DJ | Fora | O SO já faz; a festa é código morto |
| SSO, cookie, TLS, servidor HTTP | Fora do núcleo | Só a `wai-remoto` precisa disso |
| — | **Novo no núcleo** | Árvore de arquivos, editor com tree-sitter, busca, revisão de diff, extensões |

## 10. Metas e fases

**Metas de desempenho:**

| Métrica | Central hoje | Meta |
|---|---|---|
| Atualização da tela | Polling de 0,7 a 15 s, snapshot completo | Por evento; nenhum timer parado |
| Processos por ação | 3 a 8; `claude agents` a cada 5 s | Zero |
| Atraso do texto do Claude | Até 2 s + releitura de até 4 MB | Menos de 1 quadro depois do token chegar |
| CPU com a tela aberta e parada | ~9% de um núcleo (medido) | ≈ 0% |
| Memória | Servidor 95 MB + aba do Chrome (medir no S3) | < 300 MB com 1 projeto, 3 sessões e 1 terminal |
| Abrir o app | Medir no S3 | < 1 s |
| Arquivo grande | Não existe | 100 MB sem travar |

**Fase 0 — spikes descartáveis:**

| Spike | O que prova | Se falhar |
|---|---|---|
| **S1 · Protocolo** | Via ACP + `claude-agent-acp`, com **API key** e com **o Claude Code do próprio usuário logado por assinatura**: mandar mensagem, stream, aprovar permissão, responder pergunta, interromper, resume e fork | `stream-json` direto + `--permission-prompt-tool` |
| **S2 · UI** | Janela GPUI + gpui-component **no Mac e no Windows** com editor (100 MB), terminal com PTY e chat markdown em streaming; medir CPU e memória | Não bateu as metas ou o Windows travou → Tauri 2 |
| **S3 · Linha de base** | Medir a Central atual: o número a vencer | — |

**Fase 1 — MVP do núcleo (macOS):**
- sessões;
- chat em streaming;
- permissões e perguntas;
- terminal;
- continuar e fork;
- notificações;
- layout salvo.

**Fase 2 — IDE:**
- árvore de arquivos;
- editor com tree-sitter;
- revisão de diff;
- busca;
- painel de agentes;
- configuração em `.wai/`.

**Fase 3 — Extensões v1 + Windows:**
- manifesto, processo JSON-RPC, painéis declarativos e permissões;
- **`wai-waycarbon` como primeira cliente**, para validar a API no uso real;
- build no Windows.

**Fase 4 — Publicação:**
- assinatura e notarização Apple (Developer ID);
- assinatura de código no Windows;
- instalador e auto-update;
- site;
- confirmação com a Anthropic sobre login (§5);
- extensões `wai-remoto`, `wai-rotinas` e `wai-navegador`.

## 11. O que ainda está em aberto

1. **Licença e dono do código.** Código aberto ou fechado, e com qual licença? Feito por funcionário da WayCarbon
   e publicado para todos, vale confirmar com a empresa se o código é seu ou dela antes da Fase 4.
   **Minha sugestão:** código aberto com Apache-2.0, a mesma licença do GPUI.
2. **Conversa com a Anthropic sobre login por assinatura.** Só você pode puxar. Não bloqueia as Fases 0 a 3.
3. **Minha decisão até você dizer o contrário:**
   - extensões v1 instalam por link ou pasta, sem catálogo e sem sandbox;
   - o catálogo aberto, com WASM, fica para depois da publicação.

## 12. Reescrever do zero × evoluir a Central

**Medições feitas em 2026-11-08 nesta máquina, com a Central rodando:**

| O quê | Resultado |
|---|---|
| Uma chamada ao tmux (pelo wrapper do Mac) | ~9 ms |
| `claude agents --json --all` | 0,43 s; 0,17 s de CPU; roda a cada 5 s |
| Releitura de 4 MB do jsonl + parse | 6 ms |
| Servidor com a aba aberta, parado | ~5% de CPU, 95 MB, 11 threads |
| Total contínuo | ~9% de um núcleo |

**Três caminhos:**

| | A · Ajustes rápidos na Central | B · Trocar o motor da Central | C · Reescrever (wai) |
|---|---|---|---|
| O que é | SSE no lugar do polling na conversa e na cena; `claude agents` só quando precisar; cache dos estáticos | Trocar tmux + raspagem por `stream-json` ou Agent SDK em Python, mantendo Python + web | IDE nova em Rust + GPUI + ACP |
| Esforço | Dias | Semanas: o motor **é** o núcleo (~1.500 linhas de `central.py` + boa parte do `app.js`) | Meses até a paridade |
| Atraso do texto | Até 2 s → quase instantâneo | Instantâneo | Instantâneo |
| CPU parada | ~9% → poucos % (estimativa) | ~1% (estimativa) | ≈ 0% (meta) |
| Fragilidade (regex presa ao 2.1.x) | Continua | Some | Some |
| Memória | Igual (aba do Chrome) | Igual | Sem Chrome |
| Mesma sessão no chat e no terminal | Mantém | Perde | Perde |
| Windows | Não (tmux, ttyd, sockets Unix) | Não sem reescrever a base | Sim |
| IDE (editor, diff, árvore) | Não | Só com Monaco na web | Nativa |
| Celular | Mantém | Mantém | Volta como extensão |
| Publicar para todos | Não: a instalação pede Homebrew, Python, tmux, ttyd, Node e certificado no chaveiro | Não, pelo mesmo motivo | Sim: um app assinado |
| Extensões | Não | Teria que criar | No plano |
| Licença | Código sem licença (abaixo) | Idem | Código novo |
| Risco técnico | Baixo | Médio | Alto (GPUI pré-1.0, tudo novo) |
| O que se joga fora | Nada | O motor | O código; fica o conhecimento |

**O ponto que mais pesa é a licença.**
- O `PROVENIENCIA.md` diz que a base veio de um ZIP (`central-do-claude-main.zip`), com autor e origem não
  informados, e que "nenhuma nova licença geral foi atribuída ao código recebido".
- O repositório está no git do Thiago (`git.rebobina.app/thiago/...`): 5 commits, desde 2026-10-29.
- Código sem licença, por padrão, não dá direito de redistribuir. Publicar uma evolução da Central exigiria a
  permissão de um autor original que nem se sabe quem é. Não é parecer jurídico; vale confirmar.
- O wai, escrito do zero e sem copiar trechos da Central (só ideias e comportamento), não herda esse problema.

**Conclusão:**
- **Só pelo desempenho, evoluir ganharia.** Os ajustes rápidos (A) tiram a maior parte do atraso em dias.
- **Mas 5 dos 6 objetivos** (nativo, Windows, IDE, publicação, extensões) **não cabem na Central**, e a licença
  impede publicá-la. Trocar o motor (B) custa semanas e deixa tudo isso de fora.
- **Recomendação: C (wai) para o produto.** A Central continua como sua ferramenta do dia a dia até o MVP do wai.
  - Fazer A só se o atraso estiver atrapalhando o seu dia: é trabalho num código que vai ser aposentado, num
    repositório que é do Thiago.
- **O que o wai aproveita da Central:**
  - a lista de funções;
  - a experiência dos menus como botões;
  - o painel de agentes;
  - o modelo de segurança (para o `wai-remoto`);
  - os cenários de teste;
  - ideias e comportamento, nunca o código.

## Fontes

**Claude Code:**
- Modo headless e stream-json: https://code.claude.com/docs/en/headless.md · https://code.claude.com/docs/en/cli-reference.md
- Agent SDK e regra de login: https://code.claude.com/docs/en/agent-sdk/overview.md
- Termos de login e assinatura: https://code.claude.com/docs/en/legal-and-compliance.md
- Hooks: https://code.claude.com/docs/en/hooks.md
- Statusline / `rate_limits`: https://code.claude.com/docs/en/statusline

**ACP:**
- https://agentclientprotocol.com/
- Adaptador do Claude: https://github.com/agentclientprotocol/claude-agent-acp
- Registro: https://zed.dev/blog/acp-registry
- Agentes externos no Zed (o agente cuida do login): https://zed.dev/docs/ai/external-agents
- JetBrains: https://blog.jetbrains.com/ai/2025/12/bring-your-own-ai-agent-to-jetbrains-ides/

**Zed e GPUI:**
- Windows: https://zed.dev/blog/zed-for-windows-is-here
- Zed 1.0: https://phoronix.com/news/Zed-1.0-Released
- Licenças: https://github.com/zed-industries/zed
- Acessibilidade: https://github.com/zed-industries/zed/issues/41138

**Componentes:**
- gpui-component: https://github.com/longbridge/gpui-component · https://crates.io/crates/gpui-pre
- Terminal: https://crates.io/crates/alacritty_terminal · https://crates.io/crates/portable-pty

**Alternativas:**
- Tauri: https://v2.tauri.app/start/
- Tauri × Electron: https://dev.to/gethopp/tauri-vs-electron-performance-bundle-size-and-the-real-trade-offs-1el4
- Wails v3: https://github.com/wailsapp/wails/releases/tag/v3.0.0
- Avalonia 12: https://avaloniaui.net/blog/avalonia-12

**Central atual:** os números de linha citados nas seções 2, 3 e 9 são de `Project/central-ia-waycarbon-claude` em
2026-11-08.
