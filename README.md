<div align="center">

# lmm

**Language Model Mesh, from your terminal.**

Explore provider presets, inspect local tools, and read your LMM model catalog.

[![Rust](https://img.shields.io/badge/Rust-1.88%2B-000000?style=flat-square&logo=rust)](Cargo.toml)
[![CLI](https://img.shields.io/badge/CLI-lmm-2563eb?style=flat-square)](#quick-start)
[![Languages](https://img.shields.io/badge/languages-English%20%7C%20简体中文-0f766e?style=flat-square)](#language)
[![License](https://img.shields.io/badge/license-AGPL--3.0-64748b?style=flat-square)](LICENSE)

**English** · [简体中文](README.zh-CN.md)

[Quick start](#quick-start) · [Commands](#commands) · [Usage guide](docs/usage.md) · [Contribute translations](docs/i18n.md)

</div>

---

## Quick start

Build the current source with the tools pinned in `mise.toml`:

```sh
git clone https://github.com/TokenNotIncluded/lmm.git
cd lmm
mise install
mise exec -- cargo install --path . --locked

lmm --help
lmm presets deepseek --json
lmm catalog
```

The Cargo package is **`lmm-cli`**; the executable is **`lmm`**. This repository builds independently of the LMM backend. Source builds require Rust and native build tools. Linux OAuth also needs a running Secret Service.

## Commands

| Task | Command | What it does |
| --- | --- | --- |
| Explore providers | `lmm presets [query]` | Search the bundled Magpie snapshot, offline |
| Explore software | `lmm catalog [query]` | Search project identities and software descriptions |
| Inspect a tool | `lmm status astrbot --instance /absolute/path` | Check local path evidence without reading configuration contents |
| Share diagnostics | `lmm doctor astrbot --report --json` | Remove local paths from an incomplete diagnostic report |
| Read account models | `lmm login` → `lmm models --json` | Authorize the CLI, then read models and prices without model calls |
| Preview integration | `lmm setup astrbot --dry-run --json` | Show a blocked plan; do not change software or configuration |

**Development preview.** Software installation, integration writes, sync, updates, restore, disconnection, removal, and launch are not implemented. These commands report blockers instead of claiming success. Catalog entries are not compatibility guarantees.

`presets` lists bundled provider metadata. `catalog` lists software. `models` reads models authorized for your account. These are separate resources.

## Language

English and Simplified Chinese are bundled in the binary. No translation download or extra runtime files are needed.

```sh
lmm --lang en --help
lmm status astrbot --lang zh-CN
LMM_LANG=zh-CN lmm catalog
```

Selection order: **`--lang` → `LMM_LANG` → `LC_ALL` → `LC_MESSAGES` → `LANG` → English**.

Environment values such as `zh_CN.UTF-8` and `zh-Hans` are recognized. Empty values are ignored. Unsupported languages fall back to English. `LC_ALL=C` selects English unless overridden by `LMM_LANG` or `--lang`.

JSON field names, status values, identifiers, prices, and timestamps do not change with language. Human-readable descriptions and messages do. Original provider names and upstream data are preserved.

## Login

```sh
lmm login
lmm models --json
lmm logout
```

Login requests **catalog and balance read access only**. It does not grant model invocation or authorize other applications. Credentials stay in the system credential store; there is no plaintext-file fallback.

A browser must reach the CLI's local callback port. `--no-browser` prints the link but is **not** device-code login. Authentication commands do not support `--non-interactive`; even `models --json` may require the operating system to unlock its credential store.

See the [usage guide](docs/usage.md) for trusted self-hosted issuers, callback timeouts, credential storage, and revocation behavior.

## Develop

```sh
python3 scripts/check-i18n.py
python3 scripts/test_i18n_checks.py
mise exec -- cargo fmt --all --check
mise exec -- cargo test --locked
mise exec -- cargo clippy --locked --all-targets --all-features -- -D warnings
mise exec -- cargo build --locked --release
```

[Contributor guide](AGENTS.md) · [Translation guide](docs/i18n.md) · [Product requirements (Chinese)](docs/requirements.zh-CN.md) · [Magpie comparison (Chinese)](docs/magpie-parity.md)

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | A query completed; inspect its reported state before assuming a tool is usable |
| `2` | Invalid or missing arguments, unknown target, selection failure, or output error |
| `3` | Blocked operation, unavailable capability, or incomplete diagnostic check |

## Attribution

Licensed under [AGPL-3.0-only](LICENSE). Provider preset data is derived from a pinned, MIT-licensed [Magpie](third_party/magpie/NOTICE.md) snapshot. LMM is implemented in Rust and does not wrap or run Magpie. See [NOTICE](NOTICE) for source attribution.
