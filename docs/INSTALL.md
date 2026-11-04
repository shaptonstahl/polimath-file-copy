# Installing Polimath File Copy

Download the file for your system from the [latest release](https://github.com/shaptonstahl/polimath-file-copy/releases/latest).

| System | Download | Notes |
|---|---|---|
| Windows 10/11 (x64) | `…_x64-setup.exe` | Recommended installer |
| | `…_x64_en-US.msi` | For managed or scripted installs |
| | `polimath-file-copy.exe` | Portable: no install, just run it |
| macOS 11 or later (Apple silicon and Intel) | `…_universal.dmg` | One download for both chip types |
| Linux (x64), Debian/Ubuntu | `…_amd64.deb` | |
| Linux (x64), Fedora/openSUSE | `…x86_64.rpm` | |
| Linux (x64), any distro | `…_amd64.AppImage` | No install; make it executable and run |

The project is free and has no budget for paid code-signing certificates, so the builds are **not code-signed**. Windows and macOS will warn you the first time. The steps below show how to get past the warning, and the [checksums](#verify-the-download) let you confirm the file is exactly what GitHub's build servers produced.

## Windows

1. Run `…_x64-setup.exe`. If you prefer the MSI, run `…_x64_en-US.msi`.
2. If **"Windows protected your PC"** (SmartScreen) appears, click **More info**, then **Run anyway**.
3. Start **Polimath File Copy** from the Start menu.

Portable option: run `polimath-file-copy.exe` from anywhere. It needs the Microsoft Edge WebView2 runtime, which Windows 10 and 11 already include.

## macOS

1. Open the `.dmg` and drag **Polimath File Copy** into **Applications**.
2. The first time you open it, macOS says it can't verify the developer. Click **Done** (or **Cancel**), then open **System Settings → Privacy & Security**, scroll down and click **Open Anyway** next to the message about Polimath File Copy.

   If you prefer Terminal, this clears the download quarantine instead:

   ```bash
   xattr -dr com.apple.quarantine "/Applications/Polimath File Copy.app"
   ```

## Linux

Debian, Ubuntu, Mint and similar:

```bash
sudo apt install ./*_amd64.deb
```

Fedora, RHEL, openSUSE and similar:

```bash
sudo dnf install ./*.x86_64.rpm
```

AppImage, which works on most distributions:

```bash
chmod +x ./*_amd64.AppImage
```

```bash
./*_amd64.AppImage
```

Linux can't tell cloud-only placeholder files from local ones, so the app warns you when a folder looks like a cloud sync folder.

## Optional: rclone

The app works on its own. If [rclone](https://rclone.org/install/) (free, MIT licensed) is installed and on your `PATH`, or `RCLONE_PATH` points to it, you can choose **rclone** as the transfer engine.

## Verify the download

Each release lists a SHA-256 checksum for every file in its notes, and attaches the same list as `SHA256SUMS.txt`. GitHub Actions computes them when it builds the release. Compare the checksum of the file you downloaded with the published one.

Windows (PowerShell):

```powershell
Get-FileHash .\polimath-file-copy.exe -Algorithm SHA256
```

macOS:

```bash
shasum -a 256 ~/Downloads/*.dmg
```

Linux, with `SHA256SUMS.txt` in the same folder:

```bash
sha256sum -c SHA256SUMS.txt --ignore-missing
```

## Uninstall

- **Windows:** Settings → Apps → Installed apps → Polimath File Copy → Uninstall.
- **macOS:** drag the app from Applications to the Trash.
- **Linux:** `sudo apt remove polimath-file-copy` or `sudo dnf remove polimath-file-copy`. For the AppImage, delete the file.
