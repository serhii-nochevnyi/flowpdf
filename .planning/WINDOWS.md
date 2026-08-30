---
schema_version: 1
open_count: 4
waived_count: 0
fixed_count: 0
total_count: 4
last_updated: 2026-08-30T18:39:36.336Z
---

# Broken Windows Ledger

> Cross-phase defect register. With `workflow.windows_enforce` enabled, `/gsd-ship` blocks while `open_count > 0`.
> Waive with `gsd-tools windows waive <id> "<reason>"` (reason required).
> Mark fixed with `gsd-tools windows fixed <id>`.

| id | phase | kind | file | line | description | status | reason | recorded_at | resolved_at |
|----|-------|------|------|------|-------------|--------|--------|-------------|-------------|
| 1 | FLOWPDF-02 | deviation | scripts/verify-dependency-locks.mjs |  | Extended the Phase 1-only lock verifier to consume the accepted Phase 2 provenance report | open |  | 2026-08-29T11:11:05.653Z |  |
| 2 | FLOWPDF-02 | deviation | vite.config.ts |  | Adapted the legacy Inspector stylesheet reference for Vite's HTML pipeline | open |  | 2026-08-29T11:11:05.753Z |  |
| 3 | FLOWPDF-02 | deviation | scripts/verify-phase2-dependencies.mjs |  | Kept the admitted dependency lock intent stable after candidate policy-age rollover | open |  | 2026-08-30T18:39:36.236Z |  |
| 4 | FLOWPDF-02 | deviation | package.json |  | Pointed the generic full test command at the project-local Playwright Chromium cache | open |  | 2026-08-30T18:39:36.336Z |  |

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
  },
  {
    "id": 3,
    "kind": "deviation",
    "phase": "FLOWPDF-02",
    "file": "scripts/verify-phase2-dependencies.mjs",
    "line": null,
    "description": "Kept the admitted dependency lock intent stable after candidate policy-age rollover",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-08-30T18:39:36.236Z",
    "resolved_at": null
  },
  {
    "id": 4,
    "kind": "deviation",
    "phase": "FLOWPDF-02",
    "file": "package.json",
    "line": null,
    "description": "Pointed the generic full test command at the project-local Playwright Chromium cache",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-08-30T18:39:36.336Z",
    "resolved_at": null
  }
]
````
