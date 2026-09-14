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
})

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
