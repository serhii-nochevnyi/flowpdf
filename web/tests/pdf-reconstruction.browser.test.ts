import { createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { expect, test } from 'vitest'

import '../src/styles.css'
import { PdfSourceStore } from '../persistence/pdf-source-store.js'
import { PdfReconstructionPanel } from '../src/pdf/pdf-reconstruction-panel.js'
import {
  PDF_READER_PROTOCOL_VERSION,
  type PdfReaderSceneDto,
  type PdfReaderWorkerResultDto,
} from '../src/pdf/pdf-protocol.js'
import {
  PDF_RECONSTRUCTION_PROTOCOL_VERSION,
  type PdfDocumentCandidateDto,
  type PdfReconstructionRequestDto,
  type PdfReconstructionWorkerResultDto,
} from '../src/pdf/pdf-reconstruction-protocol.js'
import { RevisionAwarePdfReaderScheduler } from '../src/pdf/pdf-reader-worker.js'
import { RevisionAwarePdfReconstructionScheduler } from '../src/pdf/pdf-reconstruction-worker.js'

const SOURCE_HASH = 'b'.repeat(64)

test('PDF reconstruction stores the source, shows report-first review, and accepts explicitly', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const reactRoot: Root = createRoot(root)
  const sourceStore = new PdfSourceStore(`flowpdf-pdf-reconstruction-${crypto.randomUUID()}`)
  const readerScheduler = new RevisionAwarePdfReaderScheduler(
    async (request) => readerResult(request.requestId, false),
    { verifyResultHash: () => true },
  )
  const reconstructionScheduler = new RevisionAwarePdfReconstructionScheduler(
    async (request) => reconstructionResult(request, false),
    async (request) => ({ candidate: request.result.candidate }),
    { verifyResultHash: () => true, verifyAcceptedCandidate: () => true },
  )

  reactRoot.render(
    createElement(PdfReconstructionPanel, {
      readerScheduler,
      reconstructionScheduler,
      sourceStore,
      locale: 'uk',
    }),
  )
  await settle()

  const panel = root.querySelector<HTMLElement>('[data-pdf-reconstruction]')
  if (panel === null) throw new Error('reconstruction panel is required')
  const input = panel.querySelector<HTMLInputElement>('[data-pdf-reconstruction-input]')
  if (input === null) throw new Error('reconstruction input is required')
  dispatchFile(input, new File([new Uint8Array([0x25, 0x50, 0x44, 0x46])], 'review.pdf', {
    type: 'application/pdf',
  }))
  await settle()

  expect(panel.querySelector('[data-pdf-reconstruction-file]')?.textContent).toContain('review.pdf')
  expect(panel.querySelector('[data-pdf-reconstruction-identities]')?.textContent).toContain(
    'sha256:v1:',
  )
  expect(panel.querySelector('[data-pdf-reconstruction-identities]')?.textContent).toContain(
    SOURCE_HASH,
  )
  expect(panel.querySelector('[data-pdf-reconstruction-report]')).not.toBeNull()
  expect(panel.querySelector('[data-pdf-reconstruction-block]')?.textContent).toContain('Пункт для review')
  expect(panel.querySelector('[data-pdf-reconstruction-opaque]')?.textContent).toContain('path')
  expect(panel.querySelector('[data-pdf-reconstruction-page="0"]')?.getAttribute('aria-hidden')).toBe(
    'true',
  )

  const accept = panel.querySelector<HTMLButtonElement>('[data-pdf-reconstruction-accept]')
  if (accept === null) throw new Error('accept button is required')
  expect(accept.disabled).toBe(true)
  expect(panel.querySelector('[data-pdf-reconstruction-accept-blocked]')).not.toBeNull()

  const keep = panel.querySelector<HTMLInputElement>('[data-pdf-reconstruction-review] input')
  if (keep === null) throw new Error('review decision control is required')
  keep.click()
  await settle()
  expect(accept.disabled).toBe(false)

  accept.click()
  await settle()
  expect(panel.querySelector('[data-pdf-reconstruction-accepted]')?.textContent).toContain('best-effort')
  expect(panel.querySelector('[data-editor-input-host]')).toBeNull()

  reactRoot.unmount()
  readerScheduler.dispose()
  reconstructionScheduler.dispose()
  sourceStore.close()
})

test('PDF reconstruction reports unavailable OCR without weakening the original scene boundary', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const reactRoot: Root = createRoot(root)
  const sourceStore = new PdfSourceStore(`flowpdf-pdf-reconstruction-ocr-${crypto.randomUUID()}`)
  const readerScheduler = new RevisionAwarePdfReaderScheduler(
    async (request) => readerResult(request.requestId, true),
    { verifyResultHash: () => true },
  )
  const reconstructionScheduler = new RevisionAwarePdfReconstructionScheduler(
    async (request) => reconstructionResult(request, true),
    async (request) => ({ candidate: request.result.candidate }),
    { verifyResultHash: () => true, verifyAcceptedCandidate: () => true },
  )

  reactRoot.render(
    createElement(PdfReconstructionPanel, {
      readerScheduler,
      reconstructionScheduler,
      sourceStore,
      locale: 'en',
    }),
  )
  await settle()

  const panel = root.querySelector<HTMLElement>('[data-pdf-reconstruction]')
  if (panel === null) throw new Error('reconstruction panel is required')
  const input = panel.querySelector<HTMLInputElement>('[data-pdf-reconstruction-input]')
  if (input === null) throw new Error('reconstruction input is required')
  dispatchFile(input, new File([new Uint8Array([0x25, 0x50, 0x44, 0x46])], 'scan.pdf', {
    type: 'application/pdf',
  }))
  await settle()

  const ocr = panel.querySelector<HTMLButtonElement>('[data-pdf-reconstruction-ocr]')
  if (ocr === null) throw new Error('OCR button is required')
  expect(ocr.disabled).toBe(false)
  ocr.click()
  await settle()

  expect(panel.querySelector('[data-pdf-reconstruction-status]')?.textContent).toContain(
    'FLOW_PDF_RECONSTRUCTION_OCR_UNAVAILABLE',
  )
  expect(panel.querySelector('[data-pdf-reconstruction-page="0"]')?.getAttribute('aria-hidden')).toBe(
    'true',
  )
  expect(panel.querySelector('[data-pdf-reconstruction-accepted]')).toBeNull()

  reactRoot.unmount()
  readerScheduler.dispose()
  reconstructionScheduler.dispose()
  sourceStore.close()
})

