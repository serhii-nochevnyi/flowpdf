import { describe, expect, it } from 'vitest'

import {
  LAYOUT_PROTOCOL_VERSION,
  type LayoutRequestDto,
  type LayoutWorkerResultDto,
} from '../src/layout/layout-protocol.js'
import {
  LayoutWorkerError,
  RevisionAwareLayoutScheduler,
  createWasmLayoutEngine,
  type LayoutEngine,
} from '../src/layout/layout-worker.js'

function request(revision: number, id = `request-${revision}`): LayoutRequestDto {
  return {
    protocolVersion: LAYOUT_PROTOCOL_VERSION,
    requestId: id,
    sourceRevision: revision,
    sourceHash: `hash-${revision}`,
    expectedLayoutSettingsFingerprint: 'settings-v1',
    fontCatalogIdentity: 'fonts-v1',
    hyphenationDataIdentity: null,
    viewport: { firstPage: 0, pageCount: 2 },
    serializedRequest: '{}',
  }
}

function result(
  sourceRequest: LayoutRequestDto,
  overrides: Partial<LayoutWorkerResultDto> = {},
): LayoutWorkerResultDto {
  return {
    protocolVersion: LAYOUT_PROTOCOL_VERSION,
    requestId: sourceRequest.requestId,
    sourceRevision: sourceRequest.sourceRevision,
    sourceHash: sourceRequest.sourceHash,
    layoutSettingsFingerprint: 'settings-v1',
    fontCatalogIdentity: sourceRequest.fontCatalogIdentity,
    hyphenationDataIdentity: sourceRequest.hyphenationDataIdentity,
    pages: [],
    diagnostics: [],
    resultHash: `result-${sourceRequest.sourceRevision}`,
    ...overrides,
  }
}

function deferred<T>(): {
  readonly promise: Promise<T>
  readonly resolve: (value: T) => void
  readonly reject: (reason?: unknown) => void
} {
  let resolvePromise: (value: T) => void = () => undefined
  let rejectPromise: (reason?: unknown) => void = () => undefined
  const promise = new Promise<T>((resolve, reject) => {
    resolvePromise = resolve
    rejectPromise = reject
  })
  return { promise, resolve: resolvePromise, reject: rejectPromise }
}

