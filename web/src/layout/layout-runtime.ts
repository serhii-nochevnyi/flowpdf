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
import type { RuntimeFontCatalog } from '../runtime/font-catalog.js'

export type WorkerConstructor = typeof Worker

export interface DefaultWorkerRuntime {
  readonly fontCatalog: RuntimeFontCatalog
  readonly layoutScheduler: RevisionAwareLayoutScheduler
  readonly pdfExportScheduler: RevisionAwarePdfExportScheduler
  readonly pdfReaderScheduler: RevisionAwarePdfReaderScheduler
  readonly layoutRequestFactory: NonNullable<EditorControllerDependencies['layoutRequestFactory']>
  readonly pdfExportRequestFactory: NonNullable<EditorControllerDependencies['pdfExportRequestFactory']>
  dispose(): void
}

/** Composes the production workers after the Rust-owned font catalog is ready. */
export function createDefaultWorkerRuntime(
  fontCatalog: RuntimeFontCatalog,
  workerConstructor: WorkerConstructor = Worker,
): DefaultWorkerRuntime {
  const layoutWorker = new workerConstructor(
    new URL('./layout-worker-entry.ts', import.meta.url),
    { type: 'module' },
  ) as unknown as LayoutWorkerPort
  const pdfWorker = new workerConstructor(
    new URL('../pdf/pdf-worker-entry.ts', import.meta.url),
    { type: 'module' },
  ) as unknown as PdfWorkerPort
  const pdfReaderWorker = new workerConstructor(
    new URL('../pdf/pdf-reader-worker-entry.ts', import.meta.url),
    { type: 'module' },
  ) as unknown as PdfReaderWorkerPort
  const layoutScheduler = createWorkerLayoutScheduler(layoutWorker)
  const pdfExportScheduler = createWorkerPdfExportScheduler(pdfWorker)
  const pdfReaderScheduler = createWorkerPdfReaderScheduler(pdfReaderWorker)
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
    layoutScheduler,
    pdfExportScheduler,
    pdfReaderScheduler,
    layoutRequestFactory,
    pdfExportRequestFactory,
    dispose() {
      layoutScheduler.dispose()
      pdfExportScheduler.dispose()
      pdfReaderScheduler.dispose()
      layoutWorker.terminate()
      pdfWorker.terminate()
      pdfReaderWorker.terminate()
    },
  }
}
