# Phase 2 Edge/Windows screen-reader evidence

```json
{
  "schemaVersion": 1,
  "phase": "FLOWPDF-02-accessible-rich-text-editing",
  "owner": "02-17-02",
  "target": "Microsoft Edge on Windows with a Windows screen reader",
  "status": "unavailable",
  "observed": false,
  "closure": "outstanding",
  "environment": {
    "host": "macOS arm64 workspace",
    "browser": "Microsoft Edge on Windows is not installed or available here",
    "screenReader": "Windows screen-reader UAT is not available here",
    "recordedAt": "2026-09-15T00:00:00Z",
    "build": "02-17 working tree before release-gate commit"
  },
  "reason": "This workspace cannot execute the required Edge-on-Windows plus Windows-screen-reader checkpoint.",
  "requiredSteps": [
    "Run the supported build in Microsoft Edge on Windows.",
    "Navigate the semantic document, fields, status, errors, dialogs, and confirmation prompts with the selected Windows screen reader.",
    "Repeat the smoke in Ukrainian and English and attach observed focus and announcement results.",
    "Update this record only with evidence observed on that target platform."
  ],
  "substitutionForbidden": [
    "Chromium on macOS cannot close this checkpoint.",
    "macOS VoiceOver cannot close this checkpoint."
  ]
}
```

This checkpoint remains `unavailable/outstanding`; it is not promoted by local browser or macOS fallback evidence.
