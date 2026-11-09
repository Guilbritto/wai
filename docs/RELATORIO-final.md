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

## Números ao longo das fases

Build release, neste Mac.

| Métrica | Meta | Central | S2 (Fase 0) | Fase 1 | Fase 2 | Fase 3 |
|---|---|---|---|---|---|---|
| CPU parado | < 0,5% | ~8–9% | 0,0% | **0,15%** | **0,15%** (0,18% com 5 arquivos) | **0,16%** (extensão ligada) |
| Memória (1 projeto, 3 sessões, terminal) | < 300 MB | servidor 33–95 MB + aba do Chrome | 153 MB (spike, sem sessões) | 127–133 MB | **72 MB** (119 MB com 5 arquivos) | **72 MB** |
| Janela visível | < 1 s | aba do navegador | ~180 ms | 0,23–0,27 s (0,69 s a frio) | 0,24–0,29 s | painel "My cards" em 0,22–0,29 s |
| CPU com streaming a 40 tokens/s | < 10% | texto com até 2 s de atraso | 14–17% | 6,4% | 7,5% | — |
| Arquivo de 100 MB | sem travar | não existe | rolagem 0,6–2,6 ms/quadro | — | abre em 436 ms, só leitura | — |
| Listar conversas | — | varre 1,2 GB de jsonl | 74 ms (94 sessões) | pelo protocolo | — | 93 sessões |
| Busca (`fn ` no front da Way) | — | — | — | — | 1º resultado em 6 ms, total 55 ms | — |
| Processo de uma extensão | — | — | — | — | — | ~6 MB |
| Testes automáticos | — | — | — | 71 | 159 | 250 |

**Fase 4 até a revisão final:**
- 283 testes em Rust e 15 em Go (8 em `wai-rotinas`, 7 em `wai-waycarbon`);
- `wai.app` de 44 MB e DMG de ~14 MB, depois que a checagem de atualização trouxe a pilha HTTPS;
- janela visível ~0,54 s depois de abrir o `.app` (medição da 4.1).

Os números finais do `.app` ficam na seção abaixo.

## Fase 4: medições do .app e revisão final

(preenchido na Tarefa 4.6)

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
| R19 | Sem registro nos relatórios | — |
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
| R43 | Sem registro nos relatórios | — |
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
| R58 | Sem registro: o ledger da Fase 4 cita R1–R58, mas o relatório da Fase 3 termina na R57 | — |
| R59 | Timer que dorme até a próxima rotina é agendamento por evento, não polling | Nenhum |
| R60 | Hot-exit grava uma cópia ~1 s depois da última edição **e** ao sair, então crash e Dock também ficam cobertos | Pequenas escritas na pasta de config enquanto você edita |
| R61 | Cópia > 50 MB não é gravada (com aviso); aviso único quando a cópia falha; sessão de extensão nunca toma o painel focado | Nenhum |
| R62 | A 1ª sessão de uma extensão pode abrir um painel novo (se houver < 4); arquivo apagado exige "Keep mine" antes de salvar | Um painel a mais por extensão |
| R63 | O app empacotado chama `set_application` com o próprio id (sem isso as notificações sairiam como Finder) | Nenhum |
| R64 | Aceitos: o create-dmg baixado pelo cargo-packager sem checksum (commit fixo, HTTPS); `wai.exe` sem ícone embutido | Confiança no pin do cargo-packager; cosmético |
| R65 | Adiados: o DMG em si não é notarizado (o app dentro é); rodar de novo pode criar 2º rascunho; certificado Windows em hardware precisa de `sign-command` | Um passo de notarização a mais depois |
| R66 | `wai-rotinas` pode limitar cada espera a 1 min, porque o relógio do Go para durante o sono do Mac | ~60 despertares/h numa extensão opcional |
| R67 | `wai-rotinas` recusa todo script no Windows (não há checagem de ACL na stdlib) | Sem rotinas no Windows até existir essa checagem |
| R68 | No macOS, aberto fora do terminal (sem `TERM`), o wai pega só o `PATH` do shell de login (`-i -l`, 3 s, uma vez, antes de qualquer thread) | O `.zshrc` atrasa a abertura pelo Finder: ~1,6 s só na 1ª abertura desde a R69 |
| R69 | Cache do `PATH` do shell: o 1º launch espera o shell uma vez, os seguintes usam o cache e atualizam em segundo plano | Um `PATH` novo só vale no launch seguinte |

