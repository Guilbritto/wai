# wai — notes for agents

## Release and install on this Mac

Done this way for 0.6.0 (2026-11-11). Another session may also commit here: run `git log --oneline -5 development` first and release only what is merged into `development`.

1. **Bump the version** (`X.Y.Z` = new, `A.B.C` = current):
   - `Cargo.toml`: `version = "X.Y.Z"` (workspace version).
   - `README.md`: `**Status:** X.Y.Z.`
   - `docs/extensions.md`: "implemented by wai X.Y.Z" and `"wai_version": "X.Y.Z"`.
   - `docs/user-guide.md`: "This guide describes wai X.Y.Z" and the top-bar example `` (`X.Y.Z · …`) ``.
   - `cargo update --workspace` (updates the workspace crates in `Cargo.lock`), then `grep -rn "A\.B\.C" Cargo.toml README.md docs/extensions.md docs/user-guide.md` must print nothing.
2. **Commit and tag, local only:** `git commit -m "chore: release X.Y.Z"` on `development` (no scope, no Co-Authored-By), then `git branch -f main development` (only when `main` is an ancestor of `development`, i.e. a fast-forward) and a lightweight tag `git tag vX.Y.Z`. No push: there is no remote.
3. **Build** (several minutes; run it in the background, never blocking the session):
   `export PATH="$HOME/.cargo/bin:$PATH"; CI=true cargo packager --release`
   It writes `target/release/wai.app` and `target/release/wai_X.Y.Z_aarch64.dmg`. Check `plutil -extract CFBundleShortVersionString raw target/release/wai.app/Contents/Info.plist`.
4. **Copy to the dist folder:** `rsync -a --delete target/release/wai.app/ ../wai-dist/wai.app/` and `cp target/release/wai_X.Y.Z_aarch64.dmg ../wai-dist/`.
5. **Install:** `rsync -a --delete ../wai-dist/wai.app/ /Applications/wai.app/`
   - wai is usually running, often hosting the very session that installs it. rsync writes each file to a temp name and renames it, so the running process keeps its old binary and the new one applies on the next launch. Check that `stat -f %i /Applications/wai.app/Contents/MacOS/wai` changed and the running `wai` is still alive (`ps aux | grep "[M]acOS/wai"`).
   - Never `rm -rf /Applications/wai.app` (on 2026-11-11 macOS emptied the bundle and refused to remove the folder). Never `cp`/`ditto` over the running binary in place.
   - Never quit or kill the running wai: the owner restarts it.
   - `codesign -v` says "code has no resources but signature indicates they must be present": expected for this unsigned build.
6. `cargo clean` afterwards (the disk is nearly full).
