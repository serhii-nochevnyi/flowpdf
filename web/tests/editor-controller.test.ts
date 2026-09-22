import { describe, expect, it } from 'vitest'

import {
  EditorController,
  type EditorControllerDependencies,
  type EditorPersistence,
  type WasmBoundary,
} from '../src/editor/editor-controller.js'
import {
  EditorStore,
  type EditorAcceptedSnapshot,
} from '../src/editor/editor-store.js'
import {
  StorageError,
  type RecoveryRecordsDto,
} from '../persistence/indexeddb-store.js'
import {
  MAX_COMPOSITION_UTF16_UNITS,
  MAX_COMPOSITION_UTF8_BYTES,
  MAX_PASTE_UTF8_BYTES,
  validateInputPayload,
} from '../src/editor/input-adapter.js'
import {
  FORM_PROJECTION_SELECTION_PROTOCOL_VERSION,
  type FormProjectionSelectionDto,
} from '../src/forms/form-projection.js'
import type { AcceptedLayoutDto } from '../src/layout/layout-protocol.js'
import { PDF_PROTOCOL_VERSION, type PdfExportRequestDto } from '../src/pdf/pdf-protocol.js'
import { createVoiceDictationCapture } from '../src/voice/voice-command.js'
import type { AcceptedVoiceIntentDto } from '../src/voice/voice-protocol.js'

const emptyRecords: RecoveryRecordsDto = {
  snapshots: [],
  transactions: [],
  audits: [],
  assets: [],
  sources: [],
}

