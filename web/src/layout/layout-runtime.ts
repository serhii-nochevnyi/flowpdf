import * as generatedWasm from '../../generated/flow_wasm.js'
import { PdfSourceStore } from '../../persistence/pdf-source-store.js'
import type { PdfNativeEditPanelProps } from '../pdf/pdf-native-edit-panel.js'
import type { NativePdfWasmBoundary } from '../pdf/pdf-native-edit-protocol.js'
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
import type { PdfExportRequestDto } from '../pdf/pdf-protocol.js'
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
): Pick<PdfNativeEditPanelProps, 'createCoordinator' | 'sourceStore'> & { dispose(): void } {
  const coordinators = new Set<NativePdfEditCoordinator>()
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
    const coordinator = new NativePdfEditCoordinator(
      worker as unknown as NativePdfWorkerPort, verify_native_pdf_response,
    )
    coordinators.add(coordinator)
    coordinator.subscribe(() => {
      if (coordinator.snapshot().phase === 'invalidated') coordinators.delete(coordinator)
    })
    return coordinator
  }

  let initial: NativePdfEditCoordinator | undefined
  try {
    initial = createCoordinator()
  } catch (error) {
    if (!(error instanceof NativePdfWorkerError)) throw error
  }
  return {
    sourceStore,
    createCoordinator() {
      const coordinator = initial
      initial = undefined
      return coordinator?.snapshot().phase === 'idle' ? coordinator : createCoordinator()
    },
    dispose() {
      if (disposed) return
      disposed = true
      initial = undefined
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
  const layoutWorker = (
    workerConstructor === undefined
      ? new Worker(new URL('./layout-worker-entry.ts', import.meta.url), { type: 'module' })
      : new workerConstructor(new URL('./layout-worker-entry.ts', import.meta.url), { type: 'module' })
  ) as unknown as LayoutWorkerPort
  const pdfWorker = (
    workerConstructor === undefined
      ? new Worker(new URL('../pdf/pdf-worker-entry.ts', import.meta.url), { type: 'module' })
      : new workerConstructor(new URL('../pdf/pdf-worker-entry.ts', import.meta.url), { type: 'module' })
  ) as unknown as PdfWorkerPort
  const pdfReaderWorker = (
    workerConstructor === undefined
      ? new Worker(new URL('../pdf/pdf-reader-worker-entry.ts', import.meta.url), { type: 'module' })
      : new workerConstructor(new URL('../pdf/pdf-reader-worker-entry.ts', import.meta.url), { type: 'module' })
  ) as unknown as PdfReaderWorkerPort
  const pdfReconstructionReaderWorker = (
    workerConstructor === undefined
      ? new Worker(new URL('../pdf/pdf-reader-worker-entry.ts', import.meta.url), { type: 'module' })
      : new workerConstructor(new URL('../pdf/pdf-reader-worker-entry.ts', import.meta.url), { type: 'module' })
  ) as unknown as PdfReaderWorkerPort
  const pdfReconstructionWorker = (
    workerConstructor === undefined
      ? new Worker(new URL('../pdf/pdf-reconstruction-worker-entry.ts', import.meta.url), { type: 'module' })
      : new workerConstructor(new URL('../pdf/pdf-reconstruction-worker-entry.ts', import.meta.url), { type: 'module' })
  ) as unknown as PdfReconstructionWorkerPort
  const layoutScheduler = createWorkerLayoutScheduler(layoutWorker)
  const pdfExportScheduler = createWorkerPdfExportScheduler(pdfWorker)
  const pdfReaderScheduler = createWorkerPdfReaderScheduler(pdfReaderWorker)
  const pdfReconstructionReaderScheduler = createWorkerPdfReaderScheduler(pdfReconstructionReaderWorker)
  const pdfReconstructionScheduler = createWorkerPdfReconstructionScheduler(
    pdfReconstructionWorker,
  )
  const nativePdf = createNativePdfRuntime(workerConstructor)
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
    dispose() {
      nativePdf.dispose()
      layoutScheduler.dispose()
      pdfExportScheduler.dispose()
      pdfReaderScheduler.dispose()
      pdfReconstructionReaderScheduler.dispose()
      pdfReconstructionScheduler.dispose()
      layoutWorker.terminate()
      pdfWorker.terminate()
      pdfReaderWorker.terminate()
      pdfReconstructionReaderWorker.terminate()
      pdfReconstructionWorker.terminate()
    },
  }
}
