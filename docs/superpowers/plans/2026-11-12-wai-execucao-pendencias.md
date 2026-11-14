# wai — Plano de execução das pendências (a partir da 0.6.2)

> **Para agentes:** este é o plano de execução do que sobrou do plano mestre (`2026-11-11-wai-master-plan.md`), na ordem em que vai ser feito. Cada trilha da Fase 1 tem um plano detalhado próprio (superpowers:writing-plans), citado na tabela. Executar com superpowers:subagent-driven-development, uma trilha por worktree (`.claude/worktrees/<trilha>`, branch `feat/<trilha>` a partir da `development`). Os planos das Fases 2–4 são escritos na hora de executar, porque outra sessão também versiona o wai.

**Objetivo:** fechar as Ondas 0–3 do plano mestre, exceto a atualização automática, que outro agente está fazendo (spec `docs/superpowers/specs/2026-11-12-wai-release-and-auto-update-design.md`).

**Base:** `development` em `a818cc8` (0.6.2). Já está no código: Fases 0–4, Aurora, Settings+agentes, subagentes, Onda 1 (1.1–1.3, 1.5–1.9), redesenho do chat (0.6.1) e caixa de texto com as pílulas (0.6.2). O remoto (0.2) existe: `origin` = `Guilbritto/wai`.

## Fora deste plano

- Atualização automática, release pelo GitHub e passos de release do `CLAUDE.md`: com o outro agente.
- **3.7** (checagem de atualização com proxy e redirecionamento): o código de atualização vai ser reescrito pela spec acima.
- **3.16** (DMG notarizado): depende de certificado Apple, decisão do dono.

## Respostas do dono (2026-11-12)

1. **Terminal:** usa para conversar no TUI do Claude, para `/mcp`/`/plugin` e para comandos próprios. Por isso a **3.3** (vários terminais) sobe para a Fase 1.
2. **Colegas:** Mac. O dono vai testar no Windows também: **2.8** e **3.15** ficam no fim da fila (Fase 4).
3. **Submodules (pergunta 4):** "New worktree" continua desligado em pasta com submodules. Nota do dono: *"Esse processo de worktree é simplesmente para mostrar qual worktree que a IA está usando, quem decide como vai trabalhar e organizar isso é a IA. Hoje quando estamos trabalhando dentro de inventories (por exemplo) a IA entra dentro da pasta e cria uma tree para trabalhar e aí isso deve ser mostrado ali junto com a pasta que ela está navegando."* Vira o item novo **1.10**.
4. **Toggle de servidor MCP:** pode gravar no `~/.claude.json`, só a lista de desligados do projeto, de forma atômica, preservando o resto.

## Itens novos

| Id | Item | Onde | Tamanho |
|---|---|---|---|
| 1.10 | **Pílulas seguem a IA.** As pílulas da caixa de texto (pasta · branch · agente) mostram a pasta e o worktree em que o agente está trabalhando de fato, não só os do início da sessão. Exemplo: a sessão começa em `waycarbon-ecosystem-local`, o Claude entra em `inventories/` e cria um worktree; as pílulas passam a mostrar `inventories` e o branch do worktree, com o ícone de worktree. Sinal: caminhos das chamadas de ferramenta do agente (ACP `locations`), resolvidos para a raiz git e o branch; nada de polling. | `chat/composer.rs`, `chat/model.rs`, `workspace.rs` | M |
| 1.11 | **Duplo clique no vazio de Sessions abre uma nova sessão** (pedido do dono, 12/11). Hoje o duplo clique na aba Sessions abre o rascunho, numa linha ou num grupo renomeia, e no espaço vazio da lista não faz nada. Passa a abrir o rascunho de nova sessão, como o botão New session. | `sidebar.rs` (painel Sessions da barra lateral nova) | P |
| 1.12 | **Áreas de trabalho** (pedido do dono, 12/11). Decisões do dono: (a) um grupo da barra lateral pode virar uma área de trabalho, isto é, as telas montadas com as sessões dele (até 4, `MAX_PANES`); (b) dentro da área ele muda e organiza as telas (abrir ao lado, arrastar, fechar) e a área guarda isso; (c) ao trocar de área ou clicar numa sessão de fora da área, a área atual **se esconde intacta** e aparece a outra área, ou a sessão clicada sozinha numa tela só; (d) voltar para a área traz as telas exatamente como estavam. Nas palavras do dono: *"se eu criar uma área de trabalho eu posso mudar/organizar as abas e depois se mudar a área de trabalho ou clicar em uma sessão fora da área de trabalho, a área de trabalho atual 'esconde' e é mostrado a nova área de trabalho ou a sessão que foi clicada (de forma unitária na tela)"*. A fechar na spec (padrões propostos): criar pelo cabeçalho do grupo ("Make work area"); clicar numa sessão de outra área mostra aquela área com o foco nela; fechar a tela (×) dentro da área tira a sessão da área; as áreas sobrevivem a ⌘Q; sessões de área escondida continuam rodando; área escondida não desenha (CPU parado). | `workspace.rs` (`panes`, `focused_pane`, `show`, `session_pane`), `wai-settings` (`AppState`, `SessionGroup`), `sidebar.rs` | G |

