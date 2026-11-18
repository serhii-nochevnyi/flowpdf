import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react'
import {
  hashPdfSourceBytes, PdfSourceStoreError, type PdfSourceStore,
} from '../../persistence/pdf-source-store.js'
import {
  nativePdfText, type NativePdfLocale, type NativePdfMessage,
} from './pdf-native-edit-messages.js'
import {
  NATIVE_PDF_LIMITS, fitsNativeJson, isNativeNoteCommand, nativeBytesHex, nativeHexBytes, sameNativeBytes,
  type NativePdfAnnotation, type NativePdfNoteCommand, type NativePdfRect,
  type NativePdfCommand, type NativePdfSnapshot, type NativePdfTextCommand, type NativePdfTextIsland,
  type NativePdfTextRefusal, type NativePdfGraphic, type NativePdfObjectCommand, type NativePdfObjectPage,
  type NativePdfObjectTarget,
} from './pdf-native-edit-protocol.js'
import {
  confirmedNativeReplacement, NativePdfWorkerError, type NativePdfEditCoordinator,
} from './pdf-native-edit-worker.js'
import './pdf-native-edit-panel.css'

type Page = NativePdfSnapshot['scene']['pages'][number]
type Geometry = NonNullable<Page['geometry']>
type Note = Extract<NativePdfAnnotation, { kind: 'plainNote' }>
type Coordinator = Pick<NativePdfEditCoordinator,
  'open' | 'reconcile' | 'snapshot' | 'subscribe' | 'dispose'> & {
  command(command: NativePdfCommand): Promise<NativePdfSnapshot>
}
type Message = readonly [NativePdfMessage, Readonly<Record<string, string | number>>?]
type Size = readonly [number, number]

export interface NativePdfAcceptedDocument {
  readonly filename: string
  readonly snapshot: NativePdfSnapshot
}

export interface NativePdfVerifiedPageText {
  readonly outputHash: string
  readonly pageId: number
  readonly text: readonly { readonly value: string; readonly lang?: string }[]
  readonly lang?: string
}

export interface PdfNativeEditPanelProps {
  readonly createCoordinator: () => Coordinator
  readonly sourceStore: Pick<PdfSourceStore, 'put' | 'get'>
  readonly locale?: NativePdfLocale
  readonly initialDocument?: NativePdfAcceptedDocument
  readonly pageText?: readonly NativePdfVerifiedPageText[]
}

interface Draft {
  readonly note: number | null
  readonly contents: string
  readonly x: string
  readonly y: string
}

interface PendingNoteAction {
  readonly command: NativePdfNoteCommand
  readonly before: NativePdfSnapshot
  readonly pageId: number
  readonly draft: Draft | null
}

interface TextDraft {
  readonly pageId: number
  readonly selected: boolean
  readonly refusal: NativePdfMessage | null
  readonly island: readonly number[]
  readonly replacement: string
}

type PendingAction = PendingNoteAction | {
  readonly command: NativePdfTextCommand | NativePdfObjectCommand
  readonly before: NativePdfSnapshot
  readonly pageId: number
}

type ObjectView = NativePdfObjectPage['objects'][number]
type ObjectProfile = 'image' | NativePdfGraphic['shape']
type ObjectCoordinates = readonly [string, string, string, string]
type ObjectAsset = { readonly name: string } & (
  | { readonly status: 'loading' }
  | { readonly status: 'ready'; readonly hex: string }
  | { readonly status: 'failed'; readonly reason: 'object.file.limit' | 'object.file.failed' }
)
const objectProfiles = ['image', 'rectangle', 'line', 'polygon'] as const
const coordinateLabels = ['x', 'y', 'width', 'height'] as const

function objectUnits(value: string) {
  if (!/^(?:\d+(?:[.,]\d{1,3})?|[.,]\d{1,3})$/u.test(value)) return NaN
  return Math.round(Number(value.replace(',', '.')) * 1000)
}

export function nativeObjectBounds(values: ObjectCoordinates, page: NativePdfRect): NativePdfRect | null {
  const [x, y, width, height] = values
  const [left, bottom, right, top] = page
  const l = left * 1000 + objectUnits(x)
  const b = bottom * 1000 + objectUnits(y)
  const w = objectUnits(width)
  const h = objectUnits(height)
  const bounds = [l, b, l + w, b + h] as const
  return w > 0 && h > 0 && bounds[2] <= right * 1000 && bounds[3] <= top * 1000
    && bounds.every((point) => Number.isInteger(point) && Math.abs(point) <= 1_000_000_000)
    ? bounds : null
}

function objectCoordinates([l, b, r, t]: NativePdfRect, page: NativePdfRect): ObjectCoordinates {
  return [String((l - page[0] * 1000) / 1000), String((b - page[1] * 1000) / 1000), String((r - l) / 1000), String((t - b) / 1000)]
}

function objectKey(object: ObjectView) {
  return nativeBytesHex(Uint8Array.from(object.id))
}

function moveObjectTransform([l, b, r, t]: NativePdfRect, [x, y, right, top]: NativePdfRect) {
  const sx = (right - x) / (r - l)
  const sy = (top - y) / (t - b)
  const transform = [sx, 0, 0, sy, (x - sx * l) / 1000, (y - sy * b) / 1000] as const
  return sx >= 1e-6 && sy >= 1e-6 && sx * sy >= 1e-10
    && transform.every((entry) => Math.abs(entry) <= 1_000_000) ? transform : null
}

function graphicDraft(shape: NativePdfGraphic['shape'], bounds: NativePdfRect, color: string, width: number): NativePdfGraphic {
  const [l, b, r, t] = bounds
  const rgb = [1, 3, 5].map((start) => Number.parseInt(color.slice(start, start + 2), 16)) as [number, number, number]
  // Keep painted line bounds inside the requested rectangle, including half the stroke.
  const pad = Math.ceil(width / 2)
  const points: NativePdfGraphic['points'] = shape === 'polygon'
    ? [[l, b], [r, b], [Math.floor((l + r) / 2), t]]
    : [[l + pad, b + pad], [r - pad, t - pad]]
  return { shape, points, fill: shape === 'line' ? null : rgb, stroke: shape === 'line' ? rgb : null, strokeWidth: width }
}

function pageObjects(snapshot: NativePdfSnapshot, pageId: number) {
  return snapshot.scene.objectPages?.find((entry) => entry.page === pageId)?.objects ?? []
}

export function rebindNativeObject(
  before: NativePdfSnapshot, after: NativePdfSnapshot, pageId: number, selected: ObjectView, command: NativePdfCommand,
): ObjectView | undefined {
  const previous = pageObjects(before, pageId)
  const next = pageObjects(after, pageId)
  let index = previous.findIndex((entry) => sameNativeBytes(entry.id, selected.id))
  if (index < 0) return undefined
  let expectedCount = previous.length
  if ((command.kind === 'addImage' || command.kind === 'addGraphic') && command.page === pageId) {
    expectedCount++
    index++
  }
  if (command.kind === 'removeObject' && command.target.page === pageId) {
    const removed = previous.findIndex((entry) => sameNativeBytes(entry.id, command.target.id))
    if (removed < 0 || index === removed) return undefined
    expectedCount--
    if (index > removed) index--
  }
  const candidate = next[index]
  if (next.length !== expectedCount || !candidate || candidate.kind !== selected.kind) return undefined
  const changesSelection = 'target' in command && command.target.page === pageId
    && sameNativeBytes(command.target.id, selected.id)
  return changesSelection || (candidate.profile === selected.profile && sameNativeBytes(candidate.bounds, selected.bounds))
    ? candidate : undefined
}

function imageHeader(bytes: Uint8Array): Size | null {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  if (bytes.length >= 33 && view.getUint32(0) === 0x89504e47 && view.getUint32(4) === 0x0d0a1a0a
    && view.getUint32(8) === 13 && view.getUint32(12) === 0x49484452) {
    return bytes[24] === 8 && (bytes[25] === 0 || bytes[25] === 2)
      ? [view.getUint32(16), view.getUint32(20)] : null
  }
  if (bytes[0] !== 0xff || bytes[1] !== 0xd8) return null
  let offset = 2
  while (offset + 4 <= bytes.length && bytes[offset++] === 0xff) {
    while (bytes[offset] === 0xff) offset++
    const marker = bytes[offset++]
    if (offset + 2 > bytes.length || marker === 0xda || marker === 0xd9) break
    const length = view.getUint16(offset)
    if (length < 2 || offset + length > bytes.length) break
    if (marker === 0xc0 || marker === 0xc1 || marker === 0xc2) {
      if (length < 8 || bytes[offset + 2] !== 8 || ![1, 3].includes(bytes[offset + 7]!)) return null
      return [view.getUint16(offset + 5), view.getUint16(offset + 3)]
    }
    offset += length
  }
  return null
}

