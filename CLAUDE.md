# wai — notes for agents

## Release

CI publishes releases; nobody bumps a version by hand. Every push to `main` runs `.github/workflows/release.yml`, which computes the version and tag (`v<version>`) from the conventional commits since the last tag and builds the assets. The repo's `Cargo.toml` keeps a placeholder version on purpose.

To release: another session may also commit here, so run `git log --oneline -5 development` first and release only what is merged into `development`. Merge into `development`, fast-forward `main` (`git branch -f main development`, only when `main` is an ancestor of `development`), and push both. Commits are `feat:`/`fix:` (they decide the version), no scope, no Co-Authored-By.

## Install a local build on this Mac

For testing a build, not for releasing. The installed wai updates itself from releases; a local build goes to `~/Applications/wai.app`.

1. **Build** (several minutes; run it in the background, never blocking the session):
   `export PATH="$HOME/.cargo/bin:$PATH"; CI=true cargo packager --release`
   It writes `target/release/wai.app`. Check `plutil -extract CFBundleShortVersionString raw target/release/wai.app/Contents/Info.plist`.
2. **Install:** `mkdir -p ~/Applications && rsync -a --delete target/release/wai.app/ ~/Applications/wai.app/`
   - wai is usually running, often hosting the very session that installs it. rsync writes each file to a temp name and renames it, so the running process keeps its old binary and the new one applies on the next launch. Check that `stat -f %i ~/Applications/wai.app/Contents/MacOS/wai` changed and the running `wai` is still alive (`ps aux | grep "[M]acOS/wai"`).
   - Never `rm -rf` the bundle (on 2026-11-11 macOS emptied it and refused to remove the folder). Never `cp`/`ditto` over the running binary in place.
   - Never quit or kill the running wai: the owner restarts it.
   - `codesign -v` says "code has no resources but signature indicates they must be present": expected for this unsigned build.
3. `cargo clean` afterwards (the disk is nearly full).
