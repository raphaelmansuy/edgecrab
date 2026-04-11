---
title: Updating & Uninstalling
description: Keep EdgeCrab up to date or cleanly remove it. Covers cargo install, binary, Docker, and migration of user data.
sidebar:
  order: 3
---

## Updating EdgeCrab

### Recommended

```bash
edgecrab update
```

`edgecrab update` is channel-aware. It detects whether the current install came
from npm, PyPI/pipx, cargo, Homebrew, source, or a manual binary install, then
either runs the right upgrade command or prints safe manual guidance.

### From crates.io

```bash
cargo install edgecrab-cli --force
```

The `--force` flag reinstalls even if the same version is already present. After the build completes, your config, memories, and skills in `~/.edgecrab/` are untouched.

### Pre-built Binary

Download the latest archive from [GitHub Releases](https://github.com/raphaelmansuy/edgecrab/releases) and replace the binary:

```bash
# macOS / Linux — replace in place
sudo cp edgecrab /usr/local/bin/edgecrab
edgecrab version
```

### Docker

```bash
docker pull ghcr.io/raphaelmansuy/edgecrab:latest
```

docker-compose:

```bash
docker compose pull
docker compose up -d
```

### Source Build

```bash
cd edgecrab
git pull origin main
cargo build --release
```

### Homebrew

```bash
brew update
brew upgrade edgecrab
```

---

## Check Your Current Version

```bash
edgecrab version
# EdgeCrab 0.3.0  (rustc 1.86.0, 2026-04-11)
```

---

## Uninstalling EdgeCrab

### 1. Remove the binary

```bash
# If installed via cargo
cargo uninstall edgecrab-cli

# If installed as a pre-built binary
rm /usr/local/bin/edgecrab
```

### 2. Remove configuration and state (optional)

:::caution
This deletes **all memories, skills, and session history**. Back up `~/.edgecrab/` first if you want to preserve them.
:::

```bash
rm -rf ~/.edgecrab
```

### 3. Remove shell completions (if installed)

```bash
rm ~/.zsh/completions/_edgecrab         # zsh
rm ~/.local/share/bash-completion/completions/edgecrab  # bash
rm ~/.config/fish/completions/edgecrab.fish              # fish
```

---

## Backing Up Your Data

Before updating or uninstalling, back up your user data:

```bash
cp -r ~/.edgecrab ~/.edgecrab.bak
```

Your data includes:
- `~/.edgecrab/config.yaml` — provider settings and preferences
- `~/.edgecrab/memories/` — persistent agent memory files
- `~/.edgecrab/skills/` — custom and learned skills
- `~/.edgecrab/state.db` — full session history (SQLite)

---

## Migrating to a New Machine

1. Copy `~/.edgecrab/` to the new machine
2. Install EdgeCrab (any method above)
3. Run `edgecrab doctor` to verify API keys are set

If moving from Hermes Agent, use `edgecrab migrate`. If moving from OpenClaw,
use `edgecrab claw migrate` — see [Migration](/user-guide/migration/).

---

## Staying Up to Date

EdgeCrab follows [semantic versioning](https://semver.org/). Breaking changes are announced in the [Changelog](/changelog/).

**Subscribe to releases:** Watch the [GitHub repository](https://github.com/raphaelmansuy/edgecrab) → "Releases only" to be notified of new versions.

**Automated update check:** EdgeCrab can notify you on startup when a newer release exists. Disable with `display.check_for_updates: false`. Tune the refresh cadence with `display.update_check_interval_hours`.

---

## Frequently Asked Questions

**Q: Will updating overwrite my config, memories, or skills?**

Never. Updates only replace the binary — `~/.edgecrab/` is never touched. Your full session history, memories, skills, and config survive all updates.

**Q: Do I need to re-run `edgecrab setup` after updating?**

No, unless the update adds new required config fields (rare, always documented in the changelog). Run `edgecrab doctor` to catch any new issues.

**Q: How do I update to a specific version, not the latest?**

```bash
cargo install edgecrab-cli --version 0.3.0 --force
```

For pre-built binaries, download the specific tag from GitHub Releases.

**Q: What does `edgecrab update` actually do?**

It depends on how you installed EdgeCrab:

- npm: runs `npm install -g edgecrab-cli@<version>`
- pipx: runs `pipx upgrade edgecrab-cli`
- pip: runs `python -m pip install --upgrade edgecrab-cli==<version>`
- cargo: runs `cargo install edgecrab-cli --locked --force --version <version>`
- brew: runs `brew update` then `brew upgrade edgecrab`
- source or manual binary: prints safe manual steps instead of mutating the install blindly

**Q: The new version has a new config option I want to use. How do I add it?**

```bash
edgecrab config set memory.auto_flush true
```
Or open `~/.edgecrab/config.yaml` in your editor. New options use defaults if not present — you only need to set them if you want non-default behavior.

**Q: Can I roll back to the previous version?**

Yes. Re-install any previous version from crates.io or download the binary from GitHub Releases. Your data is always compatible across patch versions and usually across minor versions (see changelog for exceptions).
