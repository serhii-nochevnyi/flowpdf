import * as wasm from '../generated/flow_wasm.js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { hashPdfSourceBytes } from '../persistence/pdf-source-store.js'
import { nativeCropBounds, nativeImageCommand } from '../src/pdf/pdf-native-edit-panel.js'
import {
  NATIVE_PDF_LIMITS,
  nativeBytesHex,
  nativeHexBytes,
  nativePageMoveRefusals,
  readNativeResponse,
  type NativePdfRequest,
  type NativePdfRedactionAction,
  type NativePdfRedactionCandidate,
  type NativePdfRedactionVerification,
  type NativePdfResponse,
  type NativePdfVerifiedSnapshot,
  type NativePdfWasmBoundary,
  type NativePdfWorkerRequest,
  type NativePdfWorkerResponse,
} from '../src/pdf/pdf-native-edit-protocol.js'
import {
  installNativePdfWorker,
  NativePdfEditCoordinator,
  type NativePdfWorkerPort,
  type NativePdfWorkerScope,
} from '../src/pdf/pdf-native-edit-worker.js'
import type { PdfReaderWorkerInboundMessage } from '../src/pdf/pdf-protocol.js'
import type { PdfReaderWorkerPort, PdfReaderWorkerScope } from '../src/pdf/pdf-reader-worker.js'

const hash = (byte: number) => Array<number>(32).fill(byte)
const storeHash = '039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81'
const original = () => ({ bytes: Uint8Array.of(1, 2, 3), originStoreHash: storeHash })
const note = (contents: string) => ({ kind: 'add', page: 0, rect: [1, 2, 3, 4], contents } as const)