## Frentes paralelas de outras sessões

| Frente | Branch | O que muda para este plano |
|---|---|---|
| Atualização automática e release pelo GitHub | `feat/release-auto-update`, `feat/release-pipeline` | As trilhas não mexem em `update.rs`, na página Updates de `settings_view.rs`, no caminho de saída do `workspace.rs`, em `.github/workflows/*` e no `CLAUDE.md`. |
| Barra lateral nova (barra de ícones, agentes embaixo da caixa de texto, gaveta do subagente acima dela), aprovada pelo dono | `feat/sidebar-redesign` | A **3.1** (parar um subagente) e a **3.2** (transcrição do subagente) saem da trilha chat e vão para a Fase 2, em cima da gaveta. A marca "open elsewhere" na History (2.1) também, porque a History vira seção do painel Sessions. As trilhas mexem o mínimo em `sidebar.rs`. |

Cada trilha faz merge da `development` no seu branch antes de entregar, para resolver os conflitos com o que essas frentes já tiverem mergeado.

## Fase 0 — Só o dono (corre em paralelo com tudo)

- [ ] **0.3** (o dono não vai desligar ainda, 12/11) Desligar a Central de forma reversível (`launchctl bootout gui/$UID/com.waycarbon.central-ia` e o `.atualizar`); deixar instalada 2 semanas.
- [ ] **1.4** Uma semana só no wai, anotando os tropeços no "Diário de tropeços" do plano mestre.
- [x] **0.9** Aprovar o desenho de grupos e renomear (lápis/duplo clique, grupos recolhíveis no Open, arrastar linha para o cabeçalho). Aprovado em 12/11.
- [ ] Julgar o vidro do balão fixo (alfa 0.72), prints em `.superpowers/sdd/master-onda-1/track-a-shots/`.
- [x] Pergunta 3: continua em `bypassPermissions`? Sim (12/11).

## Fase 1 — Agora (4 trilhas)

| Trilha | Itens | Plano detalhado | Arquivos principais |
|---|---|---|---|
| **chat** | 2.4 custo da conversa · 3.6 Revert com tab/CRLF · ajustes menores adiados da trilha A da Onda 1 (Tarefas 1, 2 e 8; as 3–7 ficaram para a Fase 2) | `2026-11-12-wai-onda-2-chat.md` | `chat/render.rs`, `chat/edits.rs`, `chat/model.rs` |
| **extensões e settings** | 2.5 pasta de dados das extensões e atualizar sem desinstalar · 3.14 `wai-rotinas` (matar o que o script iniciou ao estourar o tempo; horário que cai no sono) · toggle de servidor MCP (gravar `~/.claude.json`) · ajustes menores adiados da trilha C | `2026-11-12-wai-onda-2-ext-settings.md` | `wai-ext`, `extensions/wai-rotinas`, `settings_view.rs` (página MCP), `docs/extensions.md` |
| **sessões** | 2.6 R84 · 1.10 pílulas seguem a IA · 2.1 aviso de conversa aberta em outro lugar (a pergunta ao retomar; a marca na History fica para a Fase 2) · ajustes menores adiados da trilha B · 0.8 prova de 20 rodadas | `2026-11-12-wai-onda-2-sessions.md` | `workspace.rs` (sessões), `sidebar.rs`, `chat/composer.rs` |
| **workspace** | 3.3 vários terminais (abas) · 3.5 roda do mouse em `less`/`vim` · 3.4 abas do editor voltam ao reabrir · 3.10 trocar de pasta relê tema e `default_agent` · 3.11 `.gitignore` fora de repositório git · 3.12 hot-exit com `fsync` e limpeza | `2026-11-12-wai-onda-2-workspace.md` | `workspace.rs` (painéis), `terminal_view.rs`, `wai-terminal`, `wai-settings` (`AppState`), árvore/busca, recovery |

**Ordem de merge na `development`:** chat → extensões e settings → sessões → workspace. Sessões e workspace mexem os dois em `workspace.rs`, o ponto quente de conflito; o merge da 4ª trilha resolve os conflitos no worktree dela. Se a atualização automática for mergeada antes, cada trilha faz merge da `development` antes de entregar. A página Updates de `settings_view.rs`, `update.rs` e o caminho de saída do `workspace.rs` (pergunta antes de sair) são do outro agente: as trilhas não mexem neles.

