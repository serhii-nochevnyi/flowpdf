import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react'
import {
  hashPdfSourceBytes, PdfSourceStoreError, type PdfSourceStore,
} from '../../persistence/pdf-source-store.js'
import {
  nativePdfText, type NativePdfLocale, type NativePdfMessage,
} from './pdf-native-edit-messages.js'
import {
  NATIVE_PDF_LIMITS, fitsNativeJson, isNativeFieldCommand, isNativeNoteCommand, isNativePageCommand,
  nativeBytesHex, nativeHexBytes, sameNativeBytes,
  type NativePdfAnnotation, type NativePdfNoteCommand, type NativePdfRect,
  type NativePdfSnapshot, type NativePdfTextCommand, type NativePdfTextIsland,
  type NativePdfTextRefusal, type NativePdfGraphic, type NativePdfObjectCommand, type NativePdfObjectPage,
  type NativePdfObjectTarget, type NativePdfSessionCommand, type NativePdfPageCommand,
  type NativePdfFieldCommand, type NativePdfFieldView,
} from './pdf-native-edit-protocol.js'
import {
  confirmedNativeReplacement, NativePdfWorkerError, type NativePdfEditCoordinator,
} from './pdf-native-edit-worker.js'
import './pdf-native-edit-panel.css'

type Page = NativePdfSnapshot['scene']['pages'][number]
type Geometry = NonNullable<Page['geometry']>
type Note = Extract<NativePdfAnnotation, { kind: 'plainNote' }>
type Coordinator = Pick<NativePdfEditCoordinator,
  'open' | 'project' | 'reconcile' | 'snapshot' | 'subscribe' | 'dispose'> & {
  command(command: NativePdfSessionCommand): Promise<NativePdfSnapshot>
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

type TextIslandFingerprint = Pick<NativePdfTextIsland, 'text' | 'profile' | 'operator' | 'bounds' | 'maxAdvance'>

interface TextDraft {
  readonly pageId: number
  readonly selected: boolean
  readonly refusal: NativePdfMessage | null
  readonly island: readonly number[]
  readonly replacement: string
  readonly suspended?: { readonly stableId: number; readonly fingerprint: TextIslandFingerprint }
}

interface PendingPageAction {
  readonly pageCommand: NativePdfPageCommand
  readonly before: NativePdfSnapshot
  readonly invoker: HTMLElement | null
}

type PendingContentAction = PendingNoteAction | {
  readonly command: NativePdfTextCommand | NativePdfObjectCommand | NativePdfFieldCommand
  readonly before: NativePdfSnapshot
  readonly pageId: number
  readonly objectDraft: ObjectDraft | null
  readonly invoker: HTMLElement | null
}

type PendingAction = PendingContentAction | PendingPageAction
type PageIntent = NativePdfPageCommand extends infer Command
  ? Command extends NativePdfPageCommand ? Omit<Command, 'target'> : never
  : never

function acceptedPageIndex(snapshot: NativePdfSnapshot | undefined) {
  const state = snapshot?.pageState
  return state && state.selected !== null ? state.order.indexOf(state.selected) : 0
}

function retainedPageIndexes(before: NativePdfSnapshot, after: NativePdfSnapshot) {
  const positions = new Map(after.pageState?.order.map((stableId, index) => [stableId, index]))
  const retained = new Map<number, number>()
  before.pageState?.order.forEach((stableId, index) => {
    const next = positions.get(stableId)
    if (next !== undefined) retained.set(index, next)
  })
  return retained
}

interface ObjectDraft {
  readonly profile: ObjectProfile
  readonly coordinates: ObjectCoordinates
  readonly color: string
  readonly stroke: string
  readonly asset: ObjectAsset | null
}

interface RecoveredObjectDraft {
  readonly pageId: number
  readonly target: NativePdfObjectTarget | null
  readonly draft: ObjectDraft
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

interface FieldDraft {
  readonly type: 'text' | 'checkbox'
  readonly name: string
  readonly value: string
  readonly checked: boolean
  readonly page: number
  readonly coordinates: ObjectCoordinates
}

interface SavedFieldDraft {
  readonly field: NativePdfFieldView | null
  readonly draft: FieldDraft
  readonly feedback: NativePdfMessage | null
}

const fieldCoordinateLabels = ['field.x', 'field.y', 'field.width', 'field.height'] as const

export function nativeFieldBounds(values: ObjectCoordinates, page: NativePdfRect): NativePdfRect | null {
  if (values.some((value) => value === '' || /\D/u.test(value))) return null
  const [x, y, width, height] = values
  const left = page[0] + Number(x)
  const bottom = page[1] + Number(y)
  const w = Number(width)
  const h = Number(height)
  const rect = [left, bottom, left + w, bottom + h] as const
  return w >= 8 && h >= 8 && rect[2] <= page[2] && rect[3] <= page[3]
    && rect.every((coordinate) => Number.isInteger(coordinate) && Math.abs(coordinate) <= 1_000_000)
    ? rect : null
}

function fieldKey(field: NativePdfFieldView) {
  const reference = `${field.object.objectNumber}:${field.object.generation}`
  return field.canEditAppearance ? reference : `${reference}:${field.page}`
}

function sameFieldObject(left: NativePdfFieldView, right: NativePdfFieldView) {
  return left.object.objectNumber === right.object.objectNumber && left.object.generation === right.object.generation
}

export function rebindNativeField(
  prior: NativePdfFieldView, fields: readonly NativePdfFieldView[], page: number,
  previousFields?: readonly NativePdfFieldView[],
): NativePdfFieldView | undefined {
  const matches = fields.filter((field) => field.page === page
    && field.name === prior.name && field.kind === prior.kind && field.value === prior.value
    && field.checked === prior.checked && field.canEditAppearance === prior.canEditAppearance
    && field.refusal === prior.refusal && (prior.rect && field.rect
      ? sameNativeBytes(prior.rect, field.rect) : prior.rect === field.rect))
  const [match] = matches
  if (matches.length !== 1 || !match) return undefined
  if (!previousFields || !prior.canEditAppearance || !prior.name || !prior.kind || !prior.rect) {
    return sameFieldObject(prior, match) ? match : undefined
  }
  const previous = previousFields.filter((field) => field.page === prior.page && field.name === prior.name)
  return previous.length === 1 && sameNativeBytes(previous[0]!.id, prior.id)
    && fields.filter((field) => field.page === page && field.name === prior.name).length === 1
    ? match : undefined
}

function draftFromField(field: NativePdfFieldView, snapshot: NativePdfSnapshot): FieldDraft | null {
  const page = snapshot.scene.pages[field.page]?.geometry?.bounds
  if (!field.canEditAppearance || !field.kind || field.name === null || !field.rect || !page) return null
  const [left, bottom, right, top] = field.rect
  return {
    type: field.kind, name: field.name, value: field.value ?? '', checked: field.checked ?? false,
    page: field.page,
    coordinates: [String(left - page[0]), String(bottom - page[1]), String(right - left), String(top - bottom)],
  }
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
  before: NativePdfSnapshot, after: NativePdfSnapshot, pageId: number, selected: ObjectView,
  command: NativePdfSessionCommand, destination = pageId,
): ObjectView | undefined {
  const previous = pageObjects(before, pageId)
  const next = pageObjects(after, destination)
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
  const changesSelection = 'target' in command && 'id' in command.target && command.target.page === pageId
    && sameNativeBytes(command.target.id, selected.id)
  return changesSelection || (candidate.profile === selected.profile && sameNativeBytes(candidate.bounds, selected.bounds))
    ? candidate : undefined
}

function matchesObjectDraftTarget(target: NativePdfObjectTarget | null, object: ObjectView | undefined) {
  return target === null ? object === undefined
    : !!object && object.profile === target.profile && sameNativeBytes(object.id, target.id)
}

export function rebindRecoveredObjectDraft(
  saved: RecoveredObjectDraft, before: NativePdfSnapshot, after: NativePdfSnapshot,
  command: NativePdfSessionCommand, destination = saved.pageId,
): RecoveredObjectDraft | null {
  const { pageId, target } = saved
  if (!target) {
    if (!after.scene.objectPages?.some((page) => page.page === destination && page.canAdd)) return null
    return destination === pageId ? saved : { ...saved, pageId: destination }
  }
  if ('target' in command && 'id' in command.target
    && command.target.page === pageId && sameNativeBytes(command.target.id, target.id)) return null
  const previous = pageObjects(before, pageId).find((object) => matchesObjectDraftTarget(target, object))
  const rebound = previous && rebindNativeObject(before, after, pageId, previous, command, destination)
  return rebound ? { ...saved, pageId: destination, target: { ...target, page: destination, id: rebound.id } } : null
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

function sameTextIsland(left: TextIslandFingerprint, right: TextIslandFingerprint) {
  return left.text === right.text && left.profile === right.profile && left.operator === right.operator
    && left.maxAdvance === right.maxAdvance && left.bounds.every((value, index) => value === right.bounds[index])
}

export function rebindNativeTextDrafts(
  drafts: readonly TextDraft[], before: NativePdfSnapshot, after: NativePdfSnapshot,
  retained?: ReadonlyMap<number, number>,
): TextDraft[] {
  return drafts.flatMap((draft) => {
    const pageId = retained ? retained.get(draft.pageId) : draft.pageId
    if (pageId === undefined) return []
    const { suspended, ...active } = draft
    const stableId = before.pageState?.order[draft.pageId]
    const samePage = stableId !== undefined && after.pageState?.order[pageId] === stableId
    if (stableId !== undefined && !samePage) return []
    let fingerprint: TextIslandFingerprint
    if (suspended) {
      if (!samePage || stableId !== suspended.stableId) return []
      fingerprint = suspended.fingerprint
    } else {
      const previous = verifiedIslands(before, draft.pageId)
      const original = previous.find(({ id }) => sameNativeBytes(id, draft.island))
      if (!original || previous.filter((island) => sameTextIsland(original, island)).length !== 1) return []
      const { text, profile, operator, bounds, maxAdvance } = original
      fingerprint = { text, profile, operator, bounds, maxAdvance }
    }
    const textPage = after.scene.textPages?.find((entry) => entry.page === pageId)
    if (samePage && textPage?.refusal === 'FLOW_PDF_NATIVE_TEXT_OWNERSHIP') {
      return [{ ...active, pageId, suspended: { stableId, fingerprint } }]
    }
    const matches = verifiedIslands(after, pageId).filter((island) => sameTextIsland(fingerprint, island))
    return matches.length === 1 ? [{ ...active, pageId, island: matches[0]!.id }] : []
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

  remember(pageId: number, draft: Draft) {
    const entries = this.pages.get(pageId)
    entries?.delete(draft.note)
    this.pages.set(pageId, new Map([[draft.note, draft], ...(entries ?? [])]))
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

  reindex(before: NativePdfSnapshot, after: NativePdfSnapshot, retained: ReadonlyMap<number, number>) {
    const drafts = [...this.pages]
    this.pages.clear()
    for (const [index, entries] of drafts) {
      const next = retained.get(index)
      if (next === undefined) continue
      const original = before.scene.pages[index]
      const current = after.scene.pages[next]
      if (!original?.geometry || !current?.geometry
        || !sameNativeBytes(original.geometry.bounds, current.geometry.bounds)) continue
      const oldNotes = notesOf(original)
      const currentNotes = notesOf(current)
      for (const [noteId, draft] of entries) {
        const priorNote = oldNotes.find((note) => note.id === noteId)
        const nextNote = currentNotes.find((note) => note.id === noteId)
        if (noteId === null ? current.canAddNote
          : priorNote && nextNote && priorNote.contents === nextNote.contents && sameNativeBytes(priorNote.rect, nextNote.rect)) {
          this.save(next, draft)
        }
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

const fieldRefusalReasons = {
  FLOW_PDF_NATIVE_FIELD_PROFILE: 'field.reason.profile',
  FLOW_PDF_NATIVE_FIELD_REFERENCE: 'field.reason.reference',
  FLOW_PDF_NATIVE_FIELD_APPEARANCE: 'field.reason.appearance',
  FLOW_PDF_NATIVE_FIELD_GLYPH: 'field.reason.glyph',
  FLOW_PDF_NATIVE_FIELD_GEOMETRY: 'field.reason.geometry',
} as const satisfies Record<NonNullable<NativePdfFieldView['refusal']>, NativePdfMessage>

const refusalReasons = new Map<string, NativePdfMessage>([
  ...Object.entries(textRefusalReasons),
  ...Object.entries(objectRefusalReasons),
  ...Object.entries(fieldRefusalReasons),
  ['FLOW_PDF_NATIVE_IMPORT_REFUSED', 'error.importRefused'],
  ['FLOW_PDF_NATIVE_BINDING_MISMATCH', 'error.stale'],
  ['FLOW_PDF_NATIVE_REVISION_MISMATCH', 'error.stale'],
  ['FLOW_PDF_NATIVE_TARGET_UNAVAILABLE', 'error.stale'],
  ['FLOW_PDF_NATIVE_REQUEST_CONFLICT', 'error.stale'],
  ['FLOW_PDF_NATIVE_FINAL_PAGE', 'page.final'],
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

function refusal(cause: unknown, pageOperation = false): readonly [string | undefined, NativePdfMessage] {
  if (cause instanceof NativePdfWorkerError || cause instanceof PdfSourceStoreError) {
    const message = pageOperation && cause.code === 'FLOW_PDF_NATIVE_UNSUPPORTED' ? 'page.unsupported'
      : refusalReasons.get(cause.code)
    if (message !== undefined) return [cause.code, message]
  }
  return [undefined, 'error.retry']
}

const unsubscribeIdle = () => undefined
const idle = () => null

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
  const navigationRequest = useRef<AbortController | null>(null)
  const [navigation, setNavigation] = useState<{ readonly page: number; readonly position: number; readonly failed: boolean } | null>(null)
  const [busy, setBusy] = useState<'opening' | 'mutating' | 'reconciling' | null>(null)
  const [status, setStatus] = useState<Message | null>(null)
  const [error, setError] = useState<Message | null>(null)
  const [operationError, setOperationError] = useState<{ code: string | undefined; reason: NativePdfMessage; file: boolean } | null>(null)
  const [pageId, setPageId] = useState(() => acceptedPageIndex(initialDocument?.snapshot))
  const [blankSide, setBlankSide] = useState<'before' | 'after'>('after')
  const [movePosition, setMovePosition] = useState<number | null>(null)
  const [zoom, setZoom] = useState(0)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [textDrafts, setTextDrafts] = useState<readonly TextDraft[]>([])
  const [objectId, setObjectId] = useState<readonly number[] | null>(null)
  const [objectProfile, setObjectProfile] = useState<ObjectProfile>('image')
  const [coordinates, setCoordinates] = useState<ObjectCoordinates>(['', '', '', ''])
  const [color, setColor] = useState('#334155')
  const [stroke, setStroke] = useState('1')
  const [asset, setAsset] = useState<ObjectAsset | null>(null)
  const [fieldSelection, setFieldSelection] = useState<NativePdfFieldView | null>(null)
  const [fieldDraft, setFieldDraft] = useState<FieldDraft | null>(null)
  const [fieldFeedback, setFieldFeedback] = useState<NativePdfMessage | null>(null)
  const savedFieldDrafts = useRef(new Map<string, SavedFieldDraft>())
  const [composing, setComposing] = useState(false)
  const composition = useRef(false)
  const [confirm, setConfirm] = useState<({ readonly snapshot: NativePdfSnapshot } &
    ({ readonly note: number } | { readonly pageCommand: Extract<NativePdfPageCommand, { kind: 'deletePage' }> } | { readonly target: NativePdfObjectTarget; readonly label: string }
      | { readonly field: NativePdfFieldView; readonly label: string })) | null>(null)
  const uncertain = useRef<PendingAction | null>(null)
  const recoveredObject = useRef<RecoveredObjectDraft | null>(null)
  const pageObjectDrafts = useRef(new Map<number, RecoveredObjectDraft>())
  const recoveredDrafts = useRef(new NativeNoteDraftRecovery())
  const focusAfterRender = useRef<string | HTMLElement | { pageAction: HTMLElement | null } | { fieldAction: HTMLElement | null } | null>(null)
  const dialog = useRef<HTMLDialogElement>(null)
  const cancelConfirmation = useRef<HTMLButtonElement>(null)
  const removeTrigger = useRef<HTMLButtonElement | null>(null)

  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
      navigationRequest.current?.abort()
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
  const pageState = snapshot?.pageState
  const stableId = pageState?.order[pageIndex]
  const pageTarget = stableId === undefined ? null : { page: pageId, stableId }
  const canManagePages = canEdit && !composing && pageTarget !== null
  const targetPosition = movePosition ?? pageIndex
  const navigationBlocked = busy !== null || navigation?.failed === false
  const pageSelectorId = `${id}-page-selector`
  const reconcileId = `${id}-reconcile`
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
  const fields = snapshot?.fields ?? []
  const selectedField = fields.find((field) => fieldSelection && field.page === fieldSelection.page
    && sameNativeBytes(field.id, fieldSelection.id))
  const verifiedFieldDraft = selectedField && snapshot ? draftFromField(selectedField, snapshot) : null
  const fieldValues = fieldDraft ?? verifiedFieldDraft ?? {
    type: 'text', name: '', value: '', checked: false, page: pageIndex, coordinates: ['0', '0', '120', '24'],
  } satisfies FieldDraft
  const fieldDestination = pages[fieldValues.page]?.geometry?.bounds
  const fieldBounds = fieldDestination && nativeFieldBounds(fieldValues.coordinates, fieldDestination)
  const fieldNameValid = fieldValues.name.length > 0 && fieldValues.name.length <= 64
    && !/[^A-Za-z0-9_-]/u.test(fieldValues.name)
  const fieldValueValid = fieldValues.type === 'checkbox' || (fitsNativeJson(fieldValues.value, 1024)
    && !/[\u0000-\u001f\u007f-\u009f\uD800-\uDFFF]/u.test(fieldValues.value))
  const fieldReason = selectedField?.refusal ? fieldRefusalReasons[selectedField.refusal] : null
  const readOnlyFields = fields.filter((field) => field.refusal !== null)
  const fieldControls = canEdit && !navigationBlocked && !composing
  const fieldDescription = `${id}-field-help ${id}-field-feedback`
  const textPage = snapshot?.scene.textPages?.find((entry) => entry.page === page?.id)
  const islands = verifiedIslands(snapshot, pageId)
  const textDraft = textDrafts.find((entry) => entry.pageId === pageId && entry.selected && !entry.suspended)
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
      const trigger = removeTrigger.current
      if (trigger?.isConnected && !trigger.matches(':disabled')) trigger.focus()
      else if (!locked.current) {
        const selector = trigger?.id === `${id}-field-remove` ? `${id}-field-selector` : pageSelectorId
        document.getElementById(selector)?.focus()
      }
    }
  }, [confirm, pageSelectorId])

  useLayoutEffect(() => {
    const target = focusAfterRender.current
    if (target === null) return
    focusAfterRender.current = null
    if (typeof target === 'object' && 'pageAction' in target) {
      const action = target.pageAction
      if (action?.isConnected && !action.matches(':disabled')) action.focus()
      else document.getElementById(pageSelectorId)?.focus()
      return
    }
    if (typeof target === 'object' && 'fieldAction' in target) {
      const action = target.fieldAction
      if (action?.isConnected && !action.matches(':disabled')) action.focus()
      else document.getElementById(`${id}-field-selector`)?.focus()
      return
    }
    const element = typeof target === 'string' ? document.getElementById(target) : target
    if (element?.isConnected && !element.matches(':disabled')) element.focus()
    else if (typeof target === 'string' && target.startsWith(`${id}-object-`)) {
      (document.getElementById(`${id}-object-selector`) ?? document.getElementById(`${id}-objects`))?.focus()
    }
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

  function setFailure(cause: unknown, file: boolean, pageOperation = false) {
    // Keep only bounded, known codes across renders, never an arbitrary Error.
    const [code, reason] = refusal(cause, pageOperation)
    setOperationError({ code, reason, file })
    setStatus(null)
  }

  function clearNavigation() {
    navigationRequest.current?.abort()
    navigationRequest.current = null
    setNavigation(null)
  }

  async function open(filename: string, read: () => Promise<Uint8Array>, originalHash?: string) {
    if (locked.current) return
    locked.current = true
    clearFeedback()
    setStatus(null)
    clearNavigation()
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
      recoveredObject.current = null
      pageObjectDrafts.current.clear()
      recoveredDrafts.current = new NativeNoteDraftRecovery()
      savedFieldDrafts.current.clear()
      setFieldSelection(null)
      setFieldDraft(null)
      setFieldFeedback(null)
      setAccepted({ filename, snapshot: result })
      setPageId(acceptedPageIndex(result))
      setMovePosition(null)
      setBlankSide('after')
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

  async function choosePage(destination: Page | undefined, invoker: HTMLElement) {
    if (locked.current || navigationRequest.current || navigationBlocked || destination === undefined || destination.id === pageId) return
    rememberPageDrafts()
    const position = pages.indexOf(destination) + 1
    if (coordinator && service?.phase === 'ready' && snapshot === service.accepted) {
      const request = new AbortController()
      navigationRequest.current = request
      setNavigation({ page: destination.id, position, failed: false })
      let projected: Page | undefined
      try {
        const scene = await coordinator.project(request.signal)
        projected = scene?.pages.find((item) => item.id === destination.id)
      } catch {
        projected = undefined
      }
      if (!mounted.current || request.signal.aborted || navigationRequest.current !== request) return
      navigationRequest.current = null
      if (!projected) {
        setNavigation({ page: destination.id, position, failed: true })
        if (document.activeElement === document.body) focusAfterRender.current = `${id}-page-retry`
        return
      }
      if (document.activeElement === document.body || document.activeElement === invoker) {
        focusAfterRender.current = pageSelectorId
      }
    }
    setNavigation(null)
    const recovered = recoveredDrafts.current.get(destination.id)
    setPageId(destination.id)
    setMovePosition(null)
    const restoredObject = snapshot ? restoreObject(destination.id, snapshot) : false
    if (!restoredObject) resetObject()
    setDraft(recovered)
    setComposition(false)
    setStatus([restoredObject === 'unseen' ? 'status.objectDraftRestored'
      : restoredObject || recovered ? 'status.draftRestored' : 'navigation.position', {
      page: position,
      total: pages.length,
    }])
  }

  function rememberPageDrafts(keepNoteSelection = false) {
    if (draft && geometry) {
      const original = selected ? draftFrom(selected, geometry) : { contents: '', x: '0', y: '0' }
      if (keepNoteSelection || draft.contents !== original.contents || draft.x !== original.x || draft.y !== original.y) {
        recoveredDrafts.current.remember(pageId, draft)
      }
    }
    const profile = selectedObject?.kind === 'graphic' ? selectedObject.profile : 'image'
    const initialCoordinates = selectedObject && geometry ? objectCoordinates(selectedObject.bounds, geometry.bounds) : ['', '', '', '']
    const edited = objectForm && (asset || objectProfile !== profile || color !== '#334155' || stroke !== '1'
      || coordinates.some((value, index) => value !== initialCoordinates[index]))
    if (edited) pageObjectDrafts.current.set(pageId, {
      pageId, target: objectTarget ?? null, draft: { profile: objectProfile, coordinates, color, stroke, asset },
    })
    else pageObjectDrafts.current.delete(pageId)
  }

  function restoreObject(destination: number, current: NativePdfSnapshot, selection?: ObjectView | null, preferDraft = false) {
    const pageDraft = pageObjectDrafts.current.get(destination)
    const recovered = recoveredObject.current
    const candidates = preferDraft ? [pageDraft, recovered] : [recovered, pageDraft]
    const saved = candidates.find((entry) => entry?.pageId === destination
      && (selection === undefined || matchesObjectDraftTarget(entry.target, selection ?? undefined)))
    if (!saved) return false
    const page = current.scene.objectPages?.find((entry) => entry.page === destination)
    const match = saved.target && page?.objects.find((entry) => matchesObjectDraftTarget(saved.target, entry))
    if (pageDraft === saved) pageObjectDrafts.current.delete(destination)
    if (saved === recovered) recoveredObject.current = null
    if (saved.target ? !match : !page?.canAdd) return false
    if (saved === recovered && pageDraft && matchesObjectDraftTarget(pageDraft.target, match ?? undefined)) {
      pageObjectDrafts.current.delete(destination)
    }
    setObjectId(match?.id ?? null)
    setObjectProfile(saved.draft.profile)
    setCoordinates(saved.draft.coordinates)
    setColor(saved.draft.color)
    setStroke(saved.draft.stroke)
    setAsset(saved.draft.asset)
    return saved === recovered ? 'unseen' : 'draft'
  }

  function resetObject(target?: ObjectView) {
    setAsset(null)
    setObjectId(target?.id ?? null)
    setObjectProfile(target?.profile === 'line' || target?.profile === 'polygon' || target?.profile === 'rectangle'
      ? target.profile : 'image')
    setCoordinates(target && geometry ? objectCoordinates(target.bounds, geometry.bounds) : ['', '', '', ''])
    setColor('#334155')
    setStroke('1')
  }

  async function chooseImage(file: File) {
    const pending = { name: file.name, status: 'loading' } as const
    clearFeedback()
    setAsset(pending)
    const result = await readObjectAsset(file)
    if (!mounted.current) return
    setAsset((current) => current === pending ? result : current)
    for (const [index, saved] of pageObjectDrafts.current) {
      if (saved.draft.asset === pending) pageObjectDrafts.current.set(index, { ...saved, draft: { ...saved.draft, asset: result } })
    }
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
    if (geometry === null || locked.current || navigationRequest.current) return
    setComposition(false)
    setDraft(recoveredDrafts.current.get(pageId, note.id) ?? draftFrom(note, geometry))
    focusAfterRender.current = `${id}-contents`
  }

  function startNote() {
    if (!canAdd || locked.current || navigationRequest.current) return
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
    if (locked.current || navigationRequest.current) return
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

  function chooseField(field: NativePdfFieldView | null) {
    const drafts = savedFieldDrafts.current
    if (fieldDraft) {
      drafts.set(selectedField ? fieldKey(selectedField) : '', {
        field: selectedField ?? null, draft: fieldDraft, feedback: fieldFeedback,
      })
    }
    const key = field ? fieldKey(field) : ''
    const saved = drafts.get(key)
    drafts.delete(key)
    setFieldSelection(field)
    setFieldDraft(saved?.draft ?? null)
    setFieldFeedback(saved?.feedback ?? null)
    setComposition(false)
    clearFeedback()
  }

  function updateField(update: Partial<FieldDraft>) {
    setFieldDraft({ ...fieldValues, ...update })
    setFieldFeedback(null)
  }

  function submitField(operation: 'add' | 'save' | 'move', invoker: HTMLElement) {
    if (operation === 'save') {
      if (selectedField?.canEditAppearance && fieldValueValid) {
        const field = selectedField.id
        void act(fieldValues.type === 'text' ? { kind: 'setFieldText', field, value: fieldValues.value }
          : { kind: 'setFieldChecked', field, checked: fieldValues.checked }, invoker)
      }
      return
    }
    if (!fieldBounds) return
    const location = { page: fieldValues.page, rect: fieldBounds }
    if (operation === 'move') {
      if (selectedField?.canEditAppearance) void act({ kind: 'moveField', field: selectedField.id, ...location }, invoker)
    } else if (!selectedField && fieldNameValid && fieldValueValid) {
      const { name, value, checked, type } = fieldValues
      void act(type === 'text' ? { kind: 'addTextField', name, value, ...location }
        : { kind: 'addCheckboxField', name, checked, ...location }, invoker)
    }
  }

  function publishFieldSelection(
    result: NativePdfSnapshot, command?: NativePdfSessionCommand, retained?: ReadonlyMap<number, number>,
    previousFields?: readonly NativePdfFieldView[],
  ) {
    const nextFields = result.fields ?? []
    const destination = (index: number) => retained ? retained.get(index) : index
    const rebind = (field: NativePdfFieldView) => {
      const index = destination(field.page)
      return index === undefined ? undefined : rebindNativeField(field, nextFields, index, previousFields)
    }
    const drafts = [...savedFieldDrafts.current.values()]
    savedFieldDrafts.current.clear()
    for (const saved of drafts) {
      const field = saved.field && rebind(saved.field)
      const page = destination(saved.draft.page)
      if (field !== undefined && page !== undefined) {
        savedFieldDrafts.current.set(field ? fieldKey(field) : '', { ...saved, field, draft: { ...saved.draft, page } })
      }
    }
    if (command && isNativeFieldCommand(command) && ('name' in command
      || (selectedField && 'field' in command && sameNativeBytes(command.field, selectedField.id)))) {
      const name = 'name' in command ? command.name : selectedField?.name
      if (command.kind === 'removeField') {
        const saved = savedFieldDrafts.current.get('')
        savedFieldDrafts.current.delete('')
        setFieldSelection(null)
        setFieldDraft(saved?.draft ?? null)
        setFieldFeedback(saved?.feedback ?? null)
        return
      }
      const next = nextFields.find((field) => field.canEditAppearance && field.name === name)
      const verified = next && draftFromField(next, result)
      setFieldSelection(next ?? null)
      setFieldFeedback(null)
      let nextDraft: FieldDraft | null = null
      if (verified && fieldDraft) {
        if (command.kind === 'moveField') {
          nextDraft = { ...verified, value: fieldDraft.value, checked: fieldDraft.checked }
        } else if (command.kind === 'setFieldText' || command.kind === 'setFieldChecked') {
          nextDraft = { ...verified, page: fieldDraft.page, coordinates: fieldDraft.coordinates }
        }
      }
      setFieldDraft(nextDraft)
      return
    }
    const rebound = selectedField && rebind(selectedField)
    setFieldSelection(rebound ?? null)
    if (fieldDraft) {
      const draftDestination = destination(fieldDraft.page)
      if ((selectedField && !rebound) || draftDestination === undefined) {
        setFieldDraft(null)
        setFieldFeedback(null)
      } else if (draftDestination !== fieldDraft.page) {
        setFieldDraft({ ...fieldDraft, page: draftDestination })
      }
    }
  }

  function publish(pending: PendingContentAction, result: NativePdfSnapshot): string | null {
    const { command, before } = pending
    publishFieldSelection(result, command)
    if (recoveredObject.current) {
      recoveredObject.current = rebindRecoveredObjectDraft(recoveredObject.current, before, result, command)
    }
    for (const [index, saved] of pageObjectDrafts.current) {
      const rebound = rebindRecoveredObjectDraft(saved, before, result, command)
      if (rebound) pageObjectDrafts.current.set(index, rebound)
      else pageObjectDrafts.current.delete(index)
    }
    setAccepted((current) => current && { ...current, snapshot: result })
    const { kind } = command
    setStatus([kind === 'remove' ? 'note.removed' : 'status.accepted', { revision: result.binding.revision }])
    setTextDrafts((current) => rebindNativeTextDrafts(current.filter((entry) => {
      return command.kind !== 'replaceText' || entry.pageId !== pending.pageId
        || !sameNativeBytes(entry.island, command.island)
    }), before, result))
    const changesSelection = 'target' in command && command.target.page === pageId
      && matchesObjectDraftTarget(command.target, selectedObject)
    if (selectedObject) {
      const rebound = rebindNativeObject(before, result, pageId, selectedObject, command)
      if (!rebound) resetObject()
      else if (changesSelection) resetObject(rebound)
      else setObjectId(rebound.id)
    }
    if (isNativeFieldCommand(command)) return null
    if (!('draft' in pending)) {
      if (command.kind !== 'replaceText') {
        if (command.kind === 'addImage' || command.kind === 'addGraphic') {
          const next = pageObjects(result, pageId)
          if (pageId === command.page && next.length === pageObjects(before, pageId).length + 1) resetObject(next[0])
        }
        if (changesSelection) {
          if (kind === 'moveObject') return `${id}-object-move`
          if (kind === 'replaceGraphic' || kind === 'replaceImage') return `${id}-object-replace`
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

  function publishPage({ before, pageCommand }: PendingPageAction, result: NativePdfSnapshot, verified = true) {
    const nextPage = acceptedPageIndex(result)
    const retained = verified ? retainedPageIndexes(before, result) : new Map<number, number>()
    publishFieldSelection(result, pageCommand, retained, before.fields ?? [])
    recoveredDrafts.current.reindex(before, result, retained)
    setTextDrafts((current) => rebindNativeTextDrafts(current, before, result, retained))
    const savedObjects = [...pageObjectDrafts.current.values()]
    pageObjectDrafts.current.clear()
    for (const saved of savedObjects) {
      const destination = retained.get(saved.pageId)
      if (destination === undefined) continue
      const rebound = rebindRecoveredObjectDraft(saved, before, result, pageCommand, destination)
      if (rebound) pageObjectDrafts.current.set(destination, rebound)
    }
    const recovered = recoveredObject.current
    const destination = recovered && retained.get(recovered.pageId)
    recoveredObject.current = recovered && destination != null
      ? rebindRecoveredObjectDraft(recovered, before, result, pageCommand, destination) : null
    setAccepted((current) => current && { ...current, snapshot: result })
    setPageId(nextPage)
    setMovePosition(null)
    setDraft(recoveredDrafts.current.get(nextPage))
    if (!restoreObject(nextPage, result, undefined, true)) resetObject()
    setStatus(['page.accepted', { page: nextPage + 1, total: result.scene.pages.length, revision: result.binding.revision }])
  }

  function pageActionOwnsFocus(invoker: HTMLElement | null) {
    const active = document.activeElement
    return active === document.body || active === invoker || dialog.current?.contains(active)
  }

  function restorePageActionFocus(pending: PendingPageAction, applicable = true) {
    focusAfterRender.current = {
      pageAction: applicable && pending.pageCommand.kind !== 'deletePage' ? pending.invoker : null,
    }
  }

  function managePage(intent: PageIntent, invoker: HTMLElement) {
    if (pageTarget) void act({ ...intent, target: pageTarget }, invoker)
  }

  async function act(command: NativePdfSessionCommand, invoker: HTMLElement | null) {
    if (locked.current || composition.current || !coordinator || !canEdit || !snapshot) return
    const pageOperation = isNativePageCommand(command)
    if (!pageOperation && navigationRequest.current) return
    if (pageOperation) {
      if (!canManagePages || command.target.page !== pageTarget?.page || command.target.stableId !== pageTarget.stableId
        || (command.kind === 'deletePage' && !pageState?.canDelete)
        || (command.kind === 'reorderPage' && (command.position === pageIndex
          || command.position < 0 || command.position > (pageState?.maxPosition ?? -1)))
        || (geometry === null && command.kind !== 'reorderPage' && command.kind !== 'deletePage')) return
    } else if (isNativeFieldCommand(command)) {
      if ('field' in command && (!selectedField?.canEditAppearance
        || !sameNativeBytes(selectedField.id, command.field))) {
        setError(['error.stale'])
        return
      }
    } else if (command.kind === 'replaceText') {
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
    if (pageOperation) rememberPageDrafts(command.kind === 'rotatePage' || command.kind === 'reorderPage')
    const pending: PendingAction = pageOperation
      ? { pageCommand: command, before: snapshot, invoker }
      : isNativeNoteCommand(command)
      ? { command, before: snapshot, pageId, draft }
      : { command, before: snapshot, pageId, invoker, objectDraft: command.kind === 'replaceText' || command.kind === 'removeObject' || isNativeFieldCommand(command)
        ? null : { profile: objectProfile, coordinates, color, stroke, asset } }
    clearFeedback()
    if (isNativeFieldCommand(command)) setFieldFeedback(null)
    clearNavigation()
    setBusy('mutating')
    try {
      const result = await coordinator.command(command)
      if (!mounted.current) return
      if ('pageCommand' in pending) {
        publishPage(pending, result)
        if (pageActionOwnsFocus(invoker)) restorePageActionFocus(pending)
      } else {
        const fallback = publish(pending, result)
        if (isNativeFieldCommand(command)) {
          if (pageActionOwnsFocus(invoker)) focusAfterRender.current = { fieldAction: invoker }
        } else focusAfterRender.current = fallback ?? (document.activeElement === document.body ? invoker : null)
      }
    } catch (cause) {
      if (!mounted.current) return
      const needsReadback = coordinator.snapshot().phase === 'reconciliationRequired'
      if (needsReadback) uncertain.current = pending
      if ('pageCommand' in pending) {
        if (pageActionOwnsFocus(invoker)) {
          if (needsReadback) focusAfterRender.current = reconcileId
          else restorePageActionFocus(pending)
        }
      } else if (isNativeFieldCommand(command)) {
        if (pageActionOwnsFocus(invoker)) focusAfterRender.current = needsReadback ? reconcileId : { fieldAction: invoker }
        setFieldFeedback(refusal(cause)[1])
      } else if (document.activeElement === document.body) focusAfterRender.current = invoker
      setFailure(cause, false, pageOperation)
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
    if ('pageCommand' in pending) rememberPageDrafts(pending.pageCommand.target.page === pageId
      && (pending.pageCommand.kind === 'rotatePage' || pending.pageCommand.kind === 'reorderPage'))
    locked.current = true
    clearFeedback()
    setBusy('reconciling')
    try {
      const result = await coordinator.reconcile()
      if (!mounted.current) return
      uncertain.current = null
      if ('pageCommand' in pending) {
        if (result.status === 'unseen') {
          publishFieldSelection(result.snapshot, undefined, retainedPageIndexes(pending.before, result.snapshot),
            pending.before.fields ?? [])
          setAccepted((current) => current && { ...current, snapshot: result.snapshot })
          setStatus(['status.unseenNoDraft'])
        } else {
          publishPage(pending, result.snapshot, result.status === 'accepted')
          if (result.status === 'evicted') {
            setStatus(null)
            setError(['error.stale'])
          }
        }
        if (pageActionOwnsFocus(invoker)) restorePageActionFocus(pending,
          result.status === 'accepted' || (result.status === 'unseen' && pageTarget?.stableId === pending.pageCommand.target.stableId))
        return
      }
      let focusTarget = `${id}-${'draft' in pending ? 'notes' : isNativeFieldCommand(pending.command) ? 'field-selector' : pending.command.kind === 'replaceText' ? 'text-islands' : 'object-selector'}`
      if (result.status === 'unseen') {
        setAccepted((current) => current && { ...current, snapshot: result.snapshot })
        publishFieldSelection(result.snapshot)
        if (isNativeFieldCommand(pending.command)) {
          setStatus([fieldDraft ? 'status.unseen' : 'status.unseenNoDraft'])
        } else if (!('draft' in pending) && pending.command.kind !== 'replaceText') {
          const pageIndex = result.snapshot.scene.pages.findIndex((item) => item.id === pending.pageId)
          if (!pending.objectDraft || pageIndex < 0) setStatus(['status.unseenNoDraft'])
          else {
            recoveredObject.current = {
              pageId: pending.pageId,
              target: 'target' in pending.command ? pending.command.target : null,
              draft: pending.objectDraft,
            }
            if (pageId !== pending.pageId) setStatus(['status.unseenPage', { page: pageIndex + 1 }])
            else if (!matchesObjectDraftTarget(recoveredObject.current.target, selectedObject)) setStatus(['status.unseenObject'])
            else setStatus([restoreObject(pageId, result.snapshot, selectedObject ?? null) ? 'status.unseen' : 'status.unseenNoDraft'])
          }
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
        savedFieldDrafts.current.clear()
        setFieldSelection(null)
        setFieldDraft(null)
        setFieldFeedback(null)
        setTextDrafts([])
        recoveredObject.current = null
        pageObjectDrafts.current.clear()
        resetObject()
        setError(['error.stale'])
      }
      if (!('draft' in pending) && isNativeFieldCommand(pending.command)) {
        if (pageActionOwnsFocus(invoker)) focusAfterRender.current = { fieldAction: pending.invoker }
      } else if (document.activeElement === invoker) focusAfterRender.current = focusTarget
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
        : navigation ? t(navigation.failed ? 'navigation.error' : 'navigation.pending', { page: navigation.position })
          : status === null ? '' : t(...status)
  const bounds = geometry && t('geometry.bounds', {
    x: geometry.bounds[2] - geometry.bounds[0] - geometrySize[0],
    y: geometry.bounds[3] - geometry.bounds[1] - geometrySize[1],
  })
  const deletingPage = confirm !== null && 'pageCommand' in confirm
  const fieldLabel = (field: NativePdfFieldView) => t('field.onPage', {
    name: field.name ?? t('field.widget', { object: field.object.objectNumber, generation: field.object.generation }),
    page: field.page + 1,
  })
  const removingField = confirm !== null && 'field' in confirm
  const removingObject = confirm !== null && 'target' in confirm
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
            && <button type="button" id={reconcileId} aria-disabled={busy !== null} onClick={(event) => void reconcile(event.currentTarget)}>
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
      <p>{t('capability.later', { families: t('family.redaction') })}</p>
      {accepted === undefined && <div className="native-stack">
        <h3>{t('empty.heading')}</h3><p>{t('empty.body')}</p>
      </div>}
      <div className="native-workspace">
        <div className="native-stack">
          <div className="native-actions native-page-navigation" role="group" aria-label={t('navigation.label')}>
            <button type="button" disabled={pageIndex <= 0 || navigationBlocked}
              onClick={(event) => void choosePage(pages[pageIndex - 1], event.currentTarget)}>{t('navigation.previous')}</button>
            <button type="button" disabled={pageIndex < 0 || pageIndex >= pages.length - 1 || navigationBlocked}
              onClick={(event) => void choosePage(pages[pageIndex + 1], event.currentTarget)}>{t('navigation.next')}</button>
            <label>{t('navigation.select')}
              <select id={pageSelectorId} disabled={pages.length === 0 || navigationBlocked} value={page?.id ?? ''}
                onChange={(event) => void choosePage(pages.find((entry) => entry.id === Number(event.target.value)), event.currentTarget)}>
                {pages.map((item, index) => <option key={item.id} value={item.id}>{index + 1}</option>)}
              </select>
            </label>
            <label>{t('navigation.zoom')}
              <select disabled={geometry === null || navigationBlocked} value={zoom} onChange={(event) => setZoom(Number(event.target.value))}>
                <option value={0}>{t('navigation.fit')}</option>
                {[50, 100, 150, 200].map((percent) => <option key={percent} value={percent}>{percent}%</option>)}
              </select>
            </label>
            <button type="button" disabled={geometry === null || navigationBlocked} onClick={() => setZoom(0)}>{t('navigation.fit')}</button>
            {navigation?.failed && <button type="button" id={`${id}-page-retry`} disabled={navigationBlocked}
              onClick={(event) => void choosePage(pages.find((entry) => entry.id === navigation.page), event.currentTarget)}>{t('navigation.retry')}</button>}
          </div>
          {pageState && page && <fieldset className="native-stack native-page-tools" disabled={!canManagePages}>
            <legend>{t('page.title')}</legend>
            <div className="native-actions">
              <label>{t('page.placement')}
                <select value={blankSide} onChange={(event) => setBlankSide(event.target.value === 'before' ? 'before' : 'after')}>
                  <option value="before">{t('page.before')}</option>
                  <option value="after">{t('page.after')}</option>
                </select>
              </label>
              <button type="button" disabled={!geometry} aria-describedby={!geometry ? `${id}-page-geometry` : undefined}
                onClick={(event) => managePage({ kind: 'insertBlankPage', side: blankSide }, event.currentTarget)}>{t('page.insert')}</button>
            </div>
            <div className="native-actions">
              <label>{t('page.position')}
                <select value={targetPosition} disabled={pageState.maxPosition === 0}
                  onChange={(event) => setMovePosition(Number(event.target.value))}>
                  {pages.map((_, index) => <option key={index} value={index}>{index + 1}</option>)}
                </select>
              </label>
              <button type="button" disabled={targetPosition === pageIndex}
                onClick={(event) => managePage({ kind: 'reorderPage', position: targetPosition }, event.currentTarget)}>{t('page.reorder')}</button>
              <button type="button" disabled={pageIndex === 0}
                onClick={(event) => managePage({ kind: 'reorderPage', position: pageIndex - 1 }, event.currentTarget)}>{t('page.earlier')}</button>
              <button type="button" disabled={pageIndex >= pageState.maxPosition}
                onClick={(event) => managePage({ kind: 'reorderPage', position: pageIndex + 1 }, event.currentTarget)}>{t('page.later')}</button>
            </div>
            <div className="native-actions">
              <button type="button" disabled={!geometry} aria-describedby={!geometry ? `${id}-page-geometry` : undefined}
                onClick={(event) => managePage({ kind: 'rotatePage', degrees: -90 }, event.currentTarget)}>{t('page.left')}</button>
              <button type="button" disabled={!geometry} aria-describedby={!geometry ? `${id}-page-geometry` : undefined}
                onClick={(event) => managePage({ kind: 'rotatePage', degrees: 90 }, event.currentTarget)}>{t('page.right')}</button>
              <button type="button" disabled={!geometry} aria-describedby={!geometry ? `${id}-page-geometry` : undefined}
                onClick={(event) => managePage({ kind: 'duplicatePage' }, event.currentTarget)}>{t('page.duplicate')}</button>
              <button type="button" className="native-destructive" disabled={!pageState.canDelete}
                aria-describedby={!pageState.canDelete ? `${id}-final-page` : undefined} onClick={(event) => {
                  if (!snapshot || !pageTarget || !pageState.canDelete || !canManagePages) return
                  removeTrigger.current = event.currentTarget
                  setConfirm({ snapshot, pageCommand: { kind: 'deletePage', target: pageTarget } })
                }}>{t('page.delete')}</button>
            </div>
            {!pageState.canDelete && <p id={`${id}-final-page`} className="native-help">{t('page.final')}</p>}
            {!geometry && <p id={`${id}-page-geometry`} className="native-help">{t('page.geometry')}</p>}
          </fieldset>}
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
                disabled={navigationBlocked}
                onChange={(event) => {
                  if (locked.current || navigationRequest.current) return
                  const object = objectPage.objects.find((entry) => objectKey(entry) === event.target.value)
                  const restored = snapshot && restoreObject(pageId, snapshot, object ?? null)
                  if (restored === 'unseen') setStatus(['status.unseen'])
                  if (!restored) resetObject(object)
                  clearFeedback()
                }}>
                <option value="" disabled={!objectPage.canAdd}>{t(objectPage.canAdd ? 'object.new' : 'object.unavailable')}</option>
                {objectPage.objects.map((entry, index) => <option key={objectKey(entry)} value={objectKey(entry)}>
                  {t('object.target', { number: index + 1, page: pageIndex + 1, profile: t(`object.profile.${entry.profile}`) })}
                </option>)}
              </select>
            </label>}
            {objectForm && <fieldset className="native-stack" disabled={!canEdit || navigationBlocked}>
              <legend>{t('object.geometry')}</legend>
              <label>{t('object.profile.label')}
                <select value={objectProfile} disabled={selectedObject?.kind === 'image'} onChange={(event) => {
                  const next = objectProfiles.find((profile) => profile === event.target.value)
                  if (next) {
                    setObjectProfile(next)
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
                {selectedObject?.operations.includes('replace') && <button type="button" id={`${id}-object-replace`} disabled={!canPaint}
                  onClick={(event) => submitObject('replace', event.currentTarget)}>{t('object.replace')}</button>}
                {selectedObject?.operations.includes('move') && <button type="button" id={`${id}-object-move`}
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
                <button type="button" onClick={() => {
                  const saved = recoveredObject.current
                  if (saved?.pageId === pageId && matchesObjectDraftTarget(saved.target, selectedObject)) {
                    recoveredObject.current = null
                  }
                  const pageDraft = pageObjectDrafts.current.get(pageId)
                  if (pageDraft && matchesObjectDraftTarget(pageDraft.target, selectedObject)) pageObjectDrafts.current.delete(pageId)
                  resetObject(selectedObject)
                  clearFeedback()
                }}>{t('object.cancel')}</button>
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
                    aria-pressed={island === selectedIsland} disabled={navigationBlocked}
                    onClick={() => {
                      if (locked.current || navigationRequest.current) return
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
            {selectedIsland && textDraft && <fieldset className="native-stack" disabled={navigationBlocked}>
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
                  if (locked.current || navigationRequest.current) return
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
          <section className="native-stack native-field-tools" aria-labelledby={`${id}-fields`}>
            <h3 id={`${id}-fields`}>{t('field.title')}</h3>
            <label>{t('field.select')}
              <select id={`${id}-field-selector`} value={selectedField ? fieldKey(selectedField) : ''}
                disabled={navigationBlocked || composing || needsReconcile} aria-describedby={`${id}-field-state`}
                onChange={(event) => {
                  const key = event.currentTarget.value
                  chooseField(fields.find((field) => fieldKey(field) === key) ?? null)
                }}>
                <option value="">{t('field.new')}</option>
                {fields.map((field) => <option key={fieldKey(field)} value={fieldKey(field)}>
                  {fieldLabel(field)}
                </option>)}
              </select>
            </label>
            <div id={`${id}-field-state`} className="native-stack">
              {selectedField && <p dir="auto">{fieldLabel(selectedField)}</p>}
              {fieldReason && <p>{t('field.readOnly', { reason: t(fieldReason) })}</p>}
              {!selectedField && readOnlyFields.length > 0 && <ul className="native-field-read-only">
                {readOnlyFields.map((field) => field.refusal && <li key={fieldKey(field)}>
                  {fieldLabel(field)} — {t('field.readOnly', { reason: t(fieldRefusalReasons[field.refusal]) })}
                </li>)}
              </ul>}
              {verifiedFieldDraft && <p dir="auto">{t('field.accepted', {
                value: verifiedFieldDraft.type === 'checkbox'
                  ? t(verifiedFieldDraft.checked ? 'field.checked' : 'field.unchecked')
                  : verifiedFieldDraft.value || t('field.empty'),
              })}</p>}
              {verifiedFieldDraft && <p>{t('field.verifiedGeometry', {
                x: verifiedFieldDraft.coordinates[0], y: verifiedFieldDraft.coordinates[1],
                width: verifiedFieldDraft.coordinates[2], height: verifiedFieldDraft.coordinates[3],
              })}</p>}
              {verifiedFieldDraft && <p>{t('field.owned')}</p>}
            </div>
            {(!selectedField || verifiedFieldDraft) && <fieldset className="native-stack" disabled={!canEdit || navigationBlocked}>
              <legend>{t(selectedField ? (fieldValues.type === 'text' ? 'field.text' : 'field.checkbox') : 'field.new')}</legend>
              {!selectedField && <label>{t('field.type')}
                <select value={fieldValues.type} disabled={composing}
                  onChange={(event) => updateField({ type: event.currentTarget.value === 'checkbox' ? 'checkbox' : 'text' })}>
                  <option value="text">{t('field.text')}</option>
                  <option value="checkbox">{t('field.checkbox')}</option>
                </select>
              </label>}
              <label>{t('field.name')}
                <input className="native-text-line" readOnly={!!selectedField} value={fieldValues.name}
                  aria-invalid={!selectedField && fieldValues.name.length > 0 && !fieldNameValid}
                  aria-describedby={selectedField ? undefined : `${id}-field-name-help`}
                  onCompositionStart={() => setComposition(true)} onCompositionEnd={() => setComposition(false)}
                  onChange={(event) => updateField({ name: event.currentTarget.value })} />
              </label>
              {!selectedField && <p id={`${id}-field-name-help`} className="native-help">{t('field.nameHelp')}</p>}
              {fieldValues.type === 'text' ? <>
                <label>{t('field.value')}
                  <input className="native-text-line" dir="auto" value={fieldValues.value}
                    aria-invalid={!fieldValueValid || fieldFeedback === 'field.reason.glyph'}
                    aria-describedby={fieldDescription} onCompositionStart={() => setComposition(true)}
                    onCompositionEnd={() => setComposition(false)}
                    onChange={(event) => updateField({ value: event.currentTarget.value })} />
                </label>
                <p id={`${id}-field-help`} className="native-help">{t('field.help')}</p>
              </> : <label className="native-field-checkbox">
                <input type="checkbox" checked={fieldValues.checked}
                  aria-describedby={`${id}-field-feedback`}
                  onChange={(event) => updateField({ checked: event.currentTarget.checked })} />
                {t('field.checked')}
              </label>}
              <label>{t('field.page')}
                <select value={fieldValues.page} aria-describedby={`${id}-field-geometry`}
                  onChange={(event) => updateField({ page: Number(event.currentTarget.value) })}>
                  {pages.map((entry, index) => <option key={entry.id} value={index} disabled={!entry.geometry}>
                    {t('navigation.position', { page: index + 1, total: pages.length })}
                  </option>)}
                </select>
              </label>
              <div className="native-coordinates">
                {fieldCoordinateLabels.map((label, index) => <label key={label}>{t(label)}
                  <input type="number" min={index < 2 ? 0 : 8} step={1} value={fieldValues.coordinates[index]}
                    aria-invalid={!fieldBounds} aria-describedby={`${id}-field-geometry`}
                    onChange={(event) => {
                      const coordinates: [string, string, string, string] = [...fieldValues.coordinates]
                      coordinates[index] = event.currentTarget.value
                      updateField({ coordinates })
                    }} />
                </label>)}
              </div>
              <p id={`${id}-field-geometry`} className="native-help">{t('field.reason.geometry')}</p>
              <p id={`${id}-field-feedback`} aria-live="polite">{fieldFeedback ? t(fieldFeedback)
                : !fieldValueValid ? t('field.valueHelp') : ''}</p>
              <div className="native-actions">
                {!selectedField ? <button type="button" className="native-primary"
                  disabled={!fieldControls || !fieldNameValid || !fieldValueValid || !fieldBounds}
                  aria-describedby={`${id}-field-name-help ${id}-field-geometry ${id}-field-feedback`}
                  onClick={(event) => submitField('add', event.currentTarget)}>{t('field.add')}</button> : <>
                  <button type="button" className="native-primary" disabled={!fieldControls || !fieldValueValid}
                    aria-describedby={`${id}-field-feedback`}
                    onClick={(event) => submitField('save', event.currentTarget)}>{t('field.save')}</button>
                  <button type="button" disabled={!fieldControls || !fieldBounds}
                    aria-describedby={`${id}-field-geometry ${id}-field-feedback`}
                    onClick={(event) => submitField('move', event.currentTarget)}>{t('field.move')}</button>
                  <button type="button" id={`${id}-field-remove`} disabled={!fieldControls}
                    onClick={(event) => {
                      if (!snapshot || locked.current || composition.current) return
                      removeTrigger.current = event.currentTarget
                      setConfirm({ snapshot, field: selectedField, label: fieldLabel(selectedField) })
                    }}>{t('field.remove')}</button>
                </>}
                {fieldDraft && <button type="button" disabled={!fieldControls} onClick={() => {
                  setFieldDraft(null)
                  setFieldFeedback(null)
                  clearFeedback()
                  focusAfterRender.current = { fieldAction: null }
                }}>{t('field.cancel')}</button>}
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
                  aria-pressed={selected?.id === item.id} disabled={navigationBlocked || geometry === null}
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
            <button type="button" className={selected ? undefined : 'native-primary'} id={`${id}-add`} disabled={!canAdd || navigationBlocked}
              aria-describedby={canAdd ? undefined : `${id}-unavailable`} onClick={startNote}>{t('note.add')}</button>
          </div>}
          {!canAdd && <p className="native-help" id={`${id}-unavailable`}>{t('capability.unavailable', { reason: unavailable ?? t('note.unavailable') })}</p>}
          {draft && <fieldset className="native-stack" disabled={navigationBlocked}>
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
          <h3 id={`${id}-remove-title`}>{t(deletingPage ? 'page.confirm' : removingObject ? 'object.remove' : removingField ? 'field.remove' : 'note.remove.heading')}</h3>
          <p id={`${id}-remove-body`}>{t(deletingPage ? 'page.deleteBody' : removingObject ? 'object.remove.body' : removingField ? 'field.removeBody' : 'note.remove.body')}</p>
          <p id={`${id}-remove-target`}>{confirm && ('pageCommand' in confirm
            ? t('navigation.position', { page: confirm.pageCommand.target.page + 1, total: confirm.snapshot.scene.pages.length })
            : 'label' in confirm ? confirm.label : t('note.confirmTarget', { number: confirm.note, page: pageIndex + 1 }))}</p>
          <div className="native-actions">
            <button type="button" ref={cancelConfirmation} onClick={() => setConfirm(null)}>{t(deletingPage ? 'page.keep' : removingField ? 'field.keep' : 'note.remove.cancel')}</button>
            <button type="button" className="native-destructive" onClick={() => {
              if (confirm === null) return
              setConfirm(null)
              if (confirm.snapshot !== snapshot || !canEdit) {
                setError(['error.stale'])
                return
              }
              if ('pageCommand' in confirm) {
                if (pageTarget?.stableId !== confirm.pageCommand.target.stableId || !pageState?.canDelete) {
                  setError(['error.stale'])
                  return
                }
                void act(confirm.pageCommand, removeTrigger.current)
                return
              }
              if ('field' in confirm) {
                if (!selectedField?.canEditAppearance || !sameNativeBytes(selectedField.id, confirm.field.id)) {
                  setError(['error.stale'])
                  return
                }
                void act({ kind: 'removeField', field: confirm.field.id }, removeTrigger.current)
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
            }}>{t(deletingPage ? 'page.delete' : removingObject ? 'object.remove' : removingField ? 'field.remove' : 'note.remove')}</button>
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