describe('editor external store and controller publication', () => {
  it('keeps a cached snapshot identity until an accepted publication replaces it', () => {
    const store = new EditorStore('loading')
    const loading = store.getSnapshot()
    let notifications = 0
    const unsubscribe = store.subscribe(() => {
      notifications += 1
    })

    expect(store.getSnapshot()).toBe(loading)
    store.publishEmpty()
    const empty = store.getSnapshot()
    expect(store.getSnapshot()).toBe(empty)
    expect(empty.accepted).toBeNull()

    store.publishError('FLOW_STORAGE_WRITE_FAILED')
    const rejected = store.getSnapshot()
    expect(store.getSnapshot()).toBe(rejected)
    expect(rejected.accepted).toBeNull()
    expect(notifications).toBe(2)
    unsubscribe()
    store.publishPending('pending')
    expect(notifications).toBe(2)
  })

  it('retains the exact accepted projection when persistence rejects a command', async () => {
    const accepted = acceptedFixture()
    const stateStore = new EditorStore('loading')
    stateStore.publishAccepted(accepted, 'ready')

    const requests: unknown[] = []
    const wasm = wasmFixture(requests)
    const persistence: EditorPersistence = {
      async commit() {
        throw new StorageError('FLOW_STORAGE_WRITE_FAILED')
      },
      async commitMigration() {
        throw new Error('not used')
      },
      async commitStandaloneAudit() {
        throw new Error('not used')
      },
      async loadRecords() {
        return emptyRecords
      },
      async loadRecoveryImage() {
        return { records: emptyRecords, head: null }
      },
      async installRecoveredHead() {
        throw new Error('not used')
      },
    }
    const dependencies: EditorControllerDependencies = {
      documentStore: persistence,
      store: stateStore,
      wasm: Promise.resolve(wasm),
    }
    const controller = new EditorController({}, dependencies)

    await controller.replaceText('новий текст', accepted.editor.session.selection)
    await controller.whenIdle()

    const snapshot = controller.snapshot()
    expect(snapshot.phase).toBe('error')
    expect(snapshot.errorCode).toBe('FLOW_STORAGE_WRITE_FAILED')
    expect(snapshot.accepted).toBe(accepted)
    expect(requests).toHaveLength(1)
    expect(requests[0]).toMatchObject({
      command: {
        baseRevision: 1,
        kind: {
          type: 'replaceSelection',
          text: 'новий текст',
          selection: accepted.editor.session.selection,
        },
      },
    })
  })

  it('keeps input limits exact and rejects malformed UTF-16 before command dispatch', () => {
    expect(
      validateInputPayload('é'.repeat(MAX_COMPOSITION_UTF8_BYTES / 2), 'composition'),
    ).toBeNull()
    expect(
      validateInputPayload('a'.repeat(MAX_COMPOSITION_UTF8_BYTES + 1), 'composition'),
    ).toBe('FLOW_COMPOSITION_LIMIT')
    expect(
      validateInputPayload('a'.repeat(MAX_COMPOSITION_UTF16_UNITS), 'composition'),
    ).toBeNull()
    expect(
      validateInputPayload('a'.repeat(MAX_COMPOSITION_UTF16_UNITS + 1), 'composition'),
    ).toBe('FLOW_COMPOSITION_LIMIT')
    expect(validateInputPayload('a'.repeat(MAX_PASTE_UTF8_BYTES), 'paste')).toBeNull()
    expect(validateInputPayload('a'.repeat(MAX_PASTE_UTF8_BYTES + 1), 'paste')).toBe(
      'FLOW_PASTE_LIMIT',
    )
    const malformed = String.fromCharCode(0xd800)
    expect(validateInputPayload(malformed, 'paste')).toBe('FLOW_INVALID_UTF16_BOUNDARY')
  })

  it('commits one final dictation as one ordinary voice transaction', async () => {
    const accepted = acceptedFixture()
    const stateStore = new EditorStore('loading')
    stateStore.publishAccepted(accepted, 'ready')
    const requests: unknown[] = []
    const controller = new EditorController({}, {
      store: stateStore,
      wasm: Promise.resolve(successfulVoiceWasm(requests, accepted)),
      documentStore: successfulPersistence(),
    })

    const outcome = await controller.dispatchVoiceDictation(
      createVoiceDictationCapture(accepted, 'один фінальний фрагмент'),
    )
    await controller.whenIdle()

    expect(outcome).toMatchObject({ kind: 'committed', revision: 1 })
    expect(requests).toHaveLength(1)
    expect(requests[0]).toMatchObject({
      command: {
        modality: 'voice',
        baseRevision: 1,
        kind: {
          type: 'replaceSelection',
          text: 'один фінальний фрагмент',
          selection: accepted.editor.session.selection,
        },
      },
    })
    expect(controller.snapshot().voice).toEqual({
      phase: 'committed',
      revision: 1,
      errorCode: null,
    })
  })

  it('rejects a dictation captured before revision or selection drift without dispatch', async () => {
    const accepted = acceptedFixture()
    const stateStore = new EditorStore('loading')
    stateStore.publishAccepted(accepted, 'ready')
    const requests: unknown[] = []
    const controller = new EditorController({}, {
      store: stateStore,
      wasm: Promise.resolve(successfulVoiceWasm(requests, accepted)),
      documentStore: successfulPersistence(),
    })
    const capture = createVoiceDictationCapture(accepted, 'stale speech')

    stateStore.publishAccepted(
      {
        ...accepted,
        session: { ...accepted.session, revision: 2, canonicalHash: 'hash-2' },
      },
      'ready',
    )
    await expect(controller.dispatchVoiceDictation(capture)).resolves.toEqual({
      kind: 'rejected',
      code: 'FLOW_VOICE_SOURCE_STALE',
    })
    expect(requests).toHaveLength(0)

    stateStore.publishAccepted(accepted, 'ready')
    const selectionDrift = {
      ...accepted.editor.session.selection,
      focus: {
        ...accepted.editor.session.selection.focus,
        utf16Offset: 1,
      },
    }
    stateStore.publishAccepted(
      {
        ...accepted,
        editor: {
          ...accepted.editor,
          session: { ...accepted.editor.session, selection: selectionDrift },
        },
      },
      'ready',
    )
    await expect(controller.dispatchVoiceDictation(capture)).resolves.toEqual({
      kind: 'rejected',
      code: 'FLOW_VOICE_SELECTION_STALE',
    })
    expect(requests).toHaveLength(0)
  })

  it('dispatches a validated formatting intent through the voice command bus', async () => {
    const accepted = acceptedFixture()
    const stateStore = new EditorStore('loading')
    stateStore.publishAccepted(accepted, 'ready')
    const requests: unknown[] = []
    const controller = new EditorController({}, {
      store: stateStore,
      wasm: Promise.resolve(successfulVoiceWasm(requests, accepted)),
      documentStore: successfulPersistence(),
    })
    const intent: AcceptedVoiceIntentDto = {
      protocolVersion: 1,
      locale: 'uk-UA',
      sourceRevision: accepted.session.revision,
      sourceHash: accepted.session.canonicalHash,
      selection: accepted.editor.session.selection,
      action: { type: 'setInlineMark', mark: { kind: 'bold', value: true } },
      capability: {
        family: 'setInlineMark',
        commandType: 'setInlineMark',
        intent: 'editor.intent.setInlineMark',
        risk: 'formatting',
        confirmation: 'none',
        undo: 'reversible',
      },
    }

    await expect(controller.dispatchVoiceCommand(intent)).resolves.toMatchObject({
      kind: 'committed',
    })
    expect(requests).toHaveLength(1)
    expect(requests[0]).toMatchObject({
      command: {
        modality: 'voice',
        kind: {
          type: 'setInlineMark',
          selection: accepted.editor.session.selection,
          mark: { kind: 'bold', value: true },
        },
      },
    })
  })

  it('passes a valid form selection to export and fences a stale source selection', async () => {
    const stateStore = new EditorStore('loading')
    stateStore.publishAccepted(acceptedFixture(), 'ready')
    const acceptedLayout = layoutFixture()
    let seenSelection: FormProjectionSelectionDto | null | undefined
    let scheduled = 0
    const layoutScheduler: NonNullable<EditorControllerDependencies['layoutScheduler']> = {
      request: async (request) => ({
        kind: 'failed' as const,
        requestId: request.requestId,
        code: 'TEST_LAYOUT_UNUSED',
      }),
      cancel: () => undefined,
      accepted: () => acceptedLayout,
      snapshot: () => ({
        phase: 'ready',
        accepted: acceptedLayout,
        requestId: null,
        errorCode: null,
      }),
    }
    const pdfScheduler: NonNullable<EditorControllerDependencies['pdfExportScheduler']> = {
      request: async (request) => {
        scheduled += 1
        return { kind: 'failed' as const, requestId: request.requestId, code: 'TEST_PDF_UNUSED' }
      },
      cancel: () => undefined,
      accepted: () => null,
      snapshot: () => ({ phase: 'idle', accepted: null, requestId: null, errorCode: null }),
    }
    const selection = selectionFixture()
    const controller = new EditorController({}, {
      store: stateStore,
      wasm: Promise.resolve(wasmFixture([])),
      layoutScheduler,
      pdfExportScheduler: pdfScheduler,
      pdfExportRequestFactory: (accepted, layout, formSelection) => {
        seenSelection = formSelection
        const request: PdfExportRequestDto = {
          protocolVersion: PDF_PROTOCOL_VERSION,
          requestId: 'pdf-selection-test',
          sourceRevision: accepted.session.revision,
          sourceHash: accepted.session.canonicalHash,
          layoutSettingsFingerprint: 'settings-v1',
          layoutResultHash: layout.result.resultHash,
          serializedRequest: '{}',
        }
        return request
      },
    })

    expect(await controller.requestPdfExport(selection)).toMatchObject({
      kind: 'failed',
      code: 'TEST_PDF_UNUSED',
    })
    expect(seenSelection).toBe(selection)
    expect(scheduled).toBe(1)

    const staleSelection: FormProjectionSelectionDto = {
      ...selection,
      sourceHash: 'stale-source',
      formPlan: { ...selection.formPlan, sourceHash: 'stale-source' },
    }
    expect(await controller.requestPdfExport(staleSelection)).toMatchObject({
      kind: 'failed',
      code: 'FLOW_PDF_FORM_SELECTION_SOURCE_MISMATCH',
    })
    expect(scheduled).toBe(1)
  })

  it('uses the shared request builder when export has no custom factory', async () => {
    const stateStore = new EditorStore('loading')
    stateStore.publishAccepted(acceptedFixture(), 'ready')
    const acceptedLayout = layoutFixture()
    const seenRequests: PdfExportRequestDto[] = []
    const layoutScheduler: NonNullable<EditorControllerDependencies['layoutScheduler']> = {
      request: async (request) => ({
        kind: 'failed' as const,
        requestId: request.requestId,
        code: 'TEST_LAYOUT_UNUSED',
      }),
      cancel: () => undefined,
      accepted: () => acceptedLayout,
      snapshot: () => ({
        phase: 'ready',
        accepted: acceptedLayout,
        requestId: null,
        errorCode: null,
      }),
    }
    const pdfScheduler: NonNullable<EditorControllerDependencies['pdfExportScheduler']> = {
      request: async (request) => {
        seenRequests.push(request)
        return { kind: 'failed' as const, requestId: request.requestId, code: 'TEST_PDF_DEFAULT' }
      },
      cancel: () => undefined,
      accepted: () => null,
      snapshot: () => ({ phase: 'idle', accepted: null, requestId: null, errorCode: null }),
    }
    const controller = new EditorController({}, {
      store: stateStore,
      wasm: Promise.resolve(wasmFixture([])),
      layoutScheduler,
      pdfExportScheduler: pdfScheduler,
    })

    expect(controller.hasPdfExport()).toBe(true)
    expect(await controller.requestPdfExport(selectionFixture())).toMatchObject({
      kind: 'failed',
      code: 'TEST_PDF_DEFAULT',
    })
    expect(seenRequests).toHaveLength(1)
    const seenRequest = seenRequests[0] as PdfExportRequestDto
    expect(seenRequest.requestId).toBe('editor-pdf-1')
    expect(seenRequest.sourceRevision).toBe(1)
    expect(seenRequest.sourceHash).toBe('hash-1')
    expect(seenRequest.layoutResultHash).toBe('layout-selection-result')
    const serialized = JSON.parse(seenRequest.serializedRequest) as {
      readonly formPlan: { readonly resultHash: string } | null
      readonly flattenedFieldIds: readonly string[]
      readonly pages: readonly unknown[]
    }
    expect(serialized.formPlan?.resultHash).toBe('plan-selection')
    expect(serialized.flattenedFieldIds).toEqual([])
    expect(serialized.pages).toHaveLength(1)
  })
})

