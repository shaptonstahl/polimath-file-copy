# Building from source

Polimath File Copy is a [Tauri 2](https://tauri.app) app: a Rust back end with a TypeScript and Vite front end. Everything it needs is free and open source.

## 1. Install the prerequisites

Every OS needs these:

- [Git](https://git-scm.com/downloads)
- [Rust](https://rustup.rs) (stable, installed with `rustup`)
- [Node.js](https://nodejs.org) LTS (20 or newer), which includes `npm`

Then the OS-specific pieces below. They follow Tauri's [prerequisites guide](https://tauri.app/start/prerequisites/).

### Windows 10/11

1. [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/). In the installer, tick **Desktop development with C++**.
2. Microsoft Edge **WebView2**, which Windows 10 and 11 already include.
3. Use the MSVC Rust toolchain, which is rustup's default on Windows:

   ```powershell
   rustup default stable-msvc
   ```

### macOS

Install the Xcode command-line tools:

```bash
xcode-select --install
```

To build a universal app that runs on both Apple silicon and Intel, add both Rust targets:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

### Linux

Debian/Ubuntu:

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

Fedora:

```bash
sudo dnf check-update
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file libappindicator-gtk3-devel librsvg2-devel libxdo-devel
sudo dnf group install "c-development"
```

Arch:

```bash
sudo pacman -Syu --needed webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg xdotool
```

openSUSE:

```bash
sudo zypper in webkit2gtk3-devel libopenssl-devel curl wget file libappindicator3-1 librsvg-devel
sudo zypper in -t pattern devel_basis
```

## 2. Get the code

```bash
git clone https://github.com/shaptonstahl/polimath-file-copy.git
```

```bash
cd polimath-file-copy
```

```bash
npm ci
```

## 3. Run, test and build

Run in development mode, with live reload of the UI:

```bash
npm run tauri dev
```

Run the tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Build installers for the OS you're on:

```bash
npm run tauri build
```

On macOS, build a universal app instead:

```bash
npm run tauri build -- --target universal-apple-darwin
```

The output goes in `src-tauri/target/release/bundle/` (for a universal macOS build, `src-tauri/target/universal-apple-darwin/release/bundle/`):

| OS | Files |
|---|---|
| Windows | `nsis/…_x64-setup.exe`, `msi/…_x64_en-US.msi`, plus the portable `src-tauri/target/release/polimath-file-copy.exe` |
| macOS | `dmg/…_universal.dmg` (or `…_aarch64.dmg`), `macos/Polimath File Copy.app` |
| Linux | `deb/…_amd64.deb`, `rpm/…x86_64.rpm`, `appimage/…_amd64.AppImage` |

Tauri builds installers only for the OS it's running on, so build each OS on that OS. The release workflow does exactly that on GitHub's Windows, macOS and Linux runners.

## Read-only diagnostics

These read file metadata only. They never download, write or move anything.

Show which files in a folder are cloud-only:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --example probe -- "C:\Users\me\Dropbox"
```

Print a plan's warnings without running it:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --example plan -- move "D:\Backup" "C:\Users\me\Dropbox\Docs"
```

## Contributing

Work on a branch and open a pull request against `main`. CI builds and tests on Windows, macOS and Linux for every PR.

## Releasing (maintainers)

1. On a branch, set the new version in **both** `package.json` and `src-tauri/Cargo.toml`. `tauri.conf.json` reads the version from `package.json`. Merge the PR.
2. Tag the merge commit on `main` and push the tag:

   ```bash
   git switch main && git pull
   ```

   ```bash
   git tag v0.2.0
   ```

   ```bash
   git push origin v0.2.0
   ```

3. The [Release workflow](../.github/workflows/release.yml) then:
   - checks that the tag matches both version numbers;
   - creates a draft GitHub Release;
   - builds the Windows (setup.exe, MSI, portable exe), macOS (universal DMG) and Linux (deb, rpm, AppImage) files and attaches them;
   - computes SHA-256 checksums of every attached file, attaches `SHA256SUMS.txt`, writes the checksums into the release notes and publishes the release.

To rebuild an existing tag (after fixing the workflow, say), run **Release** from the Actions tab with that tag. The checksums are recomputed from whatever is attached at the end.
