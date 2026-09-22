import { page } from 'vitest/browser'
import { expect, test } from 'vitest'

import '../src/styles.css'
import { mountEditorApp, type EditorController } from '../src/editor/editor-app.js'
import type { EditorBlockViewDto } from '../src/editor/editor-store.js'
import type {
  LayoutRequestDto,
  LayoutWorkerResultDto,
} from '../src/layout/layout-protocol.js'
import { RevisionAwareLayoutScheduler } from '../src/layout/layout-worker.js'
import {
  PDF_PROTOCOL_VERSION,
  type PdfExportRequestDto,
  type PdfExportWorkerResultDto,
  validatePdfExportRequest,
} from '../src/pdf/pdf-protocol.js'
import { RevisionAwarePdfExportScheduler } from '../src/pdf/pdf-worker.js'

test('PDF preview stays visual-only, searchable, virtualized, and revision-safe', async () => {
  await page.viewport(1280, 1000)
  const root = document.createElement('div')
  document.body.replaceChildren(root)

  let requestNumber = 0
  let sourceNodeId = ''
  let documentId = ''
  let lastPdfRequest: PdfExportRequestDto | null = null
  const pendingPdf = new Map<string, { readonly resolve: (result: PdfExportWorkerResultDto) => void }>()
  const layoutScheduler = new RevisionAwareLayoutScheduler(
    async (request) => layoutResult(request, sourceNodeId),
    { verifyResultHash: () => true },
  )
  const pdfScheduler = new RevisionAwarePdfExportScheduler(
    async (request) => {
      lastPdfRequest = request
      const pending = deferred<PdfExportWorkerResultDto>()
      pendingPdf.set(request.requestId, { resolve: pending.resolve })
      return pending.promise
    },
    { verifyResultHash: () => true },
  )
  const controller = await mountEditorApp(
    root,
    {
      databaseName: `flowpdf-pdf-preview-${crypto.randomUUID()}`,
      locale: 'uk',
      clock: () => new Date('2026-09-21T00:00:00Z'),
    },
    {
      layoutScheduler,
      layoutRequestFactory: (accepted) => {
        const firstText = findFirstTextBlock(accepted.editor.view.document.blocks)
        if (firstText === null) return null
        sourceNodeId = firstText.nodeId
        documentId = accepted.session.documentId
        return {
          protocolVersion: 1,
          requestId: `browser-layout-${++requestNumber}`,
          sourceRevision: accepted.session.revision,
          sourceHash: accepted.session.canonicalHash,
          expectedLayoutSettingsFingerprint: null,
          fontCatalogIdentity: 'browser-fixture-fonts',
          hyphenationDataIdentity: null,
          viewport: { firstPage: 0, pageCount: 5 },
          serializedRequest: accepted.session.canonicalJson,
        }
      },
      pdfExportScheduler: pdfScheduler,
    },
  )
  await controller.initialize()
  await tick()

  root.querySelector<HTMLButtonElement>('[data-action="editor-open-older"]')?.click()
  await settle(controller)
  await tick()

  const accepted = controller.snapshot().accepted
  if (accepted === null) throw new Error('accepted editor state is required')
  const firstText = findFirstTextBlock(accepted.editor.view.document.blocks)
  if (firstText === null || firstText.text === undefined || firstText.text.length < 2) {
    throw new Error('searchable text block is required')
  }

  const preview = root.querySelector<HTMLElement>('[data-pdf-preview]')
  if (preview === null) throw new Error('PDF preview is required')
  expect(root.querySelectorAll('[data-editor-document]')).toHaveLength(1)
  expect(preview.querySelector('[data-editor-input-host]')).toBeNull()
  expect(preview.querySelectorAll('[data-pdf-preview-page][aria-hidden="true"]')).toHaveLength(2)
  expect(preview.querySelectorAll('[data-pdf-preview-page-slot]')).toHaveLength(5)
  expect(preview.querySelectorAll('[data-text-block]')).toHaveLength(0)
  expect(preview.dataset.pdfZoom).toBe('1')

  preview.querySelector<HTMLButtonElement>('[data-pdf-zoom-in]')?.click()
  await tick()
  expect(preview.dataset.pdfZoom).toBe('1.25')
  preview.querySelector<HTMLButtonElement>('[data-pdf-page-next]')?.click()
  await tick()
  expect(preview.querySelector('[data-pdf-page-position]')?.textContent).toContain('2 / 5')

  const term = firstText.text.slice(0, 2)
  const search = preview.querySelector<HTMLInputElement>('[data-pdf-search]')
  if (search === null) throw new Error('PDF search input is required')
  setInputValue(search, term)
  await tick()
  const searchResult = preview.querySelector<HTMLButtonElement>('[data-pdf-search-result]')
  if (searchResult === null) throw new Error('source-backed search result is required')
  searchResult.click()
  await settle(controller)
  const selected = controller.snapshot().accepted?.editor.session.selection
  expect(selected?.anchor.nodeId).toBe(firstText.nodeId)
  expect(selected?.focus.nodeId).toBe(firstText.nodeId)
  expect(selected?.focus.utf16Offset).toBeGreaterThan(selected?.anchor.utf16Offset ?? -1)

  const semanticInput = root.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')
  semanticInput?.focus()
  expect(document.activeElement).toBe(semanticInput)

  preview.querySelector<HTMLButtonElement>('[data-pdf-export]')?.click()
  await tick()
  expect(preview.dataset.pdfPhase).toBe('pending')
  const oldRequestId = [...pendingPdf.keys()][0]
  if (oldRequestId === undefined) throw new Error('pending PDF request is required')
  const oldRequest = pdfScheduler.snapshot().requestId
  expect(oldRequest).toBe(oldRequestId)
  if (lastPdfRequest === null) throw new Error('default PDF request is required')
  const capturedPdfRequest = lastPdfRequest as PdfExportRequestDto
  expect(validatePdfExportRequest(capturedPdfRequest)).toBeNull()
  expect(capturedPdfRequest.sourceRevision).toBe(accepted.session.revision)
  expect(capturedPdfRequest.sourceHash).toBe(accepted.session.canonicalHash)
  expect(capturedPdfRequest.layoutResultHash).toBe(
    controller.snapshot().layout.accepted?.result.resultHash,
  )
  const serializedRequest = JSON.parse(capturedPdfRequest.serializedRequest) as {
    readonly pages: readonly unknown[]
  }
  expect(serializedRequest.pages).toHaveLength(5)

  await controller.replaceSelection()
  await settle(controller)
  expect(controller.snapshot().accepted?.session.revision).toBeGreaterThan(accepted.session.revision)
  expect(root.querySelector('[data-pdf-download]')).toBeNull()
  expect(root.querySelector('[data-pdf-preview]')?.getAttribute('data-pdf-phase')).toBe('idle')
  expect(root.querySelector('[data-editor-input-host]')).not.toBeNull()

  const oldResult = pdfResult(capturedPdfRequest, documentId)
  pendingPdf.get(oldRequestId)?.resolve(oldResult)
  await tick()
  expect(root.querySelector('[data-pdf-download]')).toBeNull()

  controller.dispose()
})

