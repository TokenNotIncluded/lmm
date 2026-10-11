# Magpie provider presets

The built-in data in `presets.json` is adapted from [yetone/magpie](https://github.com/yetone/magpie), `internal/provider/presets.go`, at revision [`d5cae267a727e23f4b6e2b28aa107ecdeac5aa9c`](https://github.com/yetone/magpie/blob/d5cae267a727e23f4b6e2b28aa107ecdeac5aa9c/internal/provider/presets.go).

Copyright (c) 2026 yetone. Licensed under the MIT License; see `LICENSE`. LMM retains the upstream provider identifiers, endpoint definitions, region and plan choices, and their upstream ordering. The Go composite literal was converted to its public JSON representation, resolving the fixed helper functions and constants at that revision. The `Hosts` field is excluded, matching the upstream `json:"-"` tag. No Go source or runtime is embedded.

This is a fixed built-in catalog snapshot. It does not include Magpie's live partner feed, configured providers, authentication, model discovery, routing, API proxy, or agent rewiring. LMM's Rust command only searches and prints the bundled data. Endpoints, model names, plan descriptions, and prices in upstream notes are historical snapshot values and have not been independently verified as current vendor facts.

Upstream source SHA-256: `3f3117872ad5b416e27ec9328962186195315d721e137b3a62f7c1143e17e0a2`.
Bundled JSON SHA-256: `4a3f5e683db4e6ed921fdb4b1762830095df67c5b29c69976f38d5fe84e70b4b`.
