import { createElement } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'

import '../src/styles.css'
import {
  PDF_READER_PROTOCOL_VERSION,
  type PdfReaderWorkerResultDto,
} from '../src/pdf/pdf-protocol.js'
import { PdfReaderPanel } from '../src/pdf/pdf-reader-panel.js'
import { RevisionAwarePdfReaderScheduler } from '../src/pdf/pdf-reader-worker.js'

test('PDF reader panel keeps imported scene visual-only and surfaces diagnostics', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const result = fixtureResult('reader-browser-1')
  const scheduler = new RevisionAwarePdfReaderScheduler(
    async (request) => ({ ...result, requestId: request.requestId }),
    { verifyResultHash: () => true },
  )
  createRoot(root).render(createElement(PdfReaderPanel, { scheduler, locale: 'uk' }))
  await tick()
  await tick()

  const panel = root.querySelector<HTMLElement>('[data-pdf-reader]')
  if (panel === null) throw new Error('reader panel is required')
  const input = panel.querySelector<HTMLInputElement>('[data-pdf-reader-input]')
  if (input === null) throw new Error('reader input is required')
  const file = new File([new Uint8Array([0x25, 0x50, 0x44, 0x46])], 'fixture.pdf', {
    type: 'application/pdf',
  })
  const transfer = new DataTransfer()
  transfer.items.add(file)
  Object.defineProperty(input, 'files', { configurable: true, value: transfer.files })
  input.dispatchEvent(new Event('change', { bubbles: true }))
  await tick()
  await tick()
  await tick()
  await tick()

  expect(panel.querySelector('[data-pdf-reader-file]')?.textContent).toContain('fixture.pdf')
  expect(panel.querySelector('[data-pdf-reader-page="0"]')).not.toBeNull()
  expect(panel.querySelectorAll('.pdf-reader-text')).toHaveLength(1)
  expect(panel.querySelector('.pdf-reader-page')?.getAttribute('aria-hidden')).toBe('true')
  expect(panel.querySelector('[data-pdf-reader-diagnostic="FLOW_PDF_READER_UNSUPPORTED_OPERATOR"]')).not.toBeNull()
  expect(panel.querySelector('[data-editor-input-host]')).toBeNull()

  scheduler.dispose()
})

function fixtureResult(requestId: string): PdfReaderWorkerResultDto {
  const report = {
    diagnostics: [
      {
        code: 'FLOW_PDF_READER_UNSUPPORTED_OPERATOR',
        severity: 'warning' as const,
        pageIndex: 0,
        objectNumber: 4,
        operator: 'Do',
      },
    ],
    partial: true,
  }
  return {
    protocolVersion: PDF_READER_PROTOCOL_VERSION,
    requestId,
    summary: {
      schemaVersion: 1,
      pdfVersion: '1.7',
      sourceHash: 'browser-source',
      pageCount: 1,
    },
    scene: {
      schemaVersion: 1,
      sourceHash: 'browser-source',
      pageCount: 1,
      firstPage: 0,
      pages: [
        {
          pageIndex: 0,
          sourceObject: { objectNumber: 3, generation: 0 },
          bounds: { x: 0, y: 0, width: 12_800, height: 12_800 },
          elements: [
            {
              kind: 'text',
              value: {
                text: 'Read-only PDF text',
                rect: { x: 640, y: 10_880, width: 6_400, height: 640 },
                fontName: null,
                mapping: 'toUnicode',
                glyphs: [],
                provenance: { objectRef: { objectNumber: 4, generation: 0 }, streamOffset: 1, operator: 'Tj' },
              },
            },
            {
              kind: 'annotation',
              value: {
                subtype: 'Widget',
                kind: 'widget',
                rect: { x: 640, y: 9_600, width: 2_000, height: 640 },
                provenance: { objectRef: { objectNumber: 5, generation: 0 }, streamOffset: 2, operator: null },
              },
            },
          ],
          report,
          partial: true,
        },
      ],
      report,
      resultHash: 'browser-scene',
    },
    resultHash: 'browser-scene',
  }
}

async function tick(): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
}
