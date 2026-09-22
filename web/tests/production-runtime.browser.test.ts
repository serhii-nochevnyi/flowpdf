import { expect, test } from 'vitest'

import init, {
  apply_command,
  create_sample,
  font_catalog_identity,
  hyphenation_data_identity,
} from '../generated/flow_wasm.js'
import type { EditorAcceptedSnapshot } from '../src/editor/editor-store.js'
import { createDefaultWorkerRuntime } from '../src/layout/layout-runtime.js'
import { loadBundledFontCatalog } from '../src/runtime/font-catalog.js'

interface SampleResponse {
  readonly ok: boolean
  readonly value: {
    readonly session: {
      readonly canonicalJson: string
      readonly canonicalHash: string
      readonly revision: number
      readonly history: unknown
    }
  } | null
  readonly error: { readonly code: string } | null
}

test('production runtime wires the bundled Rust font catalog through both workers', async () => {
  await init()
  const catalog = await loadBundledFontCatalog({ font_catalog_identity, hyphenation_data_identity })
  const runtime = createDefaultWorkerRuntime(catalog)
  try {
    const sample = create_sample({
      requestedLocale: 'en-US',
      issuedAt: '2026-09-22T00:00:00Z',
    }) as SampleResponse
    expect(sample.ok, sample.error?.code).toBe(true)
    if (sample.value === null) throw new Error(sample.error?.code ?? 'sample session missing')
    const sampleDocument = JSON.parse(sample.value.session.canonicalJson) as {
      readonly content: readonly { readonly id: string }[]
    }
    const unsupportedSampleNode = sampleDocument.content[0]
    if (unsupportedSampleNode === undefined) throw new Error('sample paragraph missing')
    const supported = apply_command({
      canonicalJson: sample.value.session.canonicalJson,
      history: sample.value.session.history,
      command: {
        commandId: '00000000-0000-4000-8000-000000000901',
        baseRevision: sample.value.session.revision,
        modality: 'system',
        issuedAt: '2026-09-22T00:00:01Z',
        kind: { type: 'deleteSubtree', nodeId: unsupportedSampleNode.id },
      },
    }) as SampleResponse
    expect(supported.ok, supported.error?.code).toBe(true)
    if (supported.value === null) throw new Error(supported.error?.code ?? 'supported session missing')
    const accepted = { session: supported.value.session } as unknown as EditorAcceptedSnapshot
    const layoutRequest = runtime.layoutRequestFactory(accepted)
    if (layoutRequest === null) throw new Error('runtime layout request is required')
    const layoutOutcome = await runtime.layoutScheduler.request(layoutRequest)
    expect(layoutOutcome, JSON.stringify(layoutOutcome)).toMatchObject({ kind: 'published' })
    if (layoutOutcome.kind !== 'published') throw new Error(`layout failed: ${layoutOutcome.kind}`)
    expect(layoutOutcome.accepted.result.fontCatalogIdentity).toBe(catalog.identity)

    const pdfRequest = runtime.pdfExportRequestFactory(accepted, layoutOutcome.accepted, null)
    if (pdfRequest === null) throw new Error('runtime PDF request is required')
    const pdfOutcome = await runtime.pdfExportScheduler.request(pdfRequest)
    expect(pdfOutcome.kind).toBe('published')
    if (pdfOutcome.kind !== 'published') throw new Error(`PDF failed: ${pdfOutcome.kind}`)
    expect(pdfOutcome.accepted.result.manifest.fontCatalogIdentity).toBe(catalog.identity)
    expect(pdfOutcome.accepted.result.manifest.fontFaces).toEqual([
      catalog.faces[0]?.manifest,
    ])
  } finally {
    runtime.dispose()
  }
})
