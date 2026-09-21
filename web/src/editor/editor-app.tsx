import { createRoot } from 'react-dom/client'
import { useEffect, useRef, useState, useSyncExternalStore } from 'react'

import {
  mountFoundationInspector,
  type FoundationInspectorLocale,
} from '../foundation-inspector.js'
import {
  copy,
  EditorController,
  type FormattingCommandDto,
  type EditorAppOptions,
  type EditorLayoutScheduler,
  type EditorPdfExportScheduler,
  type SourceModality,
  type StructuralCommandDto,
} from './editor-controller.js'
import { EditorToolbar } from './editor-toolbar.js'
import { EditorShell } from './editor-shell.js'
import { SemanticDocument } from './semantic-document.js'
import { PageViewport } from '../layout/page-viewport.js'
import { PdfPreview } from '../pdf/pdf-preview.js'
import type {
  DirectionalSelectionDto,
  EditorAcceptedSnapshot,
  EditorAppPhase,
  EditorAppSnapshot,
  EditorBlockViewDto,
  EditorCapabilityDto,
  EditorDocumentViewDto,
  EditorLocale,
  EditorSessionResponseDto,
  EditorSessionStateDto,
  EditorTextSpanDto,
  EditorViewDto,
  InspectorViewDto,
  SessionDto,
} from './editor-store.js'

export { EditorController }
export { copy as editorCopy }
export type {
  EditorLayoutScheduler,
  EditorPdfExportScheduler,
  FormattingCommandDto,
  SourceModality,
  StructuralCommandDto,
}
export type {
  DirectionalSelectionDto,
  EditorAcceptedSnapshot,
  EditorAppOptions,
  EditorAppPhase,
  EditorAppSnapshot,
  EditorBlockViewDto,
  EditorCapabilityDto,
  EditorDocumentViewDto,
  EditorLocale,
  EditorSessionResponseDto,
  EditorSessionStateDto,
  EditorTextSpanDto,
  EditorViewDto,
  InspectorViewDto,
  SessionDto,
}

export type { FoundationInspectorLocale }

export interface EditorAppProps {
  readonly controller?: EditorController
  readonly options?: EditorAppOptions
}

export function EditorApp({ controller: suppliedController, options = {} }: EditorAppProps) {
  const [controller] = useState(
    () => suppliedController ?? new EditorController(options),
  )
  const snapshot = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
    controller.getSnapshot,
  )
  const locale = options.locale ?? 'uk'
  const labels = copy(locale)
  const diagnosticsRoot = useRef<HTMLDivElement>(null)
  const diagnosticsMounted = useRef(false)
  const [diagnosticsError, setDiagnosticsError] = useState(false)
  const [inputError, setInputError] = useState<string | null>(null)

  useEffect(() => {
    void controller.initialize()
    return () => controller.dispose()
  }, [controller])

  useEffect(() => {
    setInputError(null)
  }, [snapshot.accepted?.session.revision])

  const openDiagnostics = (): void => {
    if (diagnosticsMounted.current || diagnosticsRoot.current === null) return
    diagnosticsMounted.current = true
    const inspectorOptions =
      options.databaseName === undefined
        ? { locale }
        : { locale, databaseName: options.databaseName }
    void mountFoundationInspector(diagnosticsRoot.current, inspectorOptions).catch(() =>
      setDiagnosticsError(true),
    )
  }

  const busy = snapshot.phase === 'pending' || snapshot.phase === 'loading'
  const accepted = snapshot.accepted
  const visibleError = snapshot.errorCode ?? inputError
  return (
    <EditorShell
      labels={labels}
      busy={busy}
      phase={snapshot.phase}
      status={snapshot.status}
      visibleError={visibleError}
      diagnosticsRoot={diagnosticsRoot}
      diagnosticsError={diagnosticsError}
      onOpenDiagnostics={openDiagnostics}
    >
      {accepted === null ? (
        <div className="empty-state" data-empty-document="">
          <h2 className="section-heading">{labels.emptyTitle}</h2>
          <p className="secondary-text">{labels.emptyBody}</p>
          <div className="action-group">
            <button
              className="primary-action"
              data-action="editor-create"
              disabled={busy}
              onClick={() => void controller.createSample()}
            >
              {labels.create}
            </button>
            <button
              data-action="editor-open-older"
              disabled={busy}
              onClick={() => void controller.openOlderSchema()}
            >
              {labels.openOlder}
            </button>
            <button
              data-action="editor-open-last"
              disabled={busy}
              onClick={() => void controller.reloadFromStorage()}
            >
              {labels.openLast}
            </button>
          </div>
        </div>
      ) : (
        <>
          <div className="editor-toolbar" aria-label={labels.region}>
            <button
              data-action="editor-replace"
              disabled={busy}
              onClick={() => void controller.replaceSelection()}
            >
              {labels.replace}
            </button>
            <button
              data-action="editor-undo"
              disabled={busy || accepted.session.history.cursor === 0}
              onClick={() => void controller.undo()}
            >
              {labels.undo}
            </button>
            <button
              data-action="editor-redo"
              disabled={
                busy || accepted.session.history.cursor >= accepted.session.history.entries.length
              }
              onClick={() => void controller.redo()}
            >
              {labels.redo}
            </button>
            <button
              data-action="editor-reload"
              disabled={busy}
              onClick={() => void controller.reloadFromStorage()}
            >
              {labels.reload}
            </button>
            <button
              data-action="editor-recover"
              disabled={busy}
              onClick={() => void controller.recoverFromStorage()}
            >
              {labels.recover}
            </button>
          </div>
          <EditorToolbar
            view={accepted.editor.view}
            controller={controller}
            locale={locale}
          />
          <p className="durable-badge">{labels.durable}</p>
          <dl className="editor-metadata">
            <div>
              <dt>{labels.revision}</dt>
              <dd data-editor-revision="">{accepted.session.revision}</dd>
            </div>
            <div>
              <dt>{labels.hash}</dt>
              <dd className="hash-value" data-editor-hash="">
                {accepted.session.canonicalHash}
              </dd>
            </div>
          </dl>
          <section className="editor-document-region" aria-label={labels.region}>
            <SemanticDocument
              view={accepted.editor.view}
              controller={controller}
              locale={locale}
              onInputError={setInputError}
            />
          </section>
          <PageViewport
            layout={snapshot.layout}
            sourceRevision={accepted.session.revision}
            locale={locale}
          />
          {controller.hasPdfExport() ? (
            <PdfPreview
              layout={snapshot.layout}
              pdf={snapshot.pdf}
              sourceRevision={accepted.session.revision}
              sourceHash={accepted.session.canonicalHash}
              sourceBlocks={accepted.editor.view.document.blocks}
              selection={accepted.editor.session.selection}
              locale={locale}
              onExport={() => void controller.requestPdfExport()}
              onSelectSource={(selection) => void controller.setEditorSelection(selection)}
            />
          ) : null}
        </>
      )}
    </EditorShell>
  )
}

export async function mountEditorApp(
  root: HTMLElement,
  options: EditorAppOptions = {},
): Promise<EditorController> {
  const controller = new EditorController(options)
  const reactRoot = createRoot(root)
  reactRoot.render(<EditorApp controller={controller} options={options} />)
  await controller.initialize()
  return controller
}
