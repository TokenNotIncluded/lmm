# Usage guide

[English README](../README.md) · [简体中文](usage.zh-CN.md)

This guide describes the current Rust source tree, its implemented commands and their limits. `lmm-cli` is the Cargo package and `lmm` is the executable. The CLI builds independently of the parent service. The full [product requirements](requirements.zh-CN.md) remain the target; the [Magpie comparison](magpie-parity.md) distinguishes implemented behavior from remaining work.

## Build and language selection

From the repository root:

```sh
mise install
mise exec -- cargo install --path . --locked
lmm --lang en --help
```

The development toolchain is pinned to Rust 1.100.0 in `mise.toml`; `Cargo.toml` declares Rust 1.99.1 as the minimum. Native compilation tools are required. Published packages can lag this source tree. Source validation does not certify the capabilities of a separately published binary.

`--lang en` and `--lang zh-CN` work before or after a subcommand, including help. The flag overrides the first nonempty value in `LMM_LANG`, `LC_ALL`, `LC_MESSAGES` and `LANG`. Unknown environment locales fall back to English; unsupported explicit flags are rejected. Terminal presentation is translated, while CLI-generated JSON prose stays English and protocol values remain unchanged. Original vendor metadata is preserved. See [Translations](i18n.md).

## Presets, software and account data

`lmm presets [query] --json` searches the offline snapshot of 56 provider presets. It does not configure a provider, read a key, discover live models or test connectivity. Source attribution and the fixed Magpie revision are preserved under [third_party/magpie](../third_party/magpie/NOTICE.md). Static endpoints and models may become outdated.

`lmm catalog [query]` searches software identities and translated descriptions. The catalog separately records installation, integration and maintenance capabilities. It is not an installed-software list or a complete compatibility declaration. `lmm models` instead reads the model catalog authorized for the current LMM account; `lmm balance` reads the account's balance.

```sh
lmm presets deepseek --json
lmm catalog
lmm models --json
lmm balance --json
```

Model results preserve server context/capability metadata, cache prices and group/trust multipliers. Unknown fields remain `null`, and returned prices are not multiplied again. Balance V2 preserves exact integer credits, USD balance and the server conversion unit; unknown units or a failed request are not rendered as zero. These commands do not select a model or make a paid model call.

## Local inspection

Local inspection uses explicit paths in the operating system running the CLI. It does not scan other users, remote systems, networks or container instances. PATH matches are only evidence; discovered executables are not run, even for `--version`. Symlinks and inaccessible paths remain unknown, and a missing file describes only the location inspected.

```sh
lmm status astrbot --instance /absolute/path/to/AstrBot --json
lmm status cc-switch --instance /absolute/path/to/.cc-switch --json
lmm doctor astrbot --instance /absolute/path/to/AstrBot --report --json
lmm setup astrbot --dry-run --json
```

AstrBot inspection checks the selected root's `data/cmd_config.json` path. CC Switch opens an explicitly selected database in a read-only SQLite transaction, checks schema 24 and reads non-credential provider metadata. Managed installation state and database selection are reported separately. Neither establishes application authorization, the actual model route or a successful call.

`--report` removes instance and evidence paths from diagnostic output; ordinary `status` can expose local paths. Configuration values and credential contents are not included. `doctor` remains incomplete, returning `3` even if a supported configuration check passes; `--fix` still refuses writes. Status exit code `0` means the applicable query completed, not that the application is ready for use.

### Pi settings inspection

The read-only Pi inspector supports Linux and the manifest format of the official Pi 1.2.0 npm package. Supply three explicit directories:

```sh
lmm status pi --instance /absolute/project \
  --pi-package /absolute/node_modules/@earendil-works/pi-coding-agent \
  --agent-dir /absolute/pi-agent --json
lmm doctor pi --instance /absolute/project \
  --pi-package /absolute/node_modules/@earendil-works/pi-coding-agent \
  --agent-dir /absolute/pi-agent --report --json
```

This path reads the package manifest, the selected agent's `settings.json` and the project's `.pi/settings.json`. It does not need Node, run the SDK, read `auth.json`, or create a state directory or configuration lock. Linux directory and file opens reject symlink traversal at each component.

`pi_settings.provider_reference` distinguishes the pinned source, other recognized LMM sources and multiple references. Common Pi 1.2.0 npm/Git forms are recognized; local extension paths, npm aliases and unknown forms are unresolved. A known source mixed with unresolved entries reports `known_with_unresolved_sources`. Even `no_known_source` cannot exclude an extension loading LMM by another mechanism. A package reference does not prove code is installed or loaded.

`lmm_added=referenced_in_settings` and `lmm_enabled=selected_in_settings` describe a reference and a default provider field, not runtime activation. Project provider/model defaults override the corresponding global fields independently; `provider_scope` and `model_scope` report those sources. Model values are reduced to set/unset and other providers to `other`. Multiple known sources are reported without automatic deduplication.

