import * as generatedWasm from '../../generated/flow_wasm.js'
import { PdfSourceStore } from '../../persistence/pdf-source-store.js'
import type { EditorNativePdfRuntime } from '../editor/editor-app.js'
import { nativeBytesHex, type NativePdfWasmBoundary } from '../pdf/pdf-native-edit-protocol.js'
import { NativePdfEditCoordinator, NativePdfWorkerError, type NativePdfWorkerPort } from '../pdf/pdf-native-edit-worker.js'
import type { EditorAcceptedSnapshot } from '../editor/editor-store.js'
import type { EditorControllerDependencies } from '../editor/editor-controller.js'
import {
  LAYOUT_PROTOCOL_VERSION,
  type LayoutRequestDto,
} from './layout-protocol.js'
import {
  createWorkerLayoutScheduler,
  type LayoutWorkerPort,
  type RevisionAwareLayoutScheduler,
} from './layout-worker.js'
import {
  createPdfExportRequest,
} from '../pdf/pdf-request.js'
import {
  createWorkerPdfExportScheduler,
  type PdfWorkerPort,
  type RevisionAwarePdfExportScheduler,
} from '../pdf/pdf-worker.js'
import { PDF_READER_PROTOCOL_VERSION, type PdfExportRequestDto } from '../pdf/pdf-protocol.js'
import {
  createWorkerPdfReaderScheduler,
  type PdfReaderWorkerPort,
  type RevisionAwarePdfReaderScheduler,
} from '../pdf/pdf-reader-worker.js'
import {
  createWorkerPdfReconstructionScheduler,
  type PdfReconstructionWorkerPort,
  type RevisionAwarePdfReconstructionScheduler,
} from '../pdf/pdf-reconstruction-worker.js'
import type { RuntimeFontCatalog } from '../runtime/font-catalog.js'

export type WorkerConstructor = typeof Worker

export function createNativePdfRuntime(
  workerConstructor?: WorkerConstructor,
  sourceStore = new PdfSourceStore(),
): EditorNativePdfRuntime & { dispose(): void } {
  const coordinators = new Set<NativePdfEditCoordinator>()
  const pageReads = new Set<() => void>()
  let pageReadNumber = 0
  let disposed = false
  function createCoordinator() {
    if (disposed) throw new NativePdfWorkerError('FLOW_PDF_NATIVE_WORKER_FAILED')
    const { verify_native_pdf_response } = generatedWasm as Partial<NativePdfWasmBoundary>
    if (typeof verify_native_pdf_response !== 'function') {
      throw new NativePdfWorkerError('FLOW_PDF_NATIVE_WORKER_STARTUP')
    }
    let worker: Worker
    try {
      worker = workerConstructor === undefined
        ? new Worker(new URL('../pdf/pdf-native-edit-worker-entry.ts', import.meta.url), { type: 'module' })
        : new workerConstructor(new URL('../pdf/pdf-native-edit-worker-entry.ts', import.meta.url), { type: 'module' })
    } catch {
      throw new NativePdfWorkerError('FLOW_PDF_NATIVE_WORKER_STARTUP')
    }
    let coordinator: NativePdfEditCoordinator | undefined
    try {
      const acquired = new NativePdfEditCoordinator(
        worker as unknown as NativePdfWorkerPort, verify_native_pdf_response,
      )
      coordinator = acquired
      acquired.subscribe(() => {
        if (acquired.snapshot().phase === 'invalidated') coordinators.delete(acquired)
      })
      coordinators.add(acquired)
      return acquired
    } catch (error) {
      if (coordinator === undefined) worker.terminate()
      else coordinator.dispose()
      throw error
    }
  }

  let initial: NativePdfEditCoordinator | undefined
  try {
    initial = createCoordinator()
  } catch (error) {
    if (!(error instanceof NativePdfWorkerError)) {
      sourceStore.close()
      throw error
    }
  }
  return {
    sourceStore,
    createCoordinator() {
      const coordinator = initial
      initial = undefined
      return coordinator?.snapshot().phase === 'idle' ? coordinator : createCoordinator()
    },
    async extractPageText(snapshot, pageId, signal) {
      if (disposed || signal.aborted) return null
      const firstPage = snapshot.scene.pages.findIndex((page) => page.id === pageId)
      const page = snapshot.scene.pages[firstPage]
      if (!page) return null
      const outputHash = nativeBytesHex(Uint8Array.from(snapshot.outputHash))
      // @security: A request-local worker can stop parsing immediately and drops its retained scene.
      const worker = (
        workerConstructor === undefined
          ? new Worker(new URL('../pdf/pdf-reader-worker-entry.ts', import.meta.url), { type: 'module' })
          : new workerConstructor(new URL('../pdf/pdf-reader-worker-entry.ts', import.meta.url), { type: 'module' })
      ) as unknown as PdfReaderWorkerPort
      let scheduler: RevisionAwarePdfReaderScheduler | undefined
      const stop = () => {
        if (!pageReads.delete(stop)) return
        try {
          signal.removeEventListener('abort', stop)
          scheduler?.dispose()
        } finally {
          worker.terminate()
        }
      }
      pageReads.add(stop)
      try {
        scheduler = createWorkerPdfReaderScheduler(worker)
        signal.addEventListener('abort', stop, { once: true })
        const outcome = await scheduler.request({
          protocolVersion: PDF_READER_PROTOCOL_VERSION,
          requestId: `native-page-text-${++pageReadNumber}`,
          bytesHex: snapshot.outputHex,
          firstPage,
          pageCount: 1,
        })
        if (!pageReads.has(stop) || outcome.kind !== 'published') return null
        const { scene } = outcome.accepted.result
        const readPage = scene.pages[0]
        // @invariant: The reader hashes the input bytes and resolves the page from that PDF's page tree.
        if (scene.sourceHash !== outputHash || scene.pageCount !== snapshot.scene.pages.length
          || !readPage || readPage.pageIndex !== firstPage
          || readPage.sourceObject.objectNumber !== page.object.objectNumber
          || readPage.sourceObject.generation !== page.object.generation) return null
        let partial = readPage.partial || scene.report.partial
        const text: { value: string }[] = []
        for (const element of readPage.elements) {
          if (element.kind !== 'text') continue
          const run = element.value
          if (run.mapping === 'missing') partial = true
          if (!run.text.trim()) continue
          const value = run.mapping === 'missing'
            ? run.glyphs.map((glyph) => glyph.unicode ?? '\uFFFD').join('')
            : run.text
          text.push({ value })
        }
        return { outputHash, pageId, text, partial }
      } finally {
        stop()
      }
    },
    dispose() {
      if (disposed) return
      disposed = true
      initial = undefined
      for (const stop of pageReads) stop()
      for (const coordinator of coordinators) coordinator.dispose()
      sourceStore.close()
    },
  }
}