**Pronto quando:** as 4 trilhas estão na `development`, a suíte inteira passa (`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`) e a 0.8 rodou 20 vezes seguidas sem falha.

### Conferência no app (depois das 4 trilhas)

- [ ] **0.6** Ver no app rodando o que só passou por testes: modos de processamento, painel Agents, grupos e renomear, cartões de subagente, aviso do SO com a janela atrás, reabrir depois de ⌘Q, Revert, menu `/`, e as entregas da Fase 1 (custo, pílulas seguindo a IA, abas de terminal, toggle MCP).
- [ ] **0.7** CPU parado com 3 sessões abertas e as 3 extensões ligadas: < 0,5%.
- [ ] Versionar e instalar pelo processo valendo na hora (o do `CLAUDE.md` ou o automático do outro agente).

## Fase 2 — Depois do merge das frentes paralelas

Mexem em Settings, no `main.rs`, no ciclo de vida do app ou na barra lateral, os mesmos lugares das frentes de outras sessões.

| Id | Item | Depende de |
|---|---|---|
| 2.3 | Versão do adaptador em Settings > AI e aviso quando sair outra | atualização automática mergeada (mesma checagem e mesma tela) |
| 2.2 | Copiar diagnóstico; antes, o wai passa a gravar log em arquivo | idem |
| 3.13 | Um wai por vez (impedir ou avisar a 2ª instância) | idem: tem de deixar o **Restart now** abrir o wai novo enquanto o velho fecha |
| 2.7 | Testar Codex, Antigravity e Copilot de verdade | 2.6 (trilha sessões); o dono instala e faz login nas CLIs |
| 3.1 | Parar um subagente, na gaveta e na lista de agentes embaixo da caixa de texto | barra lateral nova mergeada; ponto de partida: Tarefas 3–4 de `2026-11-12-wai-onda-2-chat.md` (o adaptador 0.103.0 já para um subagente só com `session/cancel` no id dele) |
| 3.2 | Texto, raciocínio e checklist do subagente na gaveta; subagentes terminados na lista | idem |
| 2.1b | Marca "open elsewhere" nas linhas da History | barra lateral nova mergeada; usa `elsewhere::open_elsewhere` da trilha sessões |
| 1.11 | Duplo clique no vazio de Sessions abre uma nova sessão | barra lateral nova mergeada (ou entra nela, se a outra sessão aceitar) |
| 1.12 | Áreas de trabalho: spec própria (superpowers:brainstorming, com mockup), depois plano | barra lateral nova mergeada (grupos e painel Sessions mudam lá) e trilhas sessões e workspace mergeadas (mesmo `workspace.rs`, mesmas telas) |

## Fase 3 — Precisam de spec com o dono antes (superpowers:brainstorming)

- **3.8** Personalização: atalhos, fontes e temas (ampliado pelo dono). Spec aprovada: `docs/superpowers/specs/2026-11-12-wai-personalization-design.md`. Duas trilhas, nesta ordem: atalhos, depois aparência.
- **3.9** Servidor MCP `wai`: em qualquer conversa a IA muda a configuração, com aviso e Undo. Só configuração por ora; sem botão "Configure with AI". Spec aprovada: `docs/superpowers/specs/2026-11-12-wai-mcp-server-design.md`. Vem depois das duas trilhas da 3.8.
- Extensões do mapa do plano mestre (`wai-remoto` e as outras): nenhuma por ora (dono, 12/11). Voltar a elas quando for desligar a Central (0.3).

## Fase 4 — Windows (fim da fila)

Dono, 12/11: ainda não sabe se algum colega vai usar no Windows; fica no fim. A CI continua compilando e testando no Windows.

- **2.8** Primeira execução do `.exe`; `wai-rotinas` no Windows (hoje recusa todo script, R67). A CI já roda no remoto.
- **3.15** Ícone no `wai.exe` (R64), filhos do `npx` sobrevivendo ao agente, `--prefix` do `wai-navegador` num prefixo do usuário (R71).

---

## Respostas do dono (2026-11-12, rodada de perguntas)

- **Central:** fica ligada por ora (0.3 adiada).
- **0.9** aprovado.
- **Pergunta 3:** continua em bypass.
- **2.7** (Codex, Antigravity, Copilot): nenhum agora.
- **Ajustes menores adiados** das trilhas áreas, barra lateral e Fase 2 núcleo (13 itens, mais o teste instável `sending_a_draft_starts_one_session_in_its_folder`): todos numa trilha só.
- **Worktrees que sobraram:** removidas.
  - `ci-windows` e `sidebar-redesign` já estavam mergeadas.
  - `clear-command` tinha um `/clear` sem commit: virou o commit `3916ece` no branch `feat/clear-command`, a partir de `852c8e7`.
  - A trilha de ajustes faz o merge da `development` nesse branch, roda a suíte e o leva para a `development` (dono, 12/11).
