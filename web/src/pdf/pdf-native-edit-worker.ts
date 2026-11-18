import {
  NATIVE_PDF_LIMITS,
  NATIVE_PDF_PROTOCOL_VERSION,
  fitsNativeJson,
  freezeNativeDto,
  isNativeRequestId,
  isNativeStoreHash,
  isNativeWorkerRequest,
  isNativeWorkerResponse,
  matchesNativeResponse,
  nativeBytesHex,
  nativeDispatchId,
  nativeHexBytes,
  readNativeResponse,
  sameNativeBytes,
  type NativePdfCommand,
  type NativePdfNoteScene,
  type NativePdfObjectCommand,
  type NativePdfRequest,
  type NativePdfResponse,
  type NativePdfSnapshot,
  type NativePdfTextCommand,
  type NativePdfWasmBoundary,
  type NativePdfWorkerRequest,
  type NativePdfWorkerResponse,
} from './pdf-native-edit-protocol.js'

type RustVerifier = NativePdfWasmBoundary['verify_native_pdf_response']
const STARTUP = 'FLOW_PDF_NATIVE_WORKER_STARTUP'
const WORKER_FAILED = 'FLOW_PDF_NATIVE_WORKER_FAILED'
const RESPONSE_INVALID = 'FLOW_PDF_NATIVE_RESPONSE_INVALID'
const RECONCILIATION_REQUIRED = 'FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED'
const REQUEST_INVALID = 'FLOW_PDF_NATIVE_REQUEST_INVALID'
const REPLY_LOST = 'FLOW_PDF_NATIVE_REPLY_LOST'

export class NativePdfWorkerError extends Error {
  constructor(readonly code: string, readonly requestId?: number) {
    super(code)
    this.name = 'NativePdfWorkerError'
  }
}

export interface NativePdfWorkerScope {
  onmessage: ((event: { readonly data: unknown }) => void) | null
  postMessage(message: NativePdfWorkerResponse): void
}

export interface NativePdfWorkerPort {
  onmessage: ((event: MessageEvent<unknown>) => void) | null
  onerror: ((event: ErrorEvent) => void) | null
  onmessageerror: ((event: MessageEvent<unknown>) => void) | null
  postMessage(message: NativePdfWorkerRequest): void
  terminate(): void
}

function verifyReply(request: NativePdfRequest, json: string, verify: RustVerifier): NativePdfResponse {
  if (!fitsNativeJson(json, NATIVE_PDF_LIMITS.resultBytes) || verify(json) !== true) {
    throw new NativePdfWorkerError(RESPONSE_INVALID, request.requestId)
  }
  const reply = readNativeResponse(json)
  if (reply === null || !matchesNativeResponse(request, reply)) {
    throw new NativePdfWorkerError(RESPONSE_INVALID, request.requestId)
  }
  return reply
}

export function installNativePdfWorker(scope: NativePdfWorkerScope, startup: Promise<NativePdfWasmBoundary>): void {
  const ready = startup.then((wasm) => wasm, () => null)
  scope.onmessage = ({ data }) => {
    const dispatchId = nativeDispatchId(data)
    if (dispatchId === null) return
    if (!isNativeWorkerRequest(data)) {
      scope.postMessage({ dispatchId, type: 'failed', code: REQUEST_INVALID })
      return
    }
    const request = freezeNativeDto(structuredClone(data.request))
    void ready.then((wasm) => {
      if (wasm === null) {
        scope.postMessage({ dispatchId, type: 'failed', code: STARTUP })
        return
      }
      let response: NativePdfWorkerResponse
      try {
        const serialized = JSON.stringify(request)
        const limit = request.operation === 'open' ? NATIVE_PDF_LIMITS.requestBytes : NATIVE_PDF_LIMITS.commandBytes
        if (!fitsNativeJson(serialized, limit)) throw new NativePdfWorkerError(REQUEST_INVALID)
        const json = wasm[`native_pdf_${request.operation}`](serialized)
        verifyReply(request, json, (value) => wasm.verify_native_pdf_response(value))
        response = { dispatchId, type: 'response', json }
      } catch (error: unknown) {
        const code = error instanceof NativePdfWorkerError && error.code === REQUEST_INVALID
          ? REQUEST_INVALID : RESPONSE_INVALID
        response = { dispatchId, type: 'failed', code }
      }
      scope.postMessage(response)
    })
  }
}

