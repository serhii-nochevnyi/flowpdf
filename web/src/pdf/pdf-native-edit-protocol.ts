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
  imageBytes: 60 * 1024,
  imagePixels: 16_000_000,
  imageDimension: 8192,
  graphicPoints: 32,
  graphicStrokeWidth: 32_000,
  imageFormats: Object.freeze(['png', 'jpeg'] as const),
} as const)

export type NativePdfRect = readonly [number, number, number, number]
export type NativePdfNoteCommand =
  | { readonly kind: 'add'; readonly page: number; readonly rect: NativePdfRect; readonly contents: string }
  | { readonly kind: 'edit'; readonly note: number; readonly contents: string }
  | { readonly kind: 'move'; readonly note: number; readonly page: number; readonly rect: NativePdfRect }
  | { readonly kind: 'remove'; readonly note: number }

export type NativePdfTextCommand = {
  readonly kind: 'replaceText'
  readonly island: readonly number[]
  readonly text: string
}

export type NativePdfObjectProfile = 'rasterRgb8' | 'rasterGray8' | 'line' | 'rectangle' | 'polygon'

export interface NativePdfObjectTarget {
  readonly page: number
  readonly id: readonly number[]
  readonly profile: NativePdfObjectProfile
}

// @contract: Object bounds, points, and stroke widths use thousandths of a PDF point in unrotated page space.
export interface NativePdfGraphic {
  readonly shape: 'line' | 'rectangle' | 'polygon'
  readonly points: readonly (readonly [number, number])[]
  readonly fill: readonly [number, number, number] | null
  readonly stroke: readonly [number, number, number] | null
  readonly strokeWidth: number
}

export type NativePdfObjectCommand =
  | { readonly kind: 'addGraphic'; readonly page: number; readonly graphic: NativePdfGraphic }
  | { readonly kind: 'replaceGraphic'; readonly target: NativePdfObjectTarget; readonly graphic: NativePdfGraphic }
  | { readonly kind: 'addImage'; readonly page: number; readonly bounds: NativePdfRect; readonly assetHex: string }
  | { readonly kind: 'replaceImage'; readonly target: NativePdfObjectTarget; readonly assetHex: string }
  | { readonly kind: 'moveObject'; readonly target: NativePdfObjectTarget; readonly transform: readonly [number, number, number, number, number, number] }
  | { readonly kind: 'removeObject'; readonly target: NativePdfObjectTarget }

const fieldRefusals = [
  'FLOW_PDF_NATIVE_FIELD_PROFILE',
  'FLOW_PDF_NATIVE_FIELD_REFERENCE',
  'FLOW_PDF_NATIVE_FIELD_APPEARANCE',
  'FLOW_PDF_NATIVE_FIELD_GLYPH',
  'FLOW_PDF_NATIVE_FIELD_GEOMETRY',
] as const

export type NativePdfFieldCommand =
  | { readonly kind: 'addTextField'; readonly page: number; readonly rect: NativePdfRect; readonly name: string; readonly value: string }
  | { readonly kind: 'addCheckboxField'; readonly page: number; readonly rect: NativePdfRect; readonly name: string; readonly checked: boolean }
  | { readonly kind: 'setFieldText'; readonly field: readonly number[]; readonly value: string }
  | { readonly kind: 'setFieldChecked'; readonly field: readonly number[]; readonly checked: boolean }
  | { readonly kind: 'moveField'; readonly field: readonly number[]; readonly page: number; readonly rect: NativePdfRect }
  | { readonly kind: 'removeField'; readonly field: readonly number[] }

export interface NativePdfFieldView {
  readonly id: readonly number[]
  readonly object: { readonly objectNumber: number; readonly generation: number }
  readonly page: number
  readonly kind: 'text' | 'checkbox' | null
  readonly name: string | null
  readonly rect: NativePdfRect | null
  readonly value: string | null
  readonly checked: boolean | null
  readonly canEditAppearance: boolean
  readonly refusal: typeof fieldRefusals[number] | null
}

export interface NativePdfPageTarget {
  readonly page: number
  readonly stableId: number
}

export type NativePdfPageCommand =
  | { readonly kind: 'insertBlankPage'; readonly target: NativePdfPageTarget; readonly side: 'before' | 'after' }
  | { readonly kind: 'reorderPage'; readonly target: NativePdfPageTarget; readonly position: number }
  | { readonly kind: 'rotatePage'; readonly target: NativePdfPageTarget; readonly degrees: -90 | 90 }
  | { readonly kind: 'duplicatePage'; readonly target: NativePdfPageTarget }
  | { readonly kind: 'deletePage'; readonly target: NativePdfPageTarget }