- **Áreas de trabalho:** "My calls" 1–5 valem como estão; 6–10 o dono decide depois de usar.
- **Vidro do balão fixo:** o dono vê no app e diz.
- **Licença:** do dono, sem licença por ora.
- **Repositório:** `Guilbritto/wai` fica público. Tornar privado quebraria a atualização automática, que baixa as releases sem login, e custaria minutos de CI.
- **Assinatura Apple:** sem assinatura por ora.
- **Bundle id:** `app.wai.ide`.
- **`wai-waycarbon`:** pode ficar público.
- **Login do Claude:** como o Zed e o VS Code.
  - O wai nunca vê o token; o usuário entra no Claude Code sem modificação, ou usa chave de API, Bedrock ou Vertex pelo `~/.claude/settings.json`.
  - Para uso interno, dispensa a confirmação com a Anthropic.
  - Entra na trilha de ajustes:
    - um botão **Sign in** em Settings > AI, que abre uma aba de terminal com o comando de login do agente;
    - no guia do usuário, chave de API, Bedrock e a primeira abertura do app sem assinatura.
- **Regra para escolhas de configuração, atalhos, tema e login:** ver como as outras IDEs fazem (Zed primeiro, depois VS Code) e fazer igual.

## Andamento (2026-11-12, fim do dia)

- **Fase 1:**
  - chat: mergeada em `099205b`.
  - extensões e settings: mergeada em `852c8e7`.
  - sessões: mergeada em `0fc0a5a`; a 0.8 passou 20/20 depois da correção da ordem do cancel no `wai-agent`.
  - Suíte na `development` `0fc0a5a`: 697 passaram, 0 falharam, 11 ignorados.
- **Trilha workspace:** estava rodando quando a sessão fechou. Worktree `.claude/worktrees/onda2-workspace`, branch `feat/onda2-workspace` a partir de `0fc0a5a`, ledger em `<worktree>/.superpowers/sdd/2026-11-12-wai-onda-2-workspace/progress.md`.
- **Registro do controlador:** `.superpowers/sdd/master-onda-2/` (fora do git). Lá estão `progress.md` com as decisões, `track-*-report.md` com o que conferir no app, os logs da suíte e `controller-brief.md`, o modelo de instrução para cada coordenador de trilha.
- **Documentos novos:** a spec `2026-11-12-wai-work-areas-design.md` e os planos `2026-11-12-wai-work-areas.md`, `2026-11-12-wai-sidebar-followups.md` e `2026-11-12-wai-fase-2-core.md`.

## Próxima sessão: passo a passo (só execução)

Cada plano roda com um coordenador por worktree, a partir do modelo em `.superpowers/sdd/master-onda-2/controller-brief.md`. Depois de cada trilha, quem coordena faz o merge na `development`, roda a suíte inteira, `cargo clean` e remove o worktree. Antes de cada merge, rodar `git log --oneline -5 development`, porque outras sessões também commitam.

1. **Terminar a trilha workspace.** Ver em `git -C .claude/worktrees/onda2-workspace log --oneline 0fc0a5a..` o que já foi feito. Abrir um coordenador com o plano `2026-11-12-wai-onda-2-workspace.md` no mesmo worktree: ele retoma pelo ledger. Merge e suíte.
2. **Conferir no app e versionar.**
   - A 0.6 e a 0.7 desta lista.
   - Os itens "owner check" de `track-chat-report.md`, `track-ext-settings-report.md`, `track-sessions-report.md` e `track-workspace-report.md`.
   - Versionar pelo processo que estiver no `CLAUDE.md`. A frente de release mudou o fluxo: ler o `CLAUDE.md` antes.
3. **Fase 2 núcleo:** plano `2026-11-12-wai-fase-2-core.md` (2.3, 2.2, 3.13). Já está liberado, porque a atualização automática foi mergeada.
4. **Quando `feat/sidebar-redesign` estiver na `development`:**
   - primeiro o plano `2026-11-12-wai-sidebar-followups.md` (1.11, 2.1b, 3.1, 3.2);
   - depois `2026-11-12-wai-work-areas.md` (1.12).

   Um depois do outro, porque os dois mexem no painel Sessions. O 1º passo de cada um é reconferir as funções que o plano cita.
5. **Só o dono:**
   - confirmar a lista "My calls" da spec `2026-11-12-wai-work-areas-design.md` antes do passo 4;
   - a 2.7: instalar e fazer login no Codex, no Antigravity e no Copilot;
   - os itens da Fase 0.
6. **Depois:** Fase 3, que precisa de spec com o dono (3.8, 3.9, extensões). Por último, a Fase 4 (Windows).
