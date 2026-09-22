import { describe, expect, it } from 'vitest'

import type { FormSessionStateDto } from '../persistence/indexeddb-store.js'
import {
  FORM_PROJECTION_PROTOCOL_VERSION,
  FormProjectionWorkerError,
  RevisionAwareFormProjectionScheduler,
  createFormProjectionSelection,
  createFormProjectionRequest,
  createWasmFormProjectionEngine,
  validateFormProjectionSelection,
  type FormProjectionRequestDto,
  type FormProjectionResultDto,
} from '../src/forms/form-projection.js'
import {
  LAYOUT_PROTOCOL_VERSION,
  type AcceptedLayoutDto,
  type LayoutRequestDto,
} from '../src/layout/layout-protocol.js'

function layoutRequest(revision: number, id = `layout-${revision}`): LayoutRequestDto {
  return {
    protocolVersion: LAYOUT_PROTOCOL_VERSION,
    requestId: id,
    sourceRevision: revision,
    sourceHash: `source-${revision}`,
    expectedLayoutSettingsFingerprint: 'settings-v1',
    fontCatalogIdentity: 'fonts-v1',
    hyphenationDataIdentity: null,
    viewport: { firstPage: 0, pageCount: 2 },
    serializedRequest: '{}',
  }
}

function acceptedLayout(revision = 1): AcceptedLayoutDto {
  const request = layoutRequest(revision)
  return {
    request,
    result: {
      protocolVersion: LAYOUT_PROTOCOL_VERSION,
      requestId: request.requestId,
      sourceRevision: revision,
      sourceHash: `source-${revision}`,
      layoutSettingsFingerprint: 'settings-v1',
      fontCatalogIdentity: 'fonts-v1',
      hyphenationDataIdentity: null,
      pages: [],
      diagnostics: [],
      resultHash: `layout-result-${revision}`,
    },
  }
}

function session(revision = 1): FormSessionStateDto {
  return {
    schemaVersion: 1,
    documentId: '00000000-0000-4000-8000-000000000001',
    sourceRevision: revision,
    sourceHash: `source-${revision}`,
    generation: 1,
    overrides: {
      name: { type: 'text', value: 'Alice' },
    },
  }
}

function request(revision = 1, id = `projection-${revision}`): FormProjectionRequestDto {
  return createFormProjectionRequest(acceptedLayout(revision), id, session(revision))
}

function result(
  sourceRequest: FormProjectionRequestDto,
  overrides: Partial<FormProjectionResultDto> = {},
): FormProjectionResultDto {
  return {
    protocolVersion: FORM_PROJECTION_PROTOCOL_VERSION,
    requestId: sourceRequest.requestId,
    sourceRevision: sourceRequest.sourceRevision,
    sourceHash: sourceRequest.sourceHash,
    layoutResultHash: sourceRequest.layoutResultHash,
    displayListHash: 'display-list-1',
    projection: {
      schemaVersion: 2,
      sourceRevision: sourceRequest.sourceRevision,
      sourceHash: sourceRequest.sourceHash,
      displayListHash: 'display-list-1',
      widgets: [
        {
          widgetId: 'widget-1',
          fieldId: 'field-1',
          name: 'name',
          label: 'Name',
          kind: { type: 'text', multiline: false, inputHint: 'plain' },
          required: false,
          readOnly: false,
          defaultValue: { type: 'empty' },
          value: { type: 'text', value: 'Alice' },
          pageIndex: 0,
          rect: { x: 64, y: 128, width: 256, height: 32 },
          sourceNodeId: 'node-1',
          anchorOffsetUtf16: 4,
          tabOrder: 0,
        },
      ],
      review: [],
      resultHash: 'projection-hash-1',
    },
    formPlan: {
      schemaVersion: 1,
      sourceRevision: sourceRequest.sourceRevision,
      sourceHash: sourceRequest.sourceHash,
      displayListHash: 'display-list-1',
      fields: [],
      resultHash: 'plan-hash-1',
    },
    ...overrides,
  }
}

