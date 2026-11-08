# wai — Relatório da Fase 1 (núcleo no macOS)

> 2026-11-08 · branch `feat/fase-1-core` → `main` (tag `fase-1`) · 25 commits · 71 testes automáticos passando

## O que funciona

O wai abre uma pasta de projeto numa janela nativa (Rust + GPUI) e conversa com o Claude pelo protocolo ACP,
usando o login do Claude Code que já existe na máquina. O wai nunca lê, guarda nem pede credencial.

| Área | O que dá para fazer |
|---|---|
| Sessões | Criar (⌘N), retomar do histórico, fechar (⌘W), até 4 painéis lado a lado (⌘⌥→ troca o foco) |
| Chat | Texto em tempo real em markdown, bloco de pensamento recolhido, ferramentas com saída (2.000 linhas + "Show all"), diffs coloridos |
| Permissões | Cartão com o diff e os botões Sim / Sim sempre / Não; Esc interrompe o turno |
| Anexos | Arrastar imagem (≤ 5 MB vai como imagem) ou qualquer arquivo (vai como referência) |
| Cabeçalho | Título, seletor de modo (default, acceptEdits, plan, auto, bypassPermissions) e contexto usado ("43k / 1M") |
| Barra lateral | Sessões abertas com status (azul = trabalhando, âmbar "Needs you", cinza = parada, vermelho = erro) e histórico do agente filtrado pelo projeto, com a chave "All projects" |
| Terminal | Shell de verdade no painel de baixo (ctrl-`), com cores e ⌘V para colar |
| Avisos | Notificação do macOS quando uma sessão precisa de você ou termina com a janela em segundo plano |
| Retomar | Ao reabrir, volta o projeto, os painéis, as sessões (com o histórico) e o terminal |
| Falhas | Agente que cai: aviso com o fim do stderr e botão Restart. Sem login: "claude is not signed in. Open a terminal and run its login (for Claude: claude /login)." |

Capturas: `docs/screens/fase-1-shell.png`, `fase-1-chat.png`, `fase-1-sidebar.png`, `fase-1-terminal.png`,
`fase-1-restore.png`.

**Como rodar:**

```sh
cd Guilherme/wai
~/.cargo/bin/cargo run -p wai --release -- /caminho/do/projeto
```

Precisa do Node (o adaptador roda com `npx`) e do Claude Code logado. A primeira execução baixa o adaptador e
leva cerca de 10 s.

## Números (build release, este Mac)

| Métrica | Meta | wai | Central atual |
|---|---|---|---|
| CPU parado | < 0,5% | **0,15%** | ~8–9% de um núcleo |
| Memória (1 projeto, 3 sessões, terminal) | < 300 MB | **127–133 MB** | servidor 33–95 MB + aba do Chrome |
| Janela visível | < 1 s | **0,23–0,27 s** (0,69 s a frio) | aba do navegador |
| CPU com texto chegando a 40 tokens/s | < 10% | **6,4%** | texto com até 2 s de atraso |
| Atraso do texto | < 1 quadro | atualização a cada 100 ms durante o stream | até 2 s |
| Listar conversas | — | 94 sessões pelo protocolo | varre 1,2 GB de jsonl |

O teste com o Claude real (`cargo test -p wai-agent --test real_claude -- --ignored`) passou em 13,4 s:
- criou a sessão e recebeu o texto e a edição com diff;
- gravou o arquivo;
- retomou a sessão em outro processo.

Detalhes e método: `docs/spikes/fase-1-medicoes.md`.

## Como foi feito

Cada tarefa passou por:
1. um subagente implementador com TDD;
2. um revisor independente (conformidade com o plano e qualidade);
3. rodadas de correção até a aprovação.

No fim, a branch inteira teve uma revisão geral e uma leva única de correções.

A revisão geral achou um problema **crítico de segurança**, que foi corrigido antes do merge. O
`.wai/settings.toml` de um repositório clonado conseguia trocar o comando do agente, e o wai executaria esse
comando ao abrir a pasta. Agora o arquivo do projeto não pode definir comandos, shell nem ambiente.

## Decisões que eu tomei sem você (confira)

| # | Decisão | Custo se estiver errada |
|---|---|---|
| R1–R4 | Ajustes de ordem e detalhes do plano (stub do chat na 1.4, atalho ⌘⌥→, comparar caminhos canônicos, espaços no agente falso) | Nenhum relevante |
| R5 | O cartão de permissão mostra o diff do pedido ou, se faltar, o da ferramenta | Uma busca a mais |
| R6 | "Não logado" também é detectado no envio do prompt (é assim que o adaptador do Claude avisa) | Nenhum |
| R7 | A resolução de permissão vai para todas as sessões, e cada uma ignora o que não é dela | Custo O(sessões) por evento |
| R8 | As capturas de tela saem do app real atrás de uma feature `screenshot` (o harness de testes não renderiza) | Nenhum |
| R9 | O status volta a "Ready" quando uma sessão funciona depois do login | Nenhum |
| R10 | Aceitei pequenas mudanças de interface na 1.6 (registro na sessão, nome do agente no evento de permissão) | Nenhum (API interna) |
| R11 | O agente padrão inicia junto com o app, para o histórico aparecer sem esperar o ⌘N | ~100 MB do processo Node parado |
| R12 | Teste extra para o ⌘W, porque fechar cancela o turno em andamento | Um teste |
| R13 | O aviso "terminou" só aparece em fim de turno normal (não em cancelamento nem ao retomar) | Nenhum aviso em turnos cancelados (de propósito) |
| R14 | Sessões restauradas pegam o título do histórico do agente, sem mudar o formato do arquivo de estado | Nenhum |
| R15 | **Segurança:** `.wai/` do projeto não define agentes nem shell (contraria um teste do plano original) | Times não fixam o comando do agente por repositório (pode voltar com uma confirmação explícita) |
| R16 | Na leva final entraram também: colar no terminal, texto de login centralizado, configuração de projeto quebrada não impede abrir, dependências limpas | Diff um pouco maior |
| R17 | Ao sair, o grupo de processos do agente é encerrado na hora (SIGKILL), porque o GPUI dá ~200 ms ao app | A última linha do histórico pode ficar incompleta se você sair no meio de uma escrita |
| R18 | O agente herda a pasta de onde o wai foi aberto. Corrigir isso virou a **Tarefa 2.0** | Um repositório malicioso poderia trocar o adaptador **só** se você abrir o wai de dentro dele, até a 2.0 |

## Lacunas conhecidas (vão para a Fase 2 ou depois)

- **Perguntas do agente** (AskUserQuestion) e **fork de sessão** ainda não existem. Estão no plano da Fase 2
  (2.1 e 2.2).
- **Terminal:** sem rolagem para trás, sem seleção com o mouse e sempre escuro (2.9).
- **"Show all" sem limite:** uma saída gigante pode travar a tela (2.9).
- **⌘W em "Preparing…"** não cancela a sessão que está nascendo (2.9).
- **Notificações no macOS** aparecem com o nome e o ícone do Terminal até o empacotamento do app (Fase 4).
- **Windows:** compila nos crates sem GPUI. O app completo só vai ser verificado quando houver CI, o que depende de
  um repositório remoto (seu).
- **Pedido de permissão com o Claude real** não aparece nesta máquina, porque as suas configurações usam
  `bypassPermissions` com tudo liberado. O fluxo foi testado com o agente falso e no S1.

## Próximo passo

Fase 2 (IDE): `docs/superpowers/plans/2026-11-08-wai-fase-2.md`.
- Tarefa 2.0 (segurança) e perguntas do agente.
- Fork de sessão.
- Árvore de arquivos e editor com abas.
- Revisão e reversão de edições.
- Busca.
- Visão geral dos agentes.
- Modelos de conversa.