export interface DefaultWorkerRuntime {
  readonly nativePdf: ReturnType<typeof createNativePdfRuntime>
  readonly fontCatalog: RuntimeFontCatalog
  readonly layoutScheduler: RevisionAwareLayoutScheduler
  readonly pdfExportScheduler: RevisionAwarePdfExportScheduler
  readonly pdfReaderScheduler: RevisionAwarePdfReaderScheduler
  readonly pdfReconstructionReaderScheduler: RevisionAwarePdfReaderScheduler
  readonly pdfReconstructionScheduler: RevisionAwarePdfReconstructionScheduler
  readonly layoutRequestFactory: NonNullable<EditorControllerDependencies['layoutRequestFactory']>
  readonly pdfExportRequestFactory: NonNullable<EditorControllerDependencies['pdfExportRequestFactory']>
  dispose(): void
}

/** Composes the production workers after the Rust-owned font catalog is ready. */
export function createDefaultWorkerRuntime(
  fontCatalog: RuntimeFontCatalog,
  workerConstructor?: WorkerConstructor,
): DefaultWorkerRuntime {
  let layoutRequestNumber = 0
  const layoutRequestFactory = (accepted: EditorAcceptedSnapshot): LayoutRequestDto => {
    layoutRequestNumber += 1
    const requestId = `editor-layout-${layoutRequestNumber}`
    return {
      protocolVersion: LAYOUT_PROTOCOL_VERSION,
      requestId,
      sourceRevision: accepted.session.revision,
      sourceHash: accepted.session.canonicalHash,
      expectedLayoutSettingsFingerprint: null,
      fontCatalogIdentity: fontCatalog.identity,
      hyphenationDataIdentity: fontCatalog.hyphenation.identity,
      viewport: { firstPage: 0, pageCount: 256 },
      serializedRequest: JSON.stringify({
        schemaVersion: LAYOUT_PROTOCOL_VERSION,
        requestId,
        canonicalJson: accepted.session.canonicalJson,
        sourceRevision: accepted.session.revision,
        sourceHash: accepted.session.canonicalHash,
        maxPages: 2048,
        viewport: { firstPage: 0, pageCount: 256 },
        fontCatalogIdentity: fontCatalog.identity,
        hyphenationDataIdentity: fontCatalog.hyphenation.identity,
        expectedLayoutSettingsFingerprint: null,
        fonts: fontCatalog.faces.map((face) => ({
          id: face.id,
          family: face.family,
          faceIndex: face.faceIndex,
          bytes: Array.from(face.bytes),
        })),
        ukrainianHyphenation: {
          identity: fontCatalog.hyphenation.identity,
          bytes: Array.from(fontCatalog.hyphenation.bytes),
        },
      }),
    }
  }
  let pdfRequestNumber = 0
  const pdfExportRequestFactory = (
    accepted: EditorAcceptedSnapshot,
    layout: Parameters<NonNullable<EditorControllerDependencies['pdfExportRequestFactory']>>[1],
    formSelection: Parameters<NonNullable<EditorControllerDependencies['pdfExportRequestFactory']>>[2],
  ): PdfExportRequestDto | null => {
    pdfRequestNumber += 1
    return createPdfExportRequest(accepted, layout, {
      requestId: `editor-pdf-${pdfRequestNumber}`,
      formSelection,
      fontFaces: fontCatalog.faces.map((face) => face.manifest),
    })
  }

  const cleanup: (() => void)[] = []
  function dispose() {
    for (const release of cleanup.splice(0).reverse()) release()
  }

  try {
    const layoutWorker = (
      workerConstructor === undefined
        ? new Worker(new URL('./layout-worker-entry.ts', import.meta.url), { type: 'module' })
        : new workerConstructor(new URL('./layout-worker-entry.ts', import.meta.url), { type: 'module' })
    ) as unknown as LayoutWorkerPort
    cleanup.push(() => layoutWorker.terminate())
    const pdfWorker = (
      workerConstructor === undefined
        ? new Worker(new URL('../pdf/pdf-worker-entry.ts', import.meta.url), { type: 'module' })
        : new workerConstructor(new URL('../pdf/pdf-worker-entry.ts', import.meta.url), { type: 'module' })
    ) as unknown as PdfWorkerPort
    cleanup.push(() => pdfWorker.terminate())
    const pdfReaderWorker = (
      workerConstructor === undefined
        ? new Worker(new URL('../pdf/pdf-reader-worker-entry.ts', import.meta.url), { type: 'module' })
        : new workerConstructor(new URL('../pdf/pdf-reader-worker-entry.ts', import.meta.url), { type: 'module' })
    ) as unknown as PdfReaderWorkerPort
    cleanup.push(() => pdfReaderWorker.terminate())
    const pdfReconstructionReaderWorker = (
      workerConstructor === undefined
        ? new Worker(new URL('../pdf/pdf-reader-worker-entry.ts', import.meta.url), { type: 'module' })
        : new workerConstructor(new URL('../pdf/pdf-reader-worker-entry.ts', import.meta.url), { type: 'module' })
    ) as unknown as PdfReaderWorkerPort
    cleanup.push(() => pdfReconstructionReaderWorker.terminate())
    const pdfReconstructionWorker = (
      workerConstructor === undefined
        ? new Worker(new URL('../pdf/pdf-reconstruction-worker-entry.ts', import.meta.url), { type: 'module' })
        : new workerConstructor(new URL('../pdf/pdf-reconstruction-worker-entry.ts', import.meta.url), { type: 'module' })
    ) as unknown as PdfReconstructionWorkerPort
    cleanup.push(() => pdfReconstructionWorker.terminate())
    const layoutScheduler = createWorkerLayoutScheduler(layoutWorker)
    cleanup.push(() => layoutScheduler.dispose())
    const pdfExportScheduler = createWorkerPdfExportScheduler(pdfWorker)
    cleanup.push(() => pdfExportScheduler.dispose())
    const pdfReaderScheduler = createWorkerPdfReaderScheduler(pdfReaderWorker)
    cleanup.push(() => pdfReaderScheduler.dispose())
    const pdfReconstructionReaderScheduler = createWorkerPdfReaderScheduler(pdfReconstructionReaderWorker)
    cleanup.push(() => pdfReconstructionReaderScheduler.dispose())
    const pdfReconstructionScheduler = createWorkerPdfReconstructionScheduler(
      pdfReconstructionWorker,
    )
    cleanup.push(() => pdfReconstructionScheduler.dispose())
    const nativePdf = createNativePdfRuntime(workerConstructor)
    cleanup.push(() => nativePdf.dispose())
    return {
      fontCatalog,
      nativePdf,
      layoutScheduler,
      pdfExportScheduler,
      pdfReaderScheduler,
      pdfReconstructionReaderScheduler,
      pdfReconstructionScheduler,
      layoutRequestFactory,
      pdfExportRequestFactory,
      dispose,
    }
  } catch (error) {
    dispose()
    throw error
  }
}
