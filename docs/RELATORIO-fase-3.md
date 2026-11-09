# wai — Relatório da Fase 3 (extensões + Windows)

> 2026-11-09 · branch `feat/fase-3-extensions` → `main` (tag `fase-3`) · 250 testes automáticos passando (+ 2 opt-in com o Claude real, que passaram)

## O que entrou

| Área | O que dá para fazer |
|---|---|
| Segurança do editor (3.0) | Fechados os últimos caminhos em que o editor podia gravar por cima de algo que você não viu: recarga em andamento, "Dismiss" numa aba limpa e arquivo trocado por um symlink |
| Manifesto (3.1) | `wai-extension.toml` com validação rígida: id, caminhos que não saem da pasta da extensão, campos desconhecidos recusados. Contribuir agentes ou servidores MCP exige a permissão `process` |
| Instalar (3.2) | De uma pasta ou de um `git` (https/ssh). Ver detalhes abaixo |
| Host (3.3) | Extensões rodam como processos (em qualquer linguagem) que falam JSON-RPC por stdio. Ver detalhes abaixo |
| Painéis (3.4) | A extensão descreve a tela em JSON (lista, botões, campos, markdown, progresso) e o wai desenha de forma nativa, como aba na barra lateral. Árvores malformadas ou gigantes não derrubam nada |
| Paleta e contribuições (3.5) | **⌘⇧P**: ações, modelos, comandos de extensões e "New session with &lt;agente&gt;". Ver detalhes abaixo |
| **wai-waycarbon (3.6)** | Extensão em Go. Ver detalhes abaixo |
| Windows (3.7) | Ver detalhes abaixo |

**Instalar (3.2):**
- O diálogo de consentimento lista **cada permissão** antes de qualquer texto da extensão, mostra a origem e avisa que a v1 não restringe arquivos e rede.
- Nada é instalado sem você aprovar.
- O `.wai/` de um repositório só pode **sugerir** uma extensão.
- Symlinks e arquivos especiais são recusados, com limites de tamanho.
- O `git` roda com argumentos fixos e protocolos restritos.

**Host (3.3):**
- Cada chamada é conferida contra o que você aprovou.
- Há limites para mensagens de 4 MB, inundação de renders, extensão que não inicializa e quedas em série (3 reinícios).
- Os processos morrem junto com o wai: grupo de processos no macOS, Job Object no Windows.

**Paleta e contribuições (3.5):**
- Agentes e servidores MCP vindos de extensões habilitadas.
- Plugins do Claude Code sugeridos são **digitados no terminal sem Enter**, e você confirma.
- Extensões podem listar e abrir sessões e mandar prompts, com permissão e no máximo 1 abertura a cada 2 s, e nunca roubam o foco do teclado.
- Notificações têm limite de 1 a cada 2 s.

**wai-waycarbon (3.6):**
- Modelos para front, BFF e serviços Go com as convenções do time: commits sem escopo, `Refs: EC-XXXX`, branch a partir de `development`.
- Sugestão do plugin `beehive-waycarbon`.
- Painel **"My cards"** com os seus cards do Jira, via o `jira` CLI que você já usa. Os argumentos são fixos e a extensão nunca lê token.
- Clicar num card abre uma sessão com "Work on EC-XXXX: …" já escrito, sem enviar.

