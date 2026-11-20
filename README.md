# LMM CLI

[简体中文](README.zh-CN.md) · [Usage guide](docs/usage.md) · [Translation guide](docs/i18n.md)

LMM (Language Model Mesh) is an independent Rust CLI for provider presets, local software inspection, account catalog and balance reads, and explicitly scoped application adapters. The Cargo package is `lmm-cli`; the executable is `lmm`. Its build and dependency lockfile are independent of the parent service.

This source tree is a development preview. It includes a Linux adapter for an explicitly installed Pi 1.2.0 SDK and a managed CC Switch distribution adapter. A successful plan, database update or process launch does not establish that an application is active on an LMM route or that a model call works. The [usage guide](docs/usage.md), [managed distribution contract](docs/managed-distributions.md) and [native integration evidence](docs/native-credential-evidence.md) describe these boundaries.

## Build this source revision

```sh
mise install
mise exec -- cargo install --path . --locked
lmm --lang en --help
```

Use this repository's `mise.toml` for the pinned development toolchain (Rust 1.100.0); the package declares Rust 1.99.1 as its minimum. Building needs native compilation tools. Linux OAuth also needs a running Secret Service; Windows and macOS use their system credential stores. Published packages may differ from this source revision. A standalone installer for users without a development environment is not provided here.

## Choose a language

```sh
lmm --lang en catalog astrbot
lmm catalog astrbot --lang zh-CN
LMM_LANG=zh-CN lmm --help
```

The global `--lang` flag takes precedence over the first nonempty value in `LMM_LANG`, `LC_ALL`, `LC_MESSAGES` and `LANG`. Unknown environment locales fall back to English; an unsupported explicit flag is rejected. Help and terminal prose are translated. JSON, including catalog purposes, notes and recognized error messages, remains canonical English in both languages. Command names, identifiers, protocol values and numeric data do not change. See [Translations](docs/i18n.md).

## Available commands

| Task | Entry point | Result and boundary |
| --- | --- | --- |
| Provider presets | `lmm presets [query] --json` | Offline snapshot of 56 presets; no provider configuration or connectivity test |
| Software catalog | `lmm catalog [query]` | Project identities and separate installation/integration/maintenance capability fields |
| Local inspection | `lmm status [software] --json` | Path evidence, managed CC Switch state, or explicitly selected native settings |
| Shareable diagnosis | `lmm doctor [software] --report --json` | Redacts local paths; incomplete verification remains explicit |
| CLI account | `lmm login`, `models`, `balance`, `logout` | Browser PKCE and read-only account scopes; no application model-call grant |
| Managed CC Switch | `lmm install cc-switch --dry-run --json` | Fixed official release plan; applying it requires the exact `plan_id` |
| Native CC Switch configuration | `lmm setup cc-switch --help` | Separate application grant and add-only native database transactions; activation is separate |
| Pi Provider | `lmm setup pi --help` | Explicit Node, Pi 1.2.0 SDK, project and agent paths; authorization stays in Pi |

`--json` disables CLI prompts. `--non-interactive` does not wait for missing input. Credential-store operations that may require unlocking reject unattended use before accessing credentials. `--yes` cannot replace a missing adapter, a required plan ID, application consent or a server-enforced spending limit.

## Start with read-only commands

```sh
lmm presets deepseek --json
lmm catalog
lmm status astrbot --instance /absolute/path/to/AstrBot --json
lmm doctor astrbot --instance /absolute/path/to/AstrBot --report --json
lmm install cc-switch --dry-run --json
```

Explicit paths refer to the operating system running the CLI. The CLI does not scan other users, remote systems or container instances. The [usage guide](docs/usage.md) explains Pi settings inspection, CC Switch receipts, OAuth, recovery and exit codes before any write is requested.

## Develop and verify

```sh
python3 scripts/check-i18n.py
python3 scripts/test_i18n_checks.py
mise exec -- cargo fmt --all -- --check
mise exec -- cargo test --locked
mise exec -- cargo clippy --locked --all-targets --all-features -- -D warnings
```

Tests use isolated fixtures. The two Linux credential migration checks are opt-in and require an explicitly isolated Secret Service; ordinary tests do not certify live credential stores on every operating system. Local validation does not publish a crate, release or production deployment.

## Scope and attribution

The [original requirements](docs/requirements.zh-CN.md) remain the product target. The [Magpie comparison](docs/magpie-parity.md) records implemented features and remaining work against a pinned upstream revision. LMM ports behavior to Rust and does not wrap or start Magpie or depend on a Go runtime.

LMM is licensed under [AGPL-3.0-only](LICENSE). The bundled provider snapshot derives from MIT-licensed Magpie material; see [NOTICE](NOTICE) and [third-party provenance](third_party/magpie/NOTICE.md). Static presets are not a claim of current provider access or runtime compatibility.
