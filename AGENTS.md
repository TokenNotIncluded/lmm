# LMM contributor guide

This is the independent Rust CLI repository `TokenNotIncluded/lmm`. The binary
is `lmm`; the Cargo package remains `lmm-cli`. Use `mise exec -- cargo ...` for
local development and checks. Read README.md and docs/magpie-parity.md first.

Preserve unrelated work and source attribution. Keep Magpie-derived data and
notices under third_party/magpie, pinned to a reviewed upstream commit. Port
behavior to Rust; do not add a Go runtime or silently execute another client.

Do not claim planned features work. Configuration writes need a concrete preview,
explicit target, concurrency protection and tested restoration that preserves
later user edits. Keep credentials out of logs and repositories. Use isolated
fixtures instead of real account, system configuration or production writes.

Run fmt, clippy and the complete CLI tests locally. Report the tested commit and
skipped checks. A source change does not authorize publishing a crate, release
or production deployment.

Keep user-facing text in locales/en.json and locales/zh-CN.json, not Rust source.
Use stable message keys and named placeholders. Do not translate command names,
JSON field names, identifiers or protocol values. Run scripts/check-i18n.py and
read docs/i18n.md when adding messages. Keep both README versions in sync.