**Windows (3.7):**
- Caminhos sem o prefixo `\\?\`.
- Comandos resolvidos só pelo PATH, nunca pela pasta do projeto. No Windows isso é um risco real de segurança.
- CRLF preservado.
- Atalhos que não roubam ctrl-C/V/W do terminal.
- CI para macOS e Windows pronta.

Capturas em `docs/screens/fase-3-*.png`: consent, panel, palette, waycarbon.

## Números (build release, este Mac)

| Métrica | Meta | Fase 3 (extensão ligada) | Fase 2 | Central |
|---|---|---|---|---|
| CPU parado | < 0,5% | **0,16%** | 0,15% | ~8–9% |
| Memória (projeto + 3 sessões + terminal) | < 300 MB | **72 MB** | 72 MB | servidor + aba do Chrome |
| Processo da extensão WayCarbon | — | ~6 MB | — | — |
| Do launch ao painel "My cards" | — | 0,22–0,29 s | — | — |

**Teste com o Claude real:** passou 2 de 2 em 13,8 s. Cobriu:
- criar sessão;
- editar arquivo com diff;
- retomar;
- fork;
- listar as suas 93 sessões;
- pergunta como formulário, respondida com "Red".

Detalhes: `docs/spikes/fase-3-medicoes.md`.

## O que as revisões pegaram (e foi corrigido)

- **Diálogo de consentimento:** uma descrição enorme no manifesto podia empurrar a lista de permissões para fora da tela, deixando o botão Install visível. Agora as permissões vêm primeiro e a descrição é cortada.
- **Agentes e MCP sem permissão:** uma extensão podia contribuir agentes ou servidores MCP sem declarar `process`. Agora é exigido, e o consentimento mostra isso.
- **CI do Windows:** a CI cairia no primeiro push, porque os testes do app não compilavam no Windows. Também falhariam os testes em Go e a checagem de licenças.
- **Foco do teclado:** uma extensão que abria sessão roubava o foco; você podia estar digitando no terminal e mandar texto para o Claude.
- **Colar em arquivo CRLF:** gravava `\r\r\n`.
- **Reinícios:** extensões que já tinham caído voltavam a reiniciar a cada troca de projeto.

## Decisões que eu tomei sem você (confira)

| # | Decisão | Custo se estiver errada |
|---|---|---|
| R44–R46 | Divisão do trabalho entre tarefas; a API de sessão ganhou servidores MCP | Nenhum |
| R45 | WayCarbon chama `jira me` e depois `jira issue list ...`, sem shell | Um processo a mais por atualização |
| R47 | O aviso de "mudou no disco" depende de a aba estar editada ou não | Nenhum |
| R48–R49 | Agentes e MCP exigem `process`; modelos com campo errado são recusados; NUL é recusado | Extensões só de agente precisam declarar `process` |
| R50 | O consentimento mostra a origem; `git` endurecido (sem LFS, protocolos https/ssh); desinstalar não trava a interface | Nenhum |
| R51–R52 | O orçamento de reinícios zera quando você reabilita a extensão; no Windows, processos netos via Job Object | Mais 3 tentativas a cada reabilitação |
| R53 | O painel ganhou "lista que ocupa o painel" e valores iniciais nos campos (para o "My cards") | Esquema um pouco maior |
| R54 | WayCarbon: "Loading…" imediato, `jira` em segundo plano, chaves de card validadas | Nenhum |
| R55 | Licenças: `[licenses.private] ignore = true`, porque você ainda não escolheu a licença | Nenhum |
| R56 | Leva final: abrir sessão por extensão limitado a 1 a cada 2 s; ⌘N sempre tem prioridade; `ssh -o BatchMode=yes` ao instalar; comandos sem caminho só pelo PATH absoluto também no macOS | A config de ssh customizada não vale ao instalar extensões |
| R57 | Sobras viraram a Tarefa 4.0: extensões tagarelas podem abrir até 4 painéis; largura mínima de lista; nota sobre o PATH do MCP | Painéis extras ocasionais até a 4.0 |

## Lacunas conhecidas

- **Windows só preparado no código.** O app completo não compila no Mac para Windows, porque precisa do SDK do Windows. Só a CI no `windows-latest` confirma, e ela depende do repositório remoto.
- **A v1 não isola extensões.** Elas rodam com os seus poderes; o diálogo diz isso. Isolamento real (sandbox) fica para depois de existir um catálogo público.
- **O "My cards" só foi testado com um `jira` falso.** Com o seu `jira` real deve funcionar, porque são os mesmos comandos que você já usa, mas falta conferir na prática.
- **Sair pelo Dock ou fazer logout com edição não salva** ainda perde a edição. A **Tarefa 4.0** resolve guardando os buffers ao sair.

## Próximo passo

Fase 4, publicação: `docs/superpowers/plans/2026-11-08-wai-fase-4.md`.
- Recuperar edições não salvas após qualquer saída.
- `.app`/DMG e instalador do Windows.
- Workflow de release com assinatura opcional.
- Checagem de atualização desligada por padrão.
- Extensões `wai-navegador`, `wai-rotinas` e o desenho do `wai-remoto`.
- Documentação.
- Relatório final com a sua lista de pendências.
