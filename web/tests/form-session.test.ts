import { describe, expect, it } from 'vitest'

import {
  FormSessionBridgeError,
  FormSessionCoordinator,
  RustFormSessionBridge,
  resolveVoiceFieldTarget,
  type FormSessionApiResponse,
  type FormSessionPersistence,
  type FormSessionRequestDto,
  type FormSessionResponseDto,
  type FormSessionWasmBoundary,
} from '../src/forms/form-session.js'
import type {
  FormSessionIdentityDto,
  FormSessionStateDto,
} from '../persistence/indexeddb-store.js'

const identity: FormSessionIdentityDto = {
  documentId: 'document-1',
  sourceRevision: 3,
  sourceHash: 'source-hash-3',
}

function emptySession(): FormSessionStateDto {
  return {
    ...identity,
    schemaVersion: 1,
    generation: 0,
    overrides: {},
  }
}

class MemoryFormSessionPersistence implements FormSessionPersistence {
  session: FormSessionStateDto | null = null
  readonly saves: { readonly session: FormSessionStateDto; readonly expected: number | null }[] = []

  async loadFormSession(): Promise<FormSessionStateDto | null> {
    return this.session
  }

  async saveFormSession(
    session: FormSessionStateDto,
    expectedGeneration: number | null,
  ): Promise<void> {
    this.saves.push({ session, expected: expectedGeneration })
    this.session = session
  }
}

function wasmFixture(
  requests: FormSessionRequestDto[],
  options: { readonly failureCode?: string; readonly identity?: FormSessionIdentityDto } = {},
): FormSessionWasmBoundary {
  return {
    apply_form_session(request: unknown): FormSessionApiResponse<FormSessionResponseDto> {
      const typed = request as FormSessionRequestDto
      requests.push(typed)
      if (options.failureCode !== undefined) {
        return {
          ok: false,
          value: null,
          error: { code: options.failureCode, message: 'rejected by Rust', audit: null },
        }
      }
      if (typed.action.type === 'start') {
        return accepted({ ...emptySession(), ...(options.identity ?? {}) })
      }
      if (typed.session === null) {
        return {
          ok: false,
          value: null,
          error: { code: 'FLOW_FORM_SESSION_MISSING', message: 'missing', audit: null },
        }
      }
      const session = typed.session
      let next: FormSessionStateDto = session
      if (typed.action.type === 'setValue') {
        next = {
          ...session,
          generation: session.generation + 1,
          overrides: { ...session.overrides, [typed.action.fieldId]: typed.action.value },
        }
      } else if (typed.action.type === 'clearValue') {
        const { [typed.action.fieldId]: _removed, ...overrides } = session.overrides
        next = { ...session, generation: session.generation + 1, overrides }
      }
      return accepted({
        ...next,
        ...(options.identity ?? {}),
      })
    },
  }
}

function accepted(session: FormSessionStateDto): FormSessionApiResponse<FormSessionResponseDto> {
  return {
    ok: true,
    value: { protocolVersion: 1, session },
    error: null,
  }
}

describe('RustFormSessionBridge', () => {
  it('routes start, set, validate, and clear through Rust before persistence', async () => {
    const requests: FormSessionRequestDto[] = []
    const persistence = new MemoryFormSessionPersistence()
    const bridge = new RustFormSessionBridge(wasmFixture(requests), persistence)

    const started = await bridge.restoreOrStart('{"documentId":"document-1"}', identity)
    const filled = await bridge.setValue(
      '{"documentId":"document-1"}',
      identity,
      started,
      'name',
      { type: 'text', value: 'Олена' },
    )
    const validated = bridge.validate('{"documentId":"document-1"}', identity, filled)
    const cleared = await bridge.clearValue(
      '{"documentId":"document-1"}',
      identity,
      validated,
      'name',
    )

    expect(cleared.overrides).toEqual({})
    expect(requests.map(({ action }) => action.type)).toEqual([
      'start',
      'setValue',
      'validate',
      'clearValue',
    ])
    expect(requests[1]).toMatchObject({
      protocolVersion: 1,
      canonicalJson: '{"documentId":"document-1"}',
      session: started,
      action: { type: 'setValue', fieldId: 'name' },
    })
    expect(persistence.saves.map(({ expected }) => expected)).toEqual([null, 0, 1])
    expect(persistence.saves.map(({ session }) => session.generation)).toEqual([0, 1, 2])
  })

  it('does not persist Rust-rejected actions and preserves the stable error code', async () => {
    const requests: FormSessionRequestDto[] = []
    const persistence = new MemoryFormSessionPersistence()
    const bridge = new RustFormSessionBridge(
      wasmFixture(requests, { failureCode: 'FLOW_FORM_SESSION_UNKNOWN_FIELD' }),
      persistence,
    )

    await expect(
      bridge.setValue('{"documentId":"document-1"}', identity, emptySession(), 'missing', {
        type: 'text',
        value: 'value',
      }),
    ).rejects.toMatchObject({
      code: 'FLOW_FORM_SESSION_UNKNOWN_FIELD',
    })
    expect(persistence.saves).toEqual([])
    expect(requests).toHaveLength(1)
  })

  it('rejects malformed or source-mismatched successful responses', async () => {
    const persistence = new MemoryFormSessionPersistence()
    const malformed = new RustFormSessionBridge(
      { apply_form_session: () => ({ ok: true, value: null, error: null }) },
      persistence,
    )
    await expect(
      malformed.restoreOrStart('{}', identity),
    ).rejects.toMatchObject({ code: 'FLOW_UNKNOWN_CORE_ERROR' })

    const mismatched = new RustFormSessionBridge(
      wasmFixture([], {
        identity: { ...identity, sourceHash: 'other-source-hash' },
      }),
      persistence,
    )
    await expect(mismatched.restoreOrStart('{}', identity)).rejects.toMatchObject({
      code: 'FLOW_FORM_SESSION_IDENTITY_MISMATCH',
    })
  })
})

