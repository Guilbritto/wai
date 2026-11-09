# wai — Relatório da Fase 2 (IDE)

> 2026-11-08 · branch `feat/fase-2-ide` → `main` (tag `fase-2`) · 159 testes automáticos passando (+ 2 testes opt-in com o Claude real)

## O que entrou

| Área | O que dá para fazer |
|---|---|
| Segurança (2.0) | O agente sempre roda a partir da pasta de configuração do wai, nunca de dentro de um repositório: um `.npmrc` malicioso não escolhe o código |
| Perguntas do agente (2.1) | Quando o Claude usa AskUserQuestion, aparece um formulário no chat (opções com descrição, múltipla escolha, texto). Formulários que o wai ainda não sabe mostrar são recusados com aviso, como manda o protocolo |
| Fork (2.2) | "Fork" numa sessão aberta ou do histórico abre a cópia ao lado, com o histórico |
| Arquivos (2.3) | Aba Files: árvore que respeita o `.gitignore` (inclusive o global), abre pasta por pasta e se atualiza sozinha |
| Editor (2.4) | Abas, cores para 11 linguagens, ⌘S atômico (mantém permissões e symlinks). Arquivo > 20 MB abre só leitura; > 200 MB e binários são recusados. Mudança externa: recarrega se a aba está limpa; se está editada, mostra "Reload / Keep mine". Sair ou trocar de projeto com edição não salva pergunta antes |
| Revisão de edições (2.5) | No fim de cada turno, um bloco "Edits (N files)" com **Open** (na linha certa) e **Revert** |
| Busca (2.6) | Aba Search, com regex e maiúsculas/minúsculas e resultados chegando aos poucos; clicar abre na linha |
| Agentes (2.7) | Aba Agents: um cartão por sessão com status, plano, ferramenta atual e contexto usado. É o "Escritório" da Central, em 2D |
| Modelos (2.8) | `templates.toml` do usuário e `.wai/templates.toml` do projeto. "New from template" abre a sessão com o prompt já escrito, sem enviar |
| Acabamento (2.9) | Terminal com 10 mil linhas de rolagem, seleção com o mouse e ⌘C. "Show all" limitado, com "Open full output in editor". ⌘W em "Preparing…" cancela. Mensagens claras para regex inválida e arquivo que não é UTF-8 |

**Sobre o Revert:**
- Ele funciona com o formato real das edições do Claude, que chegam como trechos. Ele desfaz o trecho só quando esse trecho aparece **exatamente uma vez** no arquivo, alinhado às linhas.
- Ele **nunca apaga um arquivo que já existia**.
- Ele mostra antes o que vai acontecer e recusa se o arquivo mudou depois da edição.

Capturas em `docs/screens/fase-2-*.png`: question, files, editor, edits, agents, templates, terminal.

## Números (build release, este Mac)

| Métrica | Meta | Fase 2 | Fase 1 | Central |
|---|---|---|---|---|
| CPU parado (projeto + 3 sessões + terminal) | < 0,5% | **0,15%** | 0,15% | ~8–9% |
| CPU parado com 5 arquivos abertos | < 0,5% | **0,18%** | — | — |
| Memória (sem arquivos / com 5 arquivos) | < 300 MB | **72 / 119 MB** | 127–133 MB | servidor + aba do Chrome |
| CPU com streaming a 40 tokens/s | < 10% | **7,5%** | 6,4% | — |
| Abrir arquivo de 100 MB | sem travar além do custo conhecido | 436 ms, só leitura, depois ~640–880 MB | — | — |
| Busca `fn ` no front da WayCarbon | — | 1º resultado em 6 ms, total 55 ms (cache quente); 40 / 358 ms (frio) | — | — |
| Janela visível | < 1 s | 0,24–0,29 s | 0,23–0,27 s | — |

**Teste com o Claude real:** passou em 16,5 s.
- Os fluxos da Fase 1 continuam funcionando.
- O fork, recarregado, repete o histórico.
- A pergunta "Pick a color: red or blue?" chegou como formulário; o wai escolheu "Red" e o Claude respondeu "Red".

Detalhes e método: `docs/spikes/fase-2-medicoes.md`.

## O que as revisões pegaram

Cada tarefa passou por implementação com TDD, revisão independente e rodadas de correção. A revisão geral da fase achou dois problemas **críticos**, corrigidos antes do merge:

1. **⌘S podia sobrescrever mudanças feitas no disco.** Isso valia para edições do agente, para um Revert e, em arquivos fora da pasta vigiada, sem aviso nenhum. Agora o salvar recusa e pede "Reload" ou "Keep mine".
2. **Um repositório malicioso podia redirecionar o Salvar para outro arquivo**, como o `~/.zshrc`. Bastava deixar um symlink com o nome do arquivo temporário; o revisor confirmou com uma prova de conceito. Agora o arquivo temporário é sempre criado do zero, sem seguir symlinks.

Também foi corrigido: com vários agentes abertos (modelos e forks), responder o pedido de um agente fechava o cartão de outro.

## Decisões que eu tomei sem você (confira)

| # | Decisão | Custo se estiver errada |
|---|---|---|
| R20 | ⌘W é contextual: no editor fecha a aba, no chat fecha a sessão | Um atalho |
| R21 | "Open full output" grava um arquivo temporário em `~/.config/wai/scratch/` (limpo ao abrir o wai) e abre só leitura | Arquivos temporários |
| R22–R24 | Perguntas: mostrar a descrição das opções; recusar formulários que o wai não sabe exibir; manter "Needs you" enquanto houver cartão sem resposta | Formulários exóticos ficam sem resposta |
| R25 | Fork só aparece se o agente também sabe recarregar sessões | Nenhum |
| R26–R27 | Árvore respeita os ignores globais do git; o watcher limita lotes enormes (10 mil caminhos) | Uma releitura completa de vez em quando |
| R28–R30 | Sair ou trocar de projeto com edição não salva pergunta antes. Arquivos que passam de 20 MB mostram o aviso em vez de recarregar sozinhos | Um diálogo e um clique a mais |
| R31 | Recarga que falha marca a aba como desatualizada; permissões são definidas antes de gravar; caminhos são normalizados ao abrir | Nenhum |
| R32–R35 | Revert por trecho único, nunca apaga arquivo pré-existente, abre na linha das `locations` do ACP, fica desabilitado enquanto o agente trabalha | Arquivos com tabs ou CRLF não são revertidos (o Claude normaliza os trechos) |
| R36–R37 | Plano no cartão limitado a 8 linhas; "Done" só para turno terminado de verdade | Nenhum |
| R38 | Pequenos ajustes de segurança e de texto incorporados na 2.9 | Tarefa maior |
| R39–R41 | Correções da revisão final (salvar seguro, symlink, agentes separados, leitura do `.wai/` limitada a 1 MB, etc.) | Um clique a mais após "Keep mine" |
| R42 | Dois caminhos estreitos que ainda podem gravar por cima do que você não viu (uma corrida durante a recarga e o "Dismiss" numa aba limpa) viraram a **Tarefa 3.0** | Sobrescrita rara até a 3.0 |

## Lacunas conhecidas

- **Sair pelo Dock ou fazer logout com edições não salvas não pergunta nada.** O GPUI não tem o gancho do macOS para isso. ⌘Q e o botão de fechar perguntam. Correção futura: guardar os buffers ao sair e restaurá-los ao abrir, ou um patch no GPUI.
- **Revert recusa arquivos indentados com tab** (Go, Makefile) e arquivos com CRLF, porque o Claude normaliza os trechos.
- **Abas do editor não voltam** ao reabrir o wai. As sessões e o layout voltam.
- **A busca e a árvore não respeitam `.gitignore` em pastas que não são repositório git.** É a mesma regra do git e do ripgrep.
- **O terminal em less/vim** não rola com a roda do mouse.
- **Windows** continua só preparado no código. A verificação depende do repositório remoto e da CI.

## Próximo passo

Fase 3, extensões e Windows: `docs/superpowers/plans/2026-11-08-wai-fase-3.md`.
- 3.0 fecha os dois caminhos de sobrescrita que sobraram.
- Modelo de extensões, com manifesto, instalação com consentimento e permissões.
- Host de extensões por JSON-RPC.
- Painéis declarativos.
- Paleta de comandos (⌘⇧P).
- Extensão `wai-waycarbon` em Go: modelos por repositório, plugin `beehive-waycarbon` e painel "My cards" do Jira.
- Preparação para Windows.