## Lacunas conhecidas (atuais)

**Publicação**
- **App sem assinatura e sem notarização** até você ter os certificados. O macOS bloqueia a 1ª abertura de uma cópia baixada; o README ensina o "Open Anyway".
- **O DMG em si não é notarizado**, só o app dentro dele (R65). Depois do 1º release assinado, rode as checagens de `docs/building.md`.
- **Windows nunca rodou de verdade.** O app completo, o instalador NSIS e a assinatura só serão verificados pela CI, que depende do remoto.
  - `wai.exe` não tem ícone embutido (R64).
  - No Windows, o que um agente `npx` inicia sobrevive a ele até o stdin fechar.
- **O DMG só foi gerado com `CI=true`.** O passo que arruma a janela do DMG pelo Finder nunca rodou.
- **A notificação a partir do `.app` não foi testada de ponta a ponta.** Precisa de um turno real terminando com a janela em segundo plano.

**Uso**
- **Aberto pelo Finder, Dock ou Launchpad, o wai usa o PATH do shell de login (R68), guardado em cache (R69).**
  - O macOS dá a esses apps o PATH `/usr/bin:/bin:/usr/sbin:/sbin`, sem `npx` nem `node`. Sem `TERM`, o wai lê o PATH de `~/.config/wai/shell-path` e, em segundo plano, pergunta de novo ao `$SHELL -i -l` para a próxima abertura. Pelo terminal, continua com o PATH do terminal.
  - **Um PATH novo (outra versão do Node no nvm, por exemplo) só vale no launch seguinte.** A 1ª abertura depois da mudança ainda usa o cache.
  - Só a 1ª abertura (sem cache) espera o shell, com limite de 3 s; passou disso, segue com o PATH do launchd. O `.zshrc` desta máquina leva ~1,5–1,8 s. Nas outras aberturas esse tempo corre em segundo plano.
  - Testei com HOMEs de rascunho e um `.zshrc` de 1,5 s. A 1ª abertura esperou 1543 ms; as seguintes leram o cache (a linha do log sai ~60 ms após o exec, só o início do processo). Um agente `command = "npx"` achou o `npx` do `.zprofile`, e a troca de v1 para v2 valeu na abertura seguinte.
- **As configurações só são lidas ao abrir o wai.**
  - O `.wai/settings.toml` vale só para a pasta com que o wai foi aberto.
  - Ao trocar de pasta (⌘O), modelos e sugestões de extensão são relidos, mas tema e `default_agent` do projeto novo não.
- **A v1 não isola extensões.** Elas rodam com os seus poderes e herdam o ambiente do wai; `network` e `fs:` são só declarativos.
- **Um wai por vez.** Dois processos dividem `state.json` e `recovery/`.
- **Hot-exit:**
  - sem fsync (uma queda de energia pode deixar uma cópia vazia, que é ignorada);
  - cópias que não voltam (tamanho, erro de leitura) não são apagadas sozinhas;
  - um arquivo aberto antes de a restauração terminar fica para a próxima abertura.
- **Checagem de atualização:**
  - não usa proxy (`HTTPS_PROXY`) nem segue redirecionamento (repositório renomeado dá `HTTP 301` no log);
  - sem `updates.repo` definido, não checa nada, nem ligada (ver a lista abaixo).
- **Editor e busca:**
  - as abas do editor não voltam ao reabrir (só as não salvas);
  - o Revert recusa arquivos com tab e CRLF;
  - árvore e busca só respeitam `.gitignore` dentro de repositório git;
  - o terminal não rola com a roda do mouse dentro de `less`/`vim`.
- **Permissão com o Claude real** nunca apareceu nesta máquina, porque as suas configurações usam `bypassPermissions`. O fluxo foi testado com o agente falso.

**Extensões próprias**
- **"My cards"** só foi testado com um `jira` falso.
- **`wai-rotinas`:**
  - no Windows recusa todo script (R67);
  - um horário que cai durante o sono do Mac roda atrasado até a correção da R66 entrar.
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

1. **Crie o repositório** no GitHub (por exemplo `<dono>/wai`), vazio: sem README, sem licença e sem `.gitignore`.
2. **Empurre todas as branches e as tags de fase:**

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
