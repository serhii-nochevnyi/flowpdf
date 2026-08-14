import { expect, test } from 'vitest'

import init, { apply_command, create_sample } from '../generated/flow_wasm.js'

interface ErrorDto {
  readonly code: string
  readonly audit: unknown | null
}

interface ApiResponse<T> {
  readonly ok: boolean
  readonly value: T | null
  readonly error: ErrorDto | null
}

interface SessionDto {
  readonly canonicalJson: string
  readonly canonicalHash: string
  readonly revision: number
  readonly nextCommandTarget: {
    readonly nodeId: string
    readonly utf16Offset: number
    readonly affinity: string
  }
  readonly history: unknown
}

interface OperationResultDto {
  readonly session: SessionDto
  readonly commit: {
    readonly transaction: {
      readonly beforeHash: string
      readonly afterHash: string
      readonly forwardOperations: readonly unknown[]
      readonly inverseOperations: readonly unknown[]
      readonly anchorMapping: { readonly transformations: readonly unknown[] }
    }
  }
}

function response<T>(value: unknown): ApiResponse<T> {
  return value as ApiResponse<T>
}

function commandId(suffix: number): string {
  return `00000000-0000-4000-8000-${String(suffix).padStart(12, '0')}`
}

test('WASM command DTO preserves the native transaction, UTF-16, stale, and duplicate contracts', async () => {
  await init()

  const createdResponse = response<OperationResultDto>(
    create_sample({ requestedLocale: 'uk-UA' }),
  )
  expect(createdResponse.ok).toBe(true)
  const created = createdResponse.value
  expect(created).not.toBeNull()
  if (created === null) return

  const insertCommand = {
    commandId: commandId(801),
    baseRevision: created.session.revision,
    modality: 'voice',
    issuedAt: '2026-08-14T20:04:01Z',
    kind: {
      type: 'insertText',
      target: created.session.nextCommandTarget,
      text: ' — голосова вставка',
    },
  }
  const appliedResponse = response<OperationResultDto>(
    apply_command({
      canonicalJson: created.session.canonicalJson,
      history: created.session.history,
      command: insertCommand,
    }),
  )
  expect(appliedResponse.ok).toBe(true)
  const applied = appliedResponse.value
  expect(applied).not.toBeNull()
  if (applied === null) return

  expect(applied.session.revision).toBe(2)
  expect(applied.session.canonicalHash).not.toBe(created.session.canonicalHash)
  expect(applied.commit.transaction.beforeHash).toBe(created.session.canonicalHash)
  expect(applied.commit.transaction.afterHash).toBe(applied.session.canonicalHash)
  expect(applied.commit.transaction.forwardOperations).toHaveLength(1)
  expect(applied.commit.transaction.inverseOperations).toHaveLength(1)
  expect(applied.commit.transaction.anchorMapping.transformations).toHaveLength(1)

  const stale = response<OperationResultDto>(
    apply_command({
      canonicalJson: applied.session.canonicalJson,
      history: applied.session.history,
      command: {
        ...insertCommand,
        commandId: commandId(802),
        baseRevision: 1,
      },
    }),
  )
  expect(stale).toMatchObject({ ok: false, value: null, error: { code: 'FLOW_STALE_REVISION' } })

  const canonicalDocument = JSON.parse(created.session.canonicalJson) as {
    readonly content: readonly { readonly id: string; readonly text: string }[]
  }
  const emojiNode = canonicalDocument.content[0]
  if (emojiNode === undefined) throw new Error('sample paragraph is required')
  const invalidUtf16 = response<OperationResultDto>(
    apply_command({
      canonicalJson: created.session.canonicalJson,
      history: created.session.history,
      command: {
        ...insertCommand,
        commandId: commandId(803),
        kind: {
          type: 'insertText',
          target: {
            nodeId: emojiNode.id,
            utf16Offset: emojiNode.text.indexOf('😀') + 1,
            affinity: 'forward',
          },
          text: 'X',
        },
      },
    }),
  )
  expect(invalidUtf16).toMatchObject({
    ok: false,
    value: null,
    error: { code: 'FLOW_INVALID_UTF16_BOUNDARY' },
  })

  const duplicate = response<OperationResultDto>(
    apply_command({
      canonicalJson: applied.session.canonicalJson,
      history: applied.session.history,
      command: {
        ...insertCommand,
        baseRevision: applied.session.revision,
      },
    }),
  )
  expect(duplicate).toMatchObject({
    ok: false,
    value: null,
    error: { code: 'FLOW_DUPLICATE_COMMAND' },
  })
})