describe('FormSessionCoordinator', () => {
  it('publishes one source-bound session and serializes accepted mutations', async () => {
    const requests: FormSessionRequestDto[] = []
    const persistence = new MemoryFormSessionPersistence()
    const coordinator = new FormSessionCoordinator({
      wasm: Promise.resolve(wasmFixture(requests)),
      persistence,
    })
    let notifications = 0
    const unsubscribe = coordinator.subscribe(() => {
      notifications += 1
    })

    await coordinator.synchronize('{"revision":3}', identity)
    expect(coordinator.getSnapshot()).toMatchObject({
      phase: 'ready',
      identity,
      session: { generation: 0, overrides: {} },
      errorCode: null,
    })
    await coordinator.setValue(
      '{"revision":3}',
      identity,
      'name',
      { type: 'text', value: 'Олена' },
    )
    expect(coordinator.getSnapshot().session?.overrides).toEqual({
      name: { type: 'text', value: 'Олена' },
    })
    await coordinator.clearValue('{"revision":3}', identity, 'name')
    expect(coordinator.getSnapshot().session?.overrides).toEqual({})
    expect(persistence.saves.map(({ expected }) => expected)).toEqual([null, 0, 1])
    expect(requests.map(({ action }) => action.type)).toEqual([
      'start',
      'setValue',
      'clearValue',
    ])
    expect(notifications).toBeGreaterThanOrEqual(4)
    unsubscribe()
  })

  it('drops the previous session on a source change before loading the new identity', async () => {
    const requests: FormSessionRequestDto[] = []
    const persistence = new MemoryFormSessionPersistence()
    const coordinator = new FormSessionCoordinator({
      wasm: Promise.resolve(
        wasmFixture(requests, { identity: { ...identity, sourceRevision: 4 } }),
      ),
      persistence,
    })
    const otherIdentity = { ...identity, sourceRevision: 4 }

    await coordinator.synchronize('{"revision":4}', otherIdentity)
    expect(coordinator.getSnapshot()).toMatchObject({
      phase: 'ready',
      identity: otherIdentity,
      session: { sourceRevision: 4 },
    })
  })

  it('resolves next and previous voice targets only from the ordered valid-field projection', () => {
    const fields = [
      { fieldId: 'field-b', tabOrder: 1 },
      { fieldId: 'field-a', tabOrder: 0 },
      { fieldId: 'field-c', tabOrder: 2 },
    ]
    expect(resolveVoiceFieldTarget(fields, 'field-a', 'next')).toEqual({
      kind: 'resolved',
      fieldId: 'field-b',
    })
    expect(resolveVoiceFieldTarget(fields, 'field-b', 'previous')).toEqual({
      kind: 'resolved',
      fieldId: 'field-a',
    })
    expect(resolveVoiceFieldTarget(fields, undefined, 'next')).toEqual({
      kind: 'resolved',
      fieldId: 'field-a',
    })
    expect(resolveVoiceFieldTarget(fields, undefined, 'previous')).toEqual({
      kind: 'resolved',
      fieldId: 'field-c',
    })
    expect(resolveVoiceFieldTarget(fields, 'field-c', 'next')).toEqual({
      kind: 'rejected',
      code: 'FLOW_VOICE_FIELD_NAVIGATION_EDGE',
    })
    expect(resolveVoiceFieldTarget(fields, 'review-field', 'next')).toEqual({
      kind: 'rejected',
      code: 'FLOW_VOICE_FIELD_NOT_FOUND',
    })
    expect(resolveVoiceFieldTarget([], undefined, 'next')).toEqual({
      kind: 'rejected',
      code: 'FLOW_VOICE_FIELD_NOT_FOUND',
    })
  })

  it('rejects forged or ambiguous projected field order before selecting a target', () => {
    expect(
      resolveVoiceFieldTarget(
        [
          { fieldId: 'field-a', tabOrder: 0 },
          { fieldId: 'field-b', tabOrder: 0 },
        ],
        undefined,
        'next',
      ),
    ).toEqual({ kind: 'rejected', code: 'FLOW_VOICE_FIELD_TARGET_INVALID' })
    expect(
      resolveVoiceFieldTarget([{ fieldId: '', tabOrder: 0 }], undefined, 'next'),
    ).toEqual({ kind: 'rejected', code: 'FLOW_VOICE_FIELD_TARGET_INVALID' })
  })
})