Unknown versions, inaccessible files, symlinks including directory ancestors, files over 1 MiB and incompatible known field shapes remain unknown and make `status` return `3`. Missing settings are treated as unset and are not created. The files are separate snapshots, not an atomic cross-file view. Hard links, bind mounts and changes after opening are outside the symlink checks. Project trust, launch arguments, session selections and model caches can change actual behavior; this check verifies none of them or the SDK's authenticity. A project path without Pi format arguments retains ordinary path inspection.

## Plans and explicit consent

`--json` disables CLI prompts, and `--non-interactive` does not wait for missing input. `--yes` cannot bypass missing capability, required application consent or server-enforced cost limits. `--all` does not turn the software catalog into a list of installed targets: `update --all` considers only LMM-managed CC Switch, while `setup --all` still has no executable application targets.

Supported adapters use their own native transactions, journals and exact scope checks. Unsupported software continues to return a blocked plan instead of an empty success. Installation alone does not need CLI login. Status, diagnostics and dry-run planning do not make paid model calls or create their own state directories.

### Managed CC Switch distributions

The Linux x86_64/aarch64 adapter manages a pinned official CC Switch AppImage inside an LMM-owned private directory. Its version, source commit, byte length and SHA-256 are recorded in the [managed distribution contract](managed-distributions.md) and [release manifest](../src/managed/releases.rs). It does not execute an arbitrary network `latest` release or claim upstream signature verification.

```sh
lmm install cc-switch --dry-run --json
lmm update cc-switch --dry-run --json
lmm restore cc-switch --change <change_id> --dry-run --json
lmm uninstall cc-switch --dry-run --json
```

After reviewing a plan, repeat the same arguments without `--dry-run` and with `--yes --plan-id <plan_id>`. The plan becomes invalid if managed state changes. `--yes` alone does not skip the plan. `--managed-root` selects the private program directory; `--instance` selects application data for database compatibility checks. `run --instance` is refused because it cannot rewrite CC Switch's native launch settings.

`lmm run cc-switch` starts only the verified managed program. A `process_started` result establishes only that a child process was created. Graphical-session/FUSE requirements, application health, live routing and model calls need separate acceptance. Installation does not edit PATH, system packages, desktop entries or application configuration.

The journal records pending work before download or removal, then commits state and a receipt. Resume an interrupted operation with its original plan ID. A previously committed plan returns its receipt rather than overwriting later changes. `--cancel-pending` requires the original plan and refuses cancellation after original files have been removed. Restoration needs an explicit `change_id` and preserves later user changes; a native database newer than the old program's supported schema blocks a downgrade. App data is not rolled back or removed.

### Native CC Switch application configuration

Independent application grants do not reuse the CLI read-only login. `authorize` obtains browser consent for the selected instance, group and spending scope. The server must provide the matching independent authorization and recoverable renewal contract. Credentials remain in the system vault; native Codex command authentication obtains a model bearer without copying a CLI token or another application's credentials.

`setup cc-switch --add-only` adds an LMM provider to an explicit schema 24 instance. `sync` refreshes metadata from that application's authorized catalog while retaining the original model choice. Preview and apply require exact target/plan identifiers. If a database commit outcome is unknown, query the original `change_id` with `receipt` before retrying. `restore --configuration --change` uses native receipts and refuses conflicting later edits.

A provider row is not evidence that Codex is active on the route. `--activate` and complete removal need a CC Switch native operation that checks live configuration, proxy and Stack references; an `is_current` field or inactive SQLite pointer is insufficient. The hidden `credential` command serves the native raw-token pipe and rejects JSON and terminal token display. See [native credential evidence](native-credential-evidence.md) for the pinned upstream contract and unexecuted compatibility boundaries.

### Pi Provider adapter

The Linux adapter uses an already installed official Pi 1.2.0 SDK and manages only the pinned LMM Provider in an explicitly selected project. It does not install/uninstall Pi itself or copy CLI credentials. The current source is `git:github.com/TokenNotIncluded/pi-lmm-provider@f0c54764eb3d73e50790346f0dbab834587fe27b`.

```sh
lmm setup pi --add-only --instance /absolute/project --node /absolute/node \
  --pi-package /absolute/node_modules/@earendil-works/pi-coding-agent \
  --agent-dir /absolute/pi-agent --dry-run --json
```

After reviewing the preview, repeat the same paths without `--dry-run` and add `--yes`. `setup`/`install` adds only the pinned Provider. `provider_added_authorization_pending` is not a usable-model claim. Choose LMM using `/login` inside Pi and explicitly choose a model with `/model`; `--activate` cannot substitute for those steps. Existing alternate LMM sources block duplicate installation, while current default provider/model selections are preserved.

