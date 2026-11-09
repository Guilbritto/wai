# wai-rotinas

Runs your own scripts on a schedule and tells you when their output changes. A **Routines** panel lists each routine with its last run, its status and a **Run now** button.

## Build and install

```sh
cd extensions/wai-rotinas
go build -o bin/wai-rotinas .
```

Then in wai: Extensions, install from folder, pick this folder, and review the consent (`process`, `notify`, `panels`).

## rotinas.toml

wai-rotinas runs only what you list in `rotinas.toml`, in wai's config dir (`~/.config/wai/rotinas.toml` on macOS and Linux). Edit it, then click **Reload** in the panel.

```toml
[[routine]]
name = "Inbox"                         # unique, shown in the panel and the notification
script = "/Users/me/bin/check-inbox"   # absolute path to an executable
every = "15m"                          # a Go duration of at least 1m: "30s" is refused, "2h30m" works

[[routine]]
name = "Daily report"
script = "/Users/me/bin/daily-report"
at = "09:30"                           # every day at this local time (HH:MM)
```

Each routine has a `name`, a `script`, and exactly one of `every` or `at`. Values are quoted strings (`"…"` or `'…'`); `#` starts a comment.

## Safety rules

wai-rotinas refuses to run, and says why in the panel:

- a `script` that is not an absolute path;
- a script that group or others can write, or that another user owns (only you or root);
- a script in a folder that group or others can write (sticky folders like `/tmp` are fine);
- a symlink whose target breaks any of these: the target is what gets checked and run.

`rotinas.toml` itself must follow the same rules, or nothing runs. Fix a refusal with `chmod go-w <path>`.

Scripts run directly with no arguments and no shell, with empty stdin, from wai's config dir. Wrap a command in a script if it needs arguments.

On Windows wai-rotinas cannot check who may change a file, so it refuses every script.

## How runs work

- **At most one run at a time.** A routine that comes due, or that you Run now, while another runs waits its turn (`Queued`).
- **Schedule.** `every` counts from the start of the last run; `at` is the next such time of day. A run missed while wai was closed happens once, as soon as wai starts. A routine that has never run counts from when it was loaded. Between runs the extension sleeps until the next one is due. While the computer sleeps that timer pauses, so a run due then starts once you use wai again (a click or a saved file).
- **Limits.** A run is stopped after 60 minutes (`Timed out`). Output (stdout and stderr together) is kept up to 1 MB; the rest is read and dropped.
- **Notifications.** After a run, wai-rotinas notifies only when the result changed since the last run: a different output, or a different outcome (OK, failed, timed out, refused). The first run of a routine notifies. The notification shows the first line of output, or what went wrong.
- **Memory.** The last run of each routine (time, status and a hash of its result) is kept in `rotinas-state.json` next to `rotinas.toml`, so a restart does not notify again for the same output.
- **Stopping.** Scripts run in the extension's process group (a job on Windows). When wai stops the extension, they stop with it.

## Tests

```sh
go vet ./... && go test ./...
```

They use temporary folders and `/bin/sh` scripts, never your config or real routines.
