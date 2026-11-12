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
- a script with any folder above it, up to `/`, that group or others can write or another user owns (sticky folders like `/tmp` are fine). This is checked both for the path as written and for where symlinks lead, so a script in a group-writable folder such as `/Applications` (root:admin, mode 775) is refused;
- a symlink whose target breaks any of these: the target is what gets checked and run.

`rotinas.toml` itself must follow the same rules and be at most 1 MB, or nothing runs. Fix a refusal with `chmod go-w <path>`, or move the script to a folder only you can change, such as `~/bin`.

Scripts run directly with no arguments and no shell, with empty stdin, from wai's config dir. Wrap a command in a script if it needs arguments.

On Windows wai-rotinas cannot check who may change a file, so it refuses every script.

## How runs work

- **At most one run at a time.** A routine that comes due, or that you Run now, while another runs waits its turn (`Queued`).
- **Schedule.** `every` counts from the start of the last run; `at` is the next such time of day. A run missed while wai was closed happens once, as soon as wai starts. A routine that has never run runs as soon as it is loaded (at start, or on Reload after you add it), so you see at once whether it works or is refused. Between runs the extension sleeps until the next run is due, but looks at the clock at least once a minute: a routine whose time passes while the computer sleeps runs once, within a minute of waking, however many slots it missed; the next run is counted from that run.
- **Limits.** A run is stopped after 60 minutes (`Timed out`); the script and everything it started are killed. Output (stdout and stderr together) is kept up to 1 MB; the rest is read and dropped, and the notification says `(output cut at 1 MB)`.
- **Notifications.** After a run, wai-rotinas notifies only when the result changed since the last run: a different output, or a different outcome (OK, failed, timed out, refused). The first run of a routine notifies. The notification shows the first line of output, or what went wrong, cut at 300 characters. wai drops an extension's notification that comes within 2 s of its last, so wai-rotinas leaves 2.5 s between its notifications (the next run waits too). The panel shows the result first.
- **Memory.** The last run of each routine (time, status and a hash of its result) is kept in `rotinas-state.json` next to `rotinas.toml`, so a restart does not notify again for the same output. It is saved after the notification: quitting in between repeats that notification once after a restart, but never loses it.
- **Stopping.** Scripts run in the extension's process group (a job on Windows). When wai stops the extension, they stop with it; when only its input closes, it kills the running script before it exits.

## Tests

```sh
go vet ./... && go test ./...
```

They use temporary folders and `/bin/sh` scripts, never your config or real routines.
