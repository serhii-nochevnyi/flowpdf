import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  NATIVE_PDF_LIMITS,
  nativeBytesHex,
  type NativePdfRequest,
  type NativePdfResponse,
  type NativePdfSnapshot,
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

const hash = (byte: number) => Array<number>(32).fill(byte)
const storeHash = '039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81'
const original = () => ({ bytes: Uint8Array.of(1, 2, 3), originStoreHash: storeHash })
const note = (contents: string) => ({ kind: 'add', page: 0, rect: [1, 2, 3, 4], contents } as const)

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: () => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

function fakePort(timeout = 30_000) {
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
  const verify = vi.fn((_json: string) => true)
  const coordinator = new NativePdfEditCoordinator(worker, verify, timeout)
  const take = () => {
    const next = sent.shift()
    return next === undefined
      ? new Promise<NativePdfWorkerRequest>((resolve) => { receivers.push(resolve) })
      : Promise.resolve(next)
  }
  const deliver = (dispatch: NativePdfWorkerRequest, response: NativePdfResponse) => {
    worker.onmessage?.(new MessageEvent('message', { data: { dispatchId: dispatch.dispatchId, type: 'response', json: JSON.stringify(response) } }))
  }
  return { sent, worker, verify, coordinator, take, deliver }
}

function openReply(request: NativePdfRequest): Extract<NativePdfResponse, { operation: 'open'; status: 'opened' }> {
  if (request.operation !== 'open') throw new Error('expected open')
  const binding = { sessionId: 1, epoch: request.epoch, originHash: hash(1), revision: 0 }
  const snapshot: NativePdfSnapshot = {
    binding, originStoreHash: request.originStoreHash, outputHash: hash(1), consumedRequest: 0,
    outputHex: request.bytesHex,
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

function addReply(request: NativePdfRequest, previous: NativePdfSnapshot): Extract<NativePdfResponse, { status: 'applied' }> {
  if (request.operation !== 'command' || request.command.kind !== 'add') throw new Error('expected add')
  const revision = request.binding.revision + 1
  const binding = { ...request.binding, revision }
  const outputHash = hash(10 + revision)
  const page = previous.scene.pages[0]!
  const snapshot: NativePdfSnapshot = {
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

async function opened(lane: ReturnType<typeof fakePort>) {
  const opening = lane.coordinator.open(original())
  const dispatch = await lane.take()
  lane.deliver(dispatch, openReply(dispatch.request))
  return opening
}

describe('native PDF edit coordinator', () => {
  afterEach(() => { vi.useRealTimers() })

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

  it('drops queued commands when the worker dies during a mutation', async () => {
    const lane = fakePort()
    const initial = await opened(lane)
    const first = expect(lane.coordinator.command(note('in flight'))).rejects
      .toMatchObject({ code: 'FLOW_PDF_NATIVE_WORKER_FAILED' })
    const queued = expect(lane.coordinator.command(note('queued'))).rejects
      .toMatchObject({ code: 'FLOW_PDF_NATIVE_WORKER_FAILED' })
    const dispatched = await lane.take()
    const handler = lane.worker.onmessage
    lane.worker.onmessageerror?.(new MessageEvent('messageerror'))
    await Promise.all([first, queued])
    handler?.(new MessageEvent('message', { data: { dispatchId: dispatched.dispatchId, type: 'response', json: JSON.stringify(addReply(dispatched.request, initial)) } }))
    expect(lane.sent).toHaveLength(0)
    expect(lane.coordinator.snapshot()).toMatchObject({ phase: 'invalidated', accepted: initial })
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
    lane.deliver(dispatch, reply)
    await close
    await lane.coordinator.close()
    const replacement = lane.coordinator.open(original())
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
      native_pdf_close: run, verify_native_pdf_response: verify,
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
