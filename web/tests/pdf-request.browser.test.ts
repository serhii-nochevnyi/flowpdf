import { expect, test } from 'vitest'

import init, {
  create_sample,
  export_pdf,
  verify_pdf_export_response,
} from '../generated/flow_wasm.js'
import type { EditorAcceptedSnapshot } from '../src/editor/editor-store.js'
import type { AcceptedLayoutDto } from '../src/layout/layout-protocol.js'
import { createPdfExportRequest } from '../src/pdf/pdf-request.js'
import { createWasmPdfExportScheduler } from '../src/pdf/pdf-worker.js'

interface SampleSession {
  readonly canonicalJson: string
  readonly canonicalHash: string
  readonly revision: number
}

interface SampleResponse {
  readonly ok: boolean
  readonly value: { readonly session: SampleSession } | null
  readonly error: { readonly code: string } | null
}

test('shared PDF request crosses the generated Rust/WASM scheduler boundary', async () => {
  await init()

  const sample = create_sample({
    requestedLocale: 'uk-UA',
    issuedAt: '2026-09-22T00:00:00Z',
  }) as SampleResponse
  expect(sample.ok, sample.error?.code).toBe(true)
  expect(sample.value).not.toBeNull()
  if (sample.value === null) throw new Error(sample.error?.code ?? 'sample session missing')

  const accepted = {
    session: sample.value.session,
  } as unknown as EditorAcceptedSnapshot
  const layout = acceptedLayout(sample.value.session)
  const request = createPdfExportRequest(accepted, layout, {
    requestId: 'browser-wasm-pdf-request',
  })
  if (request === null) throw new Error('Rust-compatible PDF request is required')

  const scheduler = createWasmPdfExportScheduler({
    export_pdf,
    verify_pdf_export_response,
    recover_owned_source: () => '',
  })
  const outcome = await scheduler.request(request)
  expect(outcome.kind).toBe('published')
  if (outcome.kind !== 'published') throw new Error(`PDF export failed: ${outcome.kind}`)
  expect(outcome.accepted.request.requestId).toBe(request.requestId)
  expect(outcome.accepted.result).toMatchObject({
    sourceRevision: request.sourceRevision,
    sourceHash: request.sourceHash,
    layoutSettingsFingerprint: request.layoutSettingsFingerprint,
    manifest: {
      sourceRevision: request.sourceRevision,
      sourceHash: request.sourceHash,
      layoutSettingsFingerprint: request.layoutSettingsFingerprint,
      layoutResultHash: request.layoutResultHash,
    },
  })
  scheduler.dispose()
})

function acceptedLayout(session: SampleSession): AcceptedLayoutDto {
  const requestId = 'browser-wasm-layout-request'
  const layoutSettingsFingerprint = 'browser-wasm-layout-settings'
  const fontCatalogIdentity = 'browser-wasm-fonts'
  return {
    request: {
      protocolVersion: 1,
      requestId,
      sourceRevision: session.revision,
      sourceHash: session.canonicalHash,
      expectedLayoutSettingsFingerprint: layoutSettingsFingerprint,
      fontCatalogIdentity,
      hyphenationDataIdentity: null,
      viewport: { firstPage: 0, pageCount: 1 },
      serializedRequest: '{}',
    },
    result: {
      protocolVersion: 1,
      requestId,
      sourceRevision: session.revision,
      sourceHash: session.canonicalHash,
      layoutSettingsFingerprint,
      fontCatalogIdentity,
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
      resultHash: 'browser-wasm-layout-result',
    },
  }
}