export interface NativePdfPageState {
  readonly order: readonly number[]
  readonly selected: number | null
  readonly canDelete: boolean
  readonly maxPosition: number
}

export type NativePdfCommand = NativePdfNoteCommand | NativePdfTextCommand | NativePdfObjectCommand
export type NativePdfEditCommand = NativePdfCommand | NativePdfPageCommand
export type NativePdfSessionCommand = NativePdfEditCommand | NativePdfFieldCommand

const objectRefusals = [
  'FLOW_PDF_NATIVE_OBJECT_STATE',
  'FLOW_PDF_NATIVE_OBJECT_GEOMETRY',
  'FLOW_PDF_NATIVE_OBJECT_ASSET',
  'FLOW_PDF_NATIVE_OBJECT_PROFILE',
] as const

export interface NativePdfObjectPage {
  readonly page: number
  readonly canAdd: boolean
  readonly objects: readonly {
    readonly id: readonly number[]
    readonly kind: 'image' | 'graphic'
    readonly profile: NativePdfObjectProfile
    readonly bounds: NativePdfRect
    readonly operations: readonly ['replace', 'move', 'remove']
  }[]
  readonly refusal: typeof objectRefusals[number] | null
}

const textRefusals = [
  'FLOW_PDF_NATIVE_TEXT_FONT',
  'FLOW_PDF_NATIVE_TEXT_MAPPING',
  'FLOW_PDF_NATIVE_TEXT_LAYOUT',
  'FLOW_PDF_NATIVE_TEXT_OWNERSHIP',
  'FLOW_PDF_NATIVE_TEXT_OVERFLOW',
] as const

export type NativePdfTextRefusal = typeof textRefusals[number]

export interface NativePdfTextIsland {
  readonly id: readonly number[]
  readonly text: string
  readonly bounds: NativePdfRect
  readonly profile: 'trueTypeWinAnsi' | 'identityHCidFontType2'
  readonly operator: 'Tj' | 'TJ'
  readonly maxAdvance: number
}

export interface NativePdfTextPage {
  readonly page: number
  readonly islands: readonly NativePdfTextIsland[]
  readonly refusal: NativePdfTextRefusal | null
}

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
  readonly textPages?: readonly NativePdfTextPage[]
  readonly objectPages?: readonly NativePdfObjectPage[]
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
  readonly pageState?: NativePdfPageState
  readonly redactionAdmission?: 'supported' | 'unavailable'
  readonly scene: NativePdfNoteScene
  readonly outputHex: string
  readonly fields?: readonly NativePdfFieldView[]
}

export interface NativePdfVerifiedSnapshot extends NativePdfSnapshot {
  readonly redactionAdmission: NonNullable<NativePdfSnapshot['redactionAdmission']>
  readonly pageState: NativePdfPageState
}

interface RequestIdentity {
  readonly protocolVersion: typeof NATIVE_PDF_PROTOCOL_VERSION
  readonly requestId: number
  readonly originStoreHash: string
}

interface BoundRequest extends RequestIdentity {
  readonly binding: NativePdfBinding
}

export type NativePdfRedactionMarkAction =
  | { readonly kind: 'addMark' | 'removeMark'; readonly pageId: number; readonly rect: NativePdfRect }
  | { readonly kind: 'clearMarks' }

export interface NativePdfRedactionCandidate {
  readonly hash: readonly number[]
  readonly validatorVersion: 1
  readonly validatorProfile: 'classic-xref-owned-144dpi-v1'
  readonly affectedPages: readonly number[]
}

export type NativePdfRedactionAction =
  | NativePdfRedactionMarkAction
  | { readonly kind: 'stage' }
  | { readonly kind: 'status'; readonly candidate: NativePdfRedactionCandidate | null }
  | { readonly kind: 'verify' | 'publish' | 'readPublished' | 'cancel'; readonly candidate: NativePdfRedactionCandidate }

type NativePdfVerificationStatus = 'pending' | 'passed' | 'failed' | 'unavailable' | 'inconclusive'

interface NativePdfRedactionCheck {
  readonly status: NativePdfVerificationStatus
  readonly coveredPages: number
}