describe('revision-aware layout worker scheduling', () => {
  it('keeps semantic editing usable and publishes only the newest complete result', async () => {
    const pending: Array<ReturnType<typeof deferred<LayoutWorkerResultDto>>> = []
    const engine: LayoutEngine = async () => {
      const next = deferred<LayoutWorkerResultDto>()
      pending.push(next)
      return next.promise
    }
    const scheduler = new RevisionAwareLayoutScheduler(engine, {
      verifyResultHash: (value) => value.resultHash.startsWith('result-'),
    })

    let activeEditorText = 'accepted revision 1'
    const first = scheduler.request(request(1))
    activeEditorText = 'typing revision 2 while pagination is pending'
    expect(activeEditorText).toContain('typing')

    const second = scheduler.request(request(2))
    pending[1]?.resolve(result(request(2)))
    const secondOutcome = await second
    expect(secondOutcome.kind).toBe('published')
    expect(activeEditorText).toContain('typing')
    expect(scheduler.accepted()?.result.sourceRevision).toBe(2)

    // The first engine deliberately ignores AbortSignal to model an out-of-
    // order worker response. Its result cannot replace the accepted revision.
    pending[0]?.resolve(result(request(1)))
    const firstOutcome = await first
    expect(firstOutcome).toMatchObject({ kind: 'cancelled', requestId: 'request-1' })
    expect(scheduler.accepted()?.result.sourceRevision).toBe(2)
  })

  it('rejects every identity guard and never publishes a partial result', async () => {
    const cases: Array<{
      readonly name: string
      readonly override: Partial<LayoutWorkerResultDto>
      readonly code: string
    }> = [
      {
        name: 'request id',
        override: { requestId: 'other-request' },
        code: 'FLOW_LAYOUT_STALE_REQUEST_ID',
      },
      {
        name: 'source revision',
        override: { sourceRevision: 9 },
        code: 'FLOW_LAYOUT_STALE_SOURCE_REVISION',
      },
      {
        name: 'settings fingerprint',
        override: { layoutSettingsFingerprint: 'settings-v2' },
        code: 'FLOW_LAYOUT_STALE_SETTINGS_FINGERPRINT',
      },
      {
        name: 'font identity',
        override: { fontCatalogIdentity: 'fonts-v2' },
        code: 'FLOW_LAYOUT_STALE_FONT_IDENTITY',
      },
      {
        name: 'data identity',
        override: { hyphenationDataIdentity: 'uk-v2' },
        code: 'FLOW_LAYOUT_STALE_DATA_IDENTITY',
      },
    ]

    for (const testCase of cases) {
      const scheduler = new RevisionAwareLayoutScheduler(
        async (requestValue) => result(requestValue, testCase.override),
        {
          verifyResultHash: () => true,
        },
      )
      const outcome = await scheduler.request(request(1, `guard-${testCase.name}`))
      expect(outcome).toMatchObject({ kind: 'discarded', code: testCase.code })
      expect(scheduler.accepted()).toBeNull()
    }

    const hashScheduler = new RevisionAwareLayoutScheduler(
      async (requestValue) => result(requestValue),
      { verifyResultHash: () => false },
    )
    const hashOutcome = await hashScheduler.request(request(1, 'guard-result-hash'))
    expect(hashOutcome).toMatchObject({
      kind: 'discarded',
      code: 'FLOW_LAYOUT_RESULT_HASH_INVALID',
    })
    expect(hashScheduler.accepted()).toBeNull()
  })

  it('cancels cooperatively and recovers after a worker failure', async () => {
    const next = deferred<LayoutWorkerResultDto>()
    let calls = 0
    const engine: LayoutEngine = async (requestValue) => {
      calls += 1
      if (calls === 1) return next.promise
      throw new LayoutWorkerError('FLOW_LAYOUT_ENGINE_FAILED')
    }
    const diagnostics: string[] = []
    const scheduler = new RevisionAwareLayoutScheduler(engine, {
      verifyResultHash: () => true,
      onDiagnostic: ({ code }) => diagnostics.push(code),
    })

    const cancelled = scheduler.request(request(1, 'cancel-me'))
    scheduler.cancel('cancel-me')
    next.resolve(result(request(1, 'cancel-me')))
    expect(await cancelled).toMatchObject({ kind: 'cancelled' })
    expect(scheduler.accepted()).toBeNull()

    const failed = await scheduler.request(request(2, 'fail-once'))
    expect(failed).toMatchObject({ kind: 'failed', code: 'FLOW_LAYOUT_ENGINE_FAILED' })
    expect(diagnostics).toContain('FLOW_LAYOUT_ENGINE_FAILED')
  })

  it('adapts the string-only WASM response and rejects tampered Rust hashes', async () => {
    const sourceRequest = request(1, 'wasm-request')
    const expected = result(sourceRequest)
    const response = JSON.stringify({
      schemaVersion: 1,
      requestId: sourceRequest.requestId,
      ok: true,
      result: {
        sourceRevision: expected.sourceRevision,
        sourceHash: expected.sourceHash,
        layoutSettingsFingerprint: expected.layoutSettingsFingerprint,
        fontCatalogIdentity: expected.fontCatalogIdentity,
        hyphenationDataIdentity: expected.hyphenationDataIdentity,
        pages: [],
        diagnostics: [],
        resultHash: expected.resultHash,
      },
      error: null,
    })
    const adapter = createWasmLayoutEngine({
      layout_document: () => response,
      verify_layout_response: () => true,
    })
    const adapted = await adapter.run(sourceRequest, new AbortController().signal)
    expect(adapted.requestId).toBe(sourceRequest.requestId)
    expect(adapter.verifyResultHash(adapted)).toBe(true)

    const rejected = createWasmLayoutEngine({
      layout_document: () => response,
      verify_layout_response: () => false,
    })
    await expect(rejected.run(sourceRequest, new AbortController().signal)).rejects.toMatchObject({
      code: 'FLOW_LAYOUT_RESULT_HASH_INVALID',
    })
  })
})