async function readObjectAsset(file: File): Promise<Exclude<ObjectAsset, { status: 'loading' }>> {
  const failed = (reason: 'object.file.limit' | 'object.file.failed') => ({ name: file.name, status: 'failed', reason } as const)
  const { imageBytes, imageDimension, imagePixels } = NATIVE_PDF_LIMITS
  if (file.size > imageBytes) return failed('object.file.limit')
  try {
    const bytes = new Uint8Array(await file.arrayBuffer())
    if (bytes.byteLength > imageBytes) return failed('object.file.limit')
    const dimensions = imageHeader(bytes)
    if (!dimensions || dimensions.some((size) => size === 0)) return failed('object.file.failed')
    const [width, height] = dimensions
    if (Math.max(width, height) > imageDimension || width * height > imagePixels) return failed('object.file.limit')
    const decoded = await createImageBitmap(new Blob([bytes]))
    decoded.close()
    return { name: file.name, status: 'ready', hex: nativeBytesHex(bytes) }
  } catch {
    return failed('object.file.failed')
  }
}

function islandKey(island: NativePdfTextIsland): string {
  return nativeBytesHex(Uint8Array.from(island.id))
}

function verifiedIslands(snapshot: NativePdfSnapshot | undefined, pageId: number) {
  const page = snapshot?.scene.textPages?.find((entry) => entry.page === pageId)
  return page?.refusal === null ? page.islands : []
}

function sameTextIsland(left: NativePdfTextIsland, right: NativePdfTextIsland) {
  return left.text === right.text && left.profile === right.profile && left.operator === right.operator
    && left.maxAdvance === right.maxAdvance && left.bounds.every((value, index) => value === right.bounds[index])
}

export function rebindNativeTextDrafts(
  drafts: readonly TextDraft[], before: NativePdfSnapshot, after: NativePdfSnapshot,
): TextDraft[] {
  return drafts.flatMap((draft) => {
    const previous = verifiedIslands(before, draft.pageId)
    const original = previous.find(({ id }) => sameNativeBytes(id, draft.island))
    if (!original || previous.filter((island) => sameTextIsland(original, island)).length !== 1) return []
    const matches = verifiedIslands(after, draft.pageId).filter((island) => sameTextIsland(original, island))
    return matches.length === 1 ? [{ ...draft, island: matches[0]!.id }] : []
  })
}

export function nativeReplacementError(text: string, composing: boolean) {
  if (composing) return 'composing'
  if (text.trim() === '') return 'required'
  if (/[\p{Cc}\p{Cs}\p{Zl}\p{Zp}]/u.test(text)) return 'invalid'
  return fitsNativeJson(text, NATIVE_PDF_LIMITS.noteBytes) ? null : 'limit'
}

function notesOf(page: Page | undefined): readonly Note[] {
  return page?.annotations.filter((item): item is Note => item.kind === 'plainNote') ?? []
}

export function nativeNoteTextError(contents: string, composing: boolean) {
  if (composing) return 'composing'
  if (contents.trim() === '') return 'required'
  if (/[\uD800-\uDFFF]/u.test(contents)) return 'invalid'
  return new TextEncoder().encode(contents).length > NATIVE_PDF_LIMITS.noteBytes ? 'limit' : null
}

export function nativeNoteRect(x: string, y: string, geometry: Geometry, size: Size): NativePdfRect | null {
  if (!/^\d+$/u.test(x) || !/^\d+$/u.test(y)) return null
  const [left, bottom, right, top] = geometry.bounds
  const l = left + Number(x)
  const b = bottom + Number(y)
  const r = l + size[0]
  const t = b + size[1]
  return [l, b, r, t].every(Number.isSafeInteger) && left <= l && l < r && r <= right
    && bottom <= b && b < t && t <= top ? [l, b, r, t] : null
}

function rotatedSize({ bounds: [left, bottom, right, top], rotation }: Geometry): Size {
  return rotation === 90 || rotation === 270 ? [top - bottom, right - left] : [right - left, top - bottom]
}

export function nativeMarkerPosition(geometry: Geometry, [l, b, r, t]: NativePdfRect): Size {
  const [left, bottom, right, top] = geometry.bounds
  const x = l - left + (r - l) / 2
  const y = b - bottom + (t - b) / 2
  const [width, height] = rotatedSize(geometry)
  switch (geometry.rotation) {
    case 90: return [100 * y / width, 100 * x / height]
    case 180: return [100 * (width - x) / width, 100 * y / height]
    case 270: return [100 * (width - y) / width, 100 * (height - x) / height]
    default: return [100 * x / width, 100 * (height - y) / height]
  }
}

