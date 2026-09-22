import { createRoot } from 'react-dom/client'
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'

import { IndexedDbDocumentStore } from '../../persistence/indexeddb-store.js'
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
  loadWasm,
} from './editor-controller.js'
import { EditorToolbar } from './editor-toolbar.js'
import { EditorShell } from './editor-shell.js'
import { SemanticDocument } from './semantic-document.js'
import { PageViewport } from '../layout/page-viewport.js'
import { PdfPreview } from '../pdf/pdf-preview.js'
import { FormSessionCoordinator, requireFormSessionWasm } from '../forms/form-session.js'
import { FormProjectionViewport } from '../forms/form-projection-viewport.js'
import {
  createFormProjectionSelection,
  createFormProjectionRequest,
  createWasmFormProjectionEngine,
  requireFormProjectionWasm,
  RevisionAwareFormProjectionScheduler,
  type FormProjectionRequestDto,
  type FormProjectionResultDto,
  type FormProjectionSelectionDto,
  type FormProjectionScheduler,
} from '../forms/form-projection.js'
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
  FormProjectionScheduler,
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
  readonly formProjectionScheduler?: FormProjectionScheduler
}

export function EditorApp({
  controller: suppliedController,
  options = {},
  formProjectionScheduler: suppliedFormProjectionScheduler,
}: EditorAppProps) {
  const [controller] = useState(
    () => suppliedController ?? new EditorController(options),
  )
  const [formSession] = useState(
    () =>
      new FormSessionCoordinator({
        wasm: loadWasm().then(requireFormSessionWasm),
        persistence: new IndexedDbDocumentStore(options.databaseName),
      }),
  )
  const [formProjectionScheduler] = useState<FormProjectionScheduler>(
    () => suppliedFormProjectionScheduler ?? createDefaultFormProjectionScheduler(),
  )
  const snapshot = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
    controller.getSnapshot,
  )
  const formSessionSnapshot = useSyncExternalStore(
    formSession.subscribe,
    formSession.getSnapshot,
    formSession.getSnapshot,
  )
  const formProjectionSnapshot = useSyncExternalStore(
    (listener) => formProjectionScheduler.subscribe(listener),
    () => formProjectionScheduler.snapshot(),
    () => formProjectionScheduler.snapshot(),
  )
  const [flattenedFieldIds, setFlattenedFieldIds] = useState<readonly string[]>([])
  const acceptedProjection = formProjectionSnapshot.accepted?.result ?? null
  const currentProjectionResult =
    snapshot.accepted !== null &&
    snapshot.layout.accepted !== null &&
    formProjectionSnapshot.phase === 'ready' &&
    acceptedProjection !== null &&
    acceptedProjection.sourceRevision === snapshot.accepted.session.revision &&
    acceptedProjection.sourceHash === snapshot.accepted.session.canonicalHash &&
    formProjectionSnapshot.accepted?.request.sourceRevision ===
      snapshot.accepted.session.revision &&
    formProjectionSnapshot.accepted.request.sourceHash ===
      snapshot.accepted.session.canonicalHash &&
    formProjectionSnapshot.accepted.request.layoutResultHash ===
      snapshot.layout.accepted.result.resultHash &&
    acceptedProjection.layoutResultHash === snapshot.layout.accepted.result.resultHash
      ? acceptedProjection
      : null
  const projectionIdentity = currentProjectionResult === null
    ? 'none'
    : `${currentProjectionResult.sourceRevision}:${currentProjectionResult.sourceHash}:${currentProjectionResult.displayListHash}:${currentProjectionResult.formPlan?.resultHash ?? 'no-plan'}`
  useEffect(() => {
    setFlattenedFieldIds([])
  }, [projectionIdentity])
  const formProjectionSelection: FormProjectionSelectionDto | null = useMemo(
    () => createFormProjectionSelection(currentProjectionResult, flattenedFieldIds),
    [currentProjectionResult, flattenedFieldIds],
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

  useEffect(() => {
    const accepted = snapshot.accepted
    if (accepted === null) return
    void formSession.synchronize(accepted.session.canonicalJson, {
      documentId: accepted.session.documentId,
      sourceRevision: accepted.session.revision,
      sourceHash: accepted.session.canonicalHash,
    })
  }, [
    formSession,
    snapshot.accepted?.session.canonicalHash,
    snapshot.accepted?.session.canonicalJson,
    snapshot.accepted?.session.documentId,
    snapshot.accepted?.session.revision,
  ])

  useEffect(() => {
    const accepted = snapshot.accepted
    const acceptedLayout = snapshot.layout.accepted
    if (accepted === null || acceptedLayout === null) {
      formProjectionScheduler.cancel()
      return
    }
    if (
      acceptedLayout.result.sourceRevision !== accepted.session.revision ||
      acceptedLayout.result.sourceHash !== accepted.session.canonicalHash ||
      acceptedLayout.request.sourceRevision !== accepted.session.revision ||
      acceptedLayout.request.sourceHash !== accepted.session.canonicalHash
    ) {
      formProjectionScheduler.cancel()
      return
    }
    const session = sameFormProjectionIdentity(
      formSessionSnapshot.identity,
      accepted.session,
    )
      ? formSessionSnapshot.session
      : null
    const request = createFormProjectionRequest(
      acceptedLayout,
      `editor-form-projection-${accepted.session.revision}-${acceptedLayout.result.resultHash}-${session?.generation ?? 'defaults'}`,
      session,
    )
    void formProjectionScheduler.request(request)
    return () => formProjectionScheduler.cancel(request.requestId)
  }, [
    formProjectionScheduler,
    formSessionSnapshot.identity,
    formSessionSnapshot.session,
    snapshot.accepted?.session.canonicalHash,
    snapshot.accepted?.session.revision,
    snapshot.layout.accepted?.request.sourceRevision,
    snapshot.layout.accepted?.result.resultHash,
  ])

  useEffect(
    () => () => {
      if (suppliedFormProjectionScheduler === undefined) {
        formProjectionScheduler.dispose?.()
      }
    },
    [formProjectionScheduler, suppliedFormProjectionScheduler],
  )

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
              canonicalJson={accepted.session.canonicalJson}
              formSession={formSession}
              formSessionSnapshot={formSessionSnapshot}
            />
          </section>
          <PageViewport
            layout={snapshot.layout}
            projection={formProjectionSnapshot}
            sourceRevision={accepted.session.revision}
            locale={locale}
          />
          {snapshot.layout.accepted === null ? null : (
            <FormProjectionViewport
              projection={formProjectionSnapshot}
              sourceRevision={accepted.session.revision}
              sourceHash={accepted.session.canonicalHash}
              locale={locale}
              selectedFieldIds={flattenedFieldIds}
              onSelectedFieldIdsChange={setFlattenedFieldIds}
            />
          )}
          {controller.hasPdfExport() ? (
            <PdfPreview
              layout={snapshot.layout}
              pdf={snapshot.pdf}
              sourceRevision={accepted.session.revision}
              sourceHash={accepted.session.canonicalHash}
              sourceBlocks={accepted.editor.view.document.blocks}
              selection={accepted.editor.session.selection}
              locale={locale}
              onExport={() => void controller.requestPdfExport(formProjectionSelection)}
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

function createDefaultFormProjectionScheduler(): FormProjectionScheduler {
  const engine = async (
    request: FormProjectionRequestDto,
    signal: AbortSignal,
  ): Promise<FormProjectionResultDto> => {
    const wasm = await loadWasm().then(requireFormProjectionWasm)
    return createWasmFormProjectionEngine(wasm).run(request, signal)
  }
  return new RevisionAwareFormProjectionScheduler(engine, {
    verifyResultHash: (result) =>
      result.projection.resultHash.trim().length > 0 &&
      (result.formPlan === null || result.formPlan.resultHash.trim().length > 0),
  })
}

function sameFormProjectionIdentity(
  identity: {
    readonly sourceRevision: number
    readonly sourceHash: string
  } | null,
  source: { readonly revision: number; readonly canonicalHash: string },
): boolean {
  return (
    identity !== null &&
    identity.sourceRevision === source.revision &&
    identity.sourceHash === source.canonicalHash
  )
}
