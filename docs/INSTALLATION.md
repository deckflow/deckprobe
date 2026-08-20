# Installation

DeckProbe is a self-contained Rust CLI. Prebuilt releases do not require Rust,
Python, Microsoft Office, LibreOffice, or external PDF libraries at runtime.

The installers use a user-writable Cargo-style prefix by default. Installing
there and running DeckProbe normally do not require root or administrator
privileges. A system-wide prefix such as `/usr/local/bin` or
`C:\Program Files` does require the corresponding elevated permission.

## macOS

The installer selects Apple Silicon or Intel automatically:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/deckflow/deckprobe/releases/latest/download/deckprobe-installer.sh | sh
```

It installs `deckprobe` under `$CARGO_HOME/bin` (or `$HOME/.cargo/bin`) and
updates `PATH` when needed. If macOS Gatekeeper reports that the developer
cannot be verified, this is because the current release is not signed with an
Apple Developer ID or notarized. Verify the release archive first, then approve
it according to the local security policy; do not treat a Gatekeeper warning as
a checksum failure.

For a manually downloaded archive, verify its checksum with:

```sh
shasum -a 256 deckprobe-<target>.tar.gz
```

## Linux

The installer detects x86-64 or ARM64 and the GNU or musl libc variant:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/deckflow/deckprobe/releases/latest/download/deckprobe-installer.sh | sh
```

The installer checks its embedded SHA-256 digest when `sha256sum` is available,
then installs under `$CARGO_HOME/bin` (or `$HOME/.cargo/bin`). A manually
downloaded archive can be checked with:

```sh
sha256sum deckprobe-<target>.tar.gz
```

On hardened systems, execution can still be blocked by a `noexec` mount,
SELinux/AppArmor, or an enterprise policy. Those are host policy decisions and
are separate from DeckProbe's file permissions.

## Windows

The prebuilt Windows release currently targets x86-64 MSVC:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/deckflow/deckprobe/releases/latest/download/deckprobe-installer.ps1 | iex"
```

Restart the shell if the PowerShell installer updated the user `Path`. The
`ExecutionPolicy Bypass` applies to this one installer invocation; it does not
grant DeckProbe administrator access. Windows SmartScreen may still identify a
downloaded binary as being from an unknown publisher because the current
release is not Authenticode-signed.

For a manually downloaded archive, verify its checksum with:

```powershell
Get-FileHash .\deckprobe-x86_64-pc-windows-msvc.zip -Algorithm SHA256
```

## Supported prebuilt targets

| Platform | Architectures | Release variants |
| --- | --- | --- |
| macOS | Apple Silicon, Intel | native `.tar.gz` |
| Linux | arm64, x86_64 | GNU and static musl `.tar.gz` |
| Windows | x86_64 | MSVC `.zip` |

Every archive has a `.sha256` sidecar, and each release includes a consolidated
`sha256.sum` plus GitHub artifact build-provenance attestations. A checksum
detects corruption or an altered download; the provenance attestation records
the CI build that produced the artifact. Neither is an operating-system code
signature. The project currently does not publish macOS Developer ID
signatures/notarization or Windows Authenticode signatures.

The one-line installer commands are a convenience for trusted environments. For
a controlled or offline installation, download and inspect the installer and
archive from the GitHub Release, verify the checksum and (where required) the
build-provenance attestation, then run the installer locally.

## Install from source

Source installation requires Rust 1.88 or newer and Git:

```sh
cargo install --git https://github.com/deckflow/deckprobe --locked deckprobe
```

The workspace uses only Cargo-managed Rust dependencies; there are no native
Office or PDF development packages to install.

`--locked` keeps the dependency resolution recorded in `Cargo.lock`. It does
not constitute a signature from DeckProbe's maintainers. For a reproducible
source build, pin the checkout to a reviewed release tag or commit and verify
the source through the repository's normal GitHub trust controls.

The locally built executable is not project-signed. At runtime it only needs
execute permission and read access to the input document; it writes reports to
standard output and does not need root privileges.

## Install the agent skill

DeckProbe ships an [Agent Skill](https://agentskills.io) that teaches a coding
agent how to drive the CLI. Three routes install identical content from
`skills/deckprobe/`:

```sh
deckprobe install --skills                  # needs the CLI; covers agents in this project
npx skills add deckflow/deckprobe           # no CLI needed; covers many more agents
```

Claude Code can also take it as a plugin, with `/plugin marketplace add
deckflow/deckprobe` followed by `/plugin install deckprobe@deckflow`.

`deckprobe install --skills` writes to the project by default and to the user
directory with `--global`. Both `--agent` and `--dir` choose the destination
explicitly; `deckprobe install --help` lists the supported agents, and
[the CLI reference](CLI-REFERENCE.md#installing-agent-assets) has the full
directory table.

Re-running the command upgrades a skill DeckProbe previously wrote; it refuses to
replace a `SKILL.md` it did not write unless you pass `--force`. Remove an
installed copy by deleting its directory, for example:

```sh
rm -rf .claude/skills/deckprobe
```

The skills CLI and the plugin install the instructions only, not the binary. The
skill's first step checks for `deckprobe` and falls back to
`npx -y @deckflow/deckprobe` when it is absent.

## Upgrade, custom location, and uninstall

Run the same installer command again to upgrade. To install under a specific
Cargo-style prefix, set `DECKPROBE_INSTALL_DIR` for the installer process. The
binary is placed in that prefix's `bin` directory. For example:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/deckflow/deckprobe/releases/latest/download/deckprobe-installer.sh \
  | DECKPROBE_INSTALL_DIR="$HOME/.local" sh
```

The default standalone installation can be removed with:

```sh
rm "$HOME/.cargo/bin/deckprobe"
```

If `CARGO_HOME` or `DECKPROBE_INSTALL_DIR` was set during installation, remove
`deckprobe` from that location's `bin` directory instead. A source installation
can be removed with `cargo uninstall deckprobe`.

## Maintainer release contract

Each published version, including a beta prerelease, must include a GitHub Release. Before creating its tag,
update the workspace version and changelog, then verify the matching tag plan:

```sh
dist plan --tag vX.Y.Z
```

Maintainers create the tag through the approved promotion tooling. It verifies
that the tag equals the workspace version, publishes the commit first, and then
creates the tag. That tag runs `.github/workflows/release.yml`, which builds,
checksums, attests, and publishes all archives and both installer scripts.
