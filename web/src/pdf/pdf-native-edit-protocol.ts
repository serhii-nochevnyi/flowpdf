export const NATIVE_PDF_PROTOCOL_VERSION = 1
export const NATIVE_PDF_LIMITS = Object.freeze({
  identityVersion: 1,
  countUnit: 'utf8Bytes',
  requestBytes: 16 * 1024 * 1024 + 4096,
  commandBytes: 128 * 1024,
  resultBytes: 64 * 1024 * 1024,
  pdfBytes: 8 * 1024 * 1024,
  noteBytes: 16 * 1024,
  requestHistory: 128,
  sessions: 2,
  retainedBytes: 16 * 1024 * 1024,
} as const)

export type NativePdfRect = readonly [number, number, number, number]
export type NativePdfNoteCommand =
  | { readonly kind: 'add'; readonly page: number; readonly rect: NativePdfRect; readonly contents: string }
  | { readonly kind: 'edit'; readonly note: number; readonly contents: string }
  | { readonly kind: 'move'; readonly note: number; readonly page: number; readonly rect: NativePdfRect }
  | { readonly kind: 'remove'; readonly note: number }

export interface NativePdfBinding {
  readonly epoch: readonly number[]
  readonly sessionId: number
  readonly originHash: readonly number[]
  readonly revision: number
}

interface NativePdfObjectRef {
  readonly objectNumber: number
  readonly generation: number
}

export type NativePdfAnnotation =
  | { readonly kind: 'plainNote'; readonly id: number; readonly object: NativePdfObjectRef; readonly contents: string; readonly rect: NativePdfRect }
  | { readonly kind: 'readOnly'; readonly object: NativePdfObjectRef | null; readonly profile: 'text' | 'appearance' | 'link' | 'widget' | 'other'; readonly rect: NativePdfRect | null }

export interface NativePdfNoteScene {
  readonly outputHash: readonly number[]
  readonly sceneHash: readonly number[]
  readonly pages: readonly {
    readonly id: number
    readonly object: NativePdfObjectRef
    readonly geometry: { readonly bounds: NativePdfRect; readonly rotation: number } | null
    readonly canAddNote: boolean
    readonly annotations: readonly NativePdfAnnotation[]
  }[]
}

export interface NativePdfSnapshot {
  readonly binding: NativePdfBinding
  readonly originStoreHash: string
  readonly outputHash: readonly number[]
  readonly consumedRequest: number
  readonly scene: NativePdfNoteScene
  readonly outputHex: string
}

interface RequestIdentity {
  readonly protocolVersion: typeof NATIVE_PDF_PROTOCOL_VERSION
  readonly requestId: number
  readonly originStoreHash: string
}

interface BoundRequest extends RequestIdentity {
  readonly binding: NativePdfBinding
}

export type NativePdfRequest =
  | (RequestIdentity & { readonly operation: 'open'; readonly epoch: readonly number[]; readonly bytesHex: string })
  | (BoundRequest & { readonly operation: 'command'; readonly outputHash: readonly number[]; readonly command: NativePdfNoteCommand })
  | (BoundRequest & { readonly operation: 'snapshot' })
  | (BoundRequest & { readonly operation: 'close' })

interface ResponseIdentity {
  readonly protocolVersion: typeof NATIVE_PDF_PROTOCOL_VERSION
  readonly requestId: number
  readonly resultHash: string
  readonly limits: typeof NATIVE_PDF_LIMITS
}

type BoundResponse = ResponseIdentity & {
  readonly binding: NativePdfBinding
  readonly originStoreHash: string
  readonly code: null
}

export type NativePdfResponse =
  | (BoundResponse & { readonly operation: 'open'; readonly status: 'opened'; readonly snapshot: NativePdfSnapshot })
  | (BoundResponse & { readonly operation: 'command'; readonly status: 'applied'; readonly snapshot: NativePdfSnapshot })
  | (BoundResponse & { readonly operation: 'snapshot'; readonly status: 'accepted' | 'unseen' | 'evicted'; readonly snapshot: NativePdfSnapshot })
  | (BoundResponse & { readonly operation: 'close'; readonly status: 'closed'; readonly snapshot: null })
  | (ResponseIdentity & {
    readonly operation: NativePdfRequest['operation']
    readonly status: 'failed'
    readonly binding: NativePdfBinding | null
    readonly originStoreHash: string | null
    readonly snapshot: null
    readonly code: string
  })

export interface NativePdfWasmBoundary {
  native_pdf_open(json: string): string
  native_pdf_command(json: string): string
  native_pdf_snapshot(json: string): string
  native_pdf_close(json: string): string
  verify_native_pdf_response(json: string): boolean
}