function dispatchFile(input: HTMLInputElement, file: File): void {
  const transfer = new DataTransfer()
  transfer.items.add(file)
  Object.defineProperty(input, 'files', { configurable: true, value: transfer.files })
  input.dispatchEvent(new Event('change', { bubbles: true }))
}

async function settle(): Promise<void> {
  for (let index = 0; index < 12; index += 1) {
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  }
}

function readerResult(requestId: string, scanned: boolean): PdfReaderWorkerResultDto {
  const report = { diagnostics: [], partial: false } as const
  const pageElements = scanned
    ? [
        {
          kind: 'image' as const,
          value: {
            width: 8,
            height: 8,
            mediaType: 'image/png',
            filter: 'FlateDecode',
            dataHex: '00',
            rect: { x: 640, y: 9_600, width: 4_000, height: 2_000 },
            provenance: {
              objectRef: { objectNumber: 5, generation: 0 },
              streamOffset: 2,
              operator: 'Do',
            },
          },
        },
      ]
    : [
        {
          kind: 'text' as const,
          value: {
            text: 'Оригінальний текст',
            rect: { x: 640, y: 10_880, width: 4_000, height: 640 },
            fontName: null,
            mapping: 'toUnicode' as const,
            glyphs: [],
            provenance: {
              objectRef: { objectNumber: 4, generation: 0 },
              streamOffset: 1,
              operator: 'Tj',
            },
          },
        },
        {
          kind: 'path' as const,
          value: {
            commands: [],
            rect: { x: 640, y: 9_600, width: 2_000, height: 640 },
            stroke: true,
            fill: false,
            provenance: {
              objectRef: { objectNumber: 5, generation: 0 },
              streamOffset: 2,
              operator: 'S',
            },
          },
        },
      ]
  const scene: PdfReaderSceneDto = {
    schemaVersion: 1,
    sourceHash: SOURCE_HASH,
    pageCount: 1,
    firstPage: 0,
    pages: [
      {
        pageIndex: 0,
        sourceObject: { objectNumber: 3, generation: 0 },
        bounds: { x: 0, y: 0, width: 12_800, height: 12_800 },
        elements: pageElements,
        report,
        partial: false,
      },
    ],
    report,
    resultHash: 'reader-scene-result',
  }
  return {
    protocolVersion: PDF_READER_PROTOCOL_VERSION,
    requestId,
    summary: {
      schemaVersion: 1,
      pdfVersion: '1.7',
      sourceHash: SOURCE_HASH,
      pageCount: 1,
    },
    scene,
    resultHash: scene.resultHash,
  }
}

function reconstructionResult(
  request: PdfReconstructionRequestDto,
  scanned: boolean,
): PdfReconstructionWorkerResultDto {
  const document = {
    schemaVersion: 2,
    blocks: [{ nodeId: 'reconstructed-block', text: scanned ? 'OCR draft' : 'Пункт для review' }],
    provenance: {
      kind: 'externalReconstruction',
      sourceHash: `pdf:blake3:v1:${request.sourceHash}`,
      reconstructionSchemaVersion: 1,
    },
  }
  const candidate: PdfDocumentCandidateDto = {
    document,
    canonicalJson: JSON.stringify(document),
    canonicalHash: 'flowpdf:blake3:v1:browser-candidate',
  }
  const reviewRequired = !scanned || request.ocrCandidates.length === 0
  return {
    protocolVersion: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
    requestId: request.requestId,
    schemaVersion: 1,
    sourceHash: request.sourceHash,
    sceneResultHash: request.scene.resultHash,
    candidate,
    blocks: reviewRequired
      ? [
          {
            nodeId: 'reconstructed-block',
            text: scanned ? 'OCR draft' : 'Пункт для review',
            rect: { x: 640, y: 10_880, width: 4_000, height: 640 },
            confidenceBasisPoints: scanned ? 4_000 : 7_500,
            reviewRequired: true,
            mappings: [],
          },
        ]
      : [],
    opaqueIslands: scanned
      ? []
      : [
          {
            pageIndex: 0,
            elementIndex: 1,
            rect: { x: 640, y: 9_600, width: 2_000, height: 640 },
            kind: 'path',
            objectRef: { objectNumber: 5, generation: 0 },
            operator: 'S',
          },
        ],
    report: {
      readerReport: { diagnostics: [], partial: false },
      diagnostics: [],
      reviewRequiredCount: reviewRequired ? 1 : 0,
      partial: false,
    },
    resultHash: 'reconstruction-result',
  }
}
