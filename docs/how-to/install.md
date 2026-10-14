# Install vers

vers is a single self-contained binary with no runtime dependencies. Every
install method below gives you the same binary. Pick the one that fits the tools
you already use.

## On your machine

### Standalone installer (macOS, Linux)

```sh
curl -LsSf https://github.com/kwint/vers/releases/latest/download/vers-installer.sh | sh
```

The binary is installed to `~/.local/bin`. Update it later with
`vers self update`.

### Standalone installer (Windows)

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/kwint/vers/releases/latest/download/vers-installer.ps1 | iex"
```

### Package managers

| Ecosystem      | Install                                   | Run once, no install |
|----------------|-------------------------------------------|----------------------|
| Homebrew       | `brew install kwint/tap/vers`             |                      |
| Python (uv)    | `uv tool install vers-changelog`          | `uvx --from vers-changelog vers` |
| Python (pipx)  | `pipx install vers-changelog`             | `pipx run --spec vers-changelog vers` |
| Node (npm)     | `npm install --global vers-changelog`     | `npx vers-changelog` |
| Rust           | `cargo binstall vers-changelog` or `cargo install vers-changelog --locked` | |
| Windows        | `winget install kwint.vers`               |                      |

On PyPI, npm and crates.io the package is called `vers-changelog`, because
`vers` was already taken there. The command it installs is always `vers`.

The Python and npm packages contain the native binary. They don't need Python
or Node at runtime, only to install.

### Prebuilt binaries

Download an archive for your platform from the
[releases page](https://github.com/kwint/vers/releases), extract it and put
`vers` on your `PATH`. Builds are provided for:

- Linux: x86_64 and aarch64, glibc and musl
- macOS: x86_64 and Apple Silicon
- Windows: x86_64 and aarch64

## Inside a project

If your project already has a dev-dependency file, you can add vers to it. Then
everyone on the team gets the same version without a separate install step.

```sh
npm install --save-dev vers-changelog   # then: npx vers ...
uv add --dev vers-changelog             # then: uv run vers ...
```

### Require a minimum version

To make sure everyone uses a compatible vers, however they installed it, set
`required_version` in the root `vers.toml`:

```toml
required_version = ">=1.2"
```

If the installed vers doesn't match, it stops with an error that says which
version is needed.

## In CI

### GitHub Actions

```yaml
- uses: kwint/vers-action@v1
  with:
    version: "1.2.0"     # optional; defaults to required_version, then latest
- run: vers check
```

### GitLab CI

```yaml
vers-check:
  image: alpine:3
  before_script:
    - apk add --no-cache curl git
    - curl -LsSf https://github.com/kwint/vers/releases/download/1.2.0/vers-installer.sh | sh
    - export PATH="$HOME/.local/bin:$PATH"
  script:
    - vers check
```

If your job image already has Python or Node, `uvx --from vers-changelog==1.2.0 vers check`
or `npx vers-changelog@1.2.0 check` also works without a separate install step.

### Any CI

```sh
curl -LsSf https://github.com/kwint/vers/releases/download/1.2.0/vers-installer.sh | sh
export PATH="$HOME/.local/bin:$PATH"
```

Put a version number in the URL instead of `latest` so builds are reproducible.

## Shell completions

```sh
vers completions bash > ~/.local/share/bash-completion/completions/vers
vers completions zsh  > "${fpath[1]}/_vers"
vers completions fish > ~/.config/fish/completions/vers.fish
```

PowerShell and Elvish are also supported.

## Uninstall

- **Standalone installer:** delete `~/.local/bin/vers` (`%USERPROFILE%\.local\bin\vers.exe` on Windows).
- **Package managers:** use their uninstall command, for example
  `brew uninstall vers` or `uv tool uninstall vers-changelog`.