export interface NativePdfOriginal {
  readonly bytes: Uint8Array
  readonly originStoreHash: string
}

export interface NativePdfCoordinatorState {
  readonly phase: 'idle' | 'opening' | 'ready' | 'mutating' | 'reconciliationRequired' | 'reconciling' | 'closing' | 'invalidated'
  readonly accepted: NativePdfSnapshot | null
  readonly pendingRequestId: number | null
  readonly code: string | null
  readonly reopenOriginal: boolean
}

type CommandRequest = Extract<NativePdfRequest, { operation: 'command' }>

interface Pending {
  readonly request: NativePdfRequest
  readonly resolve: (reply: NativePdfResponse) => void
  readonly reject: (error: NativePdfWorkerError) => void
}

interface QueuedCommand {
  readonly command: NativePdfCommand
  readonly resolve: (snapshot: NativePdfSnapshot) => void
  readonly reject: (error: NativePdfWorkerError) => void
}

function assertSuccess(reply: NativePdfResponse): Exclude<NativePdfResponse, { status: 'failed' }> {
  if (reply.status === 'failed') throw new NativePdfWorkerError(reply.code, reply.requestId)
  return reply
}

function expectPhase<T extends 'opened' | 'applied' | 'closed'>(
  reply: NativePdfResponse,
  status: T,
): Extract<NativePdfResponse, { status: T }> {
  const result = assertSuccess(reply)
  if (result.status !== status) throw new NativePdfWorkerError(RESPONSE_INVALID, reply.requestId)
  return result as Extract<NativePdfResponse, { status: T }>
}

function errorFor(error: unknown, requestId?: number): NativePdfWorkerError {
  return error instanceof NativePdfWorkerError ? error : new NativePdfWorkerError(WORKER_FAILED, requestId)
}

function textTarget(snapshot: NativePdfSnapshot, command: NativePdfTextCommand) {
  for (const page of snapshot.scene.textPages ?? []) {
    if (page.refusal !== null) continue
    const index = page.islands.findIndex(({ id }) => sameNativeBytes(id, command.island))
    const island = page.islands[index]
    if (island) return { page: page.page, index, island }
  }
  return null
}

export function confirmedNativeReplacement(
  command: NativePdfTextCommand, previous: NativePdfSnapshot, snapshot: NativePdfSnapshot,
) {
  const target = textTarget(previous, command)
  if (!target) return null
  const page = snapshot.scene.textPages?.find(({ page }) => page === target.page)
  const replacement = page?.refusal === null ? page.islands[target.index] : undefined
  const original = target.island
  if (!replacement || replacement.text !== command.text || replacement.profile !== original.profile
    || replacement.operator !== original.operator || sameNativeBytes(replacement.id, original.id)) return null
  const [left, bottom, right, top] = replacement.bounds
  const [l, b, r, t] = original.bounds
  return l <= left && b <= bottom && right <= r && top <= t ? replacement : null
}

