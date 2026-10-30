# Pinned native credential and configuration evidence

This is implementation evidence, not executed compatibility acceptance. No
official program was started and no model endpoint was called in this research.

## Codex

- Tag: `rust-v0.166.0`.
- Annotated tag object: `593389380740f7b4762771573c447ccfdc59742f`.
- Commit: `6ff1e1ca3b30fc8b914720681fcf5fe62e95f856`.
- License: Apache-2.0, retrieved as `/tmp/lmm-cli-codex-LICENSE`.
- [ModelProviderAuthInfo](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/protocol/src/config_types.rs)
  defines `command`, `args`, nonzero `timeout_ms`, `refresh_interval_ms`, and
  absolute `cwd`. The defaults are a five-second command deadline and a
  five-minute proactive cache interval.
- [External bearer executor](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/login/src/auth/external_bearer.rs)
  uses a direct process command with individual arguments, null stdin, piped
  stdout/stderr and timeout. A successful nonempty trimmed UTF-8 stdout is the
  token. This is a raw-token pipe, not JSON. Cache refresh and 401 recovery use
  the same command. The candidate changes the native cache to 60 seconds and
  refuses to emit an access token with less than 90 seconds of confirmed life.
- [Provider info validation](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/model-provider-info/src/lib.rs)
  rejects command auth combined with `env_key`, `experimental_bearer_token`, or
  `requires_openai_auth`. The supported wire API is Responses.
- [Provider auth construction](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/model-provider/src/auth.rs)
  creates an external-bearer-only auth manager for the command-backed provider.

The candidate uses this public configuration format and native execution
behavior; it does not copy or run the Codex implementation. A model bearer is
not a ChatGPT account credential or a claim to OpenAI subscription access.

## CC Switch

- Tag: `v4.0.22`.
- Commit: `b76426741ef93069e2f374ac01ca7ef5760e1d0e`.
- Schema: 24. Existing CLI attribution retains the upstream MIT license.
- [Codex projection](https://github.com/farion1231/cc-switch/blob/b76426741ef93069e2f374ac01ca7ef5760e1d0e/src-tauri/src/live/project/codex.rs)
  accepts `{auth,config}`. `config` is TOML. An `auth` table is an independent
  route credential source and remains in the native custom-provider table.
  CC Switch may normalize the provider name to `custom`; it does not replace
  the command with an API key and keeps `requires_openai_auth` false.
- [Native catalog parser](https://github.com/farion1231/cc-switch/blob/b76426741ef93069e2f374ac01ca7ef5760e1d0e/src-tauri/src/codex_config.rs)
  reads `settings_config.modelCatalog.models` by the `model` identifier, with
  optional `displayName`, `inputModalities` and other capability fields. The
  candidate only supplies text capability evidenced by the selected catalog
  surface; it does not invent context windows or reasoning levels.
- [Live engine](https://github.com/farion1231/cc-switch/blob/b76426741ef93069e2f374ac01ca7ef5760e1d0e/src-tauri/src/live/engine.rs)
  explicitly describes optimistic file replacement and implements `lock_app`
  using a process-local mutex and condition variable.
- [Device state](https://github.com/farion1231/cc-switch/blob/b76426741ef93069e2f374ac01ca7ef5760e1d0e/src-tauri/src/mode/state.rs)
  retains Stack membership, proxy route and unfinished native writes in local
  `live-state.json`. They are not covered by a transaction on providers.
- [Deep-link parser](https://github.com/farion1231/cc-switch/blob/b76426741ef93069e2f374ac01ca7ef5760e1d0e/src-tauri/src/deeplink/parser.rs)
  provides import resources, not an authenticated compare-and-switch API for
  an existing provider with arbitrary native command auth.

Consequently, adding/updating provider records is an actual SQLite operation.
It is not evidence that Codex is active on that route. The candidate does not
claim or attempt native-file activation, nor delete an entry based only on an
inactive SQLite pointer. Activation and complete removal require a suitable
native CC Switch operation or a separately reviewed upstream integration.