function layoutFixture(): AcceptedLayoutDto {
  return {
    request: {
      protocolVersion: 1,
      requestId: 'layout-selection-test',
      sourceRevision: 1,
      sourceHash: 'hash-1',
      expectedLayoutSettingsFingerprint: 'settings-v1',
      fontCatalogIdentity: 'fonts-v1',
      hyphenationDataIdentity: null,
      viewport: { firstPage: 0, pageCount: 1 },
      serializedRequest: '{}',
    },
    result: {
      protocolVersion: 1,
      requestId: 'layout-selection-test',
      sourceRevision: 1,
      sourceHash: 'hash-1',
      layoutSettingsFingerprint: 'settings-v1',
      fontCatalogIdentity: 'fonts-v1',
      hyphenationDataIdentity: null,
      pages: [
        {
          pageIndex: 0,
          sectionId: '00000000-0000-4000-8000-000000000701',
          pageSettings: {},
          bounds: { x: 0, y: 0, width: 4_800, height: 6_400 },
          contentRect: { x: 320, y: 320, width: 4_160, height: 5_760 },
          startReason: 'documentStart',
          header: null,
          footer: null,
          fragments: [],
        },
      ],
      diagnostics: [],
      resultHash: 'layout-selection-result',
    },
  }
}

function selectionFixture(): FormProjectionSelectionDto {
  return {
    protocolVersion: FORM_PROJECTION_SELECTION_PROTOCOL_VERSION,
    sourceRevision: 1,
    sourceHash: 'hash-1',
    displayListHash: 'display-selection',
    formPlanResultHash: 'plan-selection',
    formPlan: {
      schemaVersion: 1,
      sourceRevision: 1,
      sourceHash: 'hash-1',
      displayListHash: 'display-selection',
      fields: [],
      resultHash: 'plan-selection',
    },
    flattenedFieldIds: [],
  }
}

