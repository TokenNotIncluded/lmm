# CC Switch source attribution

The LMM CC Switch adapter was implemented from the official v4.0.22 release and
its database contract at commit
`b76426741ef93069e2f374ac01ca7ef5760e1d0e`.

- Project: https://github.com/farion1231/cc-switch
- Release: https://github.com/farion1231/cc-switch/releases/tag/v4.0.22
- Database: `src-tauri/src/database/{mod.rs,schema.rs,dao/providers.rs}`
- Copyright (c) 2025 Jason Young; MIT. The complete upstream license is retained
  in `LICENSE` alongside this file.

LMM does not bundle the CC Switch executable. The installer downloads a pinned
official release only after an exact local plan is confirmed. Database fixtures
contain a minimal synthetic projection of the documented providers table; they
contain no upstream user data or credentials. Distribution management and
configuration inspection are not claims of end-to-end application compatibility.
