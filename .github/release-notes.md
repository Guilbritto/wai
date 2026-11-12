
---

## Installing wai

The binaries are not signed, so both systems warn on the first launch. The steps below are the normal path, not a workaround.

### macOS

1. Download `wai_<version>_aarch64.dmg` and open it.
2. Copy `wai.app` to `~/Applications` (the `Applications` folder inside your home folder; create it if it does not exist). Do not use `/Applications`: only an administrator can write there, so wai could not update itself.
3. The first time, open the app once (macOS will refuse), then go to System Settings → Privacy & Security and click **Open Anyway**. Or run `xattr -dr com.apple.quarantine ~/Applications/wai.app` and open it normally.

### Windows

1. Download `wai_<version>_x64-setup.exe` and run it. It installs for your user only, into `%LOCALAPPDATA%\wai`, and needs no administrator rights.
2. SmartScreen shows "Windows protected your PC". Click **More info**, then **Run anyway**.

### Updates

Once installed, wai checks this repository for new releases, downloads them in the background and offers **Restart now**. On macOS the previous version stays next to the new one as `wai.app.old`.

Known cost: without a Developer ID, macOS sees each update as a new app and may ask again for access to folders such as Documents and Desktop.