export interface NativePdfWorkerRequest {
  readonly dispatchId: number
  readonly request: NativePdfRequest
}

export type NativePdfWorkerResponse =
  | { readonly dispatchId: number; readonly type: 'response'; readonly json: string }
  | { readonly dispatchId: number; readonly type: 'failed'; readonly code: NativePdfTransportCode }

export type NativePdfTransportCode =
  | 'FLOW_PDF_NATIVE_WORKER_STARTUP'
  | 'FLOW_PDF_NATIVE_WORKER_FAILED'
  | 'FLOW_PDF_NATIVE_REQUEST_INVALID'
  | 'FLOW_PDF_NATIVE_RESPONSE_INVALID'

const hexDigits = '0123456789abcdef'
const rustErrorCodes = new Set([
  'FLOW_PDF_NATIVE_REQUEST_DECODE',
  'FLOW_PDF_NATIVE_BINDING_MISMATCH',
  'FLOW_PDF_NATIVE_REVISION_MISMATCH',
  'FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED',
  'FLOW_PDF_NATIVE_BUDGET_EXHAUSTED',
  'FLOW_PDF_NATIVE_LIMITS_INVALID',
  'FLOW_PDF_NATIVE_UNSUPPORTED',
  'FLOW_PDF_NATIVE_GRAPH_MALFORMED',
  'FLOW_PDF_NATIVE_IMPORT_REFUSED',
  'FLOW_PDF_NATIVE_NOTE_INVALID',
  'FLOW_PDF_NATIVE_REQUEST_INVALID',
  'FLOW_PDF_NATIVE_REQUEST_CONFLICT',
  'FLOW_PDF_NATIVE_TARGET_UNAVAILABLE',
  'FLOW_PDF_NATIVE_POSTCONDITION_FAILED',
])

export function fitsNativeJson(value: unknown, limit: number): value is string {
  if (typeof value !== 'string' || value.length > limit) return false
  const firstNonAscii = value.search(/[^\x00-\x7f]/u)
  if (firstNonAscii === -1) return true
  let bytes = value.length
  for (let index = firstNonAscii; index < value.length; index++) {
    const unit = value.charCodeAt(index)
    if (unit < 0x80) continue
    bytes += unit < 0x800 ? 1 : 2
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1)
      if (next >= 0xdc00 && next <= 0xdfff) index++
    }
    if (bytes > limit) return false
  }
  return true
}

function shape(value: unknown, ...keys: string[]): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    && Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key))
}

function integer(value: unknown, min: number, max = Number.MAX_SAFE_INTEGER): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= min && value <= max
}

export function isNativeRequestId(value: unknown): value is number {
  return integer(value, 1)
}

function tuple(value: unknown, size: number, min: number, max?: number): value is readonly number[] {
  if (!Array.isArray(value) || value.length !== size) return false
  for (let index = 0; index < size; index++) {
    if (!integer(value[index], min, max)) return false
  }
  return true
}

function hash(value: unknown): value is readonly number[] {
  return tuple(value, 32, 0, 255)
}

function bytesHex(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0 && value.length <= 2 * NATIVE_PDF_LIMITS.pdfBytes
    && value.length % 2 === 0 && !/[^0-9a-f]/u.test(value)
}

export function isNativeStoreHash(value: unknown): value is string {
  return typeof value === 'string' && value.length === 64 && !/[^0-9a-f]/u.test(value)
}

function binding(value: unknown): value is NativePdfBinding {
  return shape(value, 'epoch', 'sessionId', 'originHash', 'revision')
    && tuple(value.epoch, 16, 0, 255) && isNativeRequestId(value.sessionId)
    && hash(value.originHash) && integer(value.revision, 0)
}

function rect(value: unknown): value is NativePdfRect {
  return tuple(value, 4, Number.MIN_SAFE_INTEGER)
}

function pageId(value: unknown): boolean {
  return integer(value, 0, 0xffffffff)
}

function contents(value: unknown): boolean {
  return typeof value === 'string' && !/[\uD800-\uDFFF]/u.test(value) && fitsNativeJson(value, NATIVE_PDF_LIMITS.noteBytes)
}

export function isNativeNoteCommand(value: unknown): value is NativePdfNoteCommand {
  if (shape(value, 'kind', 'page', 'rect', 'contents') && value.kind === 'add') {
    return pageId(value.page) && rect(value.rect) && contents(value.contents)
  }
  if (shape(value, 'kind', 'note', 'contents') && value.kind === 'edit') {
    return isNativeRequestId(value.note) && contents(value.contents)
  }
  if (shape(value, 'kind', 'note', 'page', 'rect') && value.kind === 'move') {
    return isNativeRequestId(value.note) && pageId(value.page) && rect(value.rect)
  }
  return shape(value, 'kind', 'note') && value.kind === 'remove' && isNativeRequestId(value.note)
}