export interface NativePdfRedactionVerification {
  readonly status: NativePdfVerificationStatus
  readonly structure: NativePdfRedactionCheck
  readonly text: NativePdfRedactionCheck
  readonly raster: NativePdfRedactionCheck
}

export interface NativePdfRedactionResult {
  readonly action: NativePdfRedactionAction['kind']
  readonly acceptedHash: readonly number[]
  readonly markCount: number
  readonly candidate: NativePdfRedactionCandidate | null
  readonly verification: NativePdfRedactionVerification | null
  readonly published: boolean
}

export type NativePdfRedactionRequest = BoundRequest & {
  readonly operation: 'redaction'
  readonly acceptedHash: readonly number[]
  readonly action: NativePdfRedactionAction
}

export type NativePdfPreviewRequest = BoundRequest & {
  readonly operation: 'preview'
  readonly outputHash: readonly number[]
  readonly pageId: number
}

export interface NativePdfPagePreview {
  readonly outputHash: readonly number[]
  readonly pageId: number
  // @contract: Crop coordinates are in thousandths of a point. RGB rows start at the top left.
  readonly cropBox: NativePdfRect
  readonly rotation: number
  readonly width: number
  readonly height: number
  readonly rgbHex: string
}

export type NativePdfRequest =
  | (RequestIdentity & { readonly operation: 'open'; readonly epoch: readonly number[]; readonly bytesHex: string })
  | (BoundRequest & { readonly operation: 'command'; readonly outputHash: readonly number[]; readonly command: NativePdfSessionCommand })
  | (BoundRequest & { readonly operation: 'snapshot' })
  | (BoundRequest & { readonly operation: 'close' })
  | NativePdfRedactionRequest
  | NativePdfPreviewRequest

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
  | (BoundResponse & { readonly operation: 'open'; readonly status: 'opened'; readonly snapshot: NativePdfVerifiedSnapshot })
  | (BoundResponse & { readonly operation: 'command'; readonly status: 'applied'; readonly snapshot: NativePdfVerifiedSnapshot })
  | (BoundResponse & { readonly operation: 'snapshot'; readonly status: 'accepted' | 'unseen' | 'evicted'; readonly snapshot: NativePdfVerifiedSnapshot })
  | (BoundResponse & { readonly operation: 'close'; readonly status: 'closed'; readonly snapshot: null })
  | (BoundResponse & { readonly operation: 'redaction'; readonly status: 'redaction'; readonly snapshot: null; readonly redaction: NativePdfRedactionResult })
  | (BoundResponse & { readonly operation: 'redaction'; readonly status: 'published'; readonly snapshot: NativePdfVerifiedSnapshot; readonly redaction: NativePdfRedactionResult })
  | (BoundResponse & { readonly operation: 'preview'; readonly status: 'preview'; readonly snapshot: null; readonly preview: NativePdfPagePreview })
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
  native_pdf_redaction(json: string): string
  native_pdf_preview?(json: string): string
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
const textRefusalCodes = new Set<string>(textRefusals)
const objectRefusalCodes = new Set<string>(objectRefusals)
const fieldRefusalCodes = new Set<string>(fieldRefusals)
const rustErrorCodes = new Set([
  ...fieldRefusals,
  ...textRefusals,
  ...objectRefusals,
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
  'FLOW_PDF_NATIVE_FINAL_PAGE',
  'FLOW_PDF_NATIVE_POSTCONDITION_FAILED',
])
const redactionErrorCodes = new Set([
  'FLOW_PDF_NATIVE_REDACTION_BUSY',
  'FLOW_PDF_NATIVE_REDACTION_CANDIDATE_UNAVAILABLE',
  'FLOW_PDF_NATIVE_REDACTION_UNVERIFIED',
  'FLOW_PDF_NATIVE_REDACTION_MARK_INVALID',
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

function redactionCandidate(value: unknown): value is NativePdfRedactionCandidate {
  return shape(value, 'hash', 'validatorVersion', 'validatorProfile', 'affectedPages')
    && hash(value.hash) && value.validatorVersion === 1 && value.validatorProfile === 'classic-xref-owned-144dpi-v1'
    && Array.isArray(value.affectedPages) && value.affectedPages.length > 0 && value.affectedPages.length <= 256
    && value.affectedPages.every(isNativeRequestId) && new Set(value.affectedPages).size === value.affectedPages.length
}

function redactionAction(value: unknown): value is NativePdfRedactionAction {
  if (shape(value, 'kind')) return value.kind === 'clearMarks' || value.kind === 'stage'
  if (shape(value, 'kind', 'pageId', 'rect')) {
    return (value.kind === 'addMark' || value.kind === 'removeMark') && isNativeRequestId(value.pageId)
      && tuple(value.rect, 4, -1_000_000_000, 1_000_000_000)
      && value.rect[0]! < value.rect[2]! && value.rect[1]! < value.rect[3]!
  }
  return shape(value, 'kind', 'candidate') && (value.kind === 'status' && value.candidate === null
    || (value.kind === 'status' || value.kind === 'verify' || value.kind === 'publish'
      || value.kind === 'readPublished' || value.kind === 'cancel')
      && redactionCandidate(value.candidate))
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

function rasterProfile(value: unknown): boolean {
  return value === 'rasterRgb8' || value === 'rasterGray8'
}

function graphicShape(value: unknown): boolean {
  return value === 'line' || value === 'rectangle' || value === 'polygon'
}

function objectTarget(value: unknown): boolean {
  return shape(value, 'page', 'id', 'profile') && pageId(value.page) && hash(value.id)
    && (rasterProfile(value.profile) || graphicShape(value.profile))
}

const objectCoordinateLimit = 1_000_000_000

function objectBounds(value: unknown): value is NativePdfRect {
  return rect(value) && value.every((coordinate) => Math.abs(coordinate) <= objectCoordinateLimit)
    && value[0] < value[2] && value[1] < value[3]
}

function graphic(value: unknown): boolean {
  if (!shape(value, 'shape', 'points', 'fill', 'stroke', 'strokeWidth') || !graphicShape(value.shape)
    || !Array.isArray(value.points)
    || (value.shape === 'polygon'
      ? value.points.length < 3 || value.points.length > NATIVE_PDF_LIMITS.graphicPoints
      : value.points.length !== 2)) return false
  return value.points.every((point: unknown) => tuple(point, 2, -objectCoordinateLimit, objectCoordinateLimit))
    && (value.fill === null || (value.shape !== 'line' && tuple(value.fill, 3, 0, 255)))
    && (value.stroke === null
      ? value.fill !== null && value.strokeWidth === 0
      : tuple(value.stroke, 3, 0, 255) && integer(value.strokeWidth, 1, NATIVE_PDF_LIMITS.graphicStrokeWidth))
}

function imageHex(value: unknown): boolean {
  return typeof value === 'string' && value.length <= 2 * NATIVE_PDF_LIMITS.imageBytes && bytesHex(value)
}

function objectCommand(value: unknown): boolean {
  if (typeof value !== 'object' || value === null) return false
  const command = value as Record<string, unknown>
  switch (command.kind) {
    case 'addGraphic':
      return shape(command, 'kind', 'page', 'graphic') && pageId(command.page) && graphic(command.graphic)
    case 'replaceGraphic':
      return shape(command, 'kind', 'target', 'graphic') && objectTarget(command.target) && graphic(command.graphic)
    case 'addImage':
      return shape(command, 'kind', 'page', 'bounds', 'assetHex') && pageId(command.page)
        && objectBounds(command.bounds) && imageHex(command.assetHex)
    case 'replaceImage':
      return shape(command, 'kind', 'target', 'assetHex') && objectTarget(command.target) && imageHex(command.assetHex)
    case 'moveObject':
      return shape(command, 'kind', 'target', 'transform') && objectTarget(command.target)
        && Array.isArray(command.transform) && command.transform.length === 6
        && Array.from(command.transform).every((number: unknown) => typeof number === 'number'
          && Number.isFinite(number) && Math.abs(number) <= 1_000_000)
    case 'removeObject':
      return shape(command, 'kind', 'target') && objectTarget(command.target)
    default:
      return false
  }
}

export function isNativePageCommand(value: unknown): value is NativePdfPageCommand {
  if (typeof value !== 'object' || value === null) return false
  const command = value as Record<string, unknown>
  if (!shape(command.target, 'page', 'stableId')
    || !integer(command.target.page, 0, 4095) || !isNativeRequestId(command.target.stableId)) return false
  switch (command.kind) {
    case 'insertBlankPage':
      return shape(command, 'kind', 'target', 'side') && (command.side === 'before' || command.side === 'after')
    case 'reorderPage':
      return shape(command, 'kind', 'target', 'position') && integer(command.position, 0, 4095)
    case 'rotatePage':
      return shape(command, 'kind', 'target', 'degrees') && (command.degrees === -90 || command.degrees === 90)
    case 'duplicatePage':
    case 'deletePage':
      return shape(command, 'kind', 'target')
    default:
      return false
  }
}

const maxFieldNameBytes = 64

function fieldText(value: unknown, limit = 1024): value is string {
  return fitsNativeJson(value, limit) && !/[\u0000-\u001f\u007f-\u009f\uD800-\uDFFF]/u.test(value)
}

function editableFieldName(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0 && value.length <= maxFieldNameBytes && !/[^A-Za-z0-9_-]/u.test(value)
}

function fieldDisplayName(value: unknown): value is string {
  return fieldText(value, maxFieldNameBytes) && value.length > 0
}

function fieldRect(value: unknown): value is NativePdfRect {
  return rect(value) && value.every((coordinate) => Math.abs(coordinate) <= 1_000_000) && value[2] - value[0] >= 8 && value[3] - value[1] >= 8
}

export function isNativeFieldCommand(value: unknown): value is NativePdfFieldCommand {
  if (typeof value !== 'object' || value === null) return false
  const command = value as Record<string, unknown>
  switch (command.kind) {
    case 'addTextField':
      return shape(command, 'kind', 'page', 'rect', 'name', 'value') && pageId(command.page)
        && fieldRect(command.rect) && editableFieldName(command.name) && fieldText(command.value)
    case 'addCheckboxField':
      return shape(command, 'kind', 'page', 'rect', 'name', 'checked') && pageId(command.page)
        && fieldRect(command.rect) && editableFieldName(command.name) && typeof command.checked === 'boolean'
    case 'setFieldText':
      return shape(command, 'kind', 'field', 'value') && hash(command.field) && fieldText(command.value)
    case 'setFieldChecked':
      return shape(command, 'kind', 'field', 'checked') && hash(command.field) && typeof command.checked === 'boolean'
    case 'moveField':
      return shape(command, 'kind', 'field', 'page', 'rect') && hash(command.field)
        && pageId(command.page) && fieldRect(command.rect)
    case 'removeField':
      return shape(command, 'kind', 'field') && hash(command.field)
    default:
      return false
  }
}

function isNativeCommand(value: unknown): value is NativePdfSessionCommand {
  return isNativeNoteCommand(value)
    || (shape(value, 'kind', 'island', 'text') && value.kind === 'replaceText'
      && hash(value.island) && contents(value.text))
    || objectCommand(value) || isNativePageCommand(value) || isNativeFieldCommand(value)
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
        || !hash(request.outputHash) || !isNativeCommand(request.command)) return false
      break
    case 'redaction':
      if (!shape(request, ...bound, 'acceptedHash', 'action') || !binding(request.binding)
        || !hash(request.acceptedHash) || !redactionAction(request.action)) return false
      break
    case 'preview':
      if (!shape(request, ...bound, 'outputHash', 'pageId') || !binding(request.binding)
        || !hash(request.outputHash) || !isNativeRequestId(request.pageId)) return false
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

function textIsland(value: unknown): boolean {
  return shape(value, 'id', 'text', 'bounds', 'profile', 'operator', 'maxAdvance') && hash(value.id)
    && typeof value.text === 'string' && value.text.length > 0 && contents(value.text)
    && rect(value.bounds) && value.bounds[0] < value.bounds[2] && value.bounds[1] < value.bounds[3]
    && (value.profile === 'trueTypeWinAnsi' || value.profile === 'identityHCidFontType2')
    && (value.operator === 'Tj' || value.operator === 'TJ') && integer(value.maxAdvance, 0)
}

function textPages(value: unknown, pageCount: number): boolean {
  if (!Array.isArray(value) || value.length === 0 || value.length > pageCount) return false
  let previous = -1
  for (const page of value) {
    if (!shape(page, 'page', 'islands', 'refusal') || !integer(page.page, previous + 1, pageCount - 1)
      || !Array.isArray(page.islands) || page.islands.length > 128) return false
    if (page.refusal === null) {
      if (page.islands.length === 0 || !page.islands.every(textIsland)) return false
    } else if (page.islands.length !== 0 || typeof page.refusal !== 'string'
      || !textRefusalCodes.has(page.refusal)) return false
    previous = page.page
  }
  return true
}

function objectPages(value: unknown, count: number): boolean {
  return Array.isArray(value) && value.length === count && value.every((page: unknown, index) =>
    shape(page, 'page', 'canAdd', 'objects', 'refusal') && page.page === index
    && Array.isArray(page.objects) && page.objects.length <= 128
    && typeof page.canAdd === 'boolean'
    && (page.refusal === null
      ? page.objects.every((object: unknown) =>
        shape(object, 'id', 'kind', 'profile', 'bounds', 'operations') && hash(object.id)
        && (object.kind === 'image' ? rasterProfile(object.profile)
          : object.kind === 'graphic' && graphicShape(object.profile))
        && objectBounds(object.bounds) && Array.isArray(object.operations)
        && object.operations.length === 3 && object.operations[0] === 'replace'
        && object.operations[1] === 'move' && object.operations[2] === 'remove')
      : page.canAdd === false && page.objects.length === 0 && typeof page.refusal === 'string'
        && objectRefusalCodes.has(page.refusal)))
}

function scene(value: unknown): value is NativePdfNoteScene {
  const keys = ['sceneHash', 'outputHash', 'pages']
  if (typeof value !== 'object' || value === null) return false
  if (Object.hasOwn(value, 'objectPages')) keys.push('objectPages')
  if (Object.hasOwn(value, 'textPages')) keys.push('textPages')
  return shape(value, ...keys)
    && hash(value.sceneHash) && hash(value.outputHash)
    && Array.isArray(value.pages) && value.pages.length <= 4096
    && (!Object.hasOwn(value, 'objectPages') || objectPages(value.objectPages, value.pages.length))
    && (!Object.hasOwn(value, 'textPages') || textPages(value.textPages, value.pages.length))
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

function pageState(value: unknown, count: number): boolean {
  return count > 0 && shape(value, 'order', 'selected', 'canDelete', 'maxPosition')
    && Array.isArray(value.order) && value.order.length === count
    && value.order.every(isNativeRequestId) && new Set(value.order).size === count
    && (value.selected === null || (isNativeRequestId(value.selected) && value.order.includes(value.selected)))
    && value.canDelete === (count > 1) && value.maxPosition === count - 1
}

function fieldViews(value: unknown, count: number): boolean {
  return Array.isArray(value) && value.length > 0 && value.every((field: unknown) =>
    shape(field, 'id', 'object', 'page', 'kind', 'name', 'rect', 'value', 'checked', 'canEditAppearance', 'refusal')
    && hash(field.id) && objectRef(field.object) && integer(field.page, 0, count - 1)
    && (field.canEditAppearance === true
      ? field.refusal === null && editableFieldName(field.name) && fieldRect(field.rect)
        && (field.kind === 'text' ? fieldText(field.value) && field.checked === null
          : field.kind === 'checkbox' && field.value === null && typeof field.checked === 'boolean')
      : field.canEditAppearance === false && (field.rect === null || fieldRect(field.rect))
        && field.kind === null && (field.name === null || fieldDisplayName(field.name))
        && field.value === null && field.checked === null
        && typeof field.refusal === 'string' && fieldRefusalCodes.has(field.refusal)))
}

function snapshot(value: unknown): value is NativePdfVerifiedSnapshot {
  if (typeof value !== 'object' || value === null) return false
  const keys = ['binding', 'originStoreHash', 'outputHash', 'consumedRequest', 'scene', 'outputHex', 'pageState', 'redactionAdmission']
  if (Object.hasOwn(value, 'fields')) keys.push('fields')
  return shape(value, ...keys)
    && binding(value.binding) && isNativeStoreHash(value.originStoreHash) && hash(value.outputHash)
    && integer(value.consumedRequest, value.binding.revision) && scene(value.scene) && bytesHex(value.outputHex)
    && pageState(value.pageState, value.scene.pages.length)
    && (value.redactionAdmission === 'supported' || value.redactionAdmission === 'unavailable')
    && (!Object.hasOwn(value, 'fields') || fieldViews(value.fields, value.scene.pages.length))
    && sameNativeBytes(value.outputHash, value.scene.outputHash)
    && (value.binding.revision === 0) === (value.consumedRequest === 0)
    && (value.binding.revision !== 0 || sameNativeBytes(value.binding.originHash, value.outputHash))
}

function verificationStatus(value: unknown): value is NativePdfVerificationStatus {
  return value === 'pending' || value === 'passed' || value === 'failed' || value === 'unavailable' || value === 'inconclusive'
}

function redactionCheck(value: unknown, pages: number): value is NativePdfRedactionCheck {
  return shape(value, 'status', 'coveredPages') && verificationStatus(value.status)
    && value.coveredPages === (value.status === 'passed' ? pages : 0)
}

function redactionVerification(value: unknown, pages: number): value is NativePdfRedactionVerification {
  if (!shape(value, 'status', 'structure', 'text', 'raster') || !verificationStatus(value.status)
    || !redactionCheck(value.structure, pages) || !redactionCheck(value.text, pages)
    || !redactionCheck(value.raster, pages)) return false
  const checks = [value.structure.status, value.text.status, value.raster.status]
  switch (value.status) {
    case 'pending': return checks.every((status) => status === 'pending')
    case 'passed': return checks.every((status) => status === 'passed')
    case 'failed': return true
    case 'unavailable': return !checks.includes('failed') && checks.includes('unavailable')
    case 'inconclusive': return !checks.includes('failed') && !checks.includes('unavailable')
      && !checks.every((status) => status === 'passed') && !checks.every((status) => status === 'pending')
  }
}

function redactionResult(value: unknown, snapshotStatus: unknown): value is NativePdfRedactionResult {
  if (!shape(value, 'action', 'acceptedHash', 'markCount', 'candidate', 'verification', 'published')
    || !hash(value.acceptedHash) || !integer(value.markCount, 0, 1024) || typeof value.published !== 'boolean') return false
  if (value.action === 'cancel') {
    return snapshotStatus === 'redaction' && !value.published && value.verification === null
      && redactionCandidate(value.candidate) && value.candidate.affectedPages.length <= value.markCount
  }
  let status: NativePdfVerificationStatus | null = null
  if (redactionCandidate(value.candidate)) {
    const count = value.candidate.affectedPages.length
    if ((value.published ? value.markCount !== 0 : count > value.markCount)
      || !redactionVerification(value.verification, count)) return false
    status = value.verification.status
  } else if (value.candidate !== null || value.verification !== null) return false
  if (value.published && status !== 'passed') return false
  if (snapshotStatus === 'published') return (value.action === 'publish' || value.action === 'readPublished') && value.published
  switch (value.action) {
    case 'addMark':
    case 'removeMark': return !value.published && status === null
    case 'clearMarks': return !value.published && status === null && value.markCount === 0
    case 'stage': return !value.published && status === 'pending'
    case 'verify': return !value.published && status !== null && status !== 'pending'
    case 'status': return true
    default: return false
  }
}

function pagePreview(value: unknown): value is NativePdfPagePreview {
  if (!shape(value, 'outputHash', 'pageId', 'cropBox', 'rotation', 'width', 'height', 'rgbHex')
    || !hash(value.outputHash) || !isNativeRequestId(value.pageId)
    || !tuple(value.cropBox, 4, -1_000_000_000, 1_000_000_000)
    || (value.rotation !== 0 && value.rotation !== 90 && value.rotation !== 180 && value.rotation !== 270)
    || !integer(value.width, 1, 8192) || !integer(value.height, 1, 8192)
    || value.width * value.height > 4_000_000 || typeof value.rgbHex !== 'string'
    || value.rgbHex.length !== value.width * value.height * 6 || /[^0-9a-f]/u.test(value.rgbHex)) return false
  const [l, b, r, t] = value.cropBox
  const sideways = value.rotation === 90 || value.rotation === 270
  return value.width === Math.ceil((sideways ? t! - b! : r! - l!) / 500)
    && value.height === Math.ceil((sideways ? r! - l! : t! - b!) / 500)
}

function isNativeResponse(value: unknown): value is NativePdfResponse {
  if (typeof value !== 'object' || value === null) return false
  const keys = ['protocolVersion', 'requestId', 'operation', 'binding', 'originStoreHash',
    'status', 'snapshot', 'code', 'limits', 'resultHash']
  const hasRedaction = Object.hasOwn(value, 'redaction')
  if (hasRedaction) keys.push('redaction')
  const hasPreview = Object.hasOwn(value, 'preview')
  if (hasPreview) keys.push('preview')
  if (!shape(value, ...keys)
    || value.protocolVersion !== NATIVE_PDF_PROTOCOL_VERSION || !integer(value.requestId, 0)
    || !isNativeStoreHash(value.resultHash)
    || (value.operation !== 'open' && value.operation !== 'command' && value.operation !== 'snapshot'
      && value.operation !== 'close' && value.operation !== 'redaction' && value.operation !== 'preview')
    || hasRedaction !== (value.operation === 'redaction' && value.status !== 'failed')
    || hasPreview !== (value.operation === 'preview' && value.status === 'preview')) return false
  const limits = value.limits
  const knownLimits = Object.entries(NATIVE_PDF_LIMITS)
  if (!shape(limits, ...knownLimits.map(([key]) => key))
    || !knownLimits.every(([key, expected]) => {
      const actual = limits[key]
      return Array.isArray(expected) ? Array.isArray(actual) && actual.length === expected.length
        && expected.every((element, index) => actual[index] === element) : actual === expected
    })) return false

  if (value.status === 'failed') {
    return value.snapshot === null && typeof value.code === 'string' && (rustErrorCodes.has(value.code)
      || (redactionErrorCodes.has(value.code) && value.requestId > 0 && (value.operation === 'redaction'
        || value.operation === 'command' && value.code === 'FLOW_PDF_NATIVE_REDACTION_BUSY')))
      && (!(textRefusalCodes.has(value.code) || objectRefusalCodes.has(value.code) || fieldRefusalCodes.has(value.code))
        || (value.operation === 'command' && value.requestId > 0))
      && (value.code !== 'FLOW_PDF_NATIVE_IMPORT_REFUSED' || (value.operation === 'open' && value.requestId > 0))
      && (value.requestId === 0 ? value.binding === null && value.originStoreHash === null
        : isNativeStoreHash(value.originStoreHash) && (value.operation === 'open' ? value.binding === null : binding(value.binding)))
  }
  if (value.requestId === 0 || value.code !== null || !binding(value.binding)
    || !isNativeStoreHash(value.originStoreHash)) return false
  if (value.operation === 'close') return value.status === 'closed' && value.snapshot === null
  if (value.operation === 'preview') {
    return value.status === 'preview' && value.snapshot === null && pagePreview(value.preview)
      && (value.binding.revision !== 0 || sameNativeBytes(value.binding.originHash, value.preview.outputHash))
  }
  if (value.operation === 'redaction') {
    if ((value.status !== 'redaction' && value.status !== 'published') || !redactionResult(value.redaction, value.status)
      || value.binding.revision === 0 && !sameNativeBytes(value.binding.originHash, value.redaction.acceptedHash)) return false
    if (value.status === 'redaction') return value.snapshot === null
  }
  if (!snapshot(value.snapshot) || !sameNativeSession(value.snapshot.binding, value.binding)
    || value.snapshot.originStoreHash !== value.originStoreHash) return false
  const base = value.binding.revision
  const current = value.snapshot.binding.revision
  const consumed = value.snapshot.consumedRequest
  switch (value.operation) {
    case 'open': return value.status === 'opened' && base === 0 && current === 0
    case 'command': return value.status === 'applied' && current === base + 1 && consumed === value.requestId
    case 'redaction': {
      const result = value.redaction as NativePdfRedactionResult
      const candidate = result.candidate
      return candidate !== null && sameNativeBytes(value.snapshot.outputHash, candidate.hash)
        && sameNativeBytes(value.snapshot.pageState.order.filter((id) => candidate.affectedPages.includes(id)), candidate.affectedPages)
        && current === base + 1 && consumed === value.requestId
    }
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
  if (response.binding === null || !sameNativeSession(request.binding, response.binding)
    || request.binding.revision !== response.binding.revision) return false
  if (request.operation === 'preview' && response.status !== 'failed') {
    return response.status === 'preview' && response.preview.pageId === request.pageId
      && sameNativeBytes(response.preview.outputHash, request.outputHash)
  }
  if (request.operation !== 'redaction' || response.status === 'failed') return true
  if (response.operation !== 'redaction' || response.redaction.action !== request.action.kind
    || !sameNativeBytes(request.acceptedHash, response.redaction.acceptedHash)) return false
  const actual = response.redaction.candidate
  const action = request.action
  if (!('candidate' in action)) return true
  const expected = action.candidate
  if (actual === null) return action.kind === 'status' && !response.redaction.published
  if (expected === null) return action.kind === 'status' && !response.redaction.published
  return actual.validatorVersion === expected.validatorVersion && actual.validatorProfile === expected.validatorProfile
    && sameNativeBytes(actual.hash, expected.hash) && sameNativeBytes(actual.affectedPages, expected.affectedPages)
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