function rustResponse(value: FormProjectionResultDto): string {
  return JSON.stringify({
    schemaVersion: FORM_PROJECTION_PROTOCOL_VERSION,
    requestId: value.requestId,
    ok: true,
    sourceRevision: value.sourceRevision,
    sourceHash: value.sourceHash,
    layoutResultHash: value.layoutResultHash,
    displayListHash: value.displayListHash,
    resultHash: 'envelope-hash-1',
    result: {
      sourceRevision: value.sourceRevision,
      sourceHash: value.sourceHash,
      layoutResultHash: value.layoutResultHash,
      displayListHash: value.displayListHash,
      projection: value.projection,
      formPlan: value.formPlan,
    },
    error: null,
  })
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

describe('Rust/WASM form projection browser boundary', () => {
  it('normalizes only verified plan fields into a source-bound flatten selection', () => {
    const sourceRequest = request()
    const base = result(sourceRequest)
    if (base.formPlan === null) throw new Error('fixture plan is required')
    const projectionResult = result(sourceRequest, {
      formPlan: {
        ...base.formPlan,
        fields: [planField('field-1', 0), planField('field-2', 1)],
      },
    })

    const selection = createFormProjectionSelection(projectionResult, ['field-2', 'field-1'])
    expect(selection?.flattenedFieldIds).toEqual(['field-1', 'field-2'])
    expect(selection && validateFormProjectionSelection(selection)).toBeNull()
    expect(createFormProjectionSelection(projectionResult, ['missing-field'])).toBeNull()
    expect(createFormProjectionSelection(projectionResult, ['field-1', 'field-1'])).toBeNull()

    if (selection === null) throw new Error('selection is required')
    expect(
      validateFormProjectionSelection({ ...selection, sourceHash: 'stale-source' }),
    ).toBe('FLOW_FORM_PROJECTION_SELECTION_PLAN_IDENTITY_INVALID')
    expect(
      validateFormProjectionSelection({
        ...selection,
        flattenedFieldIds: ['field-1', 'field-1'],
      }),
    ).toBe('FLOW_FORM_PROJECTION_SELECTION_FIELD_ID_DUPLICATE')

    const reviewResult = result(sourceRequest, {
      formPlan: null,
      projection: {
        ...projectionResult.projection,
        review: [
          {
            fieldId: 'field-1',
            sourceNodeId: 'node-missing',
            reason: { kind: 'targetMissing' },
          },
        ],
      },
    })
    expect(createFormProjectionSelection(reviewResult, [])).toBeNull()
  })

  it('serializes the opaque layout/session request and requires Rust verification', async () => {
    const sourceRequest = request()
    const expected = result(sourceRequest)
    let serializedRequest = ''
    let verified = false
    const adapter = createWasmFormProjectionEngine({
      project_form_widgets: (value) => {
        serializedRequest = value
        return rustResponse(expected)
      },
      verify_form_projection_response: () => {
        verified = true
        return true
      },
    })

    const adapted = await adapter.run(sourceRequest, new AbortController().signal)
    const wire = JSON.parse(serializedRequest) as {
      readonly schemaVersion: number
      readonly requestId: string
      readonly layoutRequestJson: string
      readonly session: FormSessionStateDto
    }
    expect(wire.schemaVersion).toBe(FORM_PROJECTION_PROTOCOL_VERSION)
    expect(wire.requestId).toBe(sourceRequest.requestId)
    expect(JSON.parse(wire.layoutRequestJson)).toEqual(sourceRequest.layout)
    expect(wire.session).toEqual(sourceRequest.session)
    expect(verified).toBe(true)
    expect(adapted.projection.widgets[0]?.value).toEqual({ type: 'text', value: 'Alice' })
    expect(adapter.verifyResultHash(adapted)).toBe(true)
  })

  it('rejects malformed, stale, envelope-mismatched, and unverified responses', async () => {
    const sourceRequest = request()
    const expected = result(sourceRequest)
    const cases: Array<{
      readonly response: string
      readonly verify: boolean
      readonly code: string
    }> = [
      {
        response: '{',
        verify: true,
        code: 'FLOW_FORM_PROJECTION_RESPONSE_DECODE',
      },
      {
        response: JSON.stringify({
          schemaVersion: FORM_PROJECTION_PROTOCOL_VERSION,
          requestId: sourceRequest.requestId,
          ok: false,
          result: null,
          error: { code: 'FLOW_FORM_PROJECTION_LAYOUT_REQUEST_DECODE' },
        }),
        verify: true,
        code: 'FLOW_FORM_PROJECTION_LAYOUT_REQUEST_DECODE',
      },
      {
        response: rustResponse({ ...expected, sourceRevision: 9 }),
        verify: true,
        code: 'FLOW_FORM_PROJECTION_STALE_SOURCE_REVISION',
      },
      {
        response: rustResponse(expected).replace('"displayListHash":"display-list-1"', '"displayListHash":"other"'),
        verify: true,
        code: 'FLOW_FORM_PROJECTION_RESULT_IDENTITY_INVALID',
      },
      {
        response: rustResponse(expected),
        verify: false,
        code: 'FLOW_FORM_PROJECTION_RESULT_HASH_INVALID',
      },
    ]

    for (const testCase of cases) {
      const adapter = createWasmFormProjectionEngine({
        project_form_widgets: () => testCase.response,
        verify_form_projection_response: () => testCase.verify,
      })
      await expect(adapter.run(sourceRequest, new AbortController().signal)).rejects.toMatchObject({
        code: testCase.code,
      })
    }
  })

  it('publishes only the newest revision and reports cancellation/failure without partial state', async () => {
    const pending: Array<ReturnType<typeof deferred<FormProjectionResultDto>>> = []
    const scheduler = new RevisionAwareFormProjectionScheduler(
      async (requestValue) => {
        const next = deferred<FormProjectionResultDto>()
        pending.push(next)
        return next.promise
      },
      { verifyResultHash: () => true },
    )

    const firstRequest = request(1, 'first')
    const secondRequest = request(2, 'second')
    const first = scheduler.request(firstRequest)
    const second = scheduler.request(secondRequest)
    pending[1]?.resolve(result(secondRequest))
    expect(await second).toMatchObject({
      kind: 'published',
      accepted: { request: { requestId: 'second' } },
    })
    pending[0]?.resolve(result(firstRequest))
    expect(await first).toMatchObject({ kind: 'cancelled', requestId: 'first' })
    expect(scheduler.accepted()?.request.requestId).toBe('second')

    const diagnostics: string[] = []
    const failed = new RevisionAwareFormProjectionScheduler(
      async () => {
        throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_ENGINE_FAILED')
      },
      {
        verifyResultHash: () => true,
        onDiagnostic: ({ code }) => diagnostics.push(code),
      },
    )
    expect(await failed.request(request(3, 'failed'))).toMatchObject({
      kind: 'failed',
      code: 'FLOW_FORM_PROJECTION_ENGINE_FAILED',
    })
    expect(failed.accepted()).toBeNull()
    expect(diagnostics).toEqual(['FLOW_FORM_PROJECTION_ENGINE_FAILED'])
  })

  it('fences an optional plan when a review-bearing projection is returned', async () => {
    const sourceRequest = request()
    const reviewResult = result(sourceRequest, {
      formPlan: null,
      projection: {
        ...result(sourceRequest).projection,
        review: [
          {
            fieldId: 'field-1',
            sourceNodeId: 'node-missing',
            reason: { kind: 'targetMissing' },
          },
        ],
      },
    })
    const scheduler = new RevisionAwareFormProjectionScheduler(
      async () => reviewResult,
      { verifyResultHash: () => true },
    )
    const outcome = await scheduler.request(sourceRequest)
    expect(outcome.kind).toBe('published')
    expect(scheduler.accepted()?.result.formPlan).toBeNull()
  })
})

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
    rect: { x: 64, y: 128, width: 256, height: 32 },
  }
}