function confirmsObjectCommand(
  command: NativePdfObjectCommand, previous: NativePdfSnapshot, snapshot: NativePdfSnapshot,
): boolean {
  const target = 'target' in command ? command.target : null
  const page = 'page' in command ? command.page : command.target.page
  const before = previous.scene.objectPages?.[page]
  const after = snapshot.scene.objectPages?.[page]
  if (before?.refusal !== null || after?.refusal !== null) return false
  const selected = target === null ? 0 : before.objects.findIndex((object) =>
    object.profile === target.profile && sameNativeBytes(object.id, target.id))
  if (selected < 0 || (target === null && !before.canAdd)) return false

  const removed = command.kind === 'removeObject'
  const original = before.objects[selected]
  const result = after.objects[selected]
  const siblingsBefore = before.objects.filter((_, index) => target === null || index !== selected)
  const siblingsAfter = after.objects.filter((_, index) => removed || index !== selected)
  if (siblingsBefore.length !== siblingsAfter.length || siblingsBefore.some((object, index) => {
    const sibling = siblingsAfter[index]
    return sibling === undefined || sibling.profile !== object.profile || !sameNativeBytes(sibling.bounds, object.bounds)
  })) return false
  if (target !== null && after.objects.some((object) => sameNativeBytes(object.id, target.id))) return false
  if (removed) return true
  if (result === undefined) return false

  switch (command.kind) {
    case 'addImage':
      return result.kind === 'image' && sameNativeBytes(result.bounds, command.bounds)
    case 'replaceImage':
      return original?.kind === 'image' && result.kind === 'image' && sameNativeBytes(result.bounds, original.bounds)
    case 'addGraphic':
    case 'replaceGraphic': {
      if (command.kind === 'replaceGraphic' && original?.kind !== 'graphic') return false
      const { shape, points, stroke, strokeWidth } = command.graphic
      const padding = stroke === null ? 0 : strokeWidth * (shape === 'polygon' ? 10 : 0.5)
      const xs = points.map(([x]) => x)
      const ys = points.map(([, y]) => y)
      const expected = [Math.min(...xs) - padding, Math.min(...ys) - padding,
        Math.max(...xs) + padding, Math.max(...ys) + padding]
      return result.profile === shape && result.bounds.every((value, index) => {
        const bound = expected[index]
        return bound !== undefined && Math.abs(value - bound) <= 0.500001
      })
    }
    case 'moveObject': {
      if (original === undefined || result.profile !== original.profile) return false
      const [a, b, c, d, e, f] = command.transform
      const [l, bottom, r, t] = original.bounds
      const x = a * l + c * bottom + e * 1000
      const y = b * l + d * bottom + f * 1000
      const z = a * r + c * t + e * 1000
      const w = b * r + d * t + f * 1000
      const expected = [Math.min(x, z), Math.min(y, w), Math.max(x, z), Math.max(y, w)]
      const tolerance = [(Math.abs(a) + Math.abs(c) + 1) / 2, (Math.abs(b) + Math.abs(d) + 1) / 2]
      return result.bounds.every((value, index) =>
        Math.abs(value - expected[index]!) <= tolerance[index % 2]! + 0.000001)
    }
  }
}

function isObjectCommand(command: NativePdfCommand): command is NativePdfObjectCommand {
  return 'target' in command || command.kind === 'addImage' || command.kind === 'addGraphic'
}

function confirmsCommand(request: CommandRequest, previous: NativePdfSnapshot, snapshot: NativePdfSnapshot): boolean {
  const command = request.command
  if (isObjectCommand(command)) {
    return snapshot.binding.revision === previous.binding.revision + 1
      && snapshot.consumedRequest === request.requestId
      && confirmsObjectCommand(command, previous, snapshot)
  }
  if (command.kind === 'replaceText') return confirmedNativeReplacement(command, previous, snapshot) !== null
  for (const page of snapshot.scene.pages) {
    for (const note of page.annotations) {
      if (note.kind !== 'plainNote') continue
      switch (command.kind) {
        case 'add':
          if (page.id === command.page && note.contents === command.contents && sameNativeBytes(note.rect, command.rect)
            && !previous.scene.pages.some((old) => old.annotations.some((annotation) =>
              annotation.kind === 'plainNote' && annotation.id === note.id))) return true
          break
        case 'edit':
          if (note.id === command.note && note.contents === command.contents) return true
          break
        case 'move':
          if (note.id === command.note && page.id === command.page && sameNativeBytes(note.rect, command.rect)) return true
          break
        case 'remove':
          if (note.id === command.note) return false
          break
      }
    }
  }
  return command.kind === 'remove'
}

export class NativePdfEditCoordinator {
  private state: NativePdfCoordinatorState = Object.freeze({
    phase: 'idle', accepted: null, pendingRequestId: null, code: null, reopenOriginal: false,
  })
  private readonly listeners = new Set<() => void>()
  private readonly pending = new Map<number, Pending>()
  private readonly queue: QueuedCommand[] = []
  private nextDispatchId = 1
  private nextRequestId = 1
  private uncertain: CommandRequest | null = null
  private original: { readonly hex: string; readonly hash: string } | null = null
  private closeFlight: Promise<void> | null = null

  constructor(
    private readonly worker: NativePdfWorkerPort,
    private readonly verify: RustVerifier,
    private readonly replyTimeoutMs = 30_000,
  ) {
    if (typeof verify !== 'function') throw new NativePdfWorkerError(RESPONSE_INVALID)
    worker.onmessage = ({ data }) => this.receive(data)
    worker.onerror = () => this.invalidate(WORKER_FAILED)
    worker.onmessageerror = () => this.invalidate(WORKER_FAILED)
  }

  snapshot(): NativePdfCoordinatorState {
    return this.state
  }

