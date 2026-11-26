import {
  NATIVE_PDF_LIMITS,
  NATIVE_PDF_PROTOCOL_VERSION,
  fitsNativeJson,
  freezeNativeDto,
  isNativePageCommand,
  isNativeFieldCommand,
  isNativeRequestId,
  isNativeStoreHash,
  isNativeWorkerRequest,
  isNativeWorkerResponse,
  matchesNativeResponse,
  nativeBytesHex,
  nativeDispatchId,
  nativeHexBytes,
  nativePageMoveRefusals,
  nativePageOperationRefusal,
  readNativeResponse,
  sameNativeBytes,
  sameNativeSession,
  type NativePdfSessionCommand,
  type NativePdfNoteScene,
  type NativePdfObjectCommand,
  type NativePdfPageCommand,
  type NativePdfPagePreview,
  type NativePdfFieldCommand,
  type NativePdfFieldView,
  type NativePdfRequest,
  type NativePdfRedactionAction,
  type NativePdfRedactionCandidate,
  type NativePdfRedactionMarkAction,
  type NativePdfRedactionRequest,
  type NativePdfRedactionResult,
  type NativePdfResponse,
  type NativePdfSnapshot,
  type NativePdfVerifiedSnapshot,
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
const REDACTION_CANCELLED = 'FLOW_PDF_NATIVE_REDACTION_CANCELLED'

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
  let failed = false
  scope.onmessage = ({ data }) => {
    const dispatchId = nativeDispatchId(data)
    if (dispatchId === null) return
    if (!isNativeWorkerRequest(data)) {
      scope.postMessage({ dispatchId, type: 'failed', code: REQUEST_INVALID })
      return
    }
    const request = freezeNativeDto(structuredClone(data.request))
    void ready.then((wasm) => {
      if (failed || wasm === null) {
        scope.postMessage({ dispatchId, type: 'failed', code: failed ? WORKER_FAILED : STARTUP })
        return
      }
      let response: NativePdfWorkerResponse
      try {
        const serialized = JSON.stringify(request)
        const limit = request.operation === 'open' ? NATIVE_PDF_LIMITS.requestBytes : NATIVE_PDF_LIMITS.commandBytes
        if (!fitsNativeJson(serialized, limit)) throw new NativePdfWorkerError(REQUEST_INVALID)
        const execute = wasm[`native_pdf_${request.operation}`]
        if (execute === undefined) throw new NativePdfWorkerError(RESPONSE_INVALID)
        const json = execute.call(wasm, serialized)
        verifyReply(request, json, (value) => wasm.verify_native_pdf_response(value))
        response = { dispatchId, type: 'response', json }
      } catch (error: unknown) {
        if (!(error instanceof NativePdfWorkerError) && request.operation !== 'preview') {
          failed = true
          scope.postMessage({ dispatchId, type: 'failed', code: WORKER_FAILED })
          return
        }
        const code = error instanceof NativePdfWorkerError && error.code === REQUEST_INVALID ? REQUEST_INVALID : RESPONSE_INVALID
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
  readonly phase: 'idle' | 'opening' | 'ready' | 'mutating' | 'marking' | 'validating'
    | 'reconciliationRequired' | 'reconciling' | 'closing' | 'invalidated'
  readonly accepted: NativePdfVerifiedSnapshot | null
  readonly redaction: NativePdfRedactionResult | null
  readonly showingPriorAccepted: boolean
  readonly pendingRequestId: number | null
  readonly code: string | null
  readonly reopenOriginal: boolean
  readonly reopenAccepted: boolean
}

type CommandRequest = Extract<NativePdfRequest, { operation: 'command' }>
type RedactionResponse = Extract<NativePdfResponse, { status: 'redaction' | 'published' }>

interface RedactionJob {
  readonly request: NativePdfRedactionRequest
  readonly previous: NativePdfVerifiedSnapshot
  readonly expectedPages: readonly number[]
  readonly expectedMarkCount: number
  candidate: NativePdfRedactionCandidate | null
  publishing: boolean
  publishedCandidate: NativePdfRedactionCandidate | null
}

interface Pending {
  readonly request: NativePdfRequest
  readonly resolve: (reply: NativePdfResponse) => void
  readonly reject: (error: NativePdfWorkerError) => void
}

interface QueuedCommand {
  readonly command: NativePdfSessionCommand
  readonly indexedPages: readonly { readonly index: number; readonly stableId: number }[]
  readonly resolve: (snapshot: NativePdfVerifiedSnapshot) => void
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

function requiresReadback(error: NativePdfWorkerError): boolean {
  return error.code === REPLY_LOST || error.code === RESPONSE_INVALID || error.code === RECONCILIATION_REQUIRED
}

export function sameNativeAcceptedOutput(left: NativePdfSnapshot, right: NativePdfSnapshot): boolean {
  return sameNativeSession(left.binding, right.binding) && left.binding.revision === right.binding.revision
    && left.originStoreHash === right.originStoreHash && sameNativeBytes(left.outputHash, right.outputHash)
    && left.outputHex === right.outputHex
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
  const selected = target === null ? before.objects.length : before.objects.findIndex((object) =>
    object.profile === target.profile && sameNativeBytes(object.id, target.id))
  if (selected < 0 || (target === null && !before.canAdd)) return false

  const removed = command.kind === 'removeObject'
  const original = target === null ? undefined : before.objects[selected]
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

function isObjectCommand(command: NativePdfSessionCommand): command is NativePdfObjectCommand {
  return ('target' in command && 'profile' in command.target) || command.kind === 'addImage' || command.kind === 'addGraphic'
}

function confirmsPageCommand(command: NativePdfPageCommand, previous: NativePdfVerifiedSnapshot, current: NativePdfVerifiedSnapshot): boolean {
  const { order, selected } = current.pageState
  const before = previous.pageState.order
  const { page: index, stableId } = command.target
  if (before[index] !== stableId || selected === null) return false
  if (command.kind === 'reorderPage'
    ? nativePageMoveRefusals(previous.pageState, index)[command.position] !== undefined
    : nativePageOperationRefusal(previous.pageState, index, command.kind) !== undefined) return false
  const expected = [...before]
  const geometry = previous.scene.pages[index]?.geometry
  let selectedExpected = stableId
  switch (command.kind) {
    case 'insertBlankPage':
    case 'duplicatePage': {
      if (before.includes(selected)) return false
      const slot = index + (command.kind === 'insertBlankPage' && command.side === 'before' ? 0 : 1)
      expected.splice(slot, 0, selected)
      selectedExpected = selected
      const created = current.scene.pages[slot]
      if (!geometry || !created?.geometry || !sameNativeBytes(geometry.bounds, created.geometry.bounds)
        || geometry.rotation !== created.geometry.rotation
        || (command.kind === 'insertBlankPage' && created.annotations.length !== 0)) return false
      break
    }
    case 'deletePage':
      expected.splice(index, 1)
      selectedExpected = expected[Math.min(index, expected.length - 1)]!
      break
    case 'reorderPage':
      if (command.position > previous.pageState.maxPosition) return false
      expected.splice(index, 1)
      expected.splice(command.position, 0, stableId)
      break
    case 'rotatePage':
      if (!geometry || !current.scene.pages[index]?.geometry) return false
      break
  }
  if (selected !== selectedExpected || !sameNativeBytes(expected, order)) return false
  const positions = new Map(before.map((id, index) => [id, index]))
  return order.every((id, afterIndex) => {
    const beforeIndex = positions.get(id)
    if (beforeIndex === undefined) return true
    const prior = previous.scene.pages[beforeIndex]?.geometry
    const after = current.scene.pages[afterIndex]?.geometry
    if (!prior || !after) return prior === after
    const rotation = command.kind === 'rotatePage' && id === stableId
      ? (prior.rotation + command.degrees + 360) % 360
      : prior.rotation
    return after.rotation === rotation && sameNativeBytes(prior.bounds, after.bounds)
  })
}

function hasFieldTarget(id: readonly number[], snapshot: NativePdfVerifiedSnapshot): boolean {
  return snapshot.fields?.some((field) => sameNativeBytes(field.id, id)) ?? false
}

function sameFieldValue(left: NativePdfFieldView, right: NativePdfFieldView): boolean {
  return left.name === right.name && left.kind === right.kind && left.page === right.page
    && left.value === right.value && left.checked === right.checked
    && left.rect !== null && right.rect !== null && sameNativeBytes(left.rect, right.rect)
}

function confirmsFieldCommand(
  command: NativePdfFieldCommand, previous: NativePdfVerifiedSnapshot, current: NativePdfVerifiedSnapshot,
): boolean {
  const previousFields = previous.fields ?? []
  const currentFields = current.fields ?? []
  const readOnly = previousFields.filter((field) => !field.canEditAppearance)
  const currentReadOnly = currentFields.filter((field) => !field.canEditAppearance)
  if (readOnly.length !== currentReadOnly.length || !readOnly.every((field, index) => {
    const next = currentReadOnly[index]
    return next !== undefined && field.page === next.page && field.refusal === next.refusal
      && (field.rect === null || next.rect === null
        ? field.rect === next.rect
        : sameNativeBytes(field.rect, next.rect))
  })) return false

  const before = previousFields.filter((field) => field.canEditAppearance)
  const after = currentFields.filter((field) => field.canEditAppearance)
  if (new Set(before.map((field) => field.name)).size !== before.length
    || new Set(after.map((field) => field.name)).size !== after.length) return false

  const target = 'field' in command ? before.find((field) => sameNativeBytes(field.id, command.field)) : undefined
  if ('field' in command && target === undefined) return false
  const name = 'name' in command ? command.name : target?.name
  const accepted = after.find((field) => field.name === name)
  const untouched = before.filter((field) => field.name !== name)
  const survivors = after.filter((field) => field.name !== name)
  if (untouched.length !== survivors.length || !untouched.every((field) =>
    survivors.some((next) => sameFieldValue(field, next)))) return false

  if (command.kind === 'removeField') return accepted === undefined
  if (accepted === undefined || accepted.rect === null) return false
  switch (command.kind) {
    case 'addTextField':
    case 'addCheckboxField':
      return !before.some((field) => field.name === name) && accepted.page === command.page
        && sameNativeBytes(accepted.rect, command.rect)
        && (command.kind === 'addTextField' ? accepted.kind === 'text' && accepted.value === command.value
          : accepted.kind === 'checkbox' && accepted.checked === command.checked)
    case 'setFieldText':
      return target?.kind === 'text' && accepted.value === command.value
        && sameFieldValue({ ...target, value: command.value }, accepted)
    case 'setFieldChecked':
      return target?.kind === 'checkbox' && accepted.checked === command.checked
        && sameFieldValue({ ...target, checked: command.checked }, accepted)
    case 'moveField':
      return target !== undefined
        && sameFieldValue({ ...target, page: command.page, rect: command.rect }, accepted)
  }
}

function plainNotes(snapshot: NativePdfVerifiedSnapshot) {
  return snapshot.scene.pages.flatMap((page) => page.annotations.flatMap((annotation) =>
    annotation.kind === 'plainNote' ? [{ page: page.id, ...annotation }] : []))
}

type PlainNote = ReturnType<typeof plainNotes>[number]

function sameNote(left: PlainNote, right: PlainNote): boolean {
  return left.id === right.id && left.page === right.page && left.contents === right.contents
    && sameNativeBytes(left.rect, right.rect)
}

function sameNotes(before: readonly PlainNote[], after: readonly PlainNote[]): boolean {
  const byId = new Map(after.map((note) => [note.id, note]))
  return byId.size === after.length && before.length === after.length && before.every((note) => {
    const next = byId.get(note.id)
    return next !== undefined && sameNote(note, next)
  })
}

function isUnchangedSnapshot(previous: NativePdfVerifiedSnapshot, current: NativePdfVerifiedSnapshot): boolean {
  return sameNativeAcceptedOutput(previous, current)
    && previous.consumedRequest === current.consumedRequest
    && sameNativeBytes(previous.scene.sceneHash, current.scene.sceneHash)
    && sameNativeBytes(previous.pageState.order, current.pageState.order)
    && previous.pageState.selected === current.pageState.selected
    && sameNotes(plainNotes(previous), plainNotes(current))
}

function confirmsCommand(request: CommandRequest, previous: NativePdfVerifiedSnapshot, snapshot: NativePdfVerifiedSnapshot): boolean {
  const command = request.command
  if (isNativePageCommand(command)) {
    if (snapshot.binding.revision !== previous.binding.revision + 1
      || snapshot.consumedRequest !== request.requestId
      || !confirmsPageCommand(command, previous, snapshot)) return false
    const oldPages = new Set(previous.pageState.order)
    const positions = new Map(snapshot.pageState.order.map((id, index) => [id, index]))
    const survivors = plainNotes(previous).flatMap((note) => {
      const page = positions.get(previous.pageState.order[note.page]!)
      return page === undefined ? [] : [{ ...note, page }]
    })
    return sameNotes(survivors, plainNotes(snapshot).filter((note) => oldPages.has(snapshot.pageState.order[note.page]!)))
  }
  if (!sameNativeBytes(snapshot.pageState.order, previous.pageState.order)
    || snapshot.pageState.selected !== previous.pageState.selected) return false
  if (!['add', 'edit', 'move', 'remove'].some((kind) => command.kind === kind)
    && !sameNotes(plainNotes(previous), plainNotes(snapshot))) return false
  if (isObjectCommand(command)) {
    return snapshot.binding.revision === previous.binding.revision + 1
      && snapshot.consumedRequest === request.requestId
      && confirmsObjectCommand(command, previous, snapshot)
  }
  if (isNativeFieldCommand(command)) {
    return snapshot.binding.revision === previous.binding.revision + 1
      && snapshot.consumedRequest === request.requestId
      && confirmsFieldCommand(command, previous, snapshot)
  }
  if (command.kind === 'replaceText') return confirmedNativeReplacement(command, previous, snapshot) !== null
  if (snapshot.binding.revision !== previous.binding.revision + 1
    || snapshot.consumedRequest !== request.requestId) return false
  const before = plainNotes(previous)
  const after = plainNotes(snapshot)
  if (command.kind === 'add') {
    const existing = new Set(before.map((note) => note.id))
    const added = after.find((note) => !existing.has(note.id))
    return added !== undefined && added.page === command.page && added.contents === command.contents
      && sameNativeBytes(added.rect, command.rect) && sameNotes([...before, added], after)
  }
  const target = before.find((note) => note.id === command.note)
  if (target === undefined) return false
  const survivors = before.filter((note) => note.id !== command.note)
  if (command.kind === 'edit') survivors.push({ ...target, contents: command.contents })
  if (command.kind === 'move') survivors.push({ ...target, page: command.page, rect: command.rect })
  return sameNotes(survivors, after)
}

export class NativePdfEditCoordinator {
  private state: NativePdfCoordinatorState = Object.freeze({
    phase: 'idle', accepted: null, pendingRequestId: null, code: null, reopenOriginal: false, reopenAccepted: false,
    redaction: null, showingPriorAccepted: false,
  })
  private readonly listeners = new Set<() => void>()
  private readonly pending = new Map<number, Pending>()
  private readonly queue: QueuedCommand[] = []
  private nextDispatchId = 1
  private nextRequestId = 1
  private uncertain: CommandRequest | null = null
  private redactionJob: RedactionJob | null = null
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
    return this.state.accepted === null || this.redactionJob !== null || this.state.phase === 'invalidated'
      ? null : nativeHexBytes(this.state.accepted.outputHex)
  }

  originalForReopen(): NativePdfOriginal | null {
    if (!this.state.reopenOriginal || this.original === null) return null
    return Object.freeze({ bytes: nativeHexBytes(this.original.hex), originStoreHash: this.original.hash })
  }

  async open(input: NativePdfOriginal): Promise<NativePdfVerifiedSnapshot> {
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

  command(command: NativePdfSessionCommand): Promise<NativePdfVerifiedSnapshot> {
    if (this.uncertain !== null) return Promise.reject(new NativePdfWorkerError(RECONCILIATION_REQUIRED, this.uncertain.requestId))
    if (this.state.phase !== 'ready' && this.state.phase !== 'mutating') {
      return Promise.reject(new NativePdfWorkerError('FLOW_PDF_NATIVE_SESSION_UNAVAILABLE'))
    }
    const current = this.state.accepted
    if (!current || !isNativeWorkerRequest({ dispatchId: 1, request: {
      ...this.boundRequest('command', 1, current), outputHash: current.outputHash, command,
    } })) return Promise.reject(new NativePdfWorkerError(REQUEST_INVALID))
    const owned = freezeNativeDto(structuredClone(command))
    if ('note' in owned && !plainNotes(current).some((note) => note.id === owned.note)) {
      return Promise.reject(new NativePdfWorkerError('FLOW_PDF_NATIVE_TARGET_UNAVAILABLE'))
    }
    const index = 'page' in owned ? owned.page : 'target' in owned ? owned.target.page : undefined
    const indices = index === undefined ? [] : [index]
    if (owned.kind === 'reorderPage') indices.push(owned.position)
    const indexedPages: QueuedCommand['indexedPages'][number][] = []
    for (const index of indices) {
      const stableId = current.pageState.order[index]
      if (stableId === undefined) return Promise.reject(new NativePdfWorkerError('FLOW_PDF_NATIVE_TARGET_UNAVAILABLE'))
      indexedPages.push({ index, stableId })
    }
    return new Promise((resolve, reject) => {
      this.queue.push({ command: owned, indexedPages, resolve, reject })
      this.pump()
    })
  }

  async markRedaction(action: NativePdfRedactionMarkAction, expectedMarkCount: number): Promise<NativePdfRedactionResult> {
    if ((action?.kind !== 'addMark' && action?.kind !== 'removeMark' && action?.kind !== 'clearMarks')
      || !Number.isSafeInteger(expectedMarkCount) || expectedMarkCount < 0) {
      throw new NativePdfWorkerError(REQUEST_INVALID)
    }
    const job = this.startRedaction(action, 'marking', [], expectedMarkCount)
    try {
      const { redaction } = await this.settleRedactionMark(job)
      return redaction
    } catch (cause: unknown) {
      const error = errorFor(cause, job.request.requestId)
      this.failRedaction(job, error)
      throw error
    }
  }

  async applyRedaction(expectedPages: readonly number[], expectedMarkCount: number, signal?: AbortSignal): Promise<NativePdfRedactionResult> {
    if (signal?.aborted) throw new NativePdfWorkerError(REDACTION_CANCELLED)
    const job = this.startRedaction({ kind: 'stage' }, 'validating', expectedPages, expectedMarkCount)
    const requestId = job.request.requestId
    try {
      const staged = await this.redactionStep(job, job.request.action)
      this.requireLive(requestId)
      const candidate = staged.redaction.candidate
      if (candidate === null) throw new NativePdfWorkerError(RESPONSE_INVALID, requestId)
      job.candidate = candidate
      this.update('validating', requestId, null, job.previous, staged.redaction)
      let result = staged.redaction
      if (!signal?.aborted) {
        result = (await this.redactionStep(job, { kind: 'verify', candidate })).redaction
      }
      this.requireLive(requestId)
      this.update('validating', requestId, null, job.previous, result)
      if (signal?.aborted || result.verification?.status !== 'passed') {
        await this.redactionStep(job, { kind: 'cancel', candidate })
        this.finishRedaction(job, result, job.previous, signal?.aborted ? REDACTION_CANCELLED : null)
        if (signal?.aborted) throw new NativePdfWorkerError(REDACTION_CANCELLED, requestId)
        return result
      }
      job.publishing = true
      const reply = await this.redactionStep(job, { kind: 'publish', candidate })
      if (reply.status !== 'published') throw new NativePdfWorkerError(RESPONSE_INVALID, requestId)
      this.finishRedaction(job, reply.redaction, reply.snapshot)
      return reply.redaction
    } catch (cause: unknown) {
      const error = errorFor(cause, requestId)
      this.failRedaction(job, error)
      throw error
    }
  }

  async reconcile(): Promise<{ readonly status: 'accepted' | 'unseen' | 'evicted'; readonly snapshot: NativePdfVerifiedSnapshot }> {
    if (this.state.phase === 'reconciliationRequired' && this.redactionJob !== null) {
      return this.reconcileRedaction(this.redactionJob)
    }
    const command = this.uncertain
    const previous = this.state.accepted
    if (this.state.phase !== 'reconciliationRequired' || command === null || previous === null) {
      throw new NativePdfWorkerError('FLOW_PDF_NATIVE_RECONCILIATION_UNAVAILABLE')
    }
    const { protocolVersion, requestId, binding, originStoreHash } = command
    this.update('reconciling', requestId, null, previous, this.state.redaction, this.state.reopenAccepted ? 'accepted' : null)
    try {
      const reply = assertSuccess(await this.send({ operation: 'snapshot', protocolVersion, requestId, binding, originStoreHash }))
      this.requireLive(requestId)
      if (reply.operation !== 'snapshot') throw new NativePdfWorkerError(RESPONSE_INVALID, requestId)
      const current = reply.snapshot
      if ((reply.status === 'unseen' && !isUnchangedSnapshot(previous, current))
        || (reply.status === 'accepted'
          && (current.consumedRequest === requestId || command.command.kind !== 'replaceText')
          && !confirmsCommand(command, previous, current))) {
        throw new NativePdfWorkerError(RESPONSE_INVALID, requestId)
      }
      this.uncertain = null
      this.nextRequestId = Math.max(this.nextRequestId, current.consumedRequest + 1)
      const redaction = reply.status === 'unseen' && this.state.redaction?.published ? this.state.redaction : null
      this.update('ready', null, null, current, redaction)
      this.pump()
      return Object.freeze({ status: reply.status, snapshot: current })
    } catch (cause: unknown) {
      const error = errorFor(cause, requestId)
      if (this.snapshot().phase === 'reconciling') {
        this.update('reconciliationRequired', requestId, error.code, previous, this.state.redaction, 'accepted')
      }
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
        || !sameNativeBytes(reply.snapshot.scene.sceneHash, accepted.scene.sceneHash)
        || !sameNotes(plainNotes(accepted), plainNotes(reply.snapshot))) return null
      return reply.snapshot.scene
    } catch (cause: unknown) {
      if (signal?.aborted || this.state.accepted !== accepted || this.state.phase !== 'ready') return null
      throw errorFor(cause, requestId)
    }
  }

  async previewPage(pageId: number, signal: AbortSignal): Promise<NativePdfPagePreview | null> {
    const accepted = this.state.accepted
    if (!accepted || signal.aborted || this.state.phase !== 'ready') return null
    const index = accepted.pageState.order.indexOf(pageId)
    const geometry = accepted.scene.pages[index]?.geometry
    if (!geometry) return null
    const request = freezeNativeDto({
      ...this.boundRequest('preview', this.nextRequestId++, accepted),
      outputHash: accepted.outputHash, pageId,
    })
    try {
      const reply = await this.send(request, signal)
      if (signal.aborted || this.state.phase !== 'ready' || this.state.accepted !== accepted
        || reply.status !== 'preview' || reply.preview.rotation !== geometry.rotation) return null
      const [l, b, r, t] = reply.preview.cropBox
      // @contract: Note geometry rounds inward to whole points within the exact raster crop.
      const bounds = [Math.ceil(l / 1000), Math.ceil(b / 1000), Math.floor(r / 1000), Math.floor(t / 1000)]
      return sameNativeBytes(bounds, geometry.bounds) ? reply.preview : null
    } catch {
      return null
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
      } catch (cause: unknown) {
        const error = errorFor(cause, requestId)
        this.invalidate(error.code)
        throw error
      } finally {
        this.closeFlight = null
      }
      this.original = null
      this.update('idle', null, null, null)
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
    this.invalidate(WORKER_FAILED, true)
  }

  private startRedaction(
    action: NativePdfRedactionAction, phase: 'marking' | 'validating', expectedPages: readonly number[],
    expectedMarkCount: number,
  ): RedactionJob {
    const previous = this.state.accepted
    if (this.state.phase !== 'ready' || previous === null || this.queue.length !== 0 || this.redactionJob !== null) {
      throw new NativePdfWorkerError('FLOW_PDF_NATIVE_SESSION_UNAVAILABLE')
    }
    if ((action.kind === 'addMark' || action.kind === 'stage') && previous.redactionAdmission !== 'supported') {
      throw new NativePdfWorkerError('FLOW_PDF_NATIVE_UNSUPPORTED')
    }
    if (action.kind === 'stage' && (expectedPages.length === 0
      || !Number.isSafeInteger(expectedMarkCount) || expectedMarkCount < expectedPages.length || expectedMarkCount > 1024
      || !sameNativeBytes(previous.pageState.order.filter((id) => expectedPages.includes(id)), expectedPages))) {
      throw new NativePdfWorkerError(REQUEST_INVALID)
    }
    const request = {
      ...this.boundRequest('redaction', this.nextRequestId++, previous),
      acceptedHash: previous.outputHash, action,
    } as const
    if (!isNativeWorkerRequest({ dispatchId: 1, request })) throw new NativePdfWorkerError(REQUEST_INVALID)
    const job = {
      request: freezeNativeDto(structuredClone(request)), previous, expectedPages: Object.freeze([...expectedPages]),
      expectedMarkCount, candidate: null, publishing: false, publishedCandidate: null,
    }
    this.redactionJob = job
    this.update(phase, request.requestId)
    return job
  }

  private async redactionStep(job: RedactionJob, action: NativePdfRedactionAction): Promise<RedactionResponse> {
    const request = { ...job.request, action }
    const reply = await this.send(request).then(assertSuccess).catch(async (cause: unknown) => {
      const error = errorFor(cause, request.requestId)
      if (action.kind === 'verify' && this.redactionJob === job && !requiresReadback(error)) {
        await this.cancelRedaction(job, action.candidate, error.code)
      }
      throw error
    })
    this.requireLive(request.requestId)
    if (reply.operation !== 'redaction' || this.redactionJob !== job) {
      throw new NativePdfWorkerError(RESPONSE_INVALID, request.requestId)
    }
    const { candidate, markCount, published } = reply.redaction
    const { order, selected } = job.previous.pageState
    if (published && !job.publishing
      || reply.status === 'published' && (!sameNativeBytes(reply.snapshot.pageState.order, order)
        || reply.snapshot.pageState.selected !== selected)) {
      throw new NativePdfWorkerError(RESPONSE_INVALID, request.requestId)
    }
    if (action.kind === 'cancel') {
      // @invariant: A cancelled candidate settles the attempt only when the retained mark count matches.
      if (markCount !== job.expectedMarkCount) throw new NativePdfWorkerError(RESPONSE_INVALID, request.requestId)
      return reply
    }
    const unpublishedCheck = !published && (action.kind === 'stage' || action.kind === 'verify' || action.kind === 'status')
    if ((unpublishedCheck && markCount !== job.expectedMarkCount)
      || (candidate !== null && !sameNativeBytes(job.expectedPages, candidate.affectedPages))) {
      if (unpublishedCheck && candidate !== null) await this.cancelRedaction(job, candidate, RESPONSE_INVALID)
      throw new NativePdfWorkerError(RESPONSE_INVALID, request.requestId)
    }
    return reply
  }

  private async cancelRedaction(job: RedactionJob, candidate: NativePdfRedactionCandidate, code: string | null): Promise<void> {
    job.candidate = candidate
    const { redaction } = await this.redactionStep(job, { kind: 'cancel', candidate })
    this.finishRedaction(job, redaction, job.previous, code)
  }

  private async reconcileRedaction(job: RedactionJob): Promise<{
    readonly status: 'accepted' | 'unseen'; readonly snapshot: NativePdfVerifiedSnapshot
  }> {
    const requestId = job.request.requestId
    this.update('reconciling', requestId, null, job.previous, this.state.redaction, this.state.reopenAccepted ? 'both' : null)
    try {
      if (job.request.action.kind !== 'stage') {
        const { snapshot } = await this.settleRedactionMark(job)
        return Object.freeze({ status: 'unseen', snapshot })
      }
      if (job.publishedCandidate === null) {
        const { redaction } = await this.redactionStep(job, { kind: 'status', candidate: job.candidate })
        if (!redaction.published) {
          if (redaction.candidate === null) this.finishRedaction(job, redaction)
          else await this.cancelRedaction(job, redaction.candidate, null)
          return Object.freeze({ status: 'unseen', snapshot: job.previous })
        }
        if (redaction.candidate === null) throw new NativePdfWorkerError(RESPONSE_INVALID, requestId)
        job.publishedCandidate = redaction.candidate
      }
      const reply = await this.redactionStep(job, { kind: 'readPublished', candidate: job.publishedCandidate })
      if (reply.status !== 'published') throw new NativePdfWorkerError(RESPONSE_INVALID, requestId)
      this.finishRedaction(job, reply.redaction, reply.snapshot)
      return Object.freeze({ status: 'accepted', snapshot: reply.snapshot })
    } catch (cause: unknown) {
      const error = errorFor(cause, requestId)
      if (this.redactionJob === job) {
        if (error.code === REPLY_LOST) {
          this.update('reconciliationRequired', requestId, error.code, job.previous, this.state.redaction, 'both')
        } else {
          this.invalidate(error.code)
        }
      }
      throw error
    }
  }

  private async settleRedactionMark(job: RedactionJob) {
    const { previous, request: { action, requestId } } = job
    const { redaction } = await this.redactionStep(job, action)
    if (redaction.markCount !== job.expectedMarkCount) {
      this.invalidate(RESPONSE_INVALID)
      throw new NativePdfWorkerError(RESPONSE_INVALID, requestId)
    }
    const reply = await this.send(this.boundRequest('snapshot', requestId, previous))
    this.requireLive(requestId)
    if (reply.operation !== 'snapshot' || reply.status !== 'unseen' || !isUnchangedSnapshot(previous, reply.snapshot)) {
      throw new NativePdfWorkerError(RESPONSE_INVALID, requestId)
    }
    const { redactionAdmission } = reply.snapshot
    const snapshot = redactionAdmission === previous.redactionAdmission
      ? previous : Object.freeze({ ...previous, redactionAdmission })
    this.finishRedaction(job, redaction, snapshot)
    return { redaction, snapshot }
  }

  private finishRedaction(
    job: RedactionJob,
    redaction: NativePdfRedactionResult,
    accepted = job.previous,
    code: string | null = null,
  ): void {
    this.requireLive(job.request.requestId)
    this.redactionJob = null
    this.update('ready', null, code, accepted, freezeNativeDto(redaction))
  }

  private failRedaction(job: RedactionJob, error: NativePdfWorkerError): void {
    if (this.redactionJob !== job) return
    if (job.candidate !== null || requiresReadback(error)) {
      this.update('reconciliationRequired', job.request.requestId, error.code)
    } else {
      this.redactionJob = null
      this.update('ready', null, error.code)
    }
  }

  private boundRequest<O extends 'close' | 'snapshot' | 'command' | 'redaction' | 'preview'>(operation: O, requestId: number, snapshot: NativePdfSnapshot) {
    return { protocolVersion: NATIVE_PDF_PROTOCOL_VERSION, operation, requestId,
      binding: snapshot.binding, originStoreHash: snapshot.originStoreHash } as const
  }

  private pump(): void {
    const current = this.state.accepted
    if (this.state.phase !== 'ready' || current === null) return
    let queued = this.queue.shift()
    const noteIds = new Set(plainNotes(current).map((note) => note.id))
    while (queued && (('note' in queued.command && !noteIds.has(queued.command.note))
      || (queued.command.kind === 'replaceText' && !textTarget(current, queued.command))
      || (isNativeFieldCommand(queued.command) && 'field' in queued.command
        && !hasFieldTarget(queued.command.field, current))
      || (isNativePageCommand(queued.command)
        && current.pageState.order[queued.command.target.page] !== queued.command.target.stableId)
      || queued.indexedPages.some(({ index, stableId }) => current.pageState.order[index] !== stableId))) {
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

  private async apply(request: CommandRequest, previous: NativePdfVerifiedSnapshot, queued: QueuedCommand): Promise<void> {
    try {
      const reply = expectPhase(await this.send(request), 'applied')
      this.requireLive(request.requestId)
      if (!confirmsCommand(request, previous, reply.snapshot)) throw new NativePdfWorkerError(RESPONSE_INVALID, request.requestId)
      this.update('ready', null, null, reply.snapshot)
      queued.resolve(reply.snapshot)
    } catch (cause: unknown) {
      const error = errorFor(cause, request.requestId)
      if (this.state.phase !== 'invalidated') {
        if (requiresReadback(error)) {
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

  private invalidate(code: string, propagateCleanupError = false): void {
    if (this.state.phase === 'invalidated') return
    const recoverRedaction = this.redactionJob !== null
      && (this.redactionJob.publishing || this.state.phase === 'reconciling' || this.state.phase === 'reconciliationRequired')
    this.worker.onmessage = null
    this.worker.onerror = null
    this.worker.onmessageerror = null
    try {
      this.worker.terminate()
    } catch (cause) {
      if (propagateCleanupError) throw cause
    } finally {
      this.uncertain = null
      this.redactionJob = null
      for (const pending of this.pending.values()) pending.reject(new NativePdfWorkerError(code, pending.request.requestId))
      for (const queued of this.queue.splice(0)) queued.reject(new NativePdfWorkerError(code))
      this.update('invalidated', null, code, this.state.accepted, null, recoverRedaction ? 'both' : 'original')
      this.listeners.clear()
    }
  }

  private update(
    phase: NativePdfCoordinatorState['phase'],
    pendingRequestId: number | null,
    code: string | null = null,
    accepted = this.state.accepted,
    redaction = accepted === this.state.accepted ? this.state.redaction : null,
    reopen: 'original' | 'accepted' | 'both' | null = phase === 'invalidated' ? 'original' : null,
  ): void {
    const showingPriorAccepted = accepted !== null && (phase === 'invalidated' || this.redactionJob?.request.action.kind === 'stage'
      || redaction?.action === 'cancel' || redaction?.verification != null && !redaction.published)
    this.state = Object.freeze({ phase, pendingRequestId, code, accepted, redaction, showingPriorAccepted,
      reopenOriginal: (reopen === 'original' || reopen === 'both') && this.original !== null,
      reopenAccepted: reopen === 'accepted' || reopen === 'both' })
    for (const listener of this.listeners) {
      try {
        listener()
      } catch {
      }
    }
  }
}