export function isNativeWorkerRequest(value: unknown): value is NativePdfWorkerRequest {
  if (!shape(value, 'dispatchId', 'request') || !isNativeRequestId(value.dispatchId)) return false
  if (typeof value.request !== 'object' || value.request === null) return false
  const request = value.request as Record<string, unknown>
  const common = ['operation', 'protocolVersion', 'requestId', 'originStoreHash']
  const bound = [...common, 'binding']
  switch (request.operation) {
    case 'open':
      if (!shape(request, ...common, 'epoch', 'bytesHex')
        || !tuple(request.epoch, 16, 0, 255) || !bytesHex(request.bytesHex)) return false
      break
    case 'command':
      if (!shape(request, ...bound, 'outputHash', 'command') || !binding(request.binding)
        || !hash(request.outputHash) || !isNativeNoteCommand(request.command)) return false
      break
    case 'snapshot':
    case 'close':
      if (!shape(request, ...bound) || !binding(request.binding)) return false
      break
    default:
      return false
  }
  return request.protocolVersion === NATIVE_PDF_PROTOCOL_VERSION
    && isNativeRequestId(request.requestId) && isNativeStoreHash(request.originStoreHash)
}

export function nativeDispatchId(value: unknown): number | null {
  if (typeof value === 'object' && value !== null && 'dispatchId' in value
    && isNativeRequestId(value.dispatchId)) return value.dispatchId
  return null
}

export function isNativeWorkerResponse(value: unknown): value is NativePdfWorkerResponse {
  if (nativeDispatchId(value) === null) return false
  if (shape(value, 'dispatchId', 'type', 'json') && value.type === 'response') {
    return typeof value.json === 'string'
  }
  return shape(value, 'dispatchId', 'type', 'code') && value.type === 'failed'
    && (value.code === 'FLOW_PDF_NATIVE_WORKER_STARTUP' || value.code === 'FLOW_PDF_NATIVE_WORKER_FAILED'
      || value.code === 'FLOW_PDF_NATIVE_REQUEST_INVALID' || value.code === 'FLOW_PDF_NATIVE_RESPONSE_INVALID')
}

function objectRef(value: unknown): boolean {
  return shape(value, 'objectNumber', 'generation') && integer(value.objectNumber, 1, 0xffffffff)
    && integer(value.generation, 0, 0xffff)
}

function annotation(value: unknown): boolean {
  if (shape(value, 'kind', 'id', 'object', 'contents', 'rect') && value.kind === 'plainNote') {
    return isNativeRequestId(value.id) && objectRef(value.object) && contents(value.contents) && rect(value.rect)
  }
  return shape(value, 'kind', 'object', 'profile', 'rect') && value.kind === 'readOnly'
    && (value.object === null || objectRef(value.object)) && (value.rect === null || rect(value.rect))
    && (value.profile === 'text' || value.profile === 'appearance' || value.profile === 'link'
      || value.profile === 'widget' || value.profile === 'other')
}

function geometry(value: unknown): boolean {
  return shape(value, 'bounds', 'rotation') && rect(value.bounds)
    && (value.rotation === 0 || value.rotation === 90 || value.rotation === 180 || value.rotation === 270)
}

function scene(value: unknown): value is NativePdfNoteScene {
  return shape(value, 'sceneHash', 'outputHash', 'pages') && hash(value.sceneHash) && hash(value.outputHash)
    && Array.isArray(value.pages) && value.pages.length <= 4096
    && value.pages.every((page: unknown, index) => shape(page, 'id', 'object', 'geometry', 'canAddNote', 'annotations')
      && page.id === index && objectRef(page.object) && (page.geometry === null || geometry(page.geometry))
      && typeof page.canAddNote === 'boolean' && Array.isArray(page.annotations)
      && page.annotations.every(annotation))
}

export function sameNativeBytes(left: readonly number[], right: readonly number[]): boolean {
  return left.length === right.length && left.every((byte, index) => byte === right[index])
}

export function sameNativeSession(left: NativePdfBinding, right: NativePdfBinding): boolean {
  return left.sessionId === right.sessionId && sameNativeBytes(left.epoch, right.epoch)
    && sameNativeBytes(left.originHash, right.originHash)
}