function fixturePdf(objects: readonly string[]): string {
  let pdf = '%PDF-1.7\n'
  const offsets = objects.map((object, index) => {
    const offset = pdf.length
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`
    return offset
  })
  const xref = pdf.length
  const size = offsets.length + 1
  pdf += `xref\n0 ${size}\n0000000000 65535 f \n`
  pdf += offsets.map((offset) => `${String(offset).padStart(10, '0')} 00000 n \n`).join('')
  return pdf + `trailer\n<< /Root 1 0 R /Size ${size} >>\nstartxref\n${xref}\n%%EOF\n`
}

async function initializeWasm() {
  const { readFileSync } = await vi.importActual<{ readFileSync(path: URL): Uint8Array }>('node:fs')
  const module = new Uint8Array(readFileSync(new URL('../generated/flow_wasm_bg.wasm', import.meta.url)))
  wasm.initSync({ module })
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: () => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

function fakePort(timeout = 30_000, verifyResponse = (_json: string) => true) {
  const sent: NativePdfWorkerRequest[] = []
  const receivers: ((message: NativePdfWorkerRequest) => void)[] = []
  const worker: NativePdfWorkerPort = {
    onmessage: null, onerror: null, onmessageerror: null,
    postMessage: (message) => {
      const copy = structuredClone(message)
      const receive = receivers.shift()
      if (receive === undefined) sent.push(copy)
      else receive(copy)
    },
    terminate: vi.fn(),
  }
  const verify = vi.fn(verifyResponse)
  const coordinator = new NativePdfEditCoordinator(worker, verify, timeout)
  const take = () => {
    const next = sent.shift()
    return next === undefined
      ? new Promise<NativePdfWorkerRequest>((resolve) => { receivers.push(resolve) })
      : Promise.resolve(next)
  }
  const deliver = (dispatch: NativePdfWorkerRequest, response: unknown) => {
    worker.onmessage?.(new MessageEvent('message', { data: { dispatchId: dispatch.dispatchId, type: 'response', json: JSON.stringify(response) } }))
  }
  return { sent, worker, verify, coordinator, take, deliver }
}

function openReply(request: NativePdfRequest): Extract<NativePdfResponse, { operation: 'open'; status: 'opened' }> {
  if (request.operation !== 'open') throw new Error('expected open')
  const binding = { sessionId: 1, epoch: request.epoch, originHash: hash(1), revision: 0 }
  const snapshot: NativePdfVerifiedSnapshot = {
    binding, originStoreHash: request.originStoreHash, outputHash: hash(1), consumedRequest: 0,
    outputHex: request.bytesHex,
    redactionAdmission: 'supported',
    pageState: { order: [196608], selected: null, canDelete: false, maxPosition: 0 },
    scene: {
      outputHash: hash(1), sceneHash: hash(10), pages: [
        { id: 0, object: { objectNumber: 3, generation: 0 }, geometry: { bounds: [0, 0, 100, 100], rotation: 0 },
          canAddNote: true, annotations: [] },
      ],
    },
  }
  return {
    protocolVersion: 1, requestId: request.requestId, operation: 'open', binding,
    originStoreHash: request.originStoreHash, status: 'opened', snapshot, code: null,
    limits: NATIVE_PDF_LIMITS, resultHash: 'aa'.repeat(32),
  }
}

function addReply(request: NativePdfRequest, previous: NativePdfVerifiedSnapshot): Extract<NativePdfResponse, { status: 'applied' }> {
  if (request.operation !== 'command' || request.command.kind !== 'add') throw new Error('expected add')
  const revision = request.binding.revision + 1
  const binding = { ...request.binding, revision }
  const outputHash = hash(10 + revision)
  const page = previous.scene.pages[0]!
  const snapshot: NativePdfVerifiedSnapshot = {
    ...previous, binding, outputHash, consumedRequest: request.requestId,
    outputHex: nativeBytesHex(Uint8Array.of(revision)),
    scene: {
      outputHash, sceneHash: hash(20 + revision), pages: [{
        ...page, annotations: [...page.annotations, {
          kind: 'plainNote', id: revision, object: { objectNumber: 3 + revision, generation: 0 },
          rect: request.command.rect, contents: request.command.contents,
        }],
      }],
    },
  }
  return {
    protocolVersion: 1, requestId: request.requestId, operation: 'command', binding: request.binding,
    originStoreHash: request.originStoreHash, status: 'applied', snapshot, code: null,
    limits: NATIVE_PDF_LIMITS, resultHash: 'bb'.repeat(32),
  }
}

const textIsland = { id: hash(41), text: 'ЇAA', bounds: [30, 140, 56, 152],
  profile: 'identityHCidFontType2', operator: 'Tj', maxAdvance: 26 } as const
const replaceText = { kind: 'replaceText', island: textIsland.id, text: 'ЇA' } as const

async function opened(lane: ReturnType<typeof fakePort>, withText = false) {
  const opening = lane.coordinator.open(original())
  const dispatch = await lane.take()
  const reply = openReply(dispatch.request)
  const { snapshot } = reply
  lane.deliver(dispatch, withText ? {
    ...reply, snapshot: { ...snapshot, scene: { ...snapshot.scene,
      textPages: [{ page: 0, refusal: null, islands: [textIsland] }],
    } },
  } : reply)
  return opening
}

function previewReply(request: NativePdfRequest): Extract<NativePdfResponse, { status: 'preview' }> {
  if (request.operation !== 'preview') throw new Error('expected preview request')
  return {
    protocolVersion: 1, requestId: request.requestId, operation: 'preview', status: 'preview',
    binding: request.binding, originStoreHash: request.originStoreHash,
    snapshot: null, code: null, limits: NATIVE_PDF_LIMITS, resultHash: 'cc'.repeat(32),
    preview: {
      pageId: request.pageId, outputHash: request.outputHash,
      cropBox: [0, 0, 100_000, 100_000], rotation: 0, width: 200, height: 200, rgbHex: 'ffffff'.repeat(40_000),
    },
  }
}

describe('accepted page preview', () => {
  it('drops aborted selections and accepted-revision changes without replacing the accepted snapshot', async () => {
    const lane = fakePort()
    const original = await opened(lane)
    const pageId = original.pageState.order[0]!
    const selection = new AbortController()
    const first = lane.coordinator.previewPage(pageId, selection.signal)
    const cancelled = await lane.take()
    selection.abort()
    lane.deliver(cancelled, previewReply(cancelled.request))
    await expect(first).resolves.toBeNull()
    expect(lane.coordinator.snapshot().accepted).toBe(original)

    const stale = lane.coordinator.previewPage(pageId, new AbortController().signal)
    const pending = await lane.take()
    const command = lane.coordinator.command(note('Review note'))
    const edit = await lane.take()
    lane.deliver(edit, addReply(edit.request, original))
    const accepted = await command
    lane.deliver(pending, previewReply(pending.request))
    await expect(stale).resolves.toBeNull()
    expect(lane.coordinator.snapshot().accepted).toBe(accepted)

    const current = lane.coordinator.previewPage(pageId, new AbortController().signal)
    const dispatch = await lane.take()
    expect(dispatch.request).toMatchObject({ binding: accepted.binding, outputHash: accepted.outputHash, pageId })
    const reply = previewReply(dispatch.request)
    lane.deliver(dispatch, reply)
    await expect(current).resolves.toEqual(reply.preview)
  })

  it('discards preview replies while the accepted revision is busy with a mark', async () => {
    const lane = fakePort()
    const original = await opened(lane)
    const pending = lane.coordinator.previewPage(mark.pageId, new AbortController().signal)
    const preview = await lane.take()
    const marking = lane.coordinator.markRedaction(mark, 1)
    const dispatch = await lane.take()

    lane.deliver(preview, previewReply(preview.request))
    await expect(pending).resolves.toBeNull()
    lane.deliver(dispatch, redactionReply(dispatch.request))
    const readback = await lane.take()
    lane.deliver(readback, markSnapshotReply(readback.request, original))
    await marking
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'ready', accepted: original })
    lane.coordinator.dispose()
  })

  it.each(['page', 'hash', 'crop', 'refusal'] as const)('fails closed on a preview %s mismatch', async (fault) => {
    const lane = fakePort()
    const original = await opened(lane)
    const pending = lane.coordinator.previewPage(original.pageState.order[0]!, new AbortController().signal)
    const dispatch = await lane.take()
    const reply = previewReply(dispatch.request)
    const { preview, ...envelope } = reply
    if (fault === 'refusal') {
      lane.deliver(dispatch, { ...envelope, status: 'failed', code: 'FLOW_PDF_NATIVE_TARGET_UNAVAILABLE' })
    } else {
      lane.deliver(dispatch, { ...reply, preview: {
        ...preview,
        ...(fault === 'page' ? { pageId: preview.pageId + 1 }
          : fault === 'hash' ? { outputHash: hash(2) } : { cropBox: [1000, 0, 101_000, 100_000] }),
      } })
    }
    await expect(pending).resolves.toBeNull()
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'ready', accepted: original })
    const saving = lane.coordinator.command(note('Still editable'))
    const edit = await lane.take()
    lane.deliver(edit, addReply(edit.request, original))
    await expect(saving).resolves.toMatchObject({ binding: { revision: 1 } })
  })
})

describe('native PDF edit coordinator', () => {
  afterEach(() => { vi.useRealTimers() })

  it.each(['applied', 'reconciled'] as const)('confirms reopened note and field changes through %s', async (mode) => {
    await initializeWasm()
    const pdf = fixturePdf([
      '<< /Type /Catalog /Pages 2 0 R >>',
      '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
      '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] >>',
    ])
    const bytes = new TextEncoder().encode(pdf)
    const originStoreHash = nativeBytesHex(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)))
    const lane = fakePort(30_000, wasm.verify_native_pdf_response)
    const opening = lane.coordinator.open({ bytes, originStoreHash })
    bytes.fill(0)
    const openedRequest = await lane.take()
    expect(openedRequest.request).toMatchObject({ bytesHex: nativeBytesHex(new TextEncoder().encode(pdf)) })
    const open = wasm.native_pdf_open(JSON.stringify(openedRequest.request))
    lane.deliver(openedRequest, JSON.parse(open) as NativePdfResponse)
    await opening

    const addingNote = lane.coordinator.command(note('Original note'))
    const addNote = await lane.take()
    lane.deliver(addNote, JSON.parse(wasm.native_pdf_command(JSON.stringify(addNote.request))))
    const withNote = await addingNote
    const target = withNote.scene.pages[0]?.annotations[0]
    if (target?.kind !== 'plainNote') throw new Error('expected a verified note')
    const moving = lane.coordinator.command({ kind: 'move', note: target.id, page: 0, rect: [10, 20, 30, 40] })
    const moveFailure = mode === 'reconciled'
      ? expect(moving).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' }) : null
    const moveNote = await lane.take()
    const moved = JSON.parse(wasm.native_pdf_command(JSON.stringify(moveNote.request))) as NativePdfResponse
    if (moved.status !== 'applied') throw new Error('expected a verified note move')
    if (moveFailure !== null) {
      lane.deliver(moveNote, { ...moved, resultHash: '00'.repeat(32) })
      await moveFailure
      const reconciling = lane.coordinator.reconcile()
      const lookup = await lane.take()
      lane.deliver(lookup, JSON.parse(wasm.native_pdf_snapshot(JSON.stringify(lookup.request))))
      await expect(reconciling).resolves.toEqual({ status: 'accepted', snapshot: moved.snapshot })
    } else {
      lane.deliver(moveNote, moved)
      await expect(moving).resolves.toEqual(moved.snapshot)
    }
    expect(lane.coordinator.snapshot().accepted?.scene.pages[0]?.annotations[0]).toMatchObject({
      id: target.id, contents: 'Original note', rect: [10, 20, 30, 40],
    })

    const adding = lane.coordinator.command({
      kind: 'addTextField', page: 0, rect: [10, 10, 160, 40], name: 'recipient', value: 'Alice',
    })
    const addDispatch = await lane.take()
    lane.deliver(addDispatch, JSON.parse(wasm.native_pdf_command(JSON.stringify(addDispatch.request))) as NativePdfResponse)
    const previous = await adding
    const field = previous.fields?.[0]
    expect(field).toMatchObject({ kind: 'text', name: 'recipient', value: 'Alice', checked: null, canEditAppearance: true })

    const editing = lane.coordinator.command({ kind: 'setFieldText', field: field!.id, value: 'Bob' })
    const editFailure = mode === 'reconciled' ? expect(editing).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' }) : null
    const editDispatch = await lane.take()
    const actual = JSON.parse(wasm.native_pdf_command(JSON.stringify(editDispatch.request))) as NativePdfResponse
    if (actual.status !== 'applied') throw new Error('expected a verified field change')
    if (mode === 'reconciled') {
      lane.deliver(editDispatch, { ...actual, resultHash: '00'.repeat(32) })
      await editFailure
      expect(lane.coordinator.snapshot().accepted).toBe(previous)
      const reconciling = lane.coordinator.reconcile()
      const lookup = await lane.take()
      lane.deliver(lookup, JSON.parse(wasm.native_pdf_snapshot(JSON.stringify(lookup.request))) as NativePdfResponse)
      await expect(reconciling).resolves.toEqual({ status: 'accepted', snapshot: actual.snapshot })
    } else {
      lane.deliver(editDispatch, actual)
      await expect(editing).resolves.toEqual(actual.snapshot)
    }
    expect(lane.coordinator.snapshot().accepted?.fields?.[0]).toMatchObject({
      name: 'recipient', kind: 'text', value: 'Bob', rect: [10, 10, 160, 40], page: 0,
    })
    const closing = lane.coordinator.close()
    const closeRequest = await lane.take()
    lane.deliver(closeRequest, JSON.parse(wasm.native_pdf_close(JSON.stringify(closeRequest.request))) as NativePdfResponse)
    await closing
    lane.coordinator.dispose()
  })

  it('requires an accepted note target and rejects altered note identities on readback', async () => {
    const lane = fakePort()
    const initial = await opened(lane)
    await expect(lane.coordinator.command({ kind: 'remove', note: 1 }))
      .rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_TARGET_UNAVAILABLE' })
    expect(lane.sent).toHaveLength(0)

    const adding = lane.coordinator.command(note('Original'))
    const addDispatch = await lane.take()
    lane.deliver(addDispatch, addReply(addDispatch.request, initial))
    const accepted = await adding
    const page = accepted.scene.pages[0]!
    const target = page.annotations[0]!
    if (target.kind !== 'plainNote') throw new Error('expected a note')
    const editing = lane.coordinator.command({ kind: 'edit', note: target.id, contents: 'Updated' })
    const refused = expect(editing).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
    const dispatch = await lane.take()
    const reply = {
      ...addReply(addDispatch.request, initial),
      requestId: dispatch.request.requestId,
      binding: accepted.binding,
    }
    const snapshot = {
      ...accepted,
      binding: { ...accepted.binding, revision: 2 },
      consumedRequest: dispatch.request.requestId,
      scene: { ...accepted.scene, pages: [{
        ...page, annotations: [{ ...target, id: target.id + 1, contents: 'Updated' }],
      }] },
    }
    lane.deliver(dispatch, { ...reply, snapshot })
    await refused
    expect(lane.coordinator.snapshot().accepted).toBe(accepted)

    const reconciling = lane.coordinator.reconcile()
    const invalidReadback = expect(reconciling).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
    const lookup = await lane.take()
    lane.deliver(lookup, { ...reply, operation: 'snapshot', status: 'accepted', snapshot })
    await invalidReadback
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'reconciliationRequired', accepted })
    lane.coordinator.dispose()
  })

  it.each(['applied', 'reconciled'] as const)('rejects field readback from a different target through %s', async (mode) => {
    const lane = fakePort()
    const opening = lane.coordinator.open(original())
    const openDispatch = await lane.take()
    const open = openReply(openDispatch.request)
    const field = {
      id: hash(40), object: { objectNumber: 4, generation: 0 }, page: 0, name: 'approval',
      kind: 'checkbox', rect: [10, 10, 35, 35], checked: false, value: null, canEditAppearance: true, refusal: null,
    } as const
    lane.deliver(openDispatch, { ...open, snapshot: { ...open.snapshot, fields: [field] } })
    const previous = await opening
    const applying = lane.coordinator.command({ kind: 'setFieldChecked', field: field.id, checked: true })
    const refused = expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
    const command = await lane.take()
    const outputHash = hash(17)
    const reply = {
      ...open, operation: 'command', requestId: command.request.requestId, status: 'applied',
      snapshot: {
        ...previous, binding: { ...previous.binding, revision: 1 }, consumedRequest: command.request.requestId,
        outputHash, outputHex: '04', scene: { ...previous.scene, outputHash, sceneHash: hash(18) },
        fields: [{ ...field, id: hash(41), name: 'other', checked: true }],
      },
    } as const
    if (mode === 'applied') {
      lane.deliver(command, reply)
      await refused
    } else {
      lane.deliver(command, { ...reply, resultHash: 'invalid' })
      await refused
      const reconciling = lane.coordinator.reconcile()
      const rejection = expect(reconciling).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
      lane.deliver(await lane.take(), { ...reply, operation: 'snapshot', status: 'accepted' })
      await rejection
    }
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'reconciliationRequired', accepted: previous })
    lane.coordinator.dispose()
  })

  it.each(['applied', 'reconciled'] as const)('publishes a verified text replacement (%s) and rejects stale queued targets', async (mode) => {
    const lane = fakePort()
    const initial = await opened(lane, true)
    const applying = lane.coordinator.command(replaceText)
    const outcome = mode === 'reconciled'
      ? expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
      : null
    const stale = expect(lane.coordinator.command(replaceText)).rejects.toMatchObject({
      code: 'FLOW_PDF_NATIVE_TARGET_UNAVAILABLE',
    })
    const dispatch = await lane.take()
    expect(dispatch.request).toMatchObject({ binding: initial.binding, outputHash: initial.outputHash, command: replaceText })
    expect(lane.coordinator.snapshot().accepted).toBe(initial)
    const outputHash = hash(42)
    const snapshot = {
      ...initial, binding: { ...initial.binding, revision: 1 },
      outputHash, outputHex: '04', consumedRequest: dispatch.request.requestId,
      scene: { ...initial.scene, outputHash, sceneHash: hash(43), textPages: [{ page: 0, refusal: null,
        islands: [{ ...textIsland, id: hash(44), text: replaceText.text }],
      }] },
    }
    const receipt = {
      protocolVersion: 1, requestId: dispatch.request.requestId, operation: 'command',
      binding: initial.binding, originStoreHash: initial.originStoreHash, status: 'applied',
      snapshot, code: null, limits: NATIVE_PDF_LIMITS, resultHash: 'bb'.repeat(32),
    } as const
    if (mode === 'reconciled') {
      lane.deliver(dispatch, { ...receipt, snapshot: {
        ...snapshot, scene: { ...snapshot.scene, textPages: [{ page: 0, refusal: null, islands: [
          { ...textIsland, id: hash(44) },
          { ...textIsland, id: hash(45), text: replaceText.text },
        ] }] },
      } })
      await outcome
      expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'reconciliationRequired', accepted: initial })
      const reconciling = lane.coordinator.reconcile()
      const lookup = await lane.take()
      lane.deliver(lookup, { ...receipt, operation: 'snapshot', status: 'accepted' })
      await expect(reconciling).resolves.toMatchObject({ status: 'accepted', snapshot })
    } else {
      lane.deliver(dispatch, receipt)
      await expect(applying).resolves.toEqual(snapshot)
    }
    await stale
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'ready', accepted: snapshot })
    expect(lane.sent).toEqual([])
    lane.coordinator.dispose()
  })

  it('accepts only the ordered object capabilities that fit the graphics-state limit', () => {
    const reply = openReply({
      protocolVersion: 1, operation: 'open', requestId: 1,
      epoch: Array<number>(16).fill(1), originStoreHash: storeHash, bytesHex: '010203',
    })
    const graphic = { id: hash(51), kind: 'graphic', profile: 'rectangle', bounds: [1000, 1000, 3000, 4000] } as const
    const image = { ...graphic, id: hash(52), kind: 'image', profile: 'rasterGray8' } as const
    const withObjects = (objects: readonly unknown[]) => readNativeResponse(JSON.stringify({
      ...reply, snapshot: { ...reply.snapshot, scene: { ...reply.snapshot.scene,
        objectPages: [{ page: 0, canAdd: true, refusal: null, objects }],
      } },
    }))
    const full = ['replace', 'move', 'remove']
    const objects = [
      { ...graphic, operations: full },
      { ...graphic, id: hash(53), operations: ['move', 'remove'] },
      { ...graphic, id: hash(54), operations: ['remove'] },
      { ...image, operations: ['replace', 'remove'] },
      { ...image, id: hash(55), operations: full },
    ]
    const decoded = withObjects(objects)
    if (decoded?.status !== 'opened') throw new Error('expected admitted object capabilities')
    expect(decoded.snapshot.scene.objectPages?.[0]?.objects).toEqual(objects)
    const selected = decoded.snapshot.scene.objectPages?.[0]?.objects[3]
    const asset = { name: 'replacement.png', status: 'ready',
      hex: '89504e470d0a1a0a0000000d49484452000000010000000108000000003a7e9b55'
        + '0000000a49444154789c63880600005d005cdc83ebd80000000049454e44ae426082' } as const
    expect(nativeImageCommand(0, selected, null, asset)).toEqual({
      kind: 'replaceImage', target: { page: 0, id: image.id, profile: image.profile }, assetHex: asset.hex,
    })

    for (const operations of [[], ['move'], ['remove', 'move'], ['move', 'move', 'remove'],
      ['replace', 'move', 'remove', 'remove'], ['rotate', 'remove'], ['replace', 'remove']]) {
      expect(withObjects([{ ...graphic, operations }])).toBeNull()
    }
    for (const operations of [['remove'], ['move', 'remove'], ['move', 'replace', 'remove']]) {
      expect(withObjects([{ ...image, operations }])).toBeNull()
    }
  })

  it.each([
    ['image', 'applied'], ['graphic', 'applied'], ['image', 'reconciled'], ['graphic', 'reconciled'],
  ] as const)('confirms an ordinary %s change (%s)', async (kind, mode) => {
    const lane = fakePort()
    const opening = lane.coordinator.open(original())
    const openDispatch = await lane.take()
    const openedReply = openReply(openDispatch.request)
    const selected = { id: hash(51), kind: 'graphic', profile: 'rectangle',
      bounds: [1000, 1000, 3000, 4000], operations: ['replace', 'move', 'remove'] } as const
    const sibling = { ...selected, id: hash(52), bounds: [7000, 7000, 8000, 9000] } as const
    const objectPage = { page: 0, canAdd: true, refusal: null, objects: [sibling, selected] } as const
    lane.deliver(openDispatch, { ...openedReply, snapshot: { ...openedReply.snapshot,
      scene: { ...openedReply.snapshot.scene, objectPages: [objectPage] },
    } })
    const initial = await opening
    const command = kind === 'image'
      ? { kind: 'addImage', page: 0, bounds: [2000, 2000, 6000, 6000],
          assetHex: '89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de'
            + '0000000c49444154789c63f8cfc0000003010100c9fe92ef0000000049454e44ae426082' } as const
      : { kind: 'moveObject', target: { page: 0, id: selected.id, profile: selected.profile },
          transform: [1, 0, 0, 1, 1, 2] } as const
    const applying = lane.coordinator.command(command)
    const dispatch = await lane.take()
    const changed = kind === 'image'
      ? { ...selected, id: hash(53), kind, profile: 'rasterRgb8', bounds: [2000, 2000, 6000, 6000] } as const
      : { ...selected, id: hash(53), bounds: [2000, 3000, 4000, 6000] } as const
    const unchangedSibling = { ...sibling, id: hash(54) }
    const unchangedTarget = { ...selected, id: hash(55) }
    const objects = kind === 'image' ? [unchangedSibling, unchangedTarget, changed] : [unchangedSibling, changed]
    const outputHash = hash(56)
    const snapshot = {
      ...initial, binding: { ...initial.binding, revision: 1 }, outputHash, outputHex: '04',
      consumedRequest: dispatch.request.requestId, scene: { ...initial.scene, outputHash,
        sceneHash: hash(57), objectPages: [{ ...objectPage, objects }] },
    }
    const receipt = { ...openedReply, operation: 'command', status: 'applied',
      requestId: dispatch.request.requestId, snapshot } as const
    if (mode === 'reconciled') {
      const refused = expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
      const wrongObjects = kind === 'image' ? [unchangedSibling, changed, unchangedTarget] : [changed, unchangedTarget]
      const wrong = { ...snapshot, scene: { ...snapshot.scene, objectPages: [{ ...objectPage, objects: wrongObjects }] } }
      lane.deliver(dispatch, { ...receipt, snapshot: wrong })
      await refused
      const retry = lane.coordinator.reconcile()
      const lookup = await lane.take()
      const rejected = expect(retry).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
      lane.deliver(lookup, { ...receipt, operation: 'snapshot', status: 'accepted', snapshot: wrong })
      await rejected
      expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'reconciliationRequired', accepted: initial })
      const reconciling = lane.coordinator.reconcile()
      lane.deliver(await lane.take(), { ...receipt, operation: 'snapshot', status: 'accepted' })
      await expect(reconciling).resolves.toMatchObject({ status: 'accepted', snapshot })
    } else {
      lane.deliver(dispatch, receipt)
      await expect(applying).resolves.toEqual(snapshot)
    }
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'ready', accepted: snapshot })
    lane.coordinator.dispose()
  })

  it('requires a current verified text island before dispatch', async () => {
    const lane = fakePort()
    const initial = await opened(lane)
    await expect(lane.coordinator.command(replaceText)).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_TARGET_UNAVAILABLE' })
    expect(lane.coordinator.snapshot().accepted).toBe(initial)
    expect(lane.sent).toEqual([])
    lane.coordinator.dispose()
  })

  it('returns to idle after a verified reader refusal and allows another open', async () => {
    const lane = fakePort()
    const opening = lane.coordinator.open(original())
    const rejected = expect(opening).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_IMPORT_REFUSED' })
    const dispatch = await lane.take()
    lane.deliver(dispatch, {
      ...openReply(dispatch.request),
      status: 'failed', binding: null, snapshot: null, code: 'FLOW_PDF_NATIVE_IMPORT_REFUSED',
    })
    await rejected
    expect(lane.verify).toHaveBeenCalledTimes(1)
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'idle', accepted: null, reopenOriginal: false })
    await opened(lane)
    expect(lane.coordinator.snapshot().phase).toBe('ready')
  })

  it.each([
    'FLOW_PDF_NATIVE_BUDGET_EXHAUSTED',
    'FLOW_PDF_NATIVE_POSTCONDITION_FAILED',
  ])('continues FIFO after a verified mutation refusal (%s)', async (code) => {
    const lane = fakePort()
    const initial = await opened(lane)
    const first = lane.coordinator.command(note('first'))
    const rejected = expect(first).rejects.toMatchObject({ code })
    const second = lane.coordinator.command(note('second'))
    const edit = await lane.take()
    lane.deliver(edit, { ...addReply(edit.request, initial), status: 'failed', snapshot: null, code })
    await rejected

    const next = await lane.take()
    expect(lane.coordinator.snapshot().accepted).toBe(initial)
    expect(lane.coordinator.outputBytes()).toEqual(original().bytes)
    expect(next.request.requestId).toBeGreaterThan(edit.request.requestId)
    expect(next.request).toMatchObject({ binding: initial.binding, outputHash: initial.outputHash })
    lane.deliver(next, addReply(next.request, initial))
    expect((await second).binding.revision).toBe(1)
    expect(lane.verify).toHaveBeenCalledTimes(3)
    lane.coordinator.dispose()
  })

  it('orders commands, owns input and output copies, and ignores projection cancellation and duplicate replies', async () => {
    const lane = fakePort()
    const input = original()
    const opening = lane.coordinator.open(input)
    input.bytes.fill(9)
    const open = await lane.take()
    lane.deliver(open, openReply(open.request))
    const initial = await opening
    expect(initial.originStoreHash).toBe(storeHash)
    expect(initial.binding.originHash).toEqual(hash(1))

    const controller = new AbortController()
    const projection = lane.coordinator.project(controller.signal)
    const read = await lane.take()
    const first = lane.coordinator.command(note('first'))
    const command = { ...note('queued'), contents: 'queued' }
    const second = lane.coordinator.command(command)
    command.contents = 'caller mutation'
    const edit = await lane.take()
    expect(lane.sent).toHaveLength(0)
    expect(edit.request).toMatchObject({ binding: { revision: 0 }, outputHash: initial.outputHash })
    await expect(lane.coordinator.open(original())).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_OPEN_BUSY' })
    await expect(lane.coordinator.close()).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_CLOSE_BUSY' })
    controller.abort()
    const firstReply = addReply(edit.request, initial)
    lane.deliver(edit, firstReply)
    const accepted = await first
    expect(await projection).toBeNull()
    lane.deliver(read, firstReply)
    lane.deliver(edit, firstReply)
    const next = await lane.take()
    expect(next.request).toMatchObject({ binding: { revision: 1 }, command: { contents: 'queued' } })
    const secondReply = addReply(next.request, accepted)
    lane.deliver(next, secondReply)
    const final = await second
    expect(final.binding.revision).toBe(2)
    expect(Object.isFrozen(final.scene.pages[0]!.annotations[0])).toBe(true)
    expect(Object.isFrozen(final.binding.originHash)).toBe(true)
    lane.coordinator.outputBytes()!.fill(99)
    expect(lane.coordinator.outputBytes()).toEqual(Uint8Array.of(2))
    expect(lane.verify).toHaveBeenCalledTimes(3)
    lane.coordinator.dispose()
  })

  it('pauses FIFO on a lost mutation reply and reconciles the same request ID before normal work', async () => {
    vi.useFakeTimers()
    const lane = fakePort(20)
    const initial = await opened(lane)
    const first = lane.coordinator.command(note('first'))
    const lost = expect(first).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
    const queued = lane.coordinator.command(note('second'))
    const edit = await lane.take()
    const accepted = addReply(edit.request, initial)
    await vi.advanceTimersByTimeAsync(20)
    await lost
    expect(lane.coordinator.snapshot().accepted).toBe(initial)
    expect(lane.sent).toHaveLength(0)
    await expect(lane.coordinator.command(note('third'))).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED' })
    const reconciling = lane.coordinator.reconcile()
    const lookup = await lane.take()
    expect(lookup.request).toMatchObject({ operation: 'snapshot', requestId: edit.request.requestId, binding: initial.binding })
    expect(lookup.dispatchId).not.toBe(edit.dispatchId)
    lane.deliver(edit, accepted)
    expect(lane.coordinator.snapshot().accepted).toBe(initial)
    lane.deliver(lookup, { ...accepted, operation: 'snapshot', status: 'accepted' })
    expect((await reconciling).snapshot.binding.revision).toBe(1)
    const next = await lane.take()
    expect(next.request).toMatchObject({ operation: 'command', binding: { revision: 1 } })
    lane.deliver(next, addReply(next.request, accepted.snapshot))
    expect((await queued).binding.revision).toBe(2)
    lane.coordinator.dispose()
  })

  it.each(['refused', 'lost'] as const)('reopens verified bytes after a %s ordinary readback without replaying edits', async (result) => {
    vi.useFakeTimers()
    const lane = fakePort(20)
    const sent = vi.spyOn(lane.worker, 'postMessage')
    const initial = await opened(lane)
    const saved = lane.coordinator.command(note('verified edit'))
    const savedDispatch = await lane.take()
    const applied = addReply(savedDispatch.request, initial)
    const bytes = nativeHexBytes(applied.snapshot.outputHex)
    lane.deliver(savedDispatch, applied)
    const accepted = await saved

    const lost = expect(lane.coordinator.command(note('uncertain edit'))).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
    const queued = expect(lane.coordinator.command(note('queued edit'))).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_WORKER_FAILED' })
    const edit = await lane.take()
    const unverified = addReply(edit.request, accepted)
    await vi.advanceTimersByTimeAsync(20)
    await lost
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'reconciliationRequired', reopenAccepted: false })

    const code = result === 'refused' ? 'FLOW_PDF_NATIVE_REVISION_MISMATCH' : 'FLOW_PDF_NATIVE_REPLY_LOST'
    const readback = expect(lane.coordinator.reconcile()).rejects.toMatchObject({ code })
    const lookup = await lane.take()
    expect(lookup.request).toMatchObject({ operation: 'snapshot', requestId: edit.request.requestId, binding: accepted.binding })
    if (result === 'refused') {
      lane.deliver(lookup, { ...unverified, operation: 'snapshot', status: 'failed', snapshot: null, code })
    } else {
      await vi.advanceTimersByTimeAsync(20)
    }
    await readback
    lane.deliver(edit, unverified)
    lane.deliver(lookup, { ...unverified, operation: 'snapshot', status: 'accepted' })
    expect(lane.coordinator.snapshot()).toMatchObject({
      phase: 'reconciliationRequired', pendingRequestId: edit.request.requestId, reopenAccepted: true, reopenOriginal: false,
    })
    expect(lane.coordinator.snapshot().accepted).toBe(accepted)
    expect(lane.coordinator.outputBytes()).toEqual(bytes)
    await expect(lane.coordinator.command(note('blocked edit'))).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED' })
    expect(sent.mock.calls.map(([message]) => message.request.operation)).toEqual(['open', 'command', 'command', 'snapshot'])

    const replacement = fakePort()
    const originStoreHash = (await hashPdfSourceBytes(bytes)).slice('sha256:v1:'.length)
    expect(originStoreHash).not.toBe(nativeBytesHex(Uint8Array.from(accepted.outputHash)))
    expect(originStoreHash).not.toBe(accepted.originStoreHash)
    const reopening = replacement.coordinator.open({ bytes: lane.coordinator.outputBytes()!, originStoreHash })
    const open = await replacement.take()
    expect(open.request).toMatchObject({ operation: 'open', originStoreHash, bytesHex: accepted.outputHex })
    const openedReply = openReply(open.request)
    replacement.deliver(open, { ...openedReply, snapshot: {
      ...openedReply.snapshot, scene: { ...openedReply.snapshot.scene, pages: accepted.scene.pages },
    } })
    const reopened = await reopening
    lane.coordinator.dispose()
    await queued
    expect(lane.sent).toEqual([])
    expect(replacement.sent).toEqual([])
    expect(replacement.coordinator.snapshot()).toMatchObject({ phase: 'ready', reopenAccepted: false })

    const next = note('new intent')
    const editing = replacement.coordinator.command(next)
    const nextDispatch = await replacement.take()
    expect(nextDispatch.request).toMatchObject({ operation: 'command', binding: reopened.binding, command: next })
    const edited = addReply(nextDispatch.request, reopened)
    const page = edited.snapshot.scene.pages[0]!
    const [previous, added] = page.annotations
    if (!previous || added?.kind !== 'plainNote') throw new Error('expected two notes')
    replacement.deliver(nextDispatch, { ...edited, snapshot: {
      ...edited.snapshot, scene: { ...edited.snapshot.scene, pages: [{
        ...page, annotations: [previous, { ...added, id: 2, object: { objectNumber: 5, generation: 0 } }],
      }] },
    } })
    await expect(editing).resolves.toMatchObject({ binding: { revision: 1 } })
    replacement.coordinator.dispose()
  })

  it.each(['unseen', 'evicted'] as const)('reconciles %s history without replaying the uncertain command', async (status) => {
    vi.useFakeTimers()
    const lane = fakePort(20)
    const initial = await opened(lane)
    const edit = lane.coordinator.command(note('possibly accepted'))
    const lost = expect(edit).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
    const sent = await lane.take()
    await vi.advanceTimersByTimeAsync(20)
    await lost
    const reconciling = lane.coordinator.reconcile()
    const lookup = await lane.take()
    const reply = addReply(sent.request, initial)
    const current = status === 'unseen' ? initial : {
      ...reply.snapshot, binding: { ...initial.binding, revision: 129 },
      consumedRequest: sent.request.requestId + 128,
    }
    lane.deliver(lookup, { ...reply, operation: 'snapshot', status, snapshot: current })
    expect(await reconciling).toEqual({ status, snapshot: current })
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'ready', accepted: current })
    expect(lane.sent).toHaveLength(0)
    const next = lane.coordinator.command(note('new intent'))
    const dispatched = await lane.take()
    expect(dispatched.request.requestId).toBeGreaterThan(current.consumedRequest)
    expect(dispatched.request.requestId).not.toBe(sent.request.requestId)
    lane.deliver(dispatched, addReply(dispatched.request, current))
    await next
    lane.coordinator.dispose()
  })

  it('rejects an eviction claim inside the retained request history window', async () => {
    vi.useFakeTimers()
    const lane = fakePort(20)
    const initial = await opened(lane)
    const edit = lane.coordinator.command(note('possibly accepted'))
    const lost = expect(edit).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
    const sent = await lane.take()
    await vi.advanceTimersByTimeAsync(20)
    await lost

    const reconciling = lane.coordinator.reconcile()
    const refused = expect(reconciling).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
    const lookup = await lane.take()
    const reply = addReply(sent.request, initial)
    const current = {
      ...reply.snapshot,
      binding: { ...initial.binding, revision: 2 },
      consumedRequest: sent.request.requestId + 1,
    }
    lane.deliver(lookup, { ...reply, operation: 'snapshot', status: 'evicted', snapshot: current })
    await refused
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'reconciliationRequired', accepted: initial })
    lane.coordinator.dispose()
  })

  it.each(['worker loss', 'disposal'] as const)('settles pending and queued commands after %s when termination throws', async (mode) => {
    vi.useFakeTimers()
    const lane = fakePort()
    const initial = await opened(lane)
    const cleanupFailure = new Error('Worker termination unavailable')
    vi.mocked(lane.worker.terminate).mockImplementation(() => { throw cleanupFailure })
    const first = expect(lane.coordinator.command(note('in flight'))).rejects
      .toMatchObject({ code: 'FLOW_PDF_NATIVE_WORKER_FAILED' })
    const queued = expect(lane.coordinator.command(note('queued'))).rejects
      .toMatchObject({ code: 'FLOW_PDF_NATIVE_WORKER_FAILED' })
    const dispatched = await lane.take()
    const handler = lane.worker.onmessage
    const listener = vi.fn()
    lane.coordinator.subscribe(listener)
    if (mode === 'worker loss') {
      expect(() => lane.worker.onmessageerror?.(new MessageEvent('messageerror'))).not.toThrow()
    } else {
      expect(() => lane.coordinator.dispose()).toThrow(cleanupFailure)
    }
    expect(vi.getTimerCount()).toBe(0)
    await Promise.all([first, queued])
    handler?.(new MessageEvent('message', { data: { dispatchId: dispatched.dispatchId, type: 'response', json: JSON.stringify(addReply(dispatched.request, initial)) } }))
    expect(lane.sent).toHaveLength(0)
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'invalidated', accepted: initial, reopenOriginal: true })
    expect(lane.coordinator.originalForReopen()).toEqual(original())
    expect(listener).toHaveBeenCalledOnce()
    expect(lane.worker.onmessage).toBeNull()
    expect(lane.worker.onerror).toBeNull()
    expect(lane.worker.onmessageerror).toBeNull()
    expect(() => lane.coordinator.dispose()).not.toThrow()
    expect(lane.worker.terminate).toHaveBeenCalledOnce()
  })

  it('refuses a forged cross-source reply and preserves the verified output on worker loss', async () => {
    const lane = fakePort()
    const initial = await opened(lane)
    const first = lane.coordinator.command(note('first'))
    const edit = await lane.take()
    const verified = addReply(edit.request, initial)
    lane.deliver(edit, verified)
    const accepted = await first
    const writing = lane.coordinator.command(note('pending'))
    const waiting = lane.take()
    const rejected = expect(writing).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
    const dispatch = await waiting
    const other = addReply(dispatch.request, accepted)
    const originHash = hash(99)
    lane.deliver(dispatch, { ...other, binding: { ...other.binding, originHash },
      snapshot: { ...other.snapshot, binding: { ...other.snapshot.binding, originHash } } })
    await rejected

    const reconciling = lane.coordinator.reconcile()
    const failed = expect(reconciling).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_WORKER_FAILED' })
    await lane.take()
    const oldHandler = lane.worker.onmessage
    const listener = vi.fn()
    lane.coordinator.subscribe(listener)
    lane.worker.onerror?.(new Event('error') as ErrorEvent)
    await failed
    oldHandler?.(new MessageEvent('message', { data: { dispatchId: dispatch.dispatchId, type: 'response', json: JSON.stringify(other) } }))
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'invalidated', accepted, reopenOriginal: true })
    expect(lane.worker.terminate).toHaveBeenCalledOnce()
    expect(lane.worker.onmessage).toBeNull()
    expect(listener).toHaveBeenCalledOnce()
    lane.coordinator.originalForReopen()!.bytes.fill(0)
    expect(lane.coordinator.originalForReopen()).toEqual(original())
    await expect(lane.coordinator.open(original())).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_OPEN_BUSY' })
  })

  it('requires Rust verification for worker replies even when the result has a hash', async () => {
    const lane = fakePort()
    lane.verify.mockReturnValue(false)
    const opening = lane.coordinator.open(original())
    const rejected = expect(opening).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
    const dispatch = await lane.take()
    lane.deliver(dispatch, openReply(dispatch.request))
    await rejected
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'invalidated', accepted: null })
  })

  it('keeps the verified import available for explicit reopen when the worker dies during open', async () => {
    const lane = fakePort()
    const opening = lane.coordinator.open(original())
    const rejected = expect(opening).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_WORKER_FAILED' })
    const dispatch = await lane.take()
    const late = lane.worker.onmessage
    lane.worker.onerror?.(new Event('error') as ErrorEvent)
    await rejected
    late?.(new MessageEvent('message', { data: {
      dispatchId: dispatch.dispatchId, type: 'response', json: JSON.stringify(openReply(dispatch.request)),
    } }))
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'invalidated', accepted: null, reopenOriginal: true })
    expect(lane.coordinator.originalForReopen()).toEqual(original())
    expect(lane.sent).toHaveLength(0)
  })

  it('checks the browser origin-store hash before dispatch and closes idempotently without resurrection', async () => {
    const lane = fakePort()
    await expect(lane.coordinator.open({ ...original(), originStoreHash: '00'.repeat(32) }))
      .rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REQUEST_INVALID' })
    expect(lane.sent).toHaveLength(0)
    const listener = vi.fn()
    const unsubscribe = lane.coordinator.subscribe(listener)
    const accepted = await opened(lane)
    expect(listener).toHaveBeenCalledTimes(2)
    unsubscribe()
    const projection = lane.coordinator.project()
    const projected = await lane.take()
    const close = lane.coordinator.close()
    expect(lane.coordinator.close()).toBe(close)
    const dispatch = await lane.take()
    if (projected.request.operation !== 'snapshot') throw new Error('expected projection')
    await expect(projection).resolves.toBeNull()
    lane.deliver(projected, {
      protocolVersion: 1, requestId: projected.request.requestId, operation: 'snapshot', status: 'unseen',
      binding: accepted.binding, originStoreHash: accepted.originStoreHash, snapshot: accepted, code: null,
      resultHash: 'ff'.repeat(32), limits: NATIVE_PDF_LIMITS,
    })
    expect(lane.verify).toHaveBeenCalledOnce()
    if (dispatch.request.operation !== 'close') throw new Error('expected close')
    await expect(lane.coordinator.open(original())).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_OPEN_BUSY' })
    const reply: NativePdfResponse = {
      protocolVersion: 1, requestId: dispatch.request.requestId, operation: 'close', status: 'closed',
      binding: accepted.binding, originStoreHash: accepted.originStoreHash, snapshot: null, code: null,
      resultHash: 'ff'.repeat(32), limits: NATIVE_PDF_LIMITS,
    }
    const late = lane.worker.onmessage
    const reopening = vi.fn(() => lane.coordinator.open(original()))
    const unsubscribeReopen = lane.coordinator.subscribe(() => {
      if (lane.coordinator.snapshot().phase === 'idle') void reopening()
    })
    lane.deliver(dispatch, reply)
    await close
    unsubscribeReopen()
    expect(reopening).toHaveBeenCalledOnce()
    const replacement = reopening.mock.results[0]!.value
    await expect(lane.coordinator.close()).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_CLOSE_BUSY' })
    const opening = await lane.take()
    late?.(new MessageEvent('message', { data: { dispatchId: dispatch.dispatchId, type: 'response', json: JSON.stringify(reply) } }))
    expect(lane.coordinator.snapshot().accepted).toBeNull()
    lane.deliver(opening, openReply(opening.request))
    expect((await replacement).binding.epoch).not.toEqual(accepted.binding.epoch)
    expect(listener).toHaveBeenCalledTimes(2)
    lane.coordinator.dispose()
  })
})

describe('native worker scope', () => {
  const request: NativePdfRequest = {
    protocolVersion: 1, requestId: 1, operation: 'open',
    originStoreHash: storeHash, epoch: Array<number>(16).fill(9), bytesHex: '010203',
  }

  it('buffers startup in order and verifies each Rust reply before posting it', async () => {
    const startup = deferred<NativePdfWasmBoundary>()
    const posted: NativePdfWorkerResponse[] = []
    const scope: NativePdfWorkerScope = { onmessage: null, postMessage: (message) => { posted.push(message) } }
    const run = vi.fn((json: string) => JSON.stringify(openReply(JSON.parse(json) as NativePdfRequest)))
    const verify = vi.fn((_json: string) => true).mockReturnValueOnce(false)
    installNativePdfWorker(scope, startup.promise)
    scope.onmessage?.({ data: { dispatchId: 1, request } })
    scope.onmessage?.({ data: { dispatchId: 2, request: { ...request, requestId: 2 } } })
    expect(posted).toEqual([])
    startup.resolve({
      native_pdf_open: run, native_pdf_command: run, native_pdf_snapshot: run,
      native_pdf_close: run, native_pdf_redaction: run, verify_native_pdf_response: verify,
    })
    await vi.waitFor(() => expect(posted).toHaveLength(2), { interval: 1 })
    expect(posted).toMatchObject([
      { dispatchId: 1, type: 'failed', code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' },
      { dispatchId: 2, type: 'response' },
    ])
    expect(run.mock.calls.map(([json]) => (JSON.parse(json) as NativePdfRequest).requestId)).toEqual([1, 2])
    expect(verify).toHaveBeenCalledTimes(2)
    scope.onmessage?.({ data: { dispatchId: 3, request: { ...request, unknown: true } } })
    expect(posted[2]).toEqual({ dispatchId: 3, type: 'failed', code: 'FLOW_PDF_NATIVE_REQUEST_INVALID' })
    expect(run).toHaveBeenCalledTimes(2)
  })

  it('reports startup failure with correlation and no document data', async () => {
    const startup = deferred<NativePdfWasmBoundary>()
    const postMessage = vi.fn()
    const scope: NativePdfWorkerScope = { onmessage: null, postMessage }
    installNativePdfWorker(scope, startup.promise)
    scope.onmessage?.({ data: { dispatchId: 1, request } })
    startup.reject()
    await vi.waitFor(() => expect(postMessage).toHaveBeenCalledWith({
      dispatchId: 1, type: 'failed', code: 'FLOW_PDF_NATIVE_WORKER_STARTUP',
    }), { interval: 1 })
  })
})

const mark = { kind: 'addMark', pageId: 196608, rect: [1, 2, 3, 4] } as const
const redactionCandidate: NativePdfRedactionCandidate = {
  hash: hash(72), validatorVersion: 1, validatorProfile: 'classic-xref-owned-144dpi-v1',
  affectedPages: [196608],
}

function redactionReply(
  request: NativePdfRequest, candidate: NativePdfRedactionCandidate | null = null,
  status: NativePdfRedactionVerification['status'] = 'pending',
): Extract<NativePdfResponse, { status: 'redaction' }> {
  if (request.operation !== 'redaction') throw new Error('expected redaction')
  const action = request.action
  if (action.kind === 'cancel') candidate ??= action.candidate
  const check = { status, coveredPages: status === 'passed' ? 1 : 0 }
  const verification = candidate === null || action.kind === 'cancel' ? null : { status, structure: check, text: check, raster: check }
  return {
    ...boundRedactionReply(request), status: 'redaction', snapshot: null,
    redaction: {
      action: action.kind, acceptedHash: request.acceptedHash, markCount: 1,
      candidate, verification, published: false,
    },
  }
}

function boundRedactionReply(request: Extract<NativePdfRequest, { operation: 'redaction' }>) {
  return {
    protocolVersion: 1, requestId: request.requestId, operation: 'redaction',
    binding: request.binding, originStoreHash: request.originStoreHash, code: null,
    limits: NATIVE_PDF_LIMITS, resultHash: 'ab'.repeat(32),
  } as const
}

function markSnapshotReply(request: NativePdfRequest, snapshot: NativePdfVerifiedSnapshot) {
  if (request.operation !== 'snapshot') throw new Error('expected snapshot')
  return {
    ...request, status: 'unseen', snapshot, code: null,
    limits: NATIVE_PDF_LIMITS, resultHash: 'cd'.repeat(32),
  } as const
}

async function redactionMarked(lane: ReturnType<typeof fakePort>) {
  const initial = await opened(lane)
  const marking = lane.coordinator.markRedaction(mark, 1)
  const dispatch = await lane.take()
  expect(dispatch.request).toMatchObject({ operation: 'redaction', action: mark })
  lane.deliver(dispatch, redactionReply(dispatch.request))
  const readback = await lane.take()
  lane.deliver(readback, markSnapshotReply(readback.request, initial))
  await marking
  return initial
}

function redactionAction(dispatch: NativePdfWorkerRequest): NativePdfRedactionAction {
  if (dispatch.request.operation !== 'redaction') throw new Error('expected redaction')
  return dispatch.request.action
}

async function openRedactionFixture(lane: ReturnType<typeof fakePort>) {
  await initializeWasm()
  const paint = '10 10 10 10 re f 40 40 10 10 re f'
  const bytes = new TextEncoder().encode(fixturePdf([
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 /MediaBox [0 0 100 100] >>',
    '<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>',
    `<< /Length ${paint.length} >>\nstream\n${paint}\nendstream`,
    '<< /Type /Page /Parent 2 0 R >>',
  ]))
  const originStoreHash = nativeBytesHex(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)))
  const opening = lane.coordinator.open({ bytes, originStoreHash })
  const dispatch = await lane.take()
  lane.deliver(dispatch, JSON.parse(wasm.native_pdf_open(JSON.stringify(dispatch.request))))
  return opening
}

async function deliverWasmRedaction(lane: ReturnType<typeof fakePort>, kind: NativePdfRedactionAction['kind']) {
  const dispatch = await lane.take()
  expect(redactionAction(dispatch).kind).toBe(kind)
  const reply = JSON.parse(wasm.native_pdf_redaction(JSON.stringify(dispatch.request))) as NativePdfResponse
  lane.deliver(dispatch, reply)
  return reply
}

async function deliverWasmMark(lane: ReturnType<typeof fakePort>, kind: 'addMark' | 'removeMark' | 'clearMarks') {
  const receipt = await deliverWasmRedaction(lane, kind)
  const readback = await lane.take()
  expect(readback.request).toMatchObject({ operation: 'snapshot', requestId: receipt.requestId })
  lane.deliver(readback, JSON.parse(wasm.native_pdf_snapshot(JSON.stringify(readback.request))))
}

async function closeRedactionFixture(lane: ReturnType<typeof fakePort>) {
  const closing = lane.coordinator.close()
  const dispatch = await lane.take()
  lane.deliver(dispatch, JSON.parse(wasm.native_pdf_close(JSON.stringify(dispatch.request))))
  await closing
  lane.coordinator.dispose()
}

describe('native redaction quarantine', () => {
  it.each([0, 90] as const)('uses the exact fractional crop for marks on a %i° page', async (rotation) => {
    await initializeWasm()
    const lane = fakePort(30_000, wasm.verify_native_pdf_response)
    const bytes = new TextEncoder().encode(fixturePdf([
      '<< /Type /Catalog /Pages 2 0 R >>',
      '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
      `<< /Type /Page /Parent 2 0 R /MediaBox [-10 -10 110 110] /CropBox [0.125 -0.375 100.625 50.125] /Rotate ${rotation} >>`,
    ]))
    const originStoreHash = nativeBytesHex(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)))
    const opening = lane.coordinator.open({ bytes, originStoreHash })
    const open = await lane.take()
    lane.deliver(open, JSON.parse(wasm.native_pdf_open(JSON.stringify(open.request))))
    const accepted = await opening
    const pageId = accepted.pageState.order[0]!
    expect(accepted.scene.pages[0]?.geometry).toEqual({ bounds: [1, 0, 100, 50], rotation })
    expect(accepted.redactionAdmission).toBe('supported')

    const reading = lane.coordinator.previewPage(pageId, new AbortController().signal)
    const previewRequest = await lane.take()
    lane.deliver(previewRequest, JSON.parse(wasm.native_pdf_preview(JSON.stringify(previewRequest.request))))
    const preview = await reading
    expect(preview).toMatchObject({ cropBox: [125, -375, 100_625, 50_125], rotation })
    if (!preview) throw new Error('expected a verified preview')

    const origin = nativeCropBounds(['0', '0', '.5', '.5'], preview.cropBox)
    const edge = nativeCropBounds(['100', '50', '.5', '.5'], preview.cropBox)
    if (!origin || !edge) throw new Error('expected two in-crop marks')
    for (const [index, rect] of [origin, edge].entries()) {
      const marking = lane.coordinator.markRedaction({ kind: 'addMark', pageId, rect }, index + 1)
      await deliverWasmMark(lane, 'addMark')
      await expect(marking).resolves.toMatchObject({ markCount: index + 1, published: false })
    }
    const outside = [edge[0], edge[1], edge[2] + 1, edge[3]] as const
    const refused = expect(lane.coordinator.markRedaction({ kind: 'addMark', pageId, rect: outside }, 3))
      .rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REDACTION_MARK_INVALID' })
    await deliverWasmRedaction(lane, 'addMark')
    await refused
    expect(lane.coordinator.snapshot().accepted).toBe(accepted)
    const clearing = lane.coordinator.markRedaction({ kind: 'clearMarks' }, 0)
    await deliverWasmMark(lane, 'clearMarks')
    await clearing
    await closeRedactionFixture(lane)
  })

  afterEach(() => { vi.useRealTimers() })

  it.each(['direct', 'retried'] as const)('requires recovery when the first mark disagrees through a %s status reply', async (mode) => {
    const lane = fakePort(500)
    const initial = await opened(lane)
    if (mode === 'retried') vi.useFakeTimers()
    const marking = lane.coordinator.markRedaction(mark, 1)
    const originalDispatch = await lane.take()
    let dispatch = originalDispatch
    let outcome: Promise<unknown> = marking
    if (mode === 'retried') {
      const lost = expect(marking).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
      await vi.advanceTimersByTimeAsync(500)
      await lost
      outcome = lane.coordinator.reconcile()
      dispatch = await lane.take()
      expect(dispatch.request).toEqual(originalDispatch.request)
    }
    const refused = expect(outcome).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
    const result = redactionReply(dispatch.request)
    const reply = { ...result, redaction: { ...result.redaction, markCount: 0 } }
    expect(readNativeResponse(JSON.stringify(reply))).toEqual(reply)
    lane.deliver(dispatch, reply)
    await refused

    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'invalidated', reopenOriginal: true, redaction: null })
    expect(lane.coordinator.snapshot().accepted).toBe(initial)
    expect(lane.coordinator.originalForReopen()).toEqual(original())
    expect(lane.coordinator.outputBytes()).toBeNull()
    await expect(lane.coordinator.command(note('blocked edit'))).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_SESSION_UNAVAILABLE' })
    await expect(lane.coordinator.applyRedaction([mark.pageId], 1)).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_SESSION_UNAVAILABLE' })
    expect(lane.sent).toEqual([])
  })

  it('retains the verified output and receipt when an ordinary command is unseen after publication', async () => {
    const lane = fakePort(500, (json) => wasm.verify_native_pdf_response(json))
    const initial = await openRedactionFixture(lane)
    const pageId = initial.pageState.order[0]!
    const marking = lane.coordinator.markRedaction({ kind: 'addMark', pageId, rect: [12_000, 12_000, 13_000, 13_000] }, 1)
    await deliverWasmMark(lane, 'addMark')
    await marking

    const secondMark = lane.coordinator.markRedaction({ kind: 'addMark', pageId, rect: [42_000, 42_000, 43_000, 43_000] }, 2)
    await deliverWasmMark(lane, 'addMark')
    await secondMark

    const applying = lane.coordinator.applyRedaction([pageId], 2)
    for (const step of ['stage', 'verify'] as const) {
      expect(await deliverWasmRedaction(lane, step)).toMatchObject({
        status: 'redaction', redaction: { markCount: 2, candidate: { affectedPages: [pageId] } },
      })
    }
    await deliverWasmRedaction(lane, 'publish')
    const receipt = await applying
    const published = lane.coordinator.snapshot().accepted!
    expect(receipt).toMatchObject({ published: true, markCount: 0, candidate: { hash: published.outputHash, affectedPages: [pageId] } })
    expect(published.binding.revision).toBe(initial.binding.revision + 1)
    expect(published.outputHex).not.toBe(initial.outputHex)

    vi.useFakeTimers()
    const command = lane.coordinator.command(note('Follow-up note'))
    const lost = expect(command).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
    const edit = await lane.take()
    expect(edit.request).toMatchObject({ operation: 'command', binding: published.binding, outputHash: published.outputHash })
    await vi.advanceTimersByTimeAsync(500)
    await lost

    const reconciling = lane.coordinator.reconcile()
    const lookup = await lane.take()
    expect(lookup.request).toMatchObject({ operation: 'snapshot', requestId: edit.request.requestId, binding: published.binding })
    lane.deliver(lookup, JSON.parse(wasm.native_pdf_snapshot(JSON.stringify(lookup.request))))
    const readback = await reconciling
    expect(readback).toEqual({ status: 'unseen', snapshot: published })
    expect(readback.snapshot).not.toBe(published)
    expect(lane.coordinator.snapshot().redaction).toBe(receipt)
    expect(lane.coordinator.outputBytes()).toEqual(nativeHexBytes(published.outputHex))
    expect(lane.sent).toEqual([])
    await closeRedactionFixture(lane)
  })

  it.each(['stage', 'verify', 'status'] as const)('refuses a different mark count on the same page in %s', async (step) => {
    const lane = fakePort(500)
    const initial = await redactionMarked(lane)
    vi.useFakeTimers()
    const applying = lane.coordinator.applyRedaction([mark.pageId], 1)
    let refused = expect(applying).rejects.toMatchObject({
      code: step === 'status' ? 'FLOW_PDF_NATIVE_REPLY_LOST' : 'FLOW_PDF_NATIVE_RESPONSE_INVALID',
    })

    let dispatch = await lane.take()
    expect(redactionAction(dispatch)).toEqual({ kind: 'stage' })
    if (step !== 'stage') {
      lane.deliver(dispatch, redactionReply(dispatch.request, redactionCandidate))
      dispatch = await lane.take()
      expect(redactionAction(dispatch)).toEqual({ kind: 'verify', candidate: redactionCandidate })
    }
    if (step === 'status') {
      await vi.advanceTimersByTimeAsync(500)
      await refused
      refused = expect(lane.coordinator.reconcile()).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
      dispatch = await lane.take()
      expect(redactionAction(dispatch)).toEqual({ kind: 'status', candidate: redactionCandidate })
    }

    const reply = redactionReply(dispatch.request, redactionCandidate, step === 'stage' ? 'pending' : 'passed')
    lane.deliver(dispatch, { ...reply, redaction: { ...reply.redaction, markCount: 2 } })
    const cancel = await lane.take()
    expect(redactionAction(cancel)).toEqual({ kind: 'cancel', candidate: redactionCandidate })
    expect(lane.coordinator.snapshot().accepted).toBe(initial)
    expect(lane.coordinator.outputBytes()).toBeNull()
    lane.deliver(cancel, redactionReply(cancel.request))
    await refused
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'ready', redaction: { action: 'cancel', published: false } })
    expect(lane.coordinator.snapshot().accepted).toBe(initial)
    expect(lane.coordinator.outputBytes()).toEqual(nativeHexBytes(initial.outputHex))
    expect(lane.sent).toEqual([])
    lane.coordinator.dispose()
  })

  it.each(['stage', 'status'] as const)('discards an unpublished candidate when %s reports a different marked page', async (step) => {
    const lane = fakePort(500, (json) => wasm.verify_native_pdf_response(json))
    const initial = await openRedactionFixture(lane)
    const [markedPage, otherPage] = initial.pageState.order
    const mark = { kind: 'addMark', pageId: markedPage!, rect: [12_000, 12_000, 13_000, 13_000] } as const
    const marking = lane.coordinator.markRedaction(mark, 1)
    await deliverWasmMark(lane, 'addMark')
    await marking

    vi.useFakeTimers()
    const applying = lane.coordinator.applyRedaction([otherPage!], 1)
    const refused = expect(applying).rejects.toMatchObject({
      code: step === 'stage' ? 'FLOW_PDF_NATIVE_RESPONSE_INVALID' : 'FLOW_PDF_NATIVE_REPLY_LOST',
    })
    const stage = await lane.take()
    expect(redactionAction(stage)).toEqual({ kind: 'stage' })
    const staged = JSON.parse(wasm.native_pdf_redaction(JSON.stringify(stage.request))) as NativePdfResponse
    expect(staged).toMatchObject({ status: 'redaction', snapshot: null, redaction: {
      published: false, candidate: { affectedPages: [markedPage] },
    } })
    let recovered = refused
    if (step === 'stage') {
      lane.deliver(stage, staged)
    } else {
      await vi.advanceTimersByTimeAsync(500)
      await refused
      const reconciling = lane.coordinator.reconcile()
      recovered = expect(reconciling).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
      const status = await deliverWasmRedaction(lane, 'status')
      expect(status).toMatchObject({ status: 'redaction', snapshot: null, redaction: {
        action: 'status', published: false, candidate: { affectedPages: [markedPage] },
      } })
    }
    expect(lane.coordinator.outputBytes()).toBeNull()
    await deliverWasmRedaction(lane, 'cancel')
    await recovered
    expect(lane.coordinator.snapshot()).toMatchObject({
      phase: 'ready', accepted: initial, showingPriorAccepted: true,
      redaction: { markCount: 1, published: false, verification: null },
    })
    expect(lane.coordinator.snapshot().accepted).toBe(initial)
    expect(lane.coordinator.outputBytes()).toEqual(nativeHexBytes(initial.outputHex))
    expect(lane.sent).toEqual([])

    const removing = lane.coordinator.markRedaction({ ...mark, kind: 'removeMark' }, 0)
    await deliverWasmMark(lane, 'removeMark')
    await expect(removing).resolves.toMatchObject({ markCount: 0 })
    await closeRedactionFixture(lane)
  })

  it('uses admission from the latest accepted revision before dispatching marks or apply', async () => {
    const lane = fakePort()
    const before = await opened(lane)
    expect(before.redactionAdmission).toBe('supported')
    const editing = lane.coordinator.command(note('ordinary edit'))
    const dispatch = await lane.take()
    const reply = addReply(dispatch.request, before)
    lane.deliver(dispatch, { ...reply, snapshot: { ...reply.snapshot, redactionAdmission: 'unavailable' } })
    const after = await editing
    expect(after.binding.revision).toBe(before.binding.revision + 1)
    await expect(lane.coordinator.markRedaction(mark, 1)).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_UNSUPPORTED' })
    await expect(lane.coordinator.applyRedaction([mark.pageId], 1)).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_UNSUPPORTED' })
    expect(lane.sent).toHaveLength(0)
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'ready', accepted: after })
  })

  it.each(['direct', 'reconciled'] as const)('refreshes admission on the same ordinary PDF after %s mark settlement', async (mode) => {
    const lane = fakePort(500, wasm.verify_native_pdf_response)
    const before = await openRedactionFixture(lane)
    const pageId = before.pageState.order[0]!
    const mark = { kind: 'addMark', pageId, rect: [12_000, 12_000, 13_000, 13_000] } as const
    expect(before.redactionAdmission).toBe('supported')

    function reportUnavailable(readback: NativePdfWorkerRequest) {
      const json = wasm.native_pdf_snapshot(JSON.stringify(readback.request))
      expect(wasm.verify_native_pdf_response(json)).toBe(true)
      const reply = readNativeResponse(json)
      if (reply?.status !== 'unseen') throw new Error('expected accepted-file readback')
      expect(reply.snapshot).toEqual(before)
      lane.verify.mockReturnValueOnce(true)
      lane.deliver(readback, { ...reply, snapshot: { ...reply.snapshot, redactionAdmission: 'unavailable' } })
    }

    if (mode === 'reconciled') vi.useFakeTimers()
    const marking = lane.coordinator.markRedaction(mark, 1)
    const receipt = await deliverWasmRedaction(lane, 'addMark')
    expect(receipt).toMatchObject({ status: 'redaction', snapshot: null, redaction: {
      candidate: null, verification: null, published: false, markCount: 1,
    } })
    const readback = await lane.take()
    expect(readback.request).toEqual({
      protocolVersion: 1, operation: 'snapshot', requestId: receipt.requestId,
      binding: before.binding, originStoreHash: before.originStoreHash,
    })
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'marking', accepted: before })
    if (mode === 'reconciled') {
      const lost = expect(marking).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
      await vi.advanceTimersByTimeAsync(500)
      await lost
      const reconciling = lane.coordinator.reconcile()
      const retried = await deliverWasmRedaction(lane, 'addMark')
      expect(retried).toEqual(receipt)
      const retryReadback = await lane.take()
      expect(retryReadback.request).toEqual(readback.request)
      reportUnavailable(retryReadback)
      await expect(reconciling).resolves.toEqual({ status: 'unseen', snapshot: { ...before, redactionAdmission: 'unavailable' } })
    } else {
      reportUnavailable(readback)
      await marking
    }

    const unavailable = lane.coordinator.snapshot().accepted
    expect(unavailable).toEqual({ ...before, redactionAdmission: 'unavailable' })
    expect(lane.coordinator.outputBytes()).toEqual(nativeHexBytes(before.outputHex))
    const secondMark = { ...mark, rect: [42_000, 42_000, 43_000, 43_000] } as const
    await expect(lane.coordinator.markRedaction(secondMark, 2)).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_UNSUPPORTED' })
    await expect(lane.coordinator.applyRedaction([pageId], 1)).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_UNSUPPORTED' })
    expect(lane.sent).toEqual([])

    const removing = lane.coordinator.markRedaction({ ...mark, kind: 'removeMark' }, 0)
    await deliverWasmMark(lane, 'removeMark')
    await expect(removing).resolves.toMatchObject({ markCount: 0 })
    expect(lane.coordinator.snapshot().accepted).toEqual(before)

    const markingAgain = lane.coordinator.markRedaction(secondMark, 1)
    await deliverWasmRedaction(lane, 'addMark')
    reportUnavailable(await lane.take())
    await markingAgain
    const clearing = lane.coordinator.markRedaction({ kind: 'clearMarks' }, 0)
    await deliverWasmMark(lane, 'clearMarks')
    await expect(clearing).resolves.toMatchObject({ markCount: 0 })
    expect(lane.coordinator.snapshot().accepted).toEqual(before)
    await closeRedactionFixture(lane)
  })

  it('requires reconciliation when mark admission readback describes different accepted bytes', async () => {
    const lane = fakePort()
    const before = await opened(lane)
    const marking = lane.coordinator.markRedaction(mark, 1)
    const failed = expect(marking).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
    const dispatch = await lane.take()
    lane.deliver(dispatch, redactionReply(dispatch.request))
    const readback = await lane.take()
    lane.deliver(readback, markSnapshotReply(readback.request, { ...before, outputHex: '04', redactionAdmission: 'unavailable' }))
    await failed
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'reconciliationRequired', accepted: before })
    expect(lane.coordinator.outputBytes()).toBeNull()

    const reconciling = lane.coordinator.reconcile()
    const retry = await lane.take()
    expect(retry.request).toEqual(dispatch.request)
    lane.deliver(retry, redactionReply(retry.request))
    const lookup = await lane.take()
    expect(lookup.request).toEqual(readback.request)
    lane.deliver(lookup, markSnapshotReply(lookup.request, before))
    await expect(reconciling).resolves.toEqual({ status: 'unseen', snapshot: before })
    lane.coordinator.dispose()
  })

  it.each(['operation', 'response verifier'] as const)('stops processing after a WASM %s throws during verification', async (failure) => {
    const posted: NativePdfWorkerResponse[] = []
    const scope: NativePdfWorkerScope = { onmessage: null, postMessage: (message) => { posted.push(message) } }
    const run = vi.fn((json: string) => {
      const request = JSON.parse(json) as NativePdfRequest
      if (request.operation !== 'redaction' || request.action.kind !== 'verify') throw new Error('unexpected operation')
      if (failure === 'operation') throw new WebAssembly.RuntimeError('private verification context')
      return JSON.stringify(redactionReply(request, redactionCandidate, 'unavailable'))
    })
    const verify = vi.fn((_json: string) => { throw new WebAssembly.RuntimeError('private verification context') })
    installNativePdfWorker(scope, Promise.resolve({
      native_pdf_open: run, native_pdf_command: run, native_pdf_snapshot: run,
      native_pdf_close: run, native_pdf_redaction: run, verify_native_pdf_response: verify,
    }))
    const base = openReply({ protocolVersion: 1, operation: 'open', requestId: 1,
      epoch: Array<number>(16).fill(1), originStoreHash: storeHash, bytesHex: '010203',
    }).snapshot
    const request = {
      protocolVersion: 1, operation: 'redaction', requestId: 2,
      binding: base.binding, originStoreHash: base.originStoreHash, acceptedHash: base.outputHash,
    }
    for (const [index, kind] of (['verify', 'status', 'cancel'] as const).entries()) {
      scope.onmessage?.({ data: { dispatchId: index + 1, request: { ...request, action: { kind, candidate: redactionCandidate } } } })
    }
    await vi.waitFor(() => expect(posted).toEqual([1, 2, 3].map((dispatchId) => ({
      dispatchId, type: 'failed', code: 'FLOW_PDF_NATIVE_WORKER_FAILED',
    }))), { interval: 1 })
    expect(run).toHaveBeenCalledOnce()
    expect(verify).toHaveBeenCalledTimes(failure === 'operation' ? 0 : 1)
  })

  it('posts only closed status bodies or stable errors for unpublished operations', async () => {
    const posted: NativePdfWorkerResponse[] = []
    const scope: NativePdfWorkerScope = { onmessage: null, postMessage: (message) => { posted.push(message) } }
    const cases = [
      { action: mark, status: 'pending' },
      { action: { kind: 'stage' }, candidate: redactionCandidate, status: 'pending' },
      { action: { kind: 'status', candidate: redactionCandidate }, candidate: redactionCandidate, status: 'pending' },
      { action: { kind: 'verify', candidate: redactionCandidate }, candidate: redactionCandidate, status: 'unavailable' },
      { action: { kind: 'status', candidate: redactionCandidate }, candidate: redactionCandidate, status: 'inconclusive' },
      { action: { kind: 'status', candidate: redactionCandidate }, candidate: redactionCandidate, status: 'failed' },
      { action: { kind: 'cancel', candidate: redactionCandidate }, status: 'pending' },
      { action: { kind: 'status', candidate: null }, status: 'pending', leak: true },
      { action: { kind: 'status', candidate: null }, status: 'pending', unexpectedPreview: true },
    ] as const
    const run = vi.fn((json: string) => {
      const request = JSON.parse(json) as Extract<NativePdfRequest, { operation: 'redaction' }>
      const value = cases[request.requestId - 2]!
      const reply = redactionReply(request, 'candidate' in value ? value.candidate : null, value.status)
      return JSON.stringify('leak' in value ? { ...reply, snapshot: { outputHex: 'feedface', scene: {} } }
        : 'unexpectedPreview' in value ? { ...reply, preview: null } : reply)
    })
    const verify = vi.fn((_json: string) => true)
    installNativePdfWorker(scope, Promise.resolve({
      native_pdf_open: run, native_pdf_command: run, native_pdf_snapshot: run,
      native_pdf_close: run, native_pdf_redaction: run, verify_native_pdf_response: verify,
    }))
    const base = openReply({ protocolVersion: 1, operation: 'open', requestId: 1,
      epoch: Array<number>(16).fill(1), originStoreHash: storeHash, bytesHex: '010203',
    }).snapshot
    const intents = cases.map(({ action }, index) => ({
      dispatchId: index + 2, request: {
        protocolVersion: 1, operation: 'redaction', requestId: index + 2,
        binding: base.binding, originStoreHash: base.originStoreHash, acceptedHash: base.outputHash, action,
      },
    }))
    for (const intent of intents) scope.onmessage?.({ data: intent })
    await vi.waitFor(() => expect(posted).toHaveLength(intents.length), { interval: 1 })
    expect(verify).toHaveBeenCalledTimes(intents.length)
    expect(run.mock.calls.map(([json]) => JSON.parse(json))).toEqual(intents.map(({ request }) => request))
    for (const [index, message] of posted.entries()) {
      expect(message.dispatchId).toBe(index + 2)
      if (index >= intents.length - 2) {
        expect(message).toEqual({ dispatchId: index + 2, type: 'failed', code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
      } else {
        if (message.type !== 'response') throw new Error('expected status')
        const payload = JSON.parse(message.json)
        expect(Object.keys(message).sort()).toEqual(['dispatchId', 'json', 'type'])
        expect(Object.keys(payload).sort()).toEqual([
          'binding', 'code', 'limits', 'operation', 'originStoreHash', 'protocolVersion',
          'redaction', 'requestId', 'resultHash', 'snapshot', 'status',
        ])
        expect(payload).toMatchObject({ operation: 'redaction', snapshot: null })
        expect(message.json).not.toMatch(/outputHex|bytesHex|scene|feedface|contents|evidence/u)
      }
    }
  })

  it.each(['failed', 'unavailable', 'inconclusive'] as const)('keeps marks and the prior revision when verification is %s', async (status) => {
    const lane = fakePort()
    const initial = await redactionMarked(lane)
    const applying = lane.coordinator.applyRedaction([mark.pageId], 1)
    const stage = await lane.take()
    expect(redactionAction(stage)).toEqual({ kind: 'stage' })
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'validating', accepted: initial, showingPriorAccepted: true })
    expect(lane.coordinator.outputBytes()).toBeNull()
    await expect(lane.coordinator.command(note('conflicting'))).rejects.toThrow()
    await expect(lane.coordinator.open(original())).rejects.toThrow()
    await expect(lane.coordinator.markRedaction({ kind: 'clearMarks' }, 0)).rejects.toThrow()
    await expect(lane.coordinator.project()).resolves.toBeNull()
    const pending = redactionReply(stage.request, redactionCandidate)
    lane.deliver(stage, pending)
    const verify = await lane.take()
    expect(redactionAction(verify)).toEqual({ kind: 'verify', candidate: redactionCandidate })
    expect(verify.request.requestId).toBe(stage.request.requestId)
    expect(lane.coordinator.snapshot().redaction?.verification?.status).toBe('pending')
    lane.deliver(stage, pending)
    lane.deliver(verify, redactionReply(verify.request, redactionCandidate, status))
    const cancel = await lane.take()
    expect(redactionAction(cancel)).toEqual({ kind: 'cancel', candidate: redactionCandidate })
    lane.deliver(cancel, redactionReply(cancel.request))
    await expect(applying).resolves.toMatchObject({ markCount: 1, verification: { status } })
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'ready', accepted: initial, showingPriorAccepted: true })
    expect(lane.sent).toEqual([])
    lane.coordinator.dispose()
  })

  it.each(['version', 'coverage', 'candidate', 'revision', 'source', 'action', 'payload'] as const)(
    'halts on an invalid %s verification reply before dispatching publish', async (mutation) => {
      const lane = fakePort()
      const initial = await redactionMarked(lane)
      const applying = lane.coordinator.applyRedaction([mark.pageId], 1)
      const refused = expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
      const stage = await lane.take()
      lane.deliver(stage, redactionReply(stage.request, redactionCandidate))
      const verify = await lane.take()
      const reply = redactionReply(verify.request, redactionCandidate, 'passed')
      const result = reply.redaction
      const candidate = redactionCandidate
      const verification = result.verification!
      switch (mutation) {
        case 'version': lane.deliver(verify, { ...reply, redaction: { ...result, candidate: { ...candidate, validatorVersion: 2 } } }); break
        case 'coverage': lane.deliver(verify, { ...reply, redaction: { ...result, verification: { ...verification, raster: { status: 'passed', coveredPages: 0 } } } }); break
        case 'candidate': lane.deliver(verify, { ...reply, redaction: { ...result, candidate: { ...candidate, hash: hash(71) } } }); break
        case 'revision': lane.deliver(verify, { ...reply, binding: { ...reply.binding, revision: 1 } }); break
        case 'source': lane.deliver(verify, { ...reply, originStoreHash: '00'.repeat(32) }); break
        case 'action': lane.deliver(verify, { ...reply, redaction: { ...result, action: 'status' } }); break
        case 'payload': {
          const withPayload = { ...reply, redaction: { ...result, outputHex: 'feedface' } }
          lane.deliver(verify, withPayload)
          break
        }
      }
      await refused
      expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'reconciliationRequired', accepted: initial })
      expect(lane.coordinator.outputBytes()).toBeNull()
      expect(lane.sent).toEqual([])
      lane.coordinator.dispose()
    },
  )

  it('looks up a lost verification by the same identity and discards the unpublished result', async () => {
    const lane = fakePort(500)
    const initial = await redactionMarked(lane)
    vi.useFakeTimers()
    const applying = lane.coordinator.applyRedaction([mark.pageId], 1)
    const lost = expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
    const stage = await lane.take()
    lane.deliver(stage, redactionReply(stage.request, redactionCandidate))
    const verify = await lane.take()
    await vi.advanceTimersByTimeAsync(500)
    await lost
    const reconciling = lane.coordinator.reconcile()
    const lookup = await lane.take()
    expect(lookup.request.requestId).toBe(verify.request.requestId)
    expect(redactionAction(lookup)).toEqual({ kind: 'status', candidate: redactionCandidate })
    lane.deliver(verify, redactionReply(verify.request, redactionCandidate, 'passed'))
    lane.deliver(lookup, redactionReply(lookup.request, redactionCandidate, 'passed'))
    const cancel = await lane.take()
    expect(redactionAction(cancel)).toEqual({ kind: 'cancel', candidate: redactionCandidate })
    lane.deliver(cancel, redactionReply(cancel.request))
    await expect(reconciling).resolves.toEqual({ status: 'unseen', snapshot: initial })
    expect(lane.sent).toEqual([])
    lane.coordinator.dispose()
  })

  it.each(['refused', 'empty with a different mark count'] as const)('offers original-file recovery when prepublication status is %s', async (status) => {
    const lane = fakePort(500)
    const initial = await redactionMarked(lane)
    vi.useFakeTimers()
    const applying = lane.coordinator.applyRedaction([mark.pageId], 1)
    const lost = expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
    const stage = await lane.take()
    lane.deliver(stage, redactionReply(stage.request, redactionCandidate))
    const verify = await lane.take()
    expect(redactionAction(verify)).toEqual({ kind: 'verify', candidate: redactionCandidate })
    await vi.advanceTimersByTimeAsync(500)
    await lost

    const code = status === 'refused' ? 'FLOW_PDF_NATIVE_REDACTION_CANDIDATE_UNAVAILABLE' : 'FLOW_PDF_NATIVE_RESPONSE_INVALID'
    const refused = expect(lane.coordinator.reconcile()).rejects.toMatchObject({ code })
    const lookup = await lane.take()
    expect(lookup.request.requestId).toBe(stage.request.requestId)
    expect(redactionAction(lookup)).toEqual({ kind: 'status', candidate: redactionCandidate })
    const reply = redactionReply(lookup.request)
    if (status === 'refused') {
      const { redaction: _redaction, ...base } = reply
      lane.deliver(lookup, { ...base, status: 'failed', code })
    } else {
      lane.deliver(lookup, { ...reply, redaction: { ...reply.redaction, markCount: 2 } })
    }
    await refused

    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'invalidated', reopenOriginal: true, redaction: null })
    expect(lane.coordinator.snapshot().accepted).toBe(initial)
    expect(lane.coordinator.originalForReopen()).toEqual(original())
    expect(lane.coordinator.outputBytes()).toBeNull()
    expect(lane.sent).toEqual([])
    expect(lane.worker.terminate).toHaveBeenCalledOnce()
  })

  it.each(['status', 'readPublished'] as const)('offers original-file recovery when %s cannot verify a publication', async (step) => {
    const lane = fakePort(500)
    const initial = await redactionMarked(lane)
    vi.useFakeTimers()
    const applying = lane.coordinator.applyRedaction([mark.pageId], 1)
    const lost = expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
    const stage = await lane.take()
    lane.deliver(stage, redactionReply(stage.request, redactionCandidate))
    const verify = await lane.take()
    lane.deliver(verify, redactionReply(verify.request, redactionCandidate, 'passed'))
    const publish = await lane.take()
    expect(redactionAction(publish)).toEqual({ kind: 'publish', candidate: redactionCandidate })
    await vi.advanceTimersByTimeAsync(500)
    await lost
    const reconciling = lane.coordinator.reconcile()
    const lookupLost = expect(reconciling).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
    const lookup = await lane.take()
    const status = redactionReply(lookup.request, redactionCandidate, 'passed')
    const publishedStatus = { ...status, redaction: { ...status.redaction, published: true, markCount: 0 } }
    let lostDispatch = lookup
    if (step === 'readPublished') {
      lane.deliver(lookup, publishedStatus)
      lostDispatch = await lane.take()
    }
    expect(redactionAction(lostDispatch)).toEqual({ kind: step, candidate: redactionCandidate })
    await vi.advanceTimersByTimeAsync(500)
    await lookupLost
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'reconciliationRequired', reopenOriginal: true })
    const lostAgain = expect(lane.coordinator.reconcile()).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REPLY_LOST' })
    const secondLookup = await lane.take()
    expect(secondLookup.request).toEqual(lostDispatch.request)
    expect(secondLookup.dispatchId).not.toBe(lostDispatch.dispatchId)
    await vi.advanceTimersByTimeAsync(500)
    await lostAgain
    expect(lane.coordinator.snapshot()).toMatchObject({
      phase: 'reconciliationRequired', accepted: initial, reopenOriginal: true, pendingRequestId: publish.request.requestId,
    })
    expect(lane.coordinator.originalForReopen()).toEqual(original())
    expect(lane.coordinator.outputBytes()).toBeNull()

    const code = step === 'status' ? 'FLOW_PDF_NATIVE_RESPONSE_INVALID' : 'FLOW_PDF_NATIVE_REDACTION_UNVERIFIED'
    const refused = expect(lane.coordinator.reconcile()).rejects.toMatchObject({ code })
    const retry = await lane.take()
    expect(retry.request).toEqual(lostDispatch.request)
    expect(retry.request.requestId).toBe(publish.request.requestId)
    if (step === 'status') {
      lane.deliver(retry, { ...publishedStatus, redaction: {
        ...publishedStatus.redaction, candidate: { ...redactionCandidate, hash: hash(71) },
      } })
    } else {
      const { redaction: _redaction, ...base } = redactionReply(retry.request)
      lane.deliver(retry, { ...base, status: 'failed', code })
    }
    await refused
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'invalidated', reopenOriginal: true })
    expect(lane.coordinator.snapshot().accepted).toBe(initial)
    expect(lane.coordinator.originalForReopen()).toEqual(original())
    expect(lane.coordinator.outputBytes()).toBeNull()
    expect(lane.sent).toEqual([])
    lane.coordinator.dispose()
  })

  it('cancels interrupted validation even if a passing reply arrives', async () => {
    const lane = fakePort()
    const initial = await redactionMarked(lane)
    const abort = new AbortController()
    const applying = lane.coordinator.applyRedaction([mark.pageId], 1, abort.signal)
    const cancelled = expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_REDACTION_CANCELLED' })
    const stage = await lane.take()
    lane.deliver(stage, redactionReply(stage.request, redactionCandidate))
    const verify = await lane.take()
    abort.abort()
    lane.deliver(verify, redactionReply(verify.request, redactionCandidate, 'passed'))
    const cancel = await lane.take()
    expect(redactionAction(cancel)).toEqual({ kind: 'cancel', candidate: redactionCandidate })
    lane.deliver(cancel, redactionReply(cancel.request))
    await cancelled
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'ready', accepted: initial, redaction: { markCount: 1 } })
    expect(lane.sent).toEqual([])
    lane.coordinator.dispose()
  })

  it.each([1, 2])('keeps downloads locked when cancellation acknowledges a different candidate after verifying %i marks', async (markCount) => {
    const lane = fakePort()
    const initial = await redactionMarked(lane)
    const applying = lane.coordinator.applyRedaction([mark.pageId], 1)
    const refused = expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
    const stage = await lane.take()
    lane.deliver(stage, redactionReply(stage.request, redactionCandidate))
    const verify = await lane.take()
    const reply = redactionReply(verify.request, redactionCandidate, markCount === 1 ? 'inconclusive' : 'passed')
    lane.deliver(verify, { ...reply, redaction: { ...reply.redaction, markCount } })
    const cancel = await lane.take()
    expect(redactionAction(cancel)).toEqual({ kind: 'cancel', candidate: redactionCandidate })
    lane.deliver(cancel, redactionReply(cancel.request, { ...redactionCandidate, hash: hash(71) }))
    await refused
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'reconciliationRequired', accepted: initial })
    expect(lane.coordinator.outputBytes()).toBeNull()
    expect(lane.sent).toEqual([])
    lane.coordinator.dispose()
  })

  it('invalidates on worker loss and ignores a captured late positive reply', async () => {
    const lane = fakePort()
    const initial = await redactionMarked(lane)
    const applying = lane.coordinator.applyRedaction([mark.pageId], 1)
    const lost = expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_WORKER_FAILED' })
    const stage = await lane.take()
    lane.deliver(stage, redactionReply(stage.request, redactionCandidate))
    const verify = await lane.take()
    const receive = lane.worker.onmessage!
    lane.worker.onerror?.(new Event('error') as ErrorEvent)
    receive(new MessageEvent('message', { data: {
      dispatchId: verify.dispatchId, type: 'response',
      json: JSON.stringify(redactionReply(verify.request, redactionCandidate, 'passed')),
    } }))
    await lost
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'invalidated', accepted: initial, redaction: null })
    expect(lane.coordinator.outputBytes()).toBeNull()
    expect(lane.sent).toEqual([])
    expect(lane.worker.terminate).toHaveBeenCalledOnce()
  })
})

describe('native PDF runtime composition', () => {
  it.each(['setup', 'disposal', 'abort', 'native teardown', 'default teardown'] as const)('releases page read workers after a scheduler fault during %s', async (stage) => {
    await initializeWasm()
    const { createDefaultWorkerRuntime, createNativePdfRuntime } = await import('../src/layout/layout-runtime.js')
    const { PdfSourceStore } = await import('../persistence/pdf-source-store.js')
    const { createWasmPdfReaderEngine, installPdfReaderWorker, RevisionAwarePdfReaderScheduler } =
      await import('../src/pdf/pdf-reader-worker.js')
    const content = 'BT /F1 12 Tf 20 80 Td (Page text) Tj ET\n'
    const bytes = new TextEncoder().encode(fixturePdf([
      '<< /Type /Catalog /Pages 2 0 R >>',
      '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
      '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',
      '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>',
      `<< /Length ${content.length} >>\nstream\n${content}endstream`,
    ]))
    const originStoreHash = nativeBytesHex(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)))
    const identity = { protocolVersion: 1, originStoreHash }
    const opened = readNativeResponse(wasm.native_pdf_open(JSON.stringify({
      ...identity, operation: 'open', requestId: 1,
      epoch: Array.from(crypto.getRandomValues(new Uint8Array(16))), bytesHex: nativeBytesHex(bytes),
    })))
    if (opened?.status !== 'opened') throw new Error('expected a verified PDF snapshot')
    const { snapshot } = opened
    const closed = readNativeResponse(wasm.native_pdf_close(JSON.stringify({
      ...identity, operation: 'close', requestId: 2, binding: snapshot.binding,
    })))
    expect(closed?.status).toBe('closed')
    const pageId = snapshot.scene.pages[0]!.id
    const outputHash = nativeBytesHex(Uint8Array.from(snapshot.outputHash))
    const failure = new Error(`reader scheduler ${stage} failed`)
    const reader = createWasmPdfReaderEngine(wasm)
    const workers: PageReaderWorker[] = []
    const ownedWorkers: PageReaderWorker[] = []
    const tearingDown = stage === 'native teardown' || stage === 'default teardown'
    let capturePageReads = false
    let holdReads = stage === 'abort' || tearingDown
    class PageReaderWorker implements PdfReaderWorkerPort {
      onmessage: PdfReaderWorkerPort['onmessage'] = null
      onerror = null
      onmessageerror = null
      terminate = vi.fn()
      private readonly scope: PdfReaderWorkerScope = {
        onmessage: null,
        postMessage: (data) => this.onmessage?.({ data }),
      }
      postMessage(message: PdfReaderWorkerInboundMessage) {
        if (holdReads && message.type === 'read') return
        this.scope.onmessage?.({ data: message })
      }

      constructor(url: URL) {
        ownedWorkers.push(this)
        if (!capturePageReads || !url.pathname.endsWith('/pdf-reader-worker-entry.ts')) return
        workers.push(this)
        installPdfReaderWorker(this.scope, reader.run, reader.verifyResultHash)
        if (stage === 'setup' && workers.length === 1) {
          Object.defineProperty(this, 'onerror', { set() { throw failure } })
        }
      }
    }
    const dispose = vi.spyOn(RevisionAwarePdfReaderScheduler.prototype, 'dispose')
    if (stage !== 'setup') dispose.mockImplementationOnce(() => { throw failure })
    const WorkerClass = PageReaderWorker as unknown as typeof Worker
    const catalog = { identity: 'fonts', faces: [], hyphenation: { identity: 'hyphenation', bytes: new Uint8Array() } }
    const owner = stage === 'default teardown'
      ? createDefaultWorkerRuntime(catalog, WorkerClass)
      : createNativePdfRuntime(WorkerClass)
    const runtime = 'nativePdf' in owner ? owner.nativePdf : owner
    const closeStore = vi.spyOn(PdfSourceStore.prototype, 'close')
    capturePageReads = true
    try {
      const failedRead = new AbortController()
      const active = runtime.extractPageText!(snapshot, pageId, failedRead.signal)
      if (tearingDown) {
        const second = runtime.extractPageText!(snapshot, pageId, new AbortController().signal)
        expect(workers).toHaveLength(2)
        expect(() => owner.dispose()).toThrow(failure)
        await expect(Promise.all([active, second])).resolves.toEqual([null, null])
        failedRead.abort()
        owner.dispose()
        expect(closeStore).toHaveBeenCalledOnce()
        expect(dispose).toHaveBeenCalledTimes(stage === 'default teardown' ? 4 : 2)
        for (const worker of ownedWorkers) expect(worker.terminate).toHaveBeenCalledOnce()
        return
      }
      if (stage === 'abort') {
        failedRead.abort()
        await expect(active).resolves.toBeNull()
      } else {
        await expect(active).rejects.toBe(failure)
      }
      expect(workers).toHaveLength(1)
      expect(workers[0]?.terminate).toHaveBeenCalledOnce()
      failedRead.abort()
      holdReads = false

      await expect(runtime.extractPageText!(snapshot, pageId, new AbortController().signal)).resolves.toMatchObject({
        outputHash, pageId, text: [{ value: 'Page text' }],
      })
      expect(workers).toHaveLength(2)
      for (const worker of workers) expect(worker.terminate).toHaveBeenCalledOnce()
    } finally {
      dispose.mockRestore()
      owner.dispose()
      closeStore.mockRestore()
    }
  })

  it('preserves a late startup failure while releasing the partially acquired runtime', async () => {
    await initializeWasm()
    const { createDefaultWorkerRuntime } = await import('../src/layout/layout-runtime.js')
    const { RevisionAwareLayoutScheduler } = await import('../src/layout/layout-worker.js')
    const { RevisionAwarePdfExportScheduler } = await import('../src/pdf/pdf-worker.js')
    const { RevisionAwarePdfReaderScheduler } = await import('../src/pdf/pdf-reader-worker.js')
    const { RevisionAwarePdfReconstructionScheduler } = await import('../src/pdf/pdf-reconstruction-worker.js')
    const { PdfSourceStore } = await import('../persistence/pdf-source-store.js')
    const workers: RuntimeWorker[] = []
    const cleanupFailure = new Error('worker termination unavailable')
    class RuntimeWorker {
      onmessage = null
      onerror = null
      onmessageerror = null
      postMessage = vi.fn()
      terminate = vi.fn()

      constructor() {
        if (workers.length === 4 || workers.length === 5) {
          this.terminate.mockImplementationOnce(() => { throw cleanupFailure })
        }
        workers.push(this)
      }
    }
    const failure = new Error('coordinator subscription unavailable')
    const subscribe = vi.spyOn(NativePdfEditCoordinator.prototype, 'subscribe')
      .mockImplementationOnce(() => { throw failure })
    const disposeLayout = vi.spyOn(RevisionAwareLayoutScheduler.prototype, 'dispose')
    const disposeExport = vi.spyOn(RevisionAwarePdfExportScheduler.prototype, 'dispose')
    const disposeReader = vi.spyOn(RevisionAwarePdfReaderScheduler.prototype, 'dispose')
    const disposeReconstruction = vi.spyOn(RevisionAwarePdfReconstructionScheduler.prototype, 'dispose')
    const disposeCoordinator = vi.spyOn(NativePdfEditCoordinator.prototype, 'dispose')
    const closeStore = vi.spyOn(PdfSourceStore.prototype, 'close')
    const catalog = { identity: 'fonts', faces: [], hyphenation: { identity: 'hyphenation', bytes: new Uint8Array() } }
    const WorkerClass = RuntimeWorker as unknown as typeof Worker

    try {
      expect(() => createDefaultWorkerRuntime(catalog, WorkerClass)).toThrow(failure)
      expect(workers).toHaveLength(6)
      for (const worker of workers) expect(worker.terminate).toHaveBeenCalledOnce()
      expect(disposeLayout).toHaveBeenCalledOnce()
      expect(disposeExport).toHaveBeenCalledOnce()
      expect(disposeReader).toHaveBeenCalledTimes(2)
      expect(disposeReconstruction).toHaveBeenCalledOnce()
      expect(disposeCoordinator).toHaveBeenCalledOnce()
      expect(closeStore).toHaveBeenCalledOnce()

      const runtime = createDefaultWorkerRuntime(catalog, WorkerClass)
      try {
        expect(runtime.nativePdf.createCoordinator().snapshot().phase).toBe('idle')
        expect(workers).toHaveLength(12)
        for (const worker of workers.slice(6)) expect(worker.terminate).not.toHaveBeenCalled()
      } finally {
        runtime.dispose()
      }
      for (const worker of workers) expect(worker.terminate).toHaveBeenCalledOnce()
    } finally {
      subscribe.mockRestore()
      disposeLayout.mockRestore()
      disposeExport.mockRestore()
      disposeReader.mockRestore()
      disposeReconstruction.mockRestore()
      disposeCoordinator.mockRestore()
      closeStore.mockRestore()
    }
  })

  it('keeps the startup error and retries through the editor entry point when mount and worker cleanup throw', async () => {
    await initializeWasm()
    const { IDBFactory } = await import('fake-indexeddb')
    const { EditorController } = await import('../src/editor/editor-controller.js')
    const fontCatalog = await import('../src/runtime/font-catalog.js')
    const loadCatalog = vi.spyOn(fontCatalog, 'loadBundledFontCatalog').mockResolvedValue({
      identity: 'fonts', faces: [], hyphenation: { identity: 'hyphenation', bytes: new Uint8Array() },
    })
    const initialize = vi.spyOn(EditorController.prototype, 'initialize')
      .mockRejectedValueOnce(new Error('FLOW_EDITOR_INITIALIZE'))
    const disposeController = vi.spyOn(EditorController.prototype, 'dispose')
    const render = vi.fn<(element: { props: { controller: typeof EditorController.prototype } }) => void>()
    const unmount = vi.fn().mockImplementationOnce(() => { throw new Error('private mount cleanup detail') })
    const createRoot = vi.fn(() => ({ render, unmount }))
    vi.doMock('react-dom/client', () => ({ createRoot }))

    class StartupElement extends EventTarget {
      children: StartupElement[] = []
      textContent = ''
      disabled = false
      readonly attributes = new Map<string, string>()

      constructor(readonly tagName: string) {
        super()
      }

      setAttribute(name: string, value: string) {
        this.attributes.set(name, value)
      }

      replaceChildren(...children: StartupElement[]) {
        this.children = children
      }
    }
    const workers: StartupWorker[] = []
    class StartupWorker {
      onmessage = null
      onerror = null
      onmessageerror = null
      postMessage = vi.fn()
      terminate = vi.fn()

      constructor() {
        if (workers.length === 4) {
          this.terminate.mockImplementationOnce(() => { throw new Error('private worker cleanup detail') })
        }
        workers.push(this)
      }
    }
    const appRoot = new StartupElement('div')
    const page = new EventTarget()
    vi.stubGlobal('document', {
      documentElement: { lang: 'en' },
      querySelector: () => appRoot,
      createElement: (tagName: string) => new StartupElement(tagName),
    })
    vi.stubGlobal('window', page)
    vi.stubGlobal('indexedDB', new IDBFactory())
    vi.stubGlobal('Worker', StartupWorker)

    try {
      // @ts-expect-error -- Vite needs the .tsx extension to select the editor entry point.
      await import('../src/main.tsx')
      await vi.waitFor(() => {
        expect(appRoot.children.map(({ textContent }) => textContent)).toEqual([
          'The editor could not start. Code: FLOW_EDITOR_INITIALIZE',
          'Retry startup',
        ])
      })
      const [alert, retry] = appRoot.children
      expect(alert?.attributes.get('role')).toBe('alert')
      expect(retry?.tagName).toBe('button')
      expect(unmount).toHaveBeenCalledOnce()
      expect(disposeController).toHaveBeenCalledOnce()
      expect(workers).toHaveLength(6)
      for (const worker of workers) expect(worker.terminate).toHaveBeenCalledOnce()

      retry!.dispatchEvent(new Event('click'))
      expect(retry?.disabled).toBe(true)
      await vi.waitFor(() => {
        expect(render.mock.calls[1]?.[0].props.controller.snapshot().phase).toBe('empty')
      })
      expect(appRoot.children).toEqual([])
      expect(createRoot).toHaveBeenCalledTimes(2)
      expect(createRoot).toHaveBeenLastCalledWith(appRoot)
      expect(workers).toHaveLength(12)
      for (const worker of workers.slice(6)) expect(worker.terminate).not.toHaveBeenCalled()
    } finally {
      page.dispatchEvent(new Event('pagehide'))
      vi.unstubAllGlobals()
      vi.doUnmock('react-dom/client')
      disposeController.mockRestore()
      initialize.mockRestore()
      loadCatalog.mockRestore()
    }
    expect(unmount).toHaveBeenCalledTimes(2)
    for (const worker of workers) expect(worker.terminate).toHaveBeenCalledOnce()
  })

  it('reports startup failure and allows another attempt without restarting the app', async () => {
    const { createNativePdfRuntime } = await import('../src/layout/layout-runtime.js')
    const terminate = vi.fn()
    const construct = vi.fn()
      .mockImplementationOnce(() => { throw new Error('unavailable') })
      .mockImplementationOnce(() => { throw new Error('still unavailable') })
    class RuntimeWorker {
      onmessage = null
      onerror = null
      onmessageerror = null
      postMessage = vi.fn()
      terminate = terminate

      constructor() {
        construct()
      }
    }
    const runtime = createNativePdfRuntime(RuntimeWorker as unknown as typeof Worker)
    try {
      expect(() => runtime.createCoordinator()).toThrow('FLOW_PDF_NATIVE_WORKER_STARTUP')
      const coordinator = runtime.createCoordinator()
      expect(coordinator.snapshot().phase).toBe('idle')
      expect(construct).toHaveBeenCalledTimes(3)
    } finally {
      runtime.dispose()
    }
    expect(terminate).toHaveBeenCalledOnce()
  })

  it('prepares one worker, isolates replacement admission and tears down owned sessions', async () => {
    const { createNativePdfRuntime } = await import('../src/layout/layout-runtime.js')
    const { PdfSourceStore } = await import('../persistence/pdf-source-store.js')
    const entryPoints: string[] = []
    const terminate = vi.fn()
    class RuntimeWorker {
      onmessage = null
      onerror = null
      onmessageerror = null
      postMessage = vi.fn()
      terminate = terminate

      constructor(url: URL, options: WorkerOptions) {
        entryPoints.push(url.pathname)
        expect(options.type).toBe('module')
      }
    }
    const store = new PdfSourceStore()
    const closeStore = vi.spyOn(store, 'close')
    const runtime = createNativePdfRuntime(RuntimeWorker as unknown as typeof Worker, store)
    expect(runtime.sourceStore).toBe(store)
    expect(entryPoints).toHaveLength(1)
    expect(entryPoints[0]).toMatch(/pdf-native-edit-worker-entry\.ts$/)
    const originalCoordinator = runtime.createCoordinator()
    expect(entryPoints).toHaveLength(1)
    const replacement = runtime.createCoordinator()
    expect(replacement).not.toBe(originalCoordinator)
    expect(entryPoints).toHaveLength(2)
    replacement.dispose()
    runtime.dispose()
    expect(originalCoordinator.snapshot().phase).toBe('invalidated')
    expect(terminate).toHaveBeenCalledTimes(2)
    expect(closeStore).toHaveBeenCalledOnce()
    expect(() => runtime.createCoordinator()).toThrow('FLOW_PDF_NATIVE_WORKER_FAILED')
  })
})

describe('verified page readback', () => {
  it('checks displaced pages in either direction while allowing moves within a parent', () => {
    const pageState = {
      order: [1, 2, 3, 4], selected: null, canDelete: true, maxPosition: 3,
      restrictions: { positionsLocked: false, pages: [
        { parent: 10, reparent: 'geometry' },
        { parent: 20, reparent: 'structure' },
        { parent: 20 },
        { parent: 20 },
      ] },
    } as const
    expect(nativePageMoveRefusals(pageState, 3)[0]).toBe('geometry')
    expect(nativePageMoveRefusals(pageState, 0)[3]).toBe('geometry')
    expect(nativePageMoveRefusals(pageState, 1)[3]).toBeUndefined()
    const withoutSourceRefusal = { ...pageState, restrictions: {
      ...pageState.restrictions, pages: [{ parent: 10 }, ...pageState.restrictions.pages.slice(1)],
    } }
    expect(nativePageMoveRefusals(withoutSourceRefusal, 0)[3]).toBe('structure')
  })

  it('validates closed page capabilities and refuses rotation without verified geometry', async () => {
    const lane = fakePort()
    const opening = lane.coordinator.open(original())
    const openDispatch = await lane.take()
    const reply = openReply(openDispatch.request)
    const before = {
      ...reply.snapshot,
      pageState: { ...reply.snapshot.pageState, restrictions: {
        positionsLocked: false, pages: [{ parent: 131072, geometry: 'geometry', reparent: 'geometry', duplicate: 'geometry' }],
      } },
      scene: { ...reply.snapshot.scene, pages: [
        { ...reply.snapshot.scene.pages[0]!, geometry: null, canAddNote: false },
      ] },
    } as const
    const opened = { ...reply, snapshot: before }
    const decodeWith = (restrictions: unknown) => readNativeResponse(JSON.stringify({
      ...opened, snapshot: { ...before, pageState: { ...before.pageState, restrictions } },
    }))
    const { restrictions } = before.pageState
    expect(decodeWith(restrictions)?.status).toBe('opened')
    expect(decodeWith(undefined)).toBeNull()
    expect(decodeWith({ ...restrictions, pages: [] })).toBeNull()
    expect(decodeWith({ ...restrictions, pages: [{ ...restrictions.pages[0], parent: 0 }] })).toBeNull()
    expect(decodeWith({ ...restrictions, pages: [{ ...restrictions.pages[0], extra: true }] })).toBeNull()
    lane.deliver(openDispatch, opened)
    await opening
    const applying = lane.coordinator.command({ kind: 'rotatePage', target: { page: 0, stableId: 196608 }, degrees: 90 })
    const refused = expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
    const dispatch = await lane.take()
    const outputHash = hash(22)
    lane.deliver(dispatch, {
      ...opened, requestId: dispatch.request.requestId, operation: 'command', status: 'applied',
      snapshot: { ...before, binding: { ...before.binding, revision: 1 }, consumedRequest: dispatch.request.requestId,
        outputHash, outputHex: '04', pageState: { ...before.pageState, selected: 196608 },
        scene: { ...before.scene, outputHash } },
    })
    await refused
    expect(lane.coordinator.snapshot().accepted).toEqual(before)
    lane.coordinator.dispose()
  })

  it.each(['applied', 'reconciled'] as const)('publishes page changes through %s', async (mode) => {
    const lane = fakePort()
    let current = await opened(lane)
    const target = (page: number, stableId: number) => ({ page, stableId })
    const source = 196608
    const blank = 4503599627370496
    const copy = blank + 1
    for (const [command, order, selected, rotations] of [
      [{ kind: 'insertBlankPage', target: target(0, source), side: 'before' }, [blank, source], blank, [0, 0]],
      [{ kind: 'rotatePage', target: target(1, source), degrees: -90 }, [blank, source], source, [0, 270]],
      [{ kind: 'duplicatePage', target: target(1, source) }, [blank, source, copy], copy, [0, 270, 270]],
      [{ kind: 'reorderPage', target: target(1, source), position: 2 }, [blank, copy, source], source, [0, 270, 270]],
      [{ kind: 'deletePage', target: target(2, source) }, [blank, copy], copy, [0, 270]],
    ] as const) {
      const applying = lane.coordinator.command(command)
      const staleNote = command.kind === 'insertBlankPage'
        ? expect(lane.coordinator.command(note('queued for the old index')))
          .rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_TARGET_UNAVAILABLE' })
        : null
      const missingPage = command.kind === 'insertBlankPage'
        ? expect(lane.coordinator.command({ ...note('queued for an absent index'), page: 1 }))
          .rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_TARGET_UNAVAILABLE' })
        : null
      const refused = mode === 'reconciled'
        ? expect(applying).rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID' })
        : null
      const dispatch = await lane.take()
      expect(dispatch.request).toMatchObject({ command, binding: current.binding })
      const revision = current.binding.revision + 1
      const outputHash = hash(60 + revision)
      const snapshot: NativePdfVerifiedSnapshot = {
        ...current, binding: { ...current.binding, revision }, consumedRequest: dispatch.request.requestId,
        outputHash, outputHex: '04',
        pageState: { order, selected, canDelete: true, maxPosition: order.length - 1 },
        scene: { outputHash, sceneHash: hash(70 + revision), pages: order.map((_, id) => ({
          id, object: { objectNumber: id + 3, generation: 0 },
          geometry: { bounds: [0, 0, 100, 100], rotation: rotations[id]! },
          canAddNote: true, annotations: [],
        })) },
      }
      const receipt = {
        protocolVersion: 1, requestId: dispatch.request.requestId, operation: 'command',
        binding: current.binding, originStoreHash: current.originStoreHash, status: 'applied',
        snapshot, code: null, limits: NATIVE_PDF_LIMITS, resultHash: 'cd'.repeat(32),
      } as const
      if (mode === 'reconciled') {
        lane.deliver(dispatch, { ...receipt, snapshot: {
          ...snapshot, pageState: { ...snapshot.pageState, selected: null },
        } })
        await refused
        expect(lane.coordinator.snapshot().accepted).toEqual(current)
        const reconciling = lane.coordinator.reconcile()
        const lookup = await lane.take()
        lane.deliver(lookup, { ...receipt, operation: 'snapshot', status: 'accepted' })
        await expect(reconciling).resolves.toMatchObject({ status: 'accepted', snapshot })
      } else {
        lane.deliver(dispatch, receipt)
        await expect(applying).resolves.toEqual(snapshot)
      }
      expect(lane.sent).toEqual([])
      await staleNote
      await missingPage
      current = snapshot
    }
    await expect(lane.coordinator.command({ kind: 'rotatePage', target: target(1, source), degrees: 90 }))
      .rejects.toMatchObject({ code: 'FLOW_PDF_NATIVE_TARGET_UNAVAILABLE' })
    expect(lane.sent).toEqual([])
    lane.coordinator.dispose()
  })
})
