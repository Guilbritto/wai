# S3 — Linha de base da Central atual

> 2026-11-08 · Central IA WayCarbon rodando neste Mac (servidor PID 16795, aba aberta no Chrome) · Claude Code 2.1.319

## Números

| Medida | Valor | Como |
|---|---|---|
| CPU do servidor, sem uso ativo | **5,0% de um núcleo** em média (65,8 s de CPU em 21 min 45 s) | `ps -o time,etime` |
| CPU do servidor, amostras | 0–6,4% | `top -l 4 -s 5` |
| Memória do servidor | 33–95 MB (RSS varia) | `ps`, `top` |
| `claude agents --json --all` | 0,43 s de relógio, **0,17 s de CPU**, a cada 5 s ≈ **3,4% de um núcleo** | `/usr/bin/time` |
| **Total contínuo** | **~8–9% de um núcleo** com a Central aberta e parada | soma |
| Uma chamada ao tmux | ~9 ms | 50 chamadas em 0,47 s |
| Releitura de 4 MB do jsonl + parse | 6 ms | jsonl de 72 MB |
| Atraso do texto do Claude na tela | até ~2 s (polling de `/api/conversa`) | pelo código (`app.js:5073`) |
| Aba do Chrome (renderer) | **não medida** | a conexão aparece no processo de rede do Chrome, e não dá para ligar ao renderer certo pelo `ps` |

## Números que o wai precisa bater

| Medida | Central | Meta do wai |
|---|---|---|
| CPU parado | ~8–9% de um núcleo | < 0,5% |
| Atraso do texto | até 2 s | < 1 quadro depois do token chegar |
| Processos por ação | 3–8 | 0 |
| Listar conversas | varre 1,2 GB de `~/.claude/projects` (cache de 1 s) | `session/list`: 74 ms (S1) |
