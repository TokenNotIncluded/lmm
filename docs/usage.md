# Usage guide

[README](../README.md) · [简体中文](usage.zh-CN.md)

## Provider presets, software, and account models

`lmm presets [query]` searches the bundled provider snapshot offline. It does not read credentials, configure providers, query their current models, or test connectivity. The source revision and licenses are recorded under [third_party/magpie](../third_party/magpie/NOTICE.md). Static endpoints and models can become outdated.

`lmm catalog [query]` searches software project identities and descriptions. All integration compatibility combinations are currently unverified. `lmm models` is different: it reads the model catalog authorized for the signed-in LMM account.

## Local inspection

```sh
lmm status astrbot --instance /absolute/path/to/AstrBot
lmm status cc-switch --instance /absolute/path/to/.cc-switch
lmm doctor astrbot --instance /absolute/path/to/AstrBot --report --json
lmm setup astrbot --dry-run --json
```

Use an absolute path in the operating system running the CLI. Explicit instance selection is currently implemented only for AstrBot and CC Switch. AstrBot checks the selected root's `data/cmd_config.json` location. CC Switch checks the `cc-switch.db` location without opening the database.

The CLI does not scan other users, projects, Docker instances, remote systems, or local networks. PATH matches are only evidence; executables are not run, even for `--version`. Symlinks and inaccessible items remain unknown. Missing files describe only the locations that were checked.

A successful `status` query does not prove the application is installed, configured, authorized, compatible, or able to make a model call. `doctor` reports incomplete diagnostics with exit code `3`. Read-only status, diagnostic, and planning commands do not make network or paid calls and do not create state directories.

`--report` removes local instance and evidence paths from diagnostic output. Normal status output can contain local paths. Configuration contents are not included.

## Plans and consent

No software-write adapter is verified in this preview. Setup, install, sync, update, restore, disconnect, uninstall, and run report blockers rather than pretending that an operation completed. `--all` returns an empty plan and exit code `3`; it does not treat the catalog as an installed-software list.

`--yes` cannot grant missing permissions, authorize payments, switch providers, or bypass missing implementation. A standalone installation plan does not require login. `--json` disables CLI interaction. `--non-interactive` does not wait for missing input.

The in-memory `restore_fields` helper reverses only scalar fields still equal to the value written by LMM. Later edits are preserved. Any conflict aborts computation for the entire target. It does not provide the `lmm restore` command with file writes, a journal, or transaction support. Object, array, and array-index restoration need dedicated adapters.

Configuration fingerprints compare bounded snapshots; they are not atomic write locks. Future writes need the application's native transaction or management interface. Client-side price checks cannot replace spending limits enforced by the server.

## Browser login

```sh
lmm login
lmm models --json
lmm logout

# Use the same trusted issuer for all three commands.
lmm login --issuer https://lmm.example.com --no-browser
```

The server must register the `lmm` client and implement the expected OAuth endpoints. The CLI requests `catalog:read balance:read` plus the group snapshot shown on the consent page. It does not request `models:invoke`, MCP access, or account administration. CLI login does not authorize AstrBot, CC Switch, Pi, or another application.

The callback binds only to `127.0.0.1` on a random port. The default wait is 180 seconds; `--timeout` accepts 10 through 600 seconds. `--no-browser` only disables automatic browser launch: the browser must still reach the same machine's loopback port. It is not device-code login or cross-device authorization for a headless server.

Login requires an interactive terminal. An existing session is not silently replaced. To change accounts, run `logout` before `login`.

The callback page follows the CLI language. Receiving a callback is not itself proof that token exchange and credential storage completed; return to the CLI to check the final result.

## Credentials and concurrent operations

Windows uses Credential Manager, macOS uses Keychain, and Linux uses Secret Service. A system unlock prompt may appear. There is no plaintext-file fallback. Linux still needs a running Secret Service even though the D-Bus build dependency is vendored.

Authentication commands reject `--non-interactive` before opening the credential store. `models --json` disables CLI text interaction, but the operating system may still request an unlock. Local fixture tests do not certify real credential-store integration on all three platforms.

OAuth operations use a secret-free `oauth.lock` under the current user's state directory:

| System | Location |
| --- | --- |
| Linux | `$XDG_STATE_HOME/lmm` or `~/.local/state/lmm` |
| macOS | `~/Library/Application Support/lmm` |
| Windows | `%LOCALAPPDATA%/lmm` |

New Unix directories and lock files use `0700` and `0600`. Unsafe existing directories are rejected. Login, refresh, and logout use the same lock to prevent concurrent credential changes.

Before refreshing, the CLI records a non-retryable state in the credential store. If a response or saved result is lost, it will not replay an old refresh token. Log out and sign in again to recover. If logout revocation fails, the local record remains available for a retry; success is not reported.

Logout revokes only this issuer's CLI token family and removes local credentials after successful revocation. It does not revoke independent application grants. This version cannot list those grants.

## Models, prices, and JSON

The account catalog does not prove a target application's compatibility. Unknown prices remain `null`; prices returned by the server are not multiplied again. This command does not call or select a model and does not run a paid test.

Language selection translates prose, not numeric data, protocol values, or JSON field names. See [Translations](i18n.md) for the precedence rules and stable error codes.

LMM does not copy third-party credentials or upload telemetry. Read-only local checks do not create state directories. OAuth operations do use the credential store and lock directory described above.
