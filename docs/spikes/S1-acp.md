# S1 — ACP com o Claude (resultado)

> 2026-11-08 · Adaptador `@agentclientprotocol/claude-agent-acp@0.101.0` · Claude Code 2.1.319 · SDK Rust
> `agent-client-protocol 5.0.0` · cliente: `spikes/s1-acp`

## Veredito: **GO com ACP**

Tudo o que o wai precisa funcionou com o **login de assinatura já existente**, sem API key.
O plano B (`stream-json`) não é necessário.

## O que foi testado

| Item | Resultado |
|---|---|
| Login | `auth_methods=[]` e funcionou sem `ANTHROPIC_API_KEY`: o adaptador usa o login do Claude Code do usuário. O wai não tocou em credencial |
| Início | 1ª execução do `npx`: 11,3 s até o `initialize` (download). Depois, do cache: **0,9 s**. `new_session`: 6 s na 1ª vez, **0,8 s** depois |
| Stream | `AgentMessageChunk` chega em pedaços ("D", "ONE"). Também chegam `ToolCall`/`ToolCallUpdate`, `UsageUpdate`, `SessionInfoUpdate` (título automático), `AvailableCommandsUpdate` (comandos e skills do usuário, para o autocompletar do "/") e `ConfigOptionUpdate`/`CurrentModeUpdate` (modo) |
| Permissão | Funciona. Vem com opções tipadas: `AllowOnce "Yes"`, `AllowAlways "Yes, allow all edits in <pasta>/ during this session"`, `RejectOnce "No"` |
| Diff | O `ToolCallUpdate` traz `ToolCallContent::Diff { path, old_text, new_text }` **antes** do pedido de permissão. Dá para mostrar o diff dentro do cartão de permissão |
| Modos | `default`, `acceptEdits`, `plan`, `auto`, `bypassPermissions`. A sessão nasce no modo padrão do usuário (aqui, `bypassPermissions`); `session/set_mode` troca o modo |
| Retomar | `load_session` funciona **em outro processo**, repete o histórico (mensagens do usuário, ferramentas, texto do agente) e a conversa continua com contexto. O wai não precisa ler os `.jsonl` do Claude |
| Listar | `session/list` devolveu **94 sessões em 74 ms**, com título, pasta e data. A Central varre 1,2 GB para isso |
| Uso | `UsageUpdate` traz contexto usado/total (`used`/`size`), custo equivalente em USD e o metadado `_claude/rateLimit` (`status`, horário de renovação…): uma fonte **oficial** para o painel de uso |
| `fs` do cliente | O adaptador **não** passa a escrita de arquivos pelo `fs/write_text_file` do cliente: o Claude Code grava direto no disco |

## O que muda no plano

1. **Sai o crate `wai-history`.** O histórico vem de `session/list` e a conversa, de `load_session`. A lista é
   atualizada por evento (sessão criada, turno terminado), sem polling.
2. **Uma conexão por agente, várias sessões.** O ACP permite várias sessões numa conexão, então o wai abre uma
   conexão com o adaptador e cria as sessões nela. Isso evita pagar 0,9 s + Node por sessão.
3. **Revisão de edições (Fase 2), sem interceptar a escrita:**
   - no modo `default`, cada edição chega como **cartão de permissão com o diff**: Aceitar / Rejeitar;
   - em qualquer modo, cada edição concluída aparece no histórico do turno com o diff e um botão
     **Reverter**, que regrava `old_text`;
   - rejeitar por trecho (hunk) não é possível antes da gravação: é tudo ou nada por edição.
4. **O modo do usuário é respeitado.** O wai mostra o seletor de modo da sessão e não sobrescreve as
   configurações do usuário. Neste Mac, o `bypassPermissions` + `allow: Bash(*), Write(*)…` faz com que
   nunca apareça pedido de permissão.
5. **Painel de uso volta para o núcleo**, alimentado por `UsageUpdate` (contexto, custo e `_claude/rateLimit`
   quando vier).
6. **Adaptador:** usar `npx -y @agentclientprotocol/claude-agent-acp@<versão fixa>`. A 1ª execução baixa o pacote
   (~11 s); o wai mostra "Preparando o agente…".

## Riscos em aberto

- O adaptador muda rápido (0.101.0 hoje). Fixar a versão e atualizar de propósito.
- Metadados `_claude/*` não fazem parte do protocolo; o painel de uso deve tolerar a ausência deles.
- `session/cancel` não foi exercitado contra o Claude real; fica coberto pelo agente falso nos testes da Fase 1.
- Fork de sessão só existe com a feature `unstable_session_fork` do SDK; fica fora do MVP.