function acceptedFixture(): EditorAcceptedSnapshot {
  const anchor = {
    nodeId: '00000000-0000-4000-8000-000000000001',
    utf16Offset: 5,
    affinity: 'backward' as const,
  }
  const focus = {
    nodeId: anchor.nodeId,
    utf16Offset: 0,
    affinity: 'forward' as const,
  }
  const selection = { anchor, focus }
  const editorView = {
    documentId: '00000000-0000-4000-8000-000000000010',
    revision: 1,
    sessionGeneration: 0,
    selection,
    pendingMarks: {
      bold: false,
      italic: false,
      underline: false,
      fontFamily: null,
      fontSizeMillipoints: null,
      color: null,
      language: null,
    },
    formatting: {
      bold: 'off',
      italic: 'off',
      underline: 'off',
      fontFamily: null,
      fontSizeMillipoints: null,
      color: null,
      language: null,
      blockStyle: { kind: 'paragraph' },
      alignment: 'start',
      spacingBeforeMillipoints: 0,
      spacingAfterMillipoints: 0,
      listKind: null,
      listItemId: null,
    },
    capabilities: [],
    document: {
      documentId: '00000000-0000-4000-8000-000000000010',
      revision: 1,
      blocks: [
        {
          kind: 'paragraph' as const,
          nodeId: anchor.nodeId,
          text: 'Текст',
          spans: [{ startUtf16: 0, endUtf16: 5 }],
        },
      ],
      fields: [],
      fieldReview: [],
    },
  } as const
  return {
    session: {
      canonicalJson: '{}',
      canonicalHash: 'hash-1',
      documentId: editorView.documentId,
      revision: 1,
      nextCommandTarget: focus,
      history: { entries: [], cursor: 0, seenCommandIds: [] },
    },
    editor: {
      session: {
        documentId: editorView.documentId,
        revision: 1,
        sessionGeneration: 0,
        selection,
        pendingMarks: {
          bold: false,
          italic: false,
          underline: false,
          fontFamily: null,
          fontSizeMillipoints: null,
          color: null,
          language: null,
        },
        formatting: {
          bold: 'off',
          italic: 'off',
          underline: 'off',
          fontFamily: null,
          fontSizeMillipoints: null,
          color: null,
          language: null,
          blockStyle: { kind: 'paragraph' },
          alignment: 'start',
          spacingBeforeMillipoints: 0,
          spacingAfterMillipoints: 0,
          listKind: null,
          listItemId: null,
        },
        capabilities: [],
      },
      view: editorView,
    },
    view: {
      documentId: editorView.documentId,
      schemaVersion: 2,
      revision: 1,
      canonicalHash: 'hash-1',
      locale: 'uk-UA',
      contentNodeCount: 1,
      fieldCount: 0,
      assetCount: 0,
      revisionProvenance: {
        documentId: editorView.documentId,
        revision: 1,
        schemaVersion: 2,
        canonicalHash: 'hash-1',
        lineage: {},
        engine: {},
        sourceHashes: [],
        previewExportProvenance: '',
      },
      audit: [],
    },
  }
}

