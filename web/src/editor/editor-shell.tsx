import type { ReactNode, RefObject } from 'react'

import type { EditorAppPhase } from './editor-store.js'
import type { copy } from './editor-controller.js'

export interface EditorShellProps {
  readonly labels: ReturnType<typeof copy>
  readonly busy: boolean
  readonly children: ReactNode
  readonly phase: EditorAppPhase
  readonly status: string
  readonly visibleError: string | null
  readonly diagnosticsRoot: RefObject<HTMLDivElement | null>
  readonly diagnosticsError: boolean
  readonly onOpenDiagnostics: () => void
}

export function EditorShell({
  labels,
  busy,
  children,
  phase,
  status,
  visibleError,
  diagnosticsRoot,
  diagnosticsError,
  onOpenDiagnostics,
}: EditorShellProps) {
  return (
    <div className="editor-shell" aria-busy={busy} data-editor-phase={phase}>
      <header className="editor-app-bar">
        <p className="eyebrow">
          {labels.product} <span className="eyebrow-separator">·</span> {labels.localOnly}
        </p>
        <h1 className="page-title">{labels.title}</h1>
        <p className="secondary-text">{labels.description}</p>
      </header>
      <main className="editor-main">
        <section className="editor-command-panel" aria-labelledby="editor-region-title">
          <h2 id="editor-region-title" className="visually-hidden">
            {labels.region}
          </h2>
          {children}
          <p
            className="status-region"
            role="status"
            data-editor-status=""
            aria-live="polite"
            aria-atomic="true"
          >
            {status}
          </p>
          {visibleError === null ? null : (
            <p className="alert-region" role="alert" aria-atomic="true" data-editor-error="">
              {labels.error}: {visibleError}
            </p>
          )}
        </section>
        <details className="diagnostics-panel" onToggle={onOpenDiagnostics}>
          <summary>{labels.diagnostics}</summary>
          <div ref={diagnosticsRoot} />
          {diagnosticsError ? (
            <p className="alert-region" role="alert" aria-atomic="true">
              {labels.diagnosticsError}
            </p>
          ) : null}
        </details>
      </main>
    </div>
  )
}
