# Build and contribute

Looking to use Tide? Start with [installation](INSTALLATION.md). This guide is
for contributors; normal users do not need a compiler, Python or terminal setup.

## Local development

Use Windows x64, stable Rust and the C++ build tools required by its Windows
toolchain. The local wrapper also supports an already prepared GNU toolchain
under the ignored `.tools` folder. PowerShell examples below use `pwsh`.

From the repository root:

```powershell
pwsh -File scripts/setup-runtime.ps1
./scripts/cargo-local.ps1 -CargoArgs @('run','-p','tide-music')
```

The runtime script downloads pinned mpv, yt-dlp and Deno tools. Account work in a
development checkout additionally needs `scripts/setup-provider.ps1` and its
Python prerequisite. Installed users get a private helper runtime through setup.
See [advanced account setup](ACCOUNT_SETUP_ADVANCED.md).

Use `scripts/cargo-local.ps1` for local compilation, tests and Clippy. It checks
the selected output folder before and after a build: existing output over 8 GiB
or free disk space below 5 GiB stops further work. It does not delete files or
enforce a live filesystem quota; one build can cross the threshold. Dev/test
profiles disable incremental snapshots and limit debug symbols. Prefer hosted
CI for full workspace and installer builds.

## Checks

```powershell
./scripts/cargo-local.ps1 -CargoArgs @('fmt','--all','--','--check')
./scripts/cargo-local.ps1 -CargoArgs @('test','--workspace','--locked')
./scripts/cargo-local.ps1 -CargoArgs @('clippy','--workspace','--all-targets','--locked','--','-D','warnings')
python scripts/check_public_files.py
python -m unittest discover -s helpers -p 'test_*.py'
python -m unittest discover -s scripts -p 'test_*.py'
node --test browser-companion/tests/background.test.cjs
```

Stage only intended source/docs, then run the public-file scan again before
publishing. Never commit sessions, keys, runtime downloads or build output.

## Native UI and playback checks

The UI smoke mode creates synthetic screens and exits:

```powershell
./scripts/cargo-local.ps1 -CargoArgs @('run','-p','tide-music','--','--smoke-ui','.local/screens','--smoke-populated')
```

Add `--smoke-library` for sample account metadata or `--fixture-audio` with a
silent WAV for native interaction checks. UI smoke modes do not load saved
accounts. `--smoke-local-listening 300 <short-audio-file>` exercises local native
transitions with temporary settings. Live autoplay/listening checks use a saved
account and need separate care with private data; see [validation](VALIDATION.md).

## Packages and release work

The [Windows workflow](../.github/workflows/ci.yml) builds the app and native host,
creates dependency notices, packages the exact allowlist, checks privacy and
checksums, builds the EXE installer, and runs a real Windows lifecycle test.
Its synthetic screenshots and integration logs are the review evidence.

`scripts/verify-clean-windows.ps1` performs real Windows registrations and
installation. It is guarded for a disposable GitHub-hosted runner. **Do not run
it on a personal PC or remove its environment checks.**

[Installer internals](INSTALLATION_DETAILS.md) ·
[Launch checklist](BETA_RELEASE.md) ·
[Release evidence and remaining gates](RELEASE_READINESS.md) ·
[Browser protocol reference](../browser-companion/README.md)
