---
schema_version: 1
open_count: 2
waived_count: 0
fixed_count: 0
total_count: 2
last_updated: 2026-08-29T11:11:05.753Z
---

# Broken Windows Ledger

> Cross-phase defect register. With `workflow.windows_enforce` enabled, `/gsd-ship` blocks while `open_count > 0`.
> Waive with `gsd-tools windows waive <id> "<reason>"` (reason required).
> Mark fixed with `gsd-tools windows fixed <id>`.

| id | phase | kind | file | line | description | status | reason | recorded_at | resolved_at |
|----|-------|------|------|------|-------------|--------|--------|-------------|-------------|
| 1 | FLOWPDF-02 | deviation | scripts/verify-dependency-locks.mjs |  | Extended the Phase 1-only lock verifier to consume the accepted Phase 2 provenance report | open |  | 2026-08-29T11:11:05.653Z |  |
| 2 | FLOWPDF-02 | deviation | vite.config.ts |  | Adapted the legacy Inspector stylesheet reference for Vite's HTML pipeline | open |  | 2026-08-29T11:11:05.753Z |  |

````json
[
  {
    "id": 1,
    "kind": "deviation",
    "phase": "FLOWPDF-02",
    "file": "scripts/verify-dependency-locks.mjs",
    "line": null,
    "description": "Extended the Phase 1-only lock verifier to consume the accepted Phase 2 provenance report",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-08-29T11:11:05.653Z",
    "resolved_at": null
  },
  {
    "id": 2,
    "kind": "deviation",
    "phase": "FLOWPDF-02",
    "file": "vite.config.ts",
    "line": null,
    "description": "Adapted the legacy Inspector stylesheet reference for Vite's HTML pipeline",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-08-29T11:11:05.753Z",
    "resolved_at": null
  }
]
````
