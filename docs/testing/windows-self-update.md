# Windows test: install and self-update

For an agent running **on a real Windows machine**. It checks what cannot be tested from the Mac: the
Windows download, the installer, and wai updating itself. Design:
`docs/superpowers/specs/2026-11-12-wai-release-and-auto-update-design.md`.

## Before you start

- Two published releases on https://github.com/Guilbritto/wai/releases: an **older** one (`A`, e.g.
  0.7.0) and the **latest** one (`B`, e.g. 0.7.1). Both must include the self-updater. Section 3
  needs a third release (`C`) published after you start.
- No administrator rights are needed for anything below. If Windows asks for them (UAC), that is a
  failure: write it down.
- To read wai's log, start it from PowerShell with stderr redirected:
  `Start-Process "$env:LOCALAPPDATA\wai\wai.exe" -RedirectStandardError "$env:TEMP\wai-err.txt"`.
- Do not delete `~\AppData\Roaming\wai`: the tests check that settings survive the update.
- For every item, record **PASS/FAIL** and the evidence: the version shown in the top bar, the
  paths, the log lines, how long it took, and a screenshot when something on screen is the result.

## 1. Download and first install (release `A`)

1. On the page for release `A`, check the assets: `wai_A_x64-setup.exe` and `SHA256SUMS.txt`.
2. Download both. `(Get-FileHash .\wai_A_x64-setup.exe -Algorithm SHA256).Hash` must match its line
   in `SHA256SUMS.txt`, ignoring case.
3. Run the installer. SmartScreen should say "Windows protected your PC". **More info → Run anyway**
   must work, and **no UAC prompt** may appear.
4. wai opens, and the top bar shows `A`.
5. **Add or remove programs** lists wai at version `A`, once. The Start menu has a wai entry.

## 2. Update `A` → `B`

6. Settings → **Updates**: the running version `A` is shown, the switch is **on** without you having
   touched it, and the repository is `Guilbritto/wai`.
7. Quit wai and open it again (release `B` is already the latest). Within about a minute, with no
   clicks, a notification says wai `B` is ready, with **Restart now**.
8. The downloaded installer is staged in wai's cache (`%LOCALAPPDATA%\wai-updates`). Its
   SHA-256 matches `SHA256SUMS.txt` of release `B`.
9. Open a file in wai's editor and type something without saving. Click **Restart now**: wai must
   ask about the unsaved edit. **Cancel** keeps wai open at `A` and installs nothing.
10. Start an agent session and send it a prompt that keeps it working for a while. Click **Restart
    now** while it is working: wai must ask before quitting. Cancel.
11. With nothing unsaved and no session working, click **Restart now**:
    - wai closes, with no installer window and no UAC prompt;
    - wai opens again **by itself, exactly once**, and the top bar shows `B`;
    - note how many seconds it took from the click to wai being open again.
12. After the update:
    - your settings are unchanged (theme, default agent, the Updates switch);
    - your sessions and history are still there;
    - the extensions still start (Extensions panel);
    - **Add or remove programs** shows `B`, still only one entry;
    - the Start menu entry opens `B`.
13. Settings → Updates → **Check now** says wai is up to date. The update cache
    (`%LOCALAPPDATA%\wai-updates`) is now gone: an up-to-date check empties it.
14. The Desktop shortcut was not recreated by the update: delete it before step 11 and check it is
    still absent now (the Start menu entry from the first install stays).

## 3. The cycle repeats (release `C`)

15. Once release `C` is published, open wai `B`. It is offered `C`, and **Restart now** brings it
    to `C` as in step 11, but first open a second wai window (start `wai.exe` again) and click
    **Restart now** in one: that window closes, nothing is killed, and the other window keeps its
    state. The install runs only after the other window is closed; close it, and wai reopens at `C`.
16. Old installers do not pile up in the update cache: only the latest one, or none, is there.

## 4. When it must not install

17. **Switch off:** turn the Updates switch off and restart wai. No notification appears, and the
    log shows no update request. Turn it back on.
18. **Other repository:** set the Repository field to another `owner/repo` that has a newer release,
    for example `Guilbritto/wai-test` if the owner provides one; otherwise skip and say so. Restart
    wai: at most a notification with **Open release page**, never **Restart now**. Nothing new is staged: the cache holds no installer. Put `Guilbritto/wai` back.
19. **Offline:** disconnect the network and open wai. It opens normally with no error notification
    at launch; the log may record the failure. **Check now** in Settings shows the error.
    Reconnect.
20. **Tampered download:** while a **Restart now** notification is showing, change one byte of the
    staged installer, then click **Restart now**. wai must refuse to run it, say why, and stay at
    its current version.
21. **Not an installed copy:** copy `wai.exe` to another folder, for example the Desktop, and run
    it from there. It must not offer **Restart now**. Settings → Updates says in one sentence why
    this copy cannot update itself, also while it shows "Up to date" (the sentence is there with
    **Check now** next to **Open release page** when a newer release exists), and the notification offers only **Open release page**.

## Report

Return a table with one row per item, in this form:

| # | Result | Evidence |
|---|--------|----------|
| 1 | PASS / FAIL / SKIPPED | versions, paths, log lines, timing, screenshot names |

Below the table, describe each FAIL in detail: what you did, what you expected, what happened, the
relevant lines of `wai-err.txt`, and the state wai was left in. If wai stopped opening at any point,
say exactly what was installed where, and recover by installing the latest release from the release
page.