  subscribe(listener: () => void): () => void {
    if (this.state.phase === 'invalidated') return () => undefined
    this.listeners.add(listener)
    return () => { this.listeners.delete(listener) }
  }

  outputBytes(): Uint8Array | null {
    return this.state.accepted === null ? null : nativeHexBytes(this.state.accepted.outputHex)
  }

  originalForReopen(): NativePdfOriginal | null {
    if (!this.state.reopenOriginal || this.original === null) return null
    return Object.freeze({ bytes: nativeHexBytes(this.original.hex), originStoreHash: this.original.hash })
  }

  async open(input: NativePdfOriginal): Promise<NativePdfSnapshot> {
    if (this.state.phase !== 'idle') throw new NativePdfWorkerError('FLOW_PDF_NATIVE_OPEN_BUSY')
    if (!(input.bytes instanceof Uint8Array) || input.bytes.length === 0
      || input.bytes.length > NATIVE_PDF_LIMITS.pdfBytes || !isNativeStoreHash(input.originStoreHash)) {
      throw new NativePdfWorkerError(REQUEST_INVALID)
    }
    const bytes = new Uint8Array(input.bytes)
    const originStoreHash = input.originStoreHash
    const requestId = this.nextRequestId++
    this.update('opening', requestId)
    let dispatched = false
    try {
      const actual = nativeBytesHex(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)))
      this.requireLive(requestId)
      if (actual !== originStoreHash) throw new NativePdfWorkerError(REQUEST_INVALID, requestId)
      const request = freezeNativeDto({
        protocolVersion: NATIVE_PDF_PROTOCOL_VERSION, operation: 'open', requestId,
        epoch: Array.from(crypto.getRandomValues(new Uint8Array(16))),
        originStoreHash, bytesHex: nativeBytesHex(bytes),
      } as const)
      this.original = { hex: request.bytesHex, hash: originStoreHash }
      dispatched = true
      const result = expectPhase(await this.send(request), 'opened')
      this.requireLive(requestId)
      this.update('ready', null, null, result.snapshot)
      return result.snapshot
    } catch (cause: unknown) {
      const error = errorFor(cause, requestId)
      if (!dispatched) {
        if (this.snapshot().phase === 'opening') this.update('idle', null, error.code)
      } else if (error.code === REPLY_LOST || error.code === RESPONSE_INVALID || error.code === WORKER_FAILED) {
        this.invalidate(error.code)
      } else if (this.snapshot().phase === 'opening') {
        this.original = null
        this.update('idle', null, error.code)
      }
      throw error
    }
  }

  command(command: NativePdfCommand): Promise<NativePdfSnapshot> {
    if (this.uncertain !== null) return Promise.reject(new NativePdfWorkerError(RECONCILIATION_REQUIRED, this.uncertain.requestId))
    if (this.state.phase !== 'ready' && this.state.phase !== 'mutating') {
      return Promise.reject(new NativePdfWorkerError('FLOW_PDF_NATIVE_SESSION_UNAVAILABLE'))
    }
    const current = this.state.accepted
    if (!current || !isNativeWorkerRequest({ dispatchId: 1, request: {
      ...this.boundRequest('command', 1, current), outputHash: current.outputHash, command,
    } })) return Promise.reject(new NativePdfWorkerError(REQUEST_INVALID))
    const owned = freezeNativeDto(structuredClone(command))
    return new Promise((resolve, reject) => {
      this.queue.push({ command: owned, resolve, reject })
      this.pump()
    })
  }

  async reconcile(): Promise<{ readonly status: 'accepted' | 'unseen' | 'evicted'; readonly snapshot: NativePdfSnapshot }> {
    const command = this.uncertain
    const previous = this.state.accepted
    if (this.state.phase !== 'reconciliationRequired' || command === null || previous === null) {
      throw new NativePdfWorkerError('FLOW_PDF_NATIVE_RECONCILIATION_UNAVAILABLE')
    }
    const { protocolVersion, requestId, binding, originStoreHash } = command
    this.update('reconciling', requestId)
    try {
      const reply = assertSuccess(await this.send({ operation: 'snapshot', protocolVersion, requestId, binding, originStoreHash }))
      this.requireLive(requestId)
      if (reply.operation !== 'snapshot') throw new NativePdfWorkerError(RESPONSE_INVALID, requestId)
      const current = reply.snapshot
      if ((reply.status === 'unseen' && (!sameNativeBytes(current.outputHash, previous.outputHash)
        || !sameNativeBytes(current.scene.sceneHash, previous.scene.sceneHash)
        || current.consumedRequest !== previous.consumedRequest))
        || (reply.status === 'accepted'
          && (current.consumedRequest === requestId || isObjectCommand(command.command))
          && !confirmsCommand(command, previous, current))) {
        throw new NativePdfWorkerError(RESPONSE_INVALID, requestId)
      }
      this.uncertain = null
      this.nextRequestId = Math.max(this.nextRequestId, current.consumedRequest + 1)
      this.update('ready', null, null, current)
      this.pump()
      return Object.freeze({ status: reply.status, snapshot: current })
    } catch (cause: unknown) {
      const error = errorFor(cause, requestId)
      if (this.snapshot().phase === 'reconciling') this.update('reconciliationRequired', requestId, error.code)
      throw error
    }
  }

  async project(signal?: AbortSignal): Promise<NativePdfNoteScene | null> {
    const accepted = this.state.accepted
    if (accepted === null || signal?.aborted || (this.state.phase !== 'ready' && this.state.phase !== 'mutating')) return null
    const requestId = this.nextRequestId++
    const request = this.boundRequest('snapshot', requestId, accepted)
    try {
      const response = await this.send(request, signal)
      if (signal?.aborted || this.state.accepted !== accepted || this.state.phase !== 'ready') return null
      const reply = assertSuccess(response)
      if (reply.operation !== 'snapshot' || reply.status !== 'unseen'
        || reply.snapshot.consumedRequest !== accepted.consumedRequest
        || !sameNativeBytes(reply.snapshot.outputHash, accepted.outputHash)
        || !sameNativeBytes(reply.snapshot.scene.sceneHash, accepted.scene.sceneHash)) return null
      return reply.snapshot.scene
    } catch (cause: unknown) {
      if (signal?.aborted || this.state.accepted !== accepted || this.state.phase !== 'ready') return null
      throw errorFor(cause, requestId)
    }
  }

  close(): Promise<void> {
    if (this.closeFlight !== null) return this.closeFlight
    if (this.state.phase === 'idle' || this.state.phase === 'invalidated') return Promise.resolve()
    const accepted = this.state.accepted
    if (this.state.phase !== 'ready' || accepted === null || this.queue.length > 0) {
      return Promise.reject(new NativePdfWorkerError('FLOW_PDF_NATIVE_CLOSE_BUSY'))
    }
    const requestId = this.nextRequestId++
    const request = this.boundRequest('close', requestId, accepted)
    this.closeFlight = Promise.resolve().then(async () => {
      try {
        expectPhase(await this.send(request), 'closed')
        this.requireLive(requestId)
        this.original = null
        this.update('idle', null, null, null)
      } catch (cause: unknown) {
        const error = errorFor(cause, requestId)
        this.invalidate(error.code)
        throw error
      } finally {
        this.closeFlight = null
      }
    })
    this.update('closing', requestId)
    for (const pending of this.pending.values()) {
      if (pending.request.operation === 'snapshot') {
        pending.reject(new NativePdfWorkerError('FLOW_PDF_NATIVE_PROJECTION_CANCELLED', pending.request.requestId))
      }
    }
    return this.closeFlight
  }

  dispose(): void {
    this.invalidate(WORKER_FAILED)
  }

  private boundRequest<O extends 'close' | 'snapshot' | 'command'>(operation: O, requestId: number, snapshot: NativePdfSnapshot) {
    return { protocolVersion: NATIVE_PDF_PROTOCOL_VERSION, operation, requestId,
      binding: snapshot.binding, originStoreHash: snapshot.originStoreHash } as const
  }

  private pump(): void {
    const current = this.state.accepted
    if (this.state.phase !== 'ready' || current === null) return
    let queued = this.queue.shift()
    while (queued?.command.kind === 'replaceText' && !textTarget(current, queued.command)) {
      queued.reject(new NativePdfWorkerError('FLOW_PDF_NATIVE_TARGET_UNAVAILABLE'))
      queued = this.queue.shift()
    }
    if (queued === undefined) return
    const requestId = this.nextRequestId++
    const request: CommandRequest = freezeNativeDto({
      protocolVersion: NATIVE_PDF_PROTOCOL_VERSION, operation: 'command', requestId,
      binding: current.binding, originStoreHash: current.originStoreHash,
      outputHash: current.outputHash, command: queued.command,
    })
    this.update('mutating', requestId)
    void this.apply(request, current, queued)
  }

  private async apply(request: CommandRequest, previous: NativePdfSnapshot, queued: QueuedCommand): Promise<void> {
    try {
      const reply = expectPhase(await this.send(request), 'applied')
      this.requireLive(request.requestId)
      if (!confirmsCommand(request, previous, reply.snapshot)) throw new NativePdfWorkerError(RESPONSE_INVALID, request.requestId)
      this.update('ready', null, null, reply.snapshot)
      queued.resolve(reply.snapshot)
    } catch (cause: unknown) {
      const error = errorFor(cause, request.requestId)
      if (this.state.phase !== 'invalidated') {
        if (error.code === REPLY_LOST || error.code === RESPONSE_INVALID || error.code === RECONCILIATION_REQUIRED) {
          this.uncertain = request
          this.update('reconciliationRequired', request.requestId, error.code)
        } else {
          this.update('ready', null, error.code)
        }
      }
      queued.reject(error)
    }
    this.pump()
  }

  private send(request: NativePdfRequest, signal?: AbortSignal): Promise<NativePdfResponse> {
    return new Promise((resolve, reject) => {
      this.requireLive(request.requestId)
      const dispatchId = this.nextDispatchId++
      if (!isNativeRequestId(request.requestId) || !isNativeRequestId(dispatchId)) {
        reject(new NativePdfWorkerError('FLOW_PDF_NATIVE_BUDGET_EXHAUSTED'))
        return
      }
      const fail = (code: string) => finish(() => reject(new NativePdfWorkerError(code, request.requestId)))
      const cancelled = () => fail('FLOW_PDF_NATIVE_PROJECTION_CANCELLED')
      const timer = setTimeout(() => fail(REPLY_LOST), this.replyTimeoutMs)
      const finish = (settle: () => void) => {
        if (!this.pending.delete(dispatchId)) return
        clearTimeout(timer)
        signal?.removeEventListener('abort', cancelled)
        settle()
      }
      this.pending.set(dispatchId, {
        request,
        resolve: (response) => finish(() => resolve(response)),
        reject: (error) => finish(() => reject(error)),
      })
      signal?.addEventListener('abort', cancelled, { once: true })
      if (signal?.aborted) {
        cancelled()
        return
      }
      try {
        this.worker.postMessage({ dispatchId, request })
      } catch {
        this.invalidate(WORKER_FAILED)
      }
    })
  }

  private receive(data: unknown): void {
    const dispatchId = nativeDispatchId(data)
    if (dispatchId === null) return
    const pending = this.pending.get(dispatchId)
    if (pending === undefined) return
    const request = pending.request
    try {
      if (!isNativeWorkerResponse(data)) throw new NativePdfWorkerError(RESPONSE_INVALID, request.requestId)
      if (data.type === 'failed') {
        if (data.code === STARTUP || data.code === WORKER_FAILED) this.invalidate(data.code)
        else pending.reject(new NativePdfWorkerError(RESPONSE_INVALID, request.requestId))
        return
      }
      pending.resolve(verifyReply(request, data.json, this.verify))
    } catch {
      pending.reject(new NativePdfWorkerError(RESPONSE_INVALID, request.requestId))
    }
  }

  private requireLive(requestId: number): void {
    if (this.state.phase === 'invalidated') throw new NativePdfWorkerError(this.state.code ?? WORKER_FAILED, requestId)
  }

  private invalidate(code: string): void {
    if (this.state.phase === 'invalidated') return
    this.worker.onmessage = null
    this.worker.onerror = null
    this.worker.onmessageerror = null
    this.worker.terminate()
    this.uncertain = null
    for (const pending of this.pending.values()) pending.reject(new NativePdfWorkerError(code, pending.request.requestId))
    for (const queued of this.queue.splice(0)) queued.reject(new NativePdfWorkerError(code))
    this.update('invalidated', null, code)
    this.listeners.clear()
  }

  private update(
    phase: NativePdfCoordinatorState['phase'],
    pendingRequestId: number | null,
    code: string | null = null,
    accepted = this.state.accepted,
  ): void {
    this.state = Object.freeze({ phase, pendingRequestId, code, accepted,
      reopenOriginal: phase === 'invalidated' && this.original !== null })
    for (const listener of this.listeners) {
      try {
        listener()
      } catch {
      }
    }
  }
}
