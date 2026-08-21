# Deferred Items — FLOWPDF-01 Durable Flow Foundation

## DEVSEC-01 — Refresh the pinned Vitest Browser Mode family

- **Detected:** 2026-08-21 during Plan 01-06 clean `npm ci --ignore-scripts` verification.
- **Evidence:** `npm audit --json` reports critical advisories `GHSA-p63j-vcc4-9vmv` and `GHSA-g8mr-85jm-7xhm` affecting the pinned `vitest`, `@vitest/browser`, and `@vitest/browser-playwright` 4.1.6 development graph. The registry reports a non-major fix in the 4.1.11 family.
- **Current exposure:** Development/test-only real-browser tooling on the local loopback workflow; FlowPDF has no Phase 1 production server or remote runtime dependency on Vitest. Do not expose Browser Mode to an untrusted network.
- **Why deferred:** Plan 01-01 requires exact versions, registry integrity, repository identity, and recorded provenance. Silently upgrading packages during Plan 01-06 would bypass that supply-chain contract.
- **Required remediation:** Verify the fixed family through `config/dependency-provenance.json` and the official provenance workflow, update all coupled Vitest packages and `package-lock.json` together, then rerun `npm ci --ignore-scripts`, `npm audit`, and `npm run check`.
- **Status:** Open; security maintenance required before any non-local Browser Mode exposure.