`update` accepts only the still-present owned source and does not chase latest versions. `sync` verifies presence and reports `catalog_refresh_pending`; `/lmm-prices` in Pi performs the actual refresh. `run pi` uses the same four explicit paths and requires an interactive terminal. Setup, sync and disconnect can return `3` after their limited phase because authorization, refresh or revocation remains pending in Pi.

Restore/disconnect removes only the exact source that LMM added through Pi's native settings transaction. It preserves later plugins and defaults and refuses restoration if the user changed the owned source object. Downloaded code may be shared by another instance and is retained. Use `/lmm-revoke` inside Pi to revoke Pi authorization; removing a settings reference does not revoke it. Uninstall does not delete Pi or unknown resources.

The secret-free journal is `.pi/lmm-owned.json` and the adapter lock is `.pi/lmm-adapter.lock` in the selected project. Pending state is persisted before a write and completed only after readback. Pi's `FileSettingsStorage.withLock` reads the latest settings before editing, avoiding replacement of a concurrent user's plugin array. The adapter never reads Pi's credential file.

## Browser login and credential stores

```sh
lmm login
lmm models --json
lmm balance --json
lmm logout

# Use the same explicitly trusted issuer for related account commands.
lmm login --issuer https://lmm.example.com --no-browser
```

The server must register the `lmm` client and implement its OAuth contract. CLI login requests `catalog:read balance:read` and the group snapshot displayed on the consent page; it does not request `models:invoke`, MCP access or account administration. A CLI session does not authorize AstrBot, CC Switch or Pi.

The callback listens only on a random `127.0.0.1` port. The default wait is 180 seconds; `--timeout` accepts 10–600 seconds. `--no-browser` disables automatic browser opening but still requires that browser to reach this machine's loopback port. It is not device-code login or cross-device headless authorization. Login requires an interactive terminal. An existing session is not silently replaced; log out before switching accounts. A callback receipt alone does not prove token exchange and storage completed.

Windows uses Credential Manager, macOS uses Keychain, and Linux uses Secret Service through zbus rather than a statically linked libdbus. A system unlock prompt may appear. There is no ordinary-file credential fallback. Authentication and applicable application commands reject `--non-interactive` before opening the store; `models --json` and `balance --json` disable CLI prompts but cannot suppress an operating-system unlock prompt.

OAuth operations share a secret-free `oauth.lock` in the current user's state directory:

| System | State directory |
| --- | --- |
| Linux | `$XDG_STATE_HOME/lmm` or `~/.local/state/lmm` |
| macOS | `~/Library/Application Support/lmm` |
| Windows | `%LOCALAPPDATA%/lmm` |

New Unix directories and lock files use `0700` and `0600`; unsafe existing directories or locks are rejected. Before refreshing, the CLI saves a non-retryable state in the vault. A lost response or failed save does not cause replay of an old refresh token; log out and obtain consent again. If logout revocation fails, the local record remains for a retry and success is not reported. Logout affects only this issuer's CLI token family, not independent application grants.

## Development, outcomes and remaining limits

Run checks from the repository root using `mise`:

```sh
python3 scripts/check-i18n.py
python3 scripts/test_i18n_checks.py
mise exec -- cargo fmt --all -- --check
mise exec -- cargo test --locked
mise exec -- cargo clippy --locked --all-targets --all-features -- -D warnings
```

Fixture tests use temporary directories, synthetic inputs and SQLite. They do not start an official AppImage, authorize a real account or make a paid model call. Two opt-in Linux credential migration tests require a deliberately isolated Secret Service and remain ignored in an ordinary test run. Local success does not certify live credential stores on all platforms or publish a release.

| Exit code | Meaning |
| --- | --- |
| `0` | A query or executable preview was produced, or a managed operation committed; inspect individual verification fields |
| `2` | Invalid/incomplete arguments, unknown target, selection failure or output error |
| `3` | Blocked operation, unsupported capability or incomplete verification |

The in-memory `restore_fields` helper restores only scalar fields still equal to LMM's previous write and rejects conflicts across the target. It is not a file transaction or authorization mechanism. Managed distribution restoration and `restore --configuration` instead use their own persistent journals, native version gates and state comparisons. A bounded fingerprint is not an atomic write lock, and client-side cost checks do not replace server-enforced spending limits.

The CLI does not copy third-party credentials or upload telemetry. Confirmed managed operations create their private state and secret-free journals; OAuth uses its separate lock and system vault. Normal status can include local paths, so review output or use `doctor --report --json` before sharing it.

## Attribution

The project uses [AGPL-3.0-only](../LICENSE). Provider preset data retains attribution and licensing for the pinned MIT-licensed Magpie source in [NOTICE](../NOTICE). Referencing this data does not imply feature parity with Magpie or a Go runtime dependency.