export function nativeDownloadName(filename: string, edited: boolean): string {
  const basename = (filename.split(/[\\/]/u).at(-1) ?? '').replace(/\.pdf$/iu, '')
  const safe = Array.from(basename.replace(/[<>:"|?*\p{Cc}\p{Cf}\p{Cs}]/gu, '_')
    .trim().replace(/^[. ]+|[. ]+$/gu, '')).slice(0, 60).join('').trimEnd()
  const stem = /^(?:con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(?:\.|$)/iu.test(safe) ? `_${safe}` : safe
  return `${stem || 'document'}${edited ? '-edited' : ''}.pdf`
}

function download(bytes: Uint8Array<ArrayBuffer>, filename: string) {
  const url = URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' }))
  try {
    const link = document.createElement('a')
    link.href = url
    link.download = filename
    link.click()
  } finally {
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
}

function draftFrom(note: Note, geometry: Geometry): Draft {
  return { note: note.id, contents: note.contents,
    x: String(note.rect[0] - geometry.bounds[0]), y: String(note.rect[1] - geometry.bounds[1]) }
}

export function nativeNoteDraftAfter(
  kind: NativePdfNoteCommand['kind'], submitted: Draft | null, note: Note, geometry: Geometry,
): Draft {
  const accepted = draftFrom(note, geometry)
  if (submitted?.note !== note.id) return accepted
  if (kind === 'edit') return { ...accepted, x: submitted.x, y: submitted.y }
  if (kind === 'move') return { ...accepted, contents: submitted.contents }
  return accepted
}

export function nativeNoteDraftAfterAcceptance(
  { command, before, pageId, draft }: PendingNoteAction, result: NativePdfSnapshot,
  displayedPageId: number, current: Draft | null,
): Draft | null {
  if (displayedPageId !== pageId || current === null) return current
  if (command.kind === 'add' ? current !== draft : current.note !== command.note) return current

  const page = result.scene.pages.find((item) => item.id === pageId)
  if (!page?.geometry) return null
  const notes = notesOf(page)
  if (command.kind === 'remove') {
    const previous = notesOf(before.scene.pages.find((item) => item.id === pageId))
    const index = previous.findIndex((note) => note.id === command.note)
    const successor = notes[Math.min(index, notes.length - 1)]
    return successor ? draftFrom(successor, page.geometry) : null
  }
  if (command.kind === 'add') {
    const priorIds = new Set(notesOf(before.scene.pages.find((item) => item.id === pageId)).map((note) => note.id))
    const added = notes.find((note) => !priorIds.has(note.id))
    return added ? draftFrom(added, page.geometry) : null
  }
  const target = notes.find((note) => note.id === command.note)
  return target ? nativeNoteDraftAfter(command.kind, current, target, page.geometry) : null
}

export class NativeNoteDraftRecovery {
  private readonly pages = new Map<number, Map<Draft['note'], Draft>>()

  get(pageId: number, note?: Draft['note']): Draft | null {
    const drafts = this.pages.get(pageId)
    return (note === undefined ? drafts?.values().next().value : drafts?.get(note)) ?? null
  }

  update(pageId: number, draft: Draft) {
    const drafts = this.pages.get(pageId)
    if (drafts?.has(draft.note)) drafts.set(draft.note, draft)
  }

  discard(pageId: number, note: Draft['note']) {
    const drafts = this.pages.get(pageId)
    drafts?.delete(note)
    if (drafts?.size === 0) this.pages.delete(pageId)
  }

  restoreUnseen(
    { pageId, draft }: Pick<PendingNoteAction, 'pageId' | 'draft'>,
    displayedPageId: number, current: Draft | null,
  ): Draft | null {
    if (!draft) return current
    this.save(pageId, draft)
    return displayedPageId === pageId && (current === null || current.note === draft.note) ? draft : current
  }

  accept({ command, pageId, draft }: PendingNoteAction, result: NativePdfSnapshot) {
    const noteId = command.kind === 'add' ? null : command.note
    if (command.kind === 'add' || command.kind === 'remove') {
      this.discard(pageId, noteId)
      return
    }
    const pending = this.get(pageId, noteId) ?? draft
    if (!pending) return
    const page = result.scene.pages.find((item) => item.id === pageId)
    const note = notesOf(page).find((item) => item.id === noteId)
    if (note && page?.geometry) {
      const next = nativeNoteDraftAfter(command.kind, pending, note, page.geometry)
      const verified = draftFrom(note, page.geometry)
      if (next.contents !== verified.contents || next.x !== verified.x || next.y !== verified.y) {
        this.save(pageId, next)
        return
      }
    }
    this.discard(pageId, noteId)
  }

  prune(result: NativePdfSnapshot) {
    for (const [pageId, drafts] of this.pages) {
      const page = result.scene.pages.find((item) => item.id === pageId)
      const ids = new Set(notesOf(page).map((note) => note.id))
      for (const note of drafts.keys()) {
        if (!page || (note !== null && !ids.has(note))) this.discard(pageId, note)
      }
    }
  }

  private save(pageId: number, draft: Draft) {
    const drafts = this.pages.get(pageId) ?? new Map<Draft['note'], Draft>()
    drafts.set(draft.note, draft)
    this.pages.set(pageId, drafts)
  }
}

const textRefusalReasons = {
  FLOW_PDF_NATIVE_TEXT_FONT: 'reason.font',
  FLOW_PDF_NATIVE_TEXT_MAPPING: 'reason.mapping',
  FLOW_PDF_NATIVE_TEXT_LAYOUT: 'reason.layout',
  FLOW_PDF_NATIVE_TEXT_OWNERSHIP: 'reason.references',
  FLOW_PDF_NATIVE_TEXT_OVERFLOW: 'reason.overflow',
} as const satisfies Record<NativePdfTextRefusal, NativePdfMessage>

const objectRefusalReasons = {
  FLOW_PDF_NATIVE_OBJECT_STATE: 'object.reason.state',
  FLOW_PDF_NATIVE_OBJECT_GEOMETRY: 'object.reason.geometry',
  FLOW_PDF_NATIVE_OBJECT_ASSET: 'object.reason.asset',
  FLOW_PDF_NATIVE_OBJECT_PROFILE: 'object.reason.profile',
} as const satisfies Record<NonNullable<NativePdfObjectPage['refusal']>, NativePdfMessage>

const refusalReasons = new Map<string, NativePdfMessage>([
  ...Object.entries(textRefusalReasons),
  ...Object.entries(objectRefusalReasons),
  ['FLOW_PDF_NATIVE_IMPORT_REFUSED', 'error.importRefused'],
  ['FLOW_PDF_NATIVE_BINDING_MISMATCH', 'error.stale'],
  ['FLOW_PDF_NATIVE_REVISION_MISMATCH', 'error.stale'],
  ['FLOW_PDF_NATIVE_TARGET_UNAVAILABLE', 'error.stale'],
  ['FLOW_PDF_NATIVE_REQUEST_CONFLICT', 'error.stale'],
  ['FLOW_PDF_NATIVE_NOTE_INVALID', 'error.retry'],
  ['FLOW_PDF_NATIVE_GRAPH_MALFORMED', 'reason.structure'],
  ['FLOW_PDF_NATIVE_UNSUPPORTED', 'reason.structure'],
  ['FLOW_PDF_NATIVE_BUDGET_EXHAUSTED', 'error.budget'],
  ['FLOW_PDF_NATIVE_LIMITS_INVALID', 'error.budget'],
  ['FLOW_PDF_NATIVE_REQUEST_INVALID', 'error.retry'],
  ['FLOW_PDF_NATIVE_REQUEST_DECODE', 'error.retry'],
  ['FLOW_PDF_NATIVE_RESPONSE_INVALID', 'error.reconcile'],
  ['FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED', 'error.reconcile'],
  ['FLOW_PDF_NATIVE_REPLY_LOST', 'error.reconcile'],
  ['FLOW_PDF_NATIVE_POSTCONDITION_FAILED', 'error.retry'],
  ['FLOW_PDF_NATIVE_WORKER_STARTUP', 'error.retry'],
  ['FLOW_PDF_NATIVE_WORKER_FAILED', 'error.session'],
  ['FLOW_PDF_SOURCE_IDENTITY_CONFLICT', 'error.retry'],
  ['FLOW_PDF_SOURCE_HASH_FAILED', 'error.retry'],
  ['FLOW_PDF_SOURCE_STORE_OPEN_FAILED', 'error.retry'],
  ['FLOW_PDF_SOURCE_STORE_WRITE_FAILED', 'error.retry'],
  ['FLOW_PDF_SOURCE_STORE_READ_FAILED', 'error.retry'],
  ['FLOW_PDF_SOURCE_STORE_BLOCKED', 'error.retry'],
  ['FLOW_PDF_SOURCE_STORE_ABORTED', 'error.retry'],
])

function refusal(cause: unknown): readonly [string | undefined, NativePdfMessage] {
  if (cause instanceof NativePdfWorkerError || cause instanceof PdfSourceStoreError) {
    const message = refusalReasons.get(cause.code)
    if (message !== undefined) return [cause.code, message]
  }
  return [undefined, 'error.retry']
}

const unsubscribeIdle = () => undefined
const idle = () => null
const familyKeys = ['pages', 'fields', 'redaction'] as const

export function PdfNativeEditPanel({
  createCoordinator, sourceStore, locale = 'uk', initialDocument, pageText,
}: PdfNativeEditPanelProps) {
  const id = useId()
  const t = (key: NativePdfMessage, parameters?: Message[1]) => nativePdfText(locale, key, parameters)
  const pointFormat = new Intl.NumberFormat(locale, { maximumFractionDigits: 3 })
  const formatPoint = (thousandths: number) => pointFormat.format(thousandths / 1000)
  const [accepted, setAccepted] = useState(initialDocument)
  const [coordinator, setCoordinator] = useState<Coordinator | null>(null)
  const coordinatorRef = useRef<Coordinator | null>(null)
  const candidateRef = useRef<Coordinator | null>(null)
  const mounted = useRef(false)
  const locked = useRef(false)
  const [busy, setBusy] = useState<'opening' | 'mutating' | 'reconciling' | null>(null)
  const [status, setStatus] = useState<Message | null>(null)
  const [error, setError] = useState<Message | null>(null)
  const [operationError, setOperationError] = useState<{ code: string | undefined; reason: NativePdfMessage; file: boolean } | null>(null)
  const [pageId, setPageId] = useState(0)
  const [zoom, setZoom] = useState(0)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [textDrafts, setTextDrafts] = useState<readonly TextDraft[]>([])
  const [objectId, setObjectId] = useState<readonly number[] | null>(null)
  const [objectProfile, setObjectProfile] = useState<ObjectProfile>('image')
  const [coordinates, setCoordinates] = useState<ObjectCoordinates>(['', '', '', ''])
  const [color, setColor] = useState('#334155')
  const [stroke, setStroke] = useState('1')
  const [asset, setAsset] = useState<ObjectAsset | null>(null)
  const assetGeneration = useRef(0)
  const [composing, setComposing] = useState(false)
  const composition = useRef(false)
  const [confirm, setConfirm] = useState<({ readonly snapshot: NativePdfSnapshot } &
    ({ readonly note: number } | { readonly target: NativePdfObjectTarget; readonly label: string })) | null>(null)
  const uncertain = useRef<PendingAction | null>(null)
  const recoveredDrafts = useRef(new NativeNoteDraftRecovery())
  const focusAfterRender = useRef<string | HTMLElement | null>(null)
  const dialog = useRef<HTMLDialogElement>(null)
  const cancelConfirmation = useRef<HTMLButtonElement>(null)
  const removeTrigger = useRef<HTMLButtonElement | null>(null)

  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
      candidateRef.current?.dispose()
      coordinatorRef.current?.dispose()
      candidateRef.current = null
      coordinatorRef.current = null
    }
  }, [])

  const service = useSyncExternalStore(
    useCallback((listener) => coordinator?.subscribe(listener) ?? unsubscribeIdle, [coordinator]),
    () => coordinator?.snapshot() ?? null,
    idle,
  )
  const snapshot = accepted?.snapshot
  const pages = snapshot?.scene.pages ?? []
  const page = pages.find((entry) => entry.id === pageId)
  const pageIndex = page === undefined ? -1 : pages.indexOf(page)
  const geometry = page?.geometry ?? null
  const notes = notesOf(page)
  const selected = notes.find((note) => note.id === draft?.note)
  const canEdit = busy === null && service?.phase === 'ready' && snapshot === service.accepted
  const objectPage = snapshot?.scene.objectPages?.find((entry) => entry.page === pageId)
  const selectedObject = objectPage?.objects.find((entry) => objectId && sameNativeBytes(entry.id, objectId))
  const objectTarget = selectedObject && { page: pageId, id: selectedObject.id, profile: selectedObject.profile }
  const objectBounds = geometry && nativeObjectBounds(coordinates, geometry.bounds)
  const objectMove = selectedObject && objectBounds && moveObjectTransform(selectedObject.bounds, objectBounds)
  const strokeWidth = objectProfile === 'line' ? objectUnits(stroke) : 0
  const strokeValid = strokeWidth >= 1 && strokeWidth <= NATIVE_PDF_LIMITS.graphicStrokeWidth
    && (!objectBounds || 2 * Math.ceil(strokeWidth / 2) < Math.min(
      objectBounds[2] - objectBounds[0], objectBounds[3] - objectBounds[1],
    ))
  const canPaint = !!objectBounds && (objectProfile === 'image' ? asset?.status === 'ready'
    : objectProfile !== 'line' || strokeValid)
  const objectPageReason = objectPage?.refusal ? objectRefusalReasons[objectPage.refusal]
    : objectPage?.canAdd === false ? 'object.addUnavailable' : null
  const objectForm = !!geometry && (objectPage?.canAdd === true || !!selectedObject)
  const textPage = snapshot?.scene.textPages?.find((entry) => entry.page === page?.id)
  const islands = verifiedIslands(snapshot, pageId)
  const textDraft = textDrafts.find((entry) => entry.pageId === pageId && entry.selected)
  const textRefusal = textDraft?.refusal
  const selectedIsland = islands.find((island) => textDraft && sameNativeBytes(textDraft.island, island.id))
  const replacementError = textDraft ? nativeReplacementError(textDraft.replacement, composing) : null
  const canReplace = canEdit && !!selectedIsland && replacementError === null
  const canAdd = canEdit && page?.canAddNote === true && geometry !== null
  const invalidated = service?.phase === 'invalidated'
  const needsReconcile = service?.phase === 'reconciliationRequired'
  const revision = snapshot?.binding.revision ?? 0
  const outputHash = snapshot ? nativeBytesHex(Uint8Array.from(snapshot.outputHash)) : ''
  const extracted = pageText?.find((entry) => entry.outputHash === outputHash && entry.pageId === page?.id)
  const geometrySize: Size = geometry === null ? [0, 0] :
    selected === undefined
      ? [Math.min(24, geometry.bounds[2] - geometry.bounds[0]), Math.min(24, geometry.bounds[3] - geometry.bounds[1])]
      : [selected.rect[2] - selected.rect[0], selected.rect[3] - selected.rect[1]]
  const rect = draft && geometry ? nativeNoteRect(draft.x, draft.y, geometry, geometrySize) : null
  const noteError = draft ? nativeNoteTextError(draft.contents, composing) : null
  const textBytes = draft ? new TextEncoder().encode(draft.contents).length : 0
  const noteLimit = t('limit.bytes', { count: NATIVE_PDF_LIMITS.noteBytes })
  const pdfLimit = t('limit.bytes', { count: NATIVE_PDF_LIMITS.pdfBytes })
  const position = page === undefined ? '' : t('navigation.position', { page: pageIndex + 1, total: pages.length })

  useLayoutEffect(() => {
    const element = dialog.current
    if (confirm !== null && element && !element.open) {
      element.showModal()
      cancelConfirmation.current?.focus()
    }
    if (confirm === null && element?.open) {
      element.close()
      removeTrigger.current?.focus()
    }
  }, [confirm])

  useLayoutEffect(() => {
    const target = focusAfterRender.current
    if (target === null) return
    focusAfterRender.current = null
    const element = typeof target === 'string' ? document.getElementById(target) : target
    if (element?.isConnected && !element.matches(':disabled')) element.focus()
    else if (target === `${id}-object-selector`) document.getElementById(`${id}-objects`)?.focus()
    else document.getElementById(`${id}-notes`)?.focus()
  })

  function setComposition(value: boolean) {
    composition.current = value
    setComposing(value)
  }

  function clearFeedback() {
    setError(null)
    setOperationError(null)
  }

  function setFailure(cause: unknown, file: boolean) {
    // Keep only bounded, known codes across renders, never an arbitrary Error.
    const [code, reason] = refusal(cause)
    setOperationError({ code, reason, file })
    setStatus(null)
  }

  async function open(filename: string, read: () => Promise<Uint8Array>, originalHash?: string) {
    if (locked.current) return
    locked.current = true
    clearFeedback()
    setStatus(null)
    setBusy('opening')
    let candidate: Coordinator | null = null
    try {
      const bytes = await read()
      if (!mounted.current) return
      if (bytes.byteLength > NATIVE_PDF_LIMITS.pdfBytes) {
        setError(['error.limit', { limit: pdfLimit }])
        return
      }
      const hash = (await hashPdfSourceBytes(bytes)).slice('sha256:v1:'.length)
      if (originalHash !== undefined && hash !== originalHash) {
        throw new PdfSourceStoreError('FLOW_PDF_SOURCE_IDENTITY_CONFLICT')
      }
      await sourceStore.put(hash, bytes)
      if (!mounted.current) return
      candidate = createCoordinator()
      candidateRef.current = candidate
      const result = await candidate.open({ bytes, originStoreHash: hash })
      if (!mounted.current) return
      const previous = coordinatorRef.current
      coordinatorRef.current = candidate
      candidateRef.current = null
      setCoordinator(candidate)
      previous?.dispose()
      uncertain.current = null
      recoveredDrafts.current = new NativeNoteDraftRecovery()
      setAccepted({ filename, snapshot: result })
      setPageId(result.scene.pages[0]?.id ?? 0)
      setZoom(0)
      setDraft(null)
      setTextDrafts([])
      resetObject()
      setComposition(false)
      setStatus([originalHash === undefined ? 'status.ready' : 'status.reopened'])
    } catch (cause) {
      candidate?.dispose()
      if (mounted.current) setFailure(cause, true)
    } finally {
      if (candidateRef.current === candidate) candidateRef.current = null
      locked.current = false
      if (mounted.current) setBusy(null)
    }
  }

  function choosePage(destination: Page | undefined) {
    if (locked.current || destination === undefined || destination.id === pageId) return
    const recovered = recoveredDrafts.current.get(destination.id)
    setPageId(destination.id)
    resetObject()
    setDraft(recovered)
    setComposition(false)
    setStatus([recovered ? 'status.draftRestored' : 'navigation.position', {
      page: pages.indexOf(destination) + 1,
      total: pages.length,
    }])
  }

  function resetObject(target?: ObjectView) {
    assetGeneration.current++
    setAsset(null)
    setObjectId(target?.id ?? null)
    setObjectProfile(target?.profile === 'line' || target?.profile === 'polygon' || target?.profile === 'rectangle'
      ? target.profile : 'image')
    setCoordinates(target && geometry ? objectCoordinates(target.bounds, geometry.bounds) : ['', '', '', ''])
    setColor('#334155')
    setStroke('1')
  }

  async function chooseImage(file: File) {
    const generation = ++assetGeneration.current
    clearFeedback()
    setAsset({ name: file.name, status: 'loading' })
    const result = await readObjectAsset(file)
    if (mounted.current && generation === assetGeneration.current) setAsset(result)
  }

  function submitObject(operation: 'add' | 'replace' | 'move', invoker: HTMLElement) {
    if (!canEdit || !objectPage || !objectBounds || asset?.status === 'loading') return
    if (operation === 'move') {
      if (!objectTarget || !objectMove || !selectedObject?.operations.includes('move')) return
      void act({ kind: 'moveObject', target: objectTarget, transform: objectMove }, invoker)
      return
    }
    if (operation === 'add' ? !objectPage.canAdd || !!objectTarget : !selectedObject?.operations.includes('replace')) return
    if (objectProfile === 'image') {
      if (asset?.status !== 'ready') return
      void act(objectTarget
        ? { kind: 'replaceImage', target: objectTarget, assetHex: asset.hex }
        : { kind: 'addImage', page: pageId, bounds: objectBounds, assetHex: asset.hex }, invoker)
    } else if (canPaint) {
      const graphic = graphicDraft(objectProfile, objectBounds, color, strokeWidth)
      void act(objectTarget ? { kind: 'replaceGraphic', target: objectTarget, graphic }
        : { kind: 'addGraphic', page: pageId, graphic }, invoker)
    }
  }

  function chooseNote(note: Note) {
    if (geometry === null || locked.current) return
    setComposition(false)
    setDraft(recoveredDrafts.current.get(pageId, note.id) ?? draftFrom(note, geometry))
    focusAfterRender.current = `${id}-contents`
  }

  function startNote() {
    if (!canAdd || locked.current) return
    setComposition(false)
    setDraft(recoveredDrafts.current.get(pageId, null) ?? { note: null, contents: '', x: '0', y: '0' })
    focusAfterRender.current = `${id}-contents`
  }

  function cancelDraft() {
    if (locked.current || composition.current) return
    if (draft) {
      recoveredDrafts.current.discard(pageId, draft.note)
      const pending = uncertain.current
      if (pending && 'draft' in pending && pending.pageId === pageId && pending.draft?.note === draft.note) {
        uncertain.current = { ...pending, draft: null }
      }
    }
    setComposition(false)
    clearFeedback()
    if (selected && geometry) {
      setDraft(draftFrom(selected, geometry))
      focusAfterRender.current = `${id}-note-${selected.id}`
    } else {
      setDraft(null)
      focusAfterRender.current = `${id}-add`
    }
  }

  function updateDraft(next: Draft) {
    if (locked.current) return
    recoveredDrafts.current.update(pageId, next)
    setDraft(next)
  }

  function updateTextDraft(next: TextDraft) {
    setTextDrafts((current) => [
      ...current.filter((entry) => entry.pageId !== next.pageId || !sameNativeBytes(entry.island, next.island))
        .map((entry) => next.selected && entry.pageId === next.pageId ? { ...entry, selected: false } : entry),
      next,
    ])
  }

  function publish(pending: PendingAction, result: NativePdfSnapshot): string | null {
    setAccepted((current) => current && { ...current, snapshot: result })
    const kind = pending.command.kind
    setStatus([kind === 'remove' ? 'note.removed' : 'status.accepted', { revision: result.binding.revision }])
    setTextDrafts((current) => rebindNativeTextDrafts(current.filter((entry) => {
      const command = pending.command
      return command.kind !== 'replaceText' || entry.pageId !== pending.pageId
        || !sameNativeBytes(entry.island, command.island)
    }), pending.before, result))
    if (selectedObject) {
      const rebound = rebindNativeObject(pending.before, result, pageId, selectedObject, pending.command)
      if (!rebound) resetObject()
      else if ('target' in pending.command) resetObject(rebound)
      else setObjectId(rebound.id)
    }
    if (!('draft' in pending)) {
      const { command, before } = pending
      if (command.kind !== 'replaceText') {
        if (command.kind === 'addImage' || command.kind === 'addGraphic') {
          const next = pageObjects(result, pageId)
          if (pageId === command.page && next.length === pageObjects(before, pageId).length + 1) resetObject(next[0])
        }
        return `${id}-object-selector`
      }
      const replacement = confirmedNativeReplacement(command, before, result)
      if (replacement && pageId === pending.pageId) return `${id}-island-button-${islandKey(replacement)}`
      return `${id}-text-islands`
    }

    const next = nativeNoteDraftAfterAcceptance(pending, result, pageId, draft)
    recoveredDrafts.current.accept(pending, result)
    if (next === draft) return null
    setDraft(next && (recoveredDrafts.current.get(pageId, next.note) ?? next))
    if (kind === 'remove' && next?.note != null) return `${id}-note-${next.note}`
    return kind === 'add' || kind === 'remove' ? `${id}-add` : null
  }

  async function act(command: NativePdfCommand, invoker: HTMLElement | null) {
    if (locked.current || composition.current || !coordinator || !canEdit || !snapshot) return
    if (command.kind === 'replaceText') {
      if (!selectedIsland || !sameNativeBytes(selectedIsland.id, command.island)
        || nativeReplacementError(command.text, false) !== null) return
    } else if (!isNativeNoteCommand(command)) {
      if ('target' in command) {
        if (!objectTarget || command.target.page !== objectTarget.page || command.target.profile !== objectTarget.profile
          || !sameNativeBytes(command.target.id, objectTarget.id)) return
      } else if (!objectPage?.canAdd || command.page !== pageId) return
    }
    const live = coordinator.snapshot()
    if (live.phase !== 'ready' || live.accepted !== snapshot) {
      setError(['error.stale'])
      return
    }
    locked.current = true
    const pending: PendingAction = isNativeNoteCommand(command)
      ? { command, before: snapshot, pageId, draft }
      : { command, before: snapshot, pageId }
    clearFeedback()
    setBusy('mutating')
    try {
      const result = await coordinator.command(command)
      if (!mounted.current) return
      focusAfterRender.current = publish(pending, result)
        ?? (document.activeElement === document.body ? invoker : null)
    } catch (cause) {
      if (!mounted.current) return
      if (coordinator.snapshot().phase === 'reconciliationRequired') {
        uncertain.current = pending
      }
      if (document.activeElement === document.body) focusAfterRender.current = invoker
      setFailure(cause, false)
      if (command.kind === 'replaceText' && coordinator.snapshot().phase === 'ready') {
        updateTextDraft({ pageId, island: command.island, replacement: command.text, selected: true, refusal: refusal(cause)[1] })
        focusAfterRender.current = `${id}-replacement`
      }
    } finally {
      locked.current = false
      if (mounted.current) setBusy(null)
    }
  }

  async function reconcile(invoker: HTMLElement) {
    const pending = uncertain.current
    if (!coordinator || !pending || locked.current) return
    locked.current = true
    clearFeedback()
    setBusy('reconciling')
    try {
      const result = await coordinator.reconcile()
      if (!mounted.current) return
      uncertain.current = null
      let focusTarget = `${id}-${'draft' in pending ? 'notes' : pending.command.kind === 'replaceText' ? 'text-islands' : 'object-selector'}`
      if (result.status === 'unseen') {
        setAccepted((current) => current && { ...current, snapshot: result.snapshot })
        if (!('draft' in pending) && pending.command.kind !== 'replaceText') {
          setStatus(['status.unseen'])
        } else if (!('draft' in pending) && pending.command.kind === 'replaceText') {
          const command = pending.command
          const target = verifiedIslands(result.snapshot, pending.pageId)
            .find(({ id }) => sameNativeBytes(id, command.island))
          if (target) {
            const isSelected = pageId === pending.pageId && selectedIsland !== undefined
              && sameNativeBytes(selectedIsland.id, target.id)
            const saved = textDrafts.find((entry) => entry.pageId === pending.pageId
              && sameNativeBytes(entry.island, target.id))
            updateTextDraft({
              pageId: pending.pageId, island: target.id, replacement: command.text,
              selected: saved?.selected ?? false, refusal: null,
            })
            if (isSelected) {
              setStatus(['status.unseen'])
              focusTarget = `${id}-replacement`
            } else {
              setStatus(['status.unseenIsland', {
                page: result.snapshot.scene.pages.findIndex((item) => item.id === pending.pageId) + 1,
              }])
            }
          } else {
            setStatus(['status.unseenNoDraft'])
          }
        } else if ('draft' in pending) {
          const recovered = recoveredDrafts.current.restoreUnseen(pending, pageId, draft)
          if (recovered !== draft) {
            setDraft(recovered)
            setComposition(false)
          }
          if (pending.draft === null) {
            setStatus(['status.unseenNoDraft'])
          } else if (pageId === pending.pageId) {
            setStatus(['status.unseen'])
          } else {
            setStatus(['status.unseenPage', {
              page: result.snapshot.scene.pages.findIndex((item) => item.id === pending.pageId) + 1,
            }])
          }
        }
      } else if (result.status === 'accepted' && result.snapshot.binding.revision === pending.before.binding.revision + 1) {
        focusTarget = publish(pending, result.snapshot) ?? focusTarget
      } else {
        setAccepted((current) => current && { ...current, snapshot: result.snapshot })
        recoveredDrafts.current.prune(result.snapshot)
        setDraft(null)
        setTextDrafts([])
        resetObject()
        setError(['error.stale'])
      }
      if (document.activeElement === invoker) focusAfterRender.current = focusTarget
    } catch (cause) {
      if (!mounted.current) return
      setFailure(cause, false)
      if (coordinator.snapshot().phase !== 'reconciliationRequired' && document.activeElement === invoker) {
        focusAfterRender.current = `${id}-notes`
      }
    } finally {
      locked.current = false
      if (mounted.current) setBusy(null)
    }
  }

  function submitDraft(kind: 'add' | 'edit' | 'move', invoker: HTMLElement) {
    if (draft === null || page === undefined || !canEdit || composition.current) return
    if (kind !== 'move' && nativeNoteTextError(draft.contents, false) !== null) {
      document.getElementById(`${id}-contents`)?.focus()
      return
    }
    if (kind !== 'edit' && rect === null) {
      const xValid = geometry && nativeNoteRect(draft.x, '0', geometry, geometrySize) !== null
      document.getElementById(`${id}-${xValid ? 'y' : 'x'}`)?.focus()
      return
    }
    if (kind === 'add' && canAdd && rect) void act({ kind, page: page.id, rect, contents: draft.contents }, invoker)
    if (kind === 'edit' && selected) void act({ kind, note: selected.id, contents: draft.contents }, invoker)
    if (kind === 'move' && selected && page.canAddNote && rect) void act({ kind, note: selected.id, page: page.id, rect }, invoker)
  }

  async function downloadOriginal() {
    if (!accepted) return
    clearFeedback()
    try {
      const bytes = await sourceStore.get(accepted.snapshot.originStoreHash)
      if (bytes === null || (await hashPdfSourceBytes(bytes)) !== `sha256:v1:${accepted.snapshot.originStoreHash}`) {
        throw new PdfSourceStoreError('FLOW_PDF_SOURCE_IDENTITY_CONFLICT')
      }
      if (!mounted.current) return
      download(new Uint8Array(bytes), nativeDownloadName(accepted.filename, false))
    } catch {
      if (mounted.current) setError(['error.download'])
    }
  }

  let failure = error === null ? '' : t(...error)
  if (operationError) {
    const { code, reason } = operationError
    failure = operationError.file
      ? [t('error.file', { code: code ?? t('error.unknown') }),
          reason === 'reason.structure' ? `${t('admission.refused', { reason: t(reason) })} ${t('error.importRefused')}`
            : reason === 'error.importRefused' ? t(reason) : ''].filter(Boolean).join(' ')
      : needsReconcile ? t('error.reconcile')
        : t('error.command', { nextAction: t(reason), code: code ?? t('error.unknown') })
  }
  if (invalidated && !failure) failure = t('error.session')
  const announcement = busy === 'opening' ? t('status.opening')
    : busy === 'mutating' ? t('status.pending')
      : busy === 'reconciling' ? t('status.reconciling')
        : status === null ? '' : t(...status)
  const bounds = geometry && t('geometry.bounds', {
    x: geometry.bounds[2] - geometry.bounds[0] - geometrySize[0],
    y: geometry.bounds[3] - geometry.bounds[1] - geometrySize[1],
  })
  const unavailable = invalidated ? t('error.session') : needsReconcile ? t('error.reconcile')
    : busy !== null ? t('capability.busy') : null

  return (
    <section className="pdf-native-edit-panel" data-pdf-native-edit="" aria-labelledby={`${id}-title`}
      onKeyDown={(event) => event.stopPropagation()} lang={locale}>
      <header className="native-stack">
        <h2 id={`${id}-title`}>{t('title')}</h2>
        <p>{t('description')}</p>
        <div className="native-actions">
          <label className={accepted ? 'native-file' : 'native-file native-primary'}>
            {t('open')}
            <input type="file" accept="application/pdf,.pdf" disabled={busy !== null || needsReconcile}
              aria-describedby={`${id}-file-limit`} onChange={(event) => {
                const file = event.currentTarget.files?.[0]
                event.currentTarget.value = ''
                if (file && !locked.current) {
                  if (file.size > NATIVE_PDF_LIMITS.pdfBytes) {
                    clearFeedback()
                    setError(['error.limit', { limit: pdfLimit }])
                  } else void open(file.name, async () => new Uint8Array(await file.arrayBuffer()))
                }
              }} />
          </label>
          <button type="button" disabled={!accepted} onClick={() => void downloadOriginal()}
            data-origin-hash={snapshot?.originStoreHash}>{t('download.original')}</button>
          <button type="button" disabled={revision === 0 || busy !== null}
            aria-describedby={`${id}-edited`} data-output-hash={snapshot ? outputHash : undefined}
            onClick={() => {
              if (!snapshot || !accepted || revision === 0 || locked.current) return
              clearFeedback()
              try {
                download(nativeHexBytes(snapshot.outputHex), nativeDownloadName(accepted.filename, true))
              } catch {
                setError(['error.download'])
              }
            }}>{t('download.edited')}</button>
          {invalidated && accepted && <button type="button" disabled={busy !== null} onClick={() => {
            const origin = accepted.snapshot.originStoreHash
            void open(accepted.filename, async () => {
              const original = await sourceStore.get(origin)
              if (original === null) throw new Error()
              return original
            }, origin)
          }}>{t('session.reopen')}</button>}
          {(needsReconcile || service?.phase === 'reconciling' || busy === 'reconciling')
            && <button type="button" aria-disabled={busy !== null} onClick={(event) => void reconcile(event.currentTarget)}>
              {t('session.reconcile')}</button>}
        </div>
        <p className="native-help" id={`${id}-file-limit`}>{t('limit.file', { limit: pdfLimit })}</p>
        {accepted && <p data-native-filename="" dir="auto"><bdi>{accepted.filename}</bdi></p>}
        <p className="native-help" id={`${id}-edited`}>{revision === 0 ? t('download.unavailable')
          : invalidated || needsReconcile ? t('output.last', { revision })
            : operationError && !operationError.file ? t('output.previous', { revision })
              : t('output.revision', { revision })}</p>
      </header>
      <p role="status" aria-live="polite" aria-atomic="true">{announcement}</p>
      <p role="alert" aria-atomic="true">{failure}</p>
      {busy === 'opening' && accepted && <p>{t('status.previousView')}</p>}
      {accepted && <p>{t('admission.ready')}</p>}
      <p>{t('capability.later', { families: familyKeys.map((key) => t(`family.${key}`)).join(', ') })}</p>
      {accepted === undefined && <div className="native-stack">
        <h3>{t('empty.heading')}</h3><p>{t('empty.body')}</p>
      </div>}
      <div className="native-workspace">
        <div className="native-stack">
          <div className="native-actions" role="group" aria-label={t('navigation.label')}>
            <button type="button" disabled={pageIndex <= 0 || busy !== null}
              onClick={() => choosePage(pages[pageIndex - 1])}>{t('navigation.previous')}</button>
            <button type="button" disabled={pageIndex < 0 || pageIndex >= pages.length - 1 || busy !== null}
              onClick={() => choosePage(pages[pageIndex + 1])}>{t('navigation.next')}</button>
            <label>{t('navigation.select')}
              <select disabled={pages.length === 0 || busy !== null} value={page?.id ?? ''}
                onChange={(event) => choosePage(pages.find((item) => item.id === Number(event.target.value)))}>
                {pages.map((item, index) => <option key={item.id} value={item.id}>{index + 1}</option>)}
              </select>
            </label>
            <label>{t('navigation.zoom')}
              <select disabled={geometry === null} value={zoom} onChange={(event) => setZoom(Number(event.target.value))}>
                <option value={0}>{t('navigation.fit')}</option>
                {[50, 100, 150, 200].map((percent) => <option key={percent} value={percent}>{percent}%</option>)}
              </select>
            </label>
            <button type="button" disabled={geometry === null} onClick={() => setZoom(0)}>{t('navigation.fit')}</button>
          </div>
          {page && <div className="native-stack">
            <p>{position}</p>
            <div className="native-viewport" role="region" aria-label={t('viewport.label')} tabIndex={0}>
              {geometry && <NativePagePaper geometry={geometry} zoom={zoom} notes={notes}
                selected={selected?.id} onSelect={chooseNote} />}
            </div>
          </div>}
        </div>
        {page && <section className="native-stack" aria-label={t('projection.label')}>
          <div className="native-stack" lang={extracted?.lang ?? ''}>
            {extracted?.text.map((fragment, index) =>
              <p key={index} lang={fragment.lang} dir="auto">{fragment.value}</p>)}
          </div>
          <p>{extracted ? t(extracted.text.length ? 'projection.partial' : 'projection.empty') : t('projection.unavailable')}</p>
          {objectPage && <section className="native-stack native-object-tools" aria-labelledby={`${id}-objects`}
            aria-describedby={objectPageReason ? `${id}-object-page-reason` : undefined}>
            <h3 id={`${id}-objects`} tabIndex={-1}>{t('object.title')}</h3>
            {objectPageReason && <p id={`${id}-object-page-reason`} className="native-help">{t(objectPageReason)}</p>}
            {(objectPage.canAdd || objectPage.objects.length > 0) && <label>{t('object.title')}
              <select id={`${id}-object-selector`} value={selectedObject ? objectKey(selectedObject) : ''}
                disabled={busy !== null}
                onChange={(event) => {
                  if (locked.current) return
                  resetObject(objectPage.objects.find((entry) => objectKey(entry) === event.target.value))
                  clearFeedback()
                }}>
                <option value="" disabled={!objectPage.canAdd}>{t(objectPage.canAdd ? 'object.new' : 'object.unavailable')}</option>
                {objectPage.objects.map((entry, index) => <option key={objectKey(entry)} value={objectKey(entry)}>
                  {t('object.target', { number: index + 1, page: pageIndex + 1, profile: t(`object.profile.${entry.profile}`) })}
                </option>)}
              </select>
            </label>}
            {objectForm && <fieldset className="native-stack" disabled={!canEdit}>
              <legend>{t('object.geometry')}</legend>
              <label>{t('object.profile.label')}
                <select value={objectProfile} disabled={selectedObject?.kind === 'image'} onChange={(event) => {
                  const next = objectProfiles.find((profile) => profile === event.target.value)
                  if (next) {
                    setObjectProfile(next)
                    assetGeneration.current++
                    setAsset(null)
                    clearFeedback()
                  }
                }}>
                  {objectProfiles.filter((profile) => selectedObject === undefined
                    || (profile === 'image') === (selectedObject.kind === 'image')).map((profile) =>
                    <option key={profile} value={profile}>{t(`object.profile.${profile}`)}</option>)}
                </select>
              </label>
              <p id={`${id}-object-bounds`} className="native-help">{t('object.geometry.help')}</p>
              <div className="native-coordinates">
                {coordinateLabels.map((label, index) => <label key={label}>{t(`object.${label}`)}
                  <input type="text" inputMode="decimal" className="native-text-line" value={coordinates[index] ?? ''}
                    aria-invalid={!objectBounds} aria-describedby={`${id}-object-bounds${objectBounds ? '' : ` ${id}-object-invalid`}`}
                    onChange={(event) => {
                      const next = [...coordinates] as [string, string, string, string]
                      next[index] = event.currentTarget.value
                      setCoordinates(next)
                      clearFeedback()
                    }} />
                </label>)}
              </div>
              {!objectBounds && <p id={`${id}-object-invalid`} className="native-help">{t('object.geometry.invalid')}</p>}
              {objectProfile === 'image' ? <>
                <label className="native-file">{t('object.file')}
                  <input type="file" accept=".png,.jpg,.jpeg,image/png,image/jpeg"
                    aria-describedby={`${id}-object-file-help ${id}-object-file-feedback`}
                    onChange={(event) => {
                      const file = event.currentTarget.files?.[0]
                      event.currentTarget.value = ''
                      if (file) void chooseImage(file)
                    }} />
                </label>
                <p id={`${id}-object-file-help`} className="native-help">{t('object.file.help', {
                  bytes: NATIVE_PDF_LIMITS.imageBytes, dimension: NATIVE_PDF_LIMITS.imageDimension,
                  pixels: NATIVE_PDF_LIMITS.imagePixels,
                })}</p>
                {selectedObject && <p className="native-help">{t('object.replaceImage.help')}</p>}
                <p id={`${id}-object-file-feedback`} className="native-help" aria-live="polite">{asset && <>
                  <bdi>{asset.name}</bdi>{' — '}{t(asset.status === 'failed' ? asset.reason : `object.file.${asset.status}`)}
                </>}</p>
              </> : <>
                <p className="native-help">{t(`object.graphic.${objectProfile}.help`)}</p>
                <label>{t('object.color')}<input type="color" value={color}
                  onChange={(event) => { setColor(event.currentTarget.value); clearFeedback() }} /></label>
                {objectProfile === 'line' && <label>{t('object.stroke')}
                  <input type="text" inputMode="decimal" className="native-text-line" value={stroke}
                    aria-invalid={!strokeValid} aria-describedby={strokeValid ? undefined : `${id}-object-stroke-error`}
                    onChange={(event) => { setStroke(event.currentTarget.value); clearFeedback() }} />
                </label>}
                {objectProfile === 'line' && !strokeValid &&
                  <p id={`${id}-object-stroke-error`} className="native-help">{t('object.stroke.invalid')}</p>}
              </>}
              {selectedObject && objectBounds && !objectMove && <p className="native-help">{t('object.reason.geometry')}</p>}
              {unavailable && <p className="native-help">{unavailable}</p>}
              {operationError?.code && operationError.code in objectRefusalReasons &&
                <p className="native-help">{t(operationError.reason)}</p>}
              <div className="native-actions">
                {!selectedObject && objectPage.canAdd && <button type="button" disabled={!canPaint}
                  onClick={(event) => submitObject('add', event.currentTarget)}>{t('object.add')}</button>}
                {selectedObject?.operations.includes('replace') && <button type="button" disabled={!canPaint}
                  onClick={(event) => submitObject('replace', event.currentTarget)}>{t('object.replace')}</button>}
                {selectedObject?.operations.includes('move') && <button type="button"
                  disabled={!objectMove || asset?.status === 'loading'}
                  onClick={(event) => submitObject('move', event.currentTarget)}>{t('object.move')}</button>}
                {selectedObject?.operations.includes('remove') && <button type="button"
                  disabled={asset?.status === 'loading'} onClick={(event) => {
                  if (!snapshot || !objectTarget || locked.current || composition.current) return
                  removeTrigger.current = event.currentTarget
                  setConfirm({ snapshot, target: objectTarget, label: t('object.target', {
                    number: objectPage.objects.indexOf(selectedObject) + 1, page: pageIndex + 1,
                    profile: t(`object.profile.${selectedObject.profile}`),
                  }) })
                }}>{t('object.remove')}</button>}
                <button type="button" onClick={() => { resetObject(selectedObject); clearFeedback() }}>{t('object.cancel')}</button>
              </div>
            </fieldset>}
          </section>}
          <section className="native-stack" aria-labelledby={`${id}-text-islands`}>
            <h3 id={`${id}-text-islands`} tabIndex={-1}
              aria-describedby={islands.length === 0 ? `${id}-text-capability` : undefined}>{t('text.title')}</h3>
            {islands.length === 0 && <p id={`${id}-text-capability`} className="native-help native-text-feedback">{textPage?.refusal
              ? t('text.readOnly', { reason: t(textRefusalReasons[textPage.refusal]) })
              : t('text.unavailable')}</p>}
            {islands.length > 0 && <ul className="native-note-list" aria-labelledby={`${id}-text-islands`}>
              {islands.map((island, index) => {
                const key = islandKey(island)
                return <li key={key}>
                  <button type="button" id={`${id}-island-button-${key}`} aria-labelledby={`${id}-island-${key}`}
                    aria-describedby={`${id}-island-preview-${key}`}
                    aria-pressed={island === selectedIsland} disabled={busy !== null}
                    onClick={() => {
                      if (locked.current) return
                      setComposition(false)
                      if (island !== selectedIsland) {
                        const saved = textDrafts.find((entry) => entry.pageId === pageId
                          && sameNativeBytes(entry.island, island.id))
                        updateTextDraft({
                          pageId, island: island.id, selected: true,
                          replacement: saved?.replacement ?? '', refusal: saved?.refusal ?? null,
                        })
                        focusAfterRender.current = `${id}-replacement`
                      } else document.getElementById(`${id}-replacement`)?.focus()
                    }}>
                    <span id={`${id}-island-${key}`}>{t('text.select', { number: index + 1 })}</span>
                    <span id={`${id}-island-preview-${key}`} className="native-note-text" lang="" dir="auto">
                      {island.text}
                    </span>
                  </button>
                </li>
              })}
            </ul>}
            {selectedIsland && textDraft && <fieldset className="native-stack" disabled={busy !== null}>
              <label htmlFor={`${id}-original-text`}>{t('text.original')}</label>
              <input id={`${id}-original-text`} className="native-text-line" type="text" readOnly
                dir="auto" lang="" value={selectedIsland.text} aria-describedby={`${id}-island-bounds`} />
              <p className="native-help" id={`${id}-island-bounds`}>{t('text.bounds', {
                left: formatPoint(selectedIsland.bounds[0]), bottom: formatPoint(selectedIsland.bounds[1]),
                right: formatPoint(selectedIsland.bounds[2]), top: formatPoint(selectedIsland.bounds[3]),
              })}</p>
              <label htmlFor={`${id}-replacement`}>{t('text.replacement')}</label>
              <input id={`${id}-replacement`} className="native-text-line" type="text" dir="auto" lang=""
                value={textDraft.replacement} readOnly={!canEdit}
                aria-describedby={`${id}-text-help ${id}-text-feedback`}
                aria-invalid={(replacementError !== null && !composing && textDraft.replacement !== '') || !!textRefusal}
                onCompositionStart={() => setComposition(true)} onCompositionEnd={() => setComposition(false)}
                onChange={(event) => {
                  if (locked.current) return
                  updateTextDraft({ ...textDraft, replacement: event.target.value, refusal: null })
                }} />
              <p className="native-help" id={`${id}-text-help`}>{t('text.help')}</p>
              <p className="native-help native-text-feedback" id={`${id}-text-feedback`}>
                {!canEdit ? (unavailable ?? t('text.unavailable'))
                  : textRefusal ? t('text.refused', { reason: t(textRefusal) })
                    : replacementError === 'limit' ? t('text.limit', { limit: noteLimit })
                      : replacementError && t(`text.${replacementError}`)}
              </p>
              <div className="native-actions">
                <button type="button" className="native-primary" disabled={!canReplace}
                  aria-describedby={`${id}-text-help ${id}-text-feedback`}
                  onClick={(event) => void act({
                    kind: 'replaceText', island: selectedIsland.id, text: textDraft.replacement,
                  }, event.currentTarget)}>{t('text.apply')}</button>
              </div>
            </fieldset>}
          </section>
          <h3 id={`${id}-notes`} tabIndex={-1}>{t('notes.title')}</h3>
          <p>{t('notes.count', { count: notes.length })}</p>
          {notes.length === 0 && <p>{t(page.canAddNote && geometry && !invalidated && !needsReconcile
            ? 'notes.empty' : 'note.emptyReadOnly')}</p>}
          {page.annotations.length > 0 && <ul className="native-note-list" aria-labelledby={`${id}-notes`}>
            {page.annotations.map((item, index) => item.kind === 'plainNote' ? (
              <li key={item.id}>
                <button type="button" id={`${id}-note-${item.id}`} data-native-note-id={item.id}
                  aria-pressed={selected?.id === item.id} disabled={busy !== null || geometry === null}
                  onClick={() => chooseNote(item)}>
                  {t('note.open', { number: item.id })}
                  {selected?.id === item.id && <span>{t('object.selected')}</span>}
                  <span className="native-note-text" lang="" dir="auto">{item.contents}</span>
                </button>
              </li>
            ) : <li key={`readonly-${item.object?.objectNumber ?? index}-${index}`}>
              {t('object.readOnly', { reason: t(`profile.${item.profile}`) })}
            </li>)}
          </ul>}
          {draft?.note !== null && <div>
            <button type="button" className={selected ? undefined : 'native-primary'} id={`${id}-add`} disabled={!canAdd}
              aria-describedby={canAdd ? undefined : `${id}-unavailable`} onClick={startNote}>{t('note.add')}</button>
          </div>}
          {!canAdd && <p className="native-help" id={`${id}-unavailable`}>{t('capability.unavailable', { reason: unavailable ?? t('note.unavailable') })}</p>}
          {draft && <fieldset className="native-stack" disabled={busy !== null}>
            <label htmlFor={`${id}-contents`}>{t('note.contents')}</label>
            <textarea id={`${id}-contents`} value={draft.contents} dir="auto" lang="" readOnly={!canEdit}
              aria-describedby={`${id}-note-limit ${id}-note-count ${id}-note-error`}
              aria-invalid={noteError !== null && !composing}
              onCompositionStart={() => setComposition(true)} onCompositionEnd={() => setComposition(false)}
              onChange={(event) => updateDraft({ ...draft, contents: event.target.value })} />
            <p className="native-help" id={`${id}-note-limit`}>{t('note.limit', { limit: noteLimit })}</p>
            <p className="native-help" id={`${id}-note-count`}>{t('limit.count', { count: textBytes, limit: NATIVE_PDF_LIMITS.noteBytes })}</p>
            <p className="native-help" id={`${id}-note-error`}>{noteError === 'limit' ? t('note.tooLong', { limit: noteLimit })
              : noteError && t(`note.${noteError}`)}</p>
            <div className="native-coordinates">
              {(['x', 'y'] as const).map((axis) => <label key={axis}>
                {t(`geometry.${axis}`)}
                <input id={`${id}-${axis}`} type="number" inputMode="numeric" min={0} step={1}
                  max={geometry ? (axis === 'x' ? geometry.bounds[2] - geometry.bounds[0] - geometrySize[0]
                    : geometry.bounds[3] - geometry.bounds[1] - geometrySize[1]) : undefined}
                  value={draft[axis]} readOnly={!canEdit}
                  aria-invalid={geometry !== null &&
                    nativeNoteRect(axis === 'x' ? draft.x : '0', axis === 'y' ? draft.y : '0', geometry, geometrySize) === null}
                  aria-describedby={`${id}-coordinates ${id}-bounds`}
                  onChange={(event) => updateDraft({ ...draft, [axis]: event.target.value })} />
              </label>)}
            </div>
            <p className="native-help" id={`${id}-coordinates`}>{t('geometry.help')}</p>
            <p className="native-help" id={`${id}-bounds`}>{bounds && (rect ? bounds : t('error.invalidGeometry', { bounds }))}</p>
            <div className="native-actions">
              {draft.note === null
                ? <button type="button" className="native-primary" id={`${id}-submit-add`} disabled={!canAdd || composing}
                    onClick={(event) => submitDraft('add', event.currentTarget)}>{t('note.add')}</button>
                : <>
                    <button type="button" className="native-primary" id={`${id}-save`} disabled={!canEdit || composing}
                      onClick={(event) => submitDraft('edit', event.currentTarget)}>{t('note.save')}</button>
                    <button type="button" disabled={!canAdd || composing}
                      aria-describedby={canAdd ? undefined : `${id}-unavailable`}
                      onClick={(event) => submitDraft('move', event.currentTarget)}>{t('note.move')}</button>
                  </>}
              <button type="button" disabled={composing} onClick={cancelDraft}>{t('note.cancel')}</button>
              {selected && <button type="button" disabled={!canEdit || composing} onClick={(event) => {
                if (!snapshot || locked.current || composition.current) return
                removeTrigger.current = event.currentTarget
                setConfirm({ snapshot, note: selected.id })
              }}>{t('note.remove')}</button>}
            </div>
          </fieldset>}
        </section>}
      </div>
      <dialog ref={dialog} onCancel={(event) => { event.preventDefault(); setConfirm(null) }}
        aria-labelledby={`${id}-remove-title`} aria-describedby={`${id}-remove-body ${id}-remove-target`}>
        <div className="native-stack">
          <h3 id={`${id}-remove-title`}>{t(confirm && 'target' in confirm ? 'object.remove' : 'note.remove.heading')}</h3>
          <p id={`${id}-remove-body`}>{t(confirm && 'target' in confirm ? 'object.remove.body' : 'note.remove.body')}</p>
          <p id={`${id}-remove-target`}>{confirm && ('target' in confirm ? confirm.label : t('note.confirmTarget', { number: confirm.note, page: pageIndex + 1 }))}</p>
          <div className="native-actions">
            <button type="button" ref={cancelConfirmation} onClick={() => setConfirm(null)}>{t('note.remove.cancel')}</button>
            <button type="button" className="native-destructive" onClick={() => {
              if (confirm === null) return
              setConfirm(null)
              if (confirm.snapshot !== snapshot || !canEdit) {
                setError(['error.stale'])
                return
              }
              if ('target' in confirm) {
                if (!objectTarget || objectTarget.page !== confirm.target.page
                  || !sameNativeBytes(objectTarget.id, confirm.target.id)) {
                  setError(['error.stale'])
                  return
                }
                void act({ kind: 'removeObject', target: confirm.target }, removeTrigger.current)
                return
              }
              if (selected?.id !== confirm.note) {
                setError(['error.stale'])
                return
              }
              void act({ kind: 'remove', note: confirm.note }, removeTrigger.current)
            }}>{t(confirm && 'target' in confirm ? 'object.remove' : 'note.remove')}</button>
          </div>
        </div>
      </dialog>
      {snapshot && <details className="native-stack">
        <summary>{t('identity.title')}</summary>
        <p>{t('identity.origin', { hash: snapshot.originStoreHash })}</p>
        <p>{t('identity.revision', { revision })}</p>
        <p>{t('identity.output', { hash: outputHash })}</p>
        <p>{t('identity.evidence')}</p>
      </details>}
    </section>
  )
}

function NativePagePaper({ geometry, notes, selected, onSelect, zoom }: {
  readonly geometry: Geometry
  readonly notes: readonly Note[]
  readonly selected: number | undefined
  readonly onSelect: (note: Note) => void
  readonly zoom: number
}) {
  const [width, height] = rotatedSize(geometry)
  const markers = notes.map((note) => {
    const [x, y] = nativeMarkerPosition(geometry, note.rect)
    return <span key={note.id} className="native-marker" data-selected={note.id === selected}
      style={{ left: `${x}%`, top: `${y}%` }} onClick={() => onSelect(note)}><span /></span>
  })
  return <div className="native-paper" aria-hidden="true"
    style={{ width: zoom === 0 ? '100%' : width * zoom / 100, aspectRatio: `${width} / ${height}` }}>
    {markers}
  </div>
}
