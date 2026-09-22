import { describe, expect, it } from 'vitest'

import type { EditorAcceptedSnapshot } from '../src/editor/editor-store.js'
import {
  FORM_PROJECTION_SELECTION_PROTOCOL_VERSION,
  type FormProjectionSelectionDto,
} from '../src/forms/form-projection.js'
import type { AcceptedLayoutDto } from '../src/layout/layout-protocol.js'
import { validatePdfExportRequest } from '../src/pdf/pdf-protocol.js'
import { createPdfExportRequest } from '../src/pdf/pdf-request.js'

const SOURCE_HASH = 'source-hash-4'

describe('Rust-compatible PDF request builder', () => {
  it('builds an ordinary bounded request from accepted source/layout identities', () => {
    const request = createPdfExportRequest(accepted(), layout(), {
      requestId: 'pdf-ordinary',
    })
    if (request === null) throw new Error('ordinary request is required')

    expect(validatePdfExportRequest(request)).toBeNull()
    expect(JSON.parse(request.serializedRequest)).toEqual({
      protocolVersion: 1,
      requestId: 'pdf-ordinary',
      sourceRevision: 4,
      sourceHash: SOURCE_HASH,
      layoutSettingsFingerprint: 'settings-v1',
      layoutResultHash: 'layout-result-4',
      fontCatalogIdentity: 'fonts-v1',
      fontFaces: [],
      hyphenationDataIdentity: null,
      canonicalJson: '{"schemaVersion":1}',
      pages: [{ bounds: { x: 0, y: 0, width: 4_800, height: 6_400 } }],
      metadata: null,
      outlines: [],
      internalLinks: [],
      formPlan: null,
      flattenedFieldIds: [],
    })
  })

  it('carries partial and all-field selection without changing the page payload', () => {
    const partial = createPdfExportRequest(accepted(), layout(), {
      requestId: 'pdf-partial',
      producer: 'FlowPDF test',
      formSelection: selection(['field-a']),
    })
    const all = createPdfExportRequest(accepted(), layout(), {
      requestId: 'pdf-all',
      formSelection: selection(['field-a', 'field-b']),
    })
    if (partial === null || all === null) throw new Error('selection requests are required')

    const partialWire = JSON.parse(partial.serializedRequest) as {
      readonly formPlan: { readonly resultHash: string }
      readonly flattenedFieldIds: readonly string[]
      readonly pages: readonly unknown[]
    }
    const allWire = JSON.parse(all.serializedRequest) as {
      readonly flattenedFieldIds: readonly string[]
      readonly pages: readonly unknown[]
    }
    expect(partialWire.formPlan.resultHash).toBe('plan-4')
    expect(partialWire.flattenedFieldIds).toEqual(['field-a'])
    expect(allWire.flattenedFieldIds).toEqual(['field-a', 'field-b'])
    expect(partialWire.pages).toEqual(allWire.pages)
  })

  it('carries only Rust-verified font manifest identities', () => {
    const request = createPdfExportRequest(accepted(), layout(), {
      requestId: 'pdf-fonts',
      fontFaces: [{ faceId: 'noto-sans', contentHash: 'blake3:font' }],
    })
    if (request === null) throw new Error('font manifest request is required')
    expect(JSON.parse(request.serializedRequest).fontFaces).toEqual([
      { faceId: 'noto-sans', contentHash: 'blake3:font' },
    ])
  })

  it('rejects stale layout and source-bound form selection before serialization', () => {
    expect(
      createPdfExportRequest(accepted(), {
        ...layout(),
        result: { ...layout().result, sourceHash: 'other-source' },
      }, { requestId: 'pdf-stale-layout' }),
    ).toBeNull()

    expect(
      createPdfExportRequest(accepted(), layout(), {
        requestId: 'pdf-stale-selection',
        formSelection: {
          ...selection(['field-a']),
          sourceHash: 'other-source',
          formPlan: { ...selection(['field-a']).formPlan, sourceHash: 'other-source' },
        },
      }),
    ).toBeNull()
  })
})

function accepted(): EditorAcceptedSnapshot {
  return {
    session: {
      canonicalJson: '{"schemaVersion":1}',
      canonicalHash: SOURCE_HASH,
      revision: 4,
    },
  } as unknown as EditorAcceptedSnapshot
}

function layout(): AcceptedLayoutDto {
  return {
    request: {
      protocolVersion: 1,
      requestId: 'layout-4',
      sourceRevision: 4,
      sourceHash: SOURCE_HASH,
      expectedLayoutSettingsFingerprint: 'settings-v1',
      fontCatalogIdentity: 'fonts-v1',
      hyphenationDataIdentity: null,
      viewport: { firstPage: 0, pageCount: 1 },
      serializedRequest: '{}',
    },
    result: {
      protocolVersion: 1,
      requestId: 'layout-4',
      sourceRevision: 4,
      sourceHash: SOURCE_HASH,
      layoutSettingsFingerprint: 'settings-v1',
      fontCatalogIdentity: 'fonts-v1',
      hyphenationDataIdentity: null,
      pages: [
        {
          pageIndex: 0,
          sectionId: 'section-4',
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
      resultHash: 'layout-result-4',
    },
  }
}

function selection(flattenedFieldIds: readonly string[]): FormProjectionSelectionDto {
  const formPlan = {
    schemaVersion: 1,
    sourceRevision: 4,
    sourceHash: SOURCE_HASH,
    displayListHash: 'display-4',
    fields: [planField('field-a', 0), planField('field-b', 1)],
    resultHash: 'plan-4',
  }
  return {
    protocolVersion: FORM_PROJECTION_SELECTION_PROTOCOL_VERSION,
    sourceRevision: 4,
    sourceHash: SOURCE_HASH,
    displayListHash: 'display-4',
    formPlanResultHash: formPlan.resultHash,
    formPlan,
    flattenedFieldIds,
  }
}

function planField(fieldId: string, tabOrder: number) {
  return {
    fieldId,
    widgetId: `widget-${fieldId}`,
    name: fieldId,
    label: fieldId,
    fieldType: 'text' as const,
    flags: 0,
    defaultValue: { type: 'empty' as const },
    value: { type: 'text' as const, value: fieldId },
    tabOrder,
    options: [],
    pageIndex: 0,
    rect: { x: 320, y: 480, width: 800, height: 64 },
  }
}