function wasmFixture(requests: unknown[]): WasmBoundary {
  const result = {
    session: {
      canonicalJson: '{}',
      canonicalHash: 'hash-2',
      documentId: '00000000-0000-4000-8000-000000000010',
      revision: 2,
      nextCommandTarget: null,
      history: { entries: [], cursor: 1, seenCommandIds: [] },
    },
    view: {
      documentId: '00000000-0000-4000-8000-000000000010',
      schemaVersion: 2,
      revision: 2,
      canonicalHash: 'hash-2',
      locale: 'uk-UA',
      contentNodeCount: 1,
      fieldCount: 0,
      assetCount: 0,
      revisionProvenance: {
        documentId: '00000000-0000-4000-8000-000000000010',
        revision: 2,
        schemaVersion: 2,
        canonicalHash: 'hash-2',
        lineage: {},
        engine: {},
        sourceHashes: [],
        previewExportProvenance: '',
      },
      audit: [],
    },
    editor: acceptedFixture().editor,
    commit: {
      replaceExisting: false,
      snapshot: null,
      transaction: {},
      audit: {},
      assets: [],
    },
  }
  return {
    default: async () => undefined,
    create_sample: () => ({ ok: false, value: null, error: null }),
    apply_command: (request: unknown) => {
      requests.push(request)
      return { ok: true, value: result, error: null }
    },
    open_document: () => ({ ok: false, value: null, error: null }),
    commit_record: () => ({
      ok: true,
      value: {
        replaceExisting: false,
        snapshot: null,
        transaction: {},
        audit: {},
        assets: [],
      },
      error: null,
    }),
    plan_standalone_audit: () => ({ ok: false, value: null, error: null }),
    query_document: () => ({ ok: false, value: null, error: null }),
    query_editor_view: () => ({ ok: false, value: null, error: null }),
    recover_document_audited: () => ({ ok: false, value: null, error: null }),
  } as unknown as WasmBoundary
}

function successfulPersistence(): EditorPersistence {
  return {
    commit: async () => undefined,
    commitMigration: async () => undefined,
    commitStandaloneAudit: async () => undefined,
    loadRecords: async () => emptyRecords,
    loadRecoveryImage: async () => ({ records: emptyRecords, head: null }),
    installRecoveredHead: async () => undefined,
  }
}

function successfulVoiceWasm(
  requests: unknown[],
  accepted: EditorAcceptedSnapshot,
): WasmBoundary {
  const operation = {
    session: accepted.session,
    view: accepted.view,
    editor: accepted.editor,
    commit: {
      replaceExisting: false,
      snapshot: null,
      transaction: {},
      audit: {},
      assets: [],
    },
  }
  return {
    default: async () => undefined,
    create_sample: () => ({ ok: false, value: null, error: null }),
    apply_command: (request: unknown) => {
      requests.push(request)
      return { ok: true, value: operation, error: null }
    },
    open_document: () => ({ ok: false, value: null, error: null }),
    commit_record: () => ({
      ok: true,
      value: {
        replaceExisting: false,
        snapshot: null,
        transaction: {},
        audit: {},
        assets: [],
      },
      error: null,
    }),
    plan_standalone_audit: () => ({ ok: false, value: null, error: null }),
    query_document: () => ({
      ok: true,
      value: {
        session: accepted.session,
        view: accepted.view,
        editor: accepted.editor,
      },
      error: null,
    }),
    query_editor_view: () => ({ ok: true, value: accepted.editor.view, error: null }),
    recover_document_audited: () => ({ ok: false, value: null, error: null }),
  } as unknown as WasmBoundary
}