function layoutResult(request: LayoutRequestDto, nodeId: string): LayoutWorkerResultDto {
  return {
    protocolVersion: 1,
    requestId: request.requestId,
    sourceRevision: request.sourceRevision,
    sourceHash: request.sourceHash,
    layoutSettingsFingerprint: 'browser-settings',
    fontCatalogIdentity: request.fontCatalogIdentity,
    hyphenationDataIdentity: request.hyphenationDataIdentity,
    pages: Array.from({ length: 5 }, (_, pageIndex) => ({
      pageIndex,
      sectionId: '00000000-0000-4000-8000-000000000701',
      pageSettings: {},
      bounds: { x: 0, y: 0, width: 4_800, height: 6_400 },
      contentRect: { x: 320, y: 320, width: 4_160, height: 5_760 },
      startReason: pageIndex === 0 ? 'documentStart' : 'natural',
      header: null,
      footer: null,
      fragments: [
        {
          id: `fragment-${request.sourceRevision}-${pageIndex}`,
          kind: 'paragraph',
          sourceNodeId: nodeId,
          source: {
            utf8Start: 0,
            utf8End: 64,
            utf16Start: 0,
            utf16End: 64,
          },
          rect: { x: 0, y: 0, width: 3_000, height: 200 },
          breakReason: null,
          derived: false,
          repeatIndex: 0,
          children: [],
        },
      ],
    })),
    diagnostics: [],
    resultHash: `layout-result-${request.sourceRevision}`,
  }
}

function pdfResult(
  request: PdfExportRequestDto,
  documentId: string,
): PdfExportWorkerResultDto {
  return {
    protocolVersion: PDF_PROTOCOL_VERSION,
    requestId: request.requestId,
    sourceRevision: request.sourceRevision,
    sourceHash: request.sourceHash,
    layoutSettingsFingerprint: request.layoutSettingsFingerprint,
    exportFingerprint: `export-${request.sourceRevision}`,
    byteHash: `byte-${request.sourceRevision}`,
    bytesHex: '255044462d312e37',
    privateSourceStreamHex: '464c4f572d534f55524345',
    manifest: {
      manifestVersion: 1,
      exportSchemaVersion: 1,
      sourceDocumentId: documentId,
      sourceRevision: request.sourceRevision,
      sourceSchemaVersion: 2,
      sourceHash: request.sourceHash,
      sourcePayloadHash: 'payload-hash',
      layoutSettingsFingerprint: request.layoutSettingsFingerprint,
      layoutResultHash: request.layoutResultHash,
      engine: 'flow-core',
      engineVersion: '0.1.0',
      fontCatalogIdentity: 'browser-fixture-fonts',
      fontFaces: [],
      hyphenationIdentity: null,
      options: {},
      exportFingerprint: `export-${request.sourceRevision}`,
    },
    supportReport: { supported: ['text'], unsupported: [] },
  }
}

function findFirstTextBlock(
  blocks: readonly EditorBlockViewDto[],
): { readonly nodeId: string; readonly text?: string } | null {
  for (const block of blocks) {
    if (typeof block.text === 'string' && block.text.length > 0) return block
    const child = findFirstTextBlock(block.children ?? [])
    if (child !== null) return child
  }
  return null
}

function deferred<T>(): {
  readonly promise: Promise<T>
  readonly resolve: (value: T) => void
} {
  let resolvePromise: (value: T) => void = () => undefined
  const promise = new Promise<T>((resolve) => {
    resolvePromise = resolve
  })
  return { promise, resolve: resolvePromise }
}

function setInputValue(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
  setter?.call(input, value)
  input.dispatchEvent(new Event('input', { bubbles: true }))
  input.dispatchEvent(new Event('change', { bubbles: true }))
}

async function settle(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await tick()
}

async function tick(): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
}