function snapshot(value: unknown): value is NativePdfSnapshot {
  return shape(value, 'binding', 'originStoreHash', 'outputHash', 'consumedRequest', 'scene', 'outputHex')
    && binding(value.binding) && isNativeStoreHash(value.originStoreHash) && hash(value.outputHash)
    && integer(value.consumedRequest, value.binding.revision) && scene(value.scene) && bytesHex(value.outputHex)
    && sameNativeBytes(value.outputHash, value.scene.outputHash)
    && (value.binding.revision === 0) === (value.consumedRequest === 0)
    && (value.binding.revision !== 0 || sameNativeBytes(value.binding.originHash, value.outputHash))
}

function isNativeResponse(value: unknown): value is NativePdfResponse {
  if (!shape(value, 'protocolVersion', 'requestId', 'operation', 'binding', 'originStoreHash',
    'status', 'snapshot', 'code', 'limits', 'resultHash')
    || value.protocolVersion !== NATIVE_PDF_PROTOCOL_VERSION || !integer(value.requestId, 0)
    || !isNativeStoreHash(value.resultHash)
    || (value.operation !== 'open' && value.operation !== 'command' && value.operation !== 'snapshot' && value.operation !== 'close')) return false
  const limits = value.limits
  const knownLimits = Object.entries(NATIVE_PDF_LIMITS)
  if (!shape(limits, ...knownLimits.map(([key]) => key))
    || !knownLimits.every(([key, expected]) => limits[key] === expected)) return false

  if (value.status === 'failed') {
    return value.snapshot === null && typeof value.code === 'string' && rustErrorCodes.has(value.code)
      && (value.code !== 'FLOW_PDF_NATIVE_IMPORT_REFUSED' || (value.operation === 'open' && value.requestId > 0))
      && (value.requestId === 0 ? value.binding === null && value.originStoreHash === null
        : isNativeStoreHash(value.originStoreHash) && (value.operation === 'open' ? value.binding === null : binding(value.binding)))
  }
  if (value.requestId === 0 || value.code !== null || !binding(value.binding)
    || !isNativeStoreHash(value.originStoreHash)) return false
  if (value.operation === 'close') return value.status === 'closed' && value.snapshot === null
  if (!snapshot(value.snapshot) || !sameNativeSession(value.snapshot.binding, value.binding)
    || value.snapshot.originStoreHash !== value.originStoreHash) return false
  const base = value.binding.revision
  const current = value.snapshot.binding.revision
  const consumed = value.snapshot.consumedRequest
  switch (value.operation) {
    case 'open': return value.status === 'opened' && base === 0 && current === 0
    case 'command': return value.status === 'applied' && current === base + 1 && consumed === value.requestId
    case 'snapshot':
      if (value.status === 'accepted') {
        return (current === base + 1 && consumed === value.requestId)
          || (current > base + 1 && consumed > value.requestId)
      }
      if (value.status === 'evicted') return current - base > NATIVE_PDF_LIMITS.requestHistory && consumed > value.requestId
      return value.status === 'unseen' && current === base && consumed < value.requestId
  }
}

export function readNativeResponse(json: string): NativePdfResponse | null {
  if (!fitsNativeJson(json, NATIVE_PDF_LIMITS.resultBytes)) return null
  try {
    const value: unknown = JSON.parse(json)
    return isNativeResponse(value) ? freezeNativeDto(value) : null
  } catch {
    return null
  }
}

export function matchesNativeResponse(request: NativePdfRequest, response: NativePdfResponse): boolean {
  if (request.operation !== response.operation || request.requestId !== response.requestId
    || request.originStoreHash !== response.originStoreHash) return false
  if (request.operation === 'open') {
    return response.status === 'failed' ? response.binding === null
      : response.status === 'opened' && sameNativeBytes(request.epoch, response.binding.epoch)
        && request.bytesHex === response.snapshot.outputHex
  }
  return response.binding !== null && sameNativeSession(request.binding, response.binding)
    && request.binding.revision === response.binding.revision
}

export function freezeNativeDto<T>(value: T): T {
  if (typeof value === 'object' && value !== null) {
    for (const child of Object.values(value)) freezeNativeDto(child)
    Object.freeze(value)
  }
  return value
}

export function nativeBytesHex(bytes: Uint8Array): string {
  const chunks: string[] = []
  for (let start = 0; start < bytes.length; start += 4096) {
    let chunk = ''
    for (const byte of bytes.subarray(start, start + 4096)) {
      chunk += hexDigits.charAt(byte >> 4) + hexDigits.charAt(byte & 15)
    }
    chunks.push(chunk)
  }
  return chunks.join('')
}

export function nativeHexBytes(hex: string): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(hex.length / 2)
  for (let index = 0; index < bytes.length; index++) {
    bytes[index] = Number.parseInt(hex.slice(index * 2, index * 2 + 2), 16)
  }
  return bytes
}
