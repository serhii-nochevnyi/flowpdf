import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import {
  NativeNoteDraftRecovery,
  PdfNativeEditPanel,
  nativeCropBounds,
  nativeDownloadName,
  nativeObjectBounds,
  nativeFieldBounds,
  redactedDownloadFor,
  rebindNativeField,
  rebindNativeObject,
  rebindRecoveredObjectDraft,
  nativeMarkerPosition,
  nativeNoteDraftAfter,
  nativeNoteDraftAfterAcceptance,
  nativeNoteRect,
  nativeNoteTextError,
  nativeRedactionOffsets,
  nativeRedactionOverlay,
  nativeReplacementError,
  rebindNativeTextDrafts,
  verifiedRedactionCandidate,
  verifiedRedactionMarkSnapshot,
  type NativePdfAcceptedDocument,
} from '../src/pdf/pdf-native-edit-panel.js'
import { nativePdfMessages, nativePdfText } from '../src/pdf/pdf-native-edit-messages.js'
import {
  NATIVE_PDF_LIMITS,
  NATIVE_PDF_PROTOCOL_VERSION,
  isNativeFieldCommand,
  readNativeResponse,
  type NativePdfAnnotation,
  type NativePdfFieldView,
  type NativePdfObjectPage,
  type NativePdfRedactionResult,
  type NativePdfSnapshot,
  type NativePdfTextIsland,
} from '../src/pdf/pdf-native-edit-protocol.js'

const geometry = { bounds: [-10, 20, 190, 120], rotation: 90 } as const
const hash = Array<number>(32).fill(42)
const ref = { objectNumber: 1, generation: 0 }
const note = (id: number): Extract<NativePdfAnnotation, { kind: 'plainNote' }> => ({
  kind: 'plainNote', id, object: { ...ref, objectNumber: id },
  contents: '<script>Привіт & hello</script>', rect: [0, 30, 24, 54],
})
const documentWith = (annotations: readonly NativePdfAnnotation[]): NativePdfAcceptedDocument => ({
  filename: '<unsafe>.pdf',
  snapshot: {
    binding: { epoch: Array<number>(16).fill(1), sessionId: 1, originHash: hash, revision: 2 },
    originStoreHash: 'a'.repeat(64), outputHash: hash, consumedRequest: 3, outputHex: '25',
    scene: { outputHash: hash, sceneHash: hash, pages: [{ id: 0, object: ref, geometry, canAddNote: true, annotations }] },
  },
})

const dependencies = {
  createCoordinator: () => { throw Error('SSR must not create a worker') },
  sourceStore: { put: async () => { throw Error('unexpected write') }, get: async () => null },
}

describe('exact redacted output', () => {
  const before = documentWith([]).snapshot
  const outputHash = Array<number>(32).fill(43)
  const pageState = { order: [10, 20], selected: 10, canDelete: true, maxPosition: 1 }
  const previous = { ...before, pageState }
  const next = {
    ...previous, binding: { ...before.binding, revision: 3 }, consumedRequest: 5,
    outputHash, scene: { ...before.scene, outputHash },
  }
  const check = { status: 'passed', coveredPages: 2 } as const
  const result = {
    action: 'publish', acceptedHash: before.outputHash, published: true, markCount: 0,
    candidate: { hash: outputHash, affectedPages: [10, 20], validatorVersion: 1, validatorProfile: 'classic-xref-owned-144dpi-v1' },
    verification: { status: 'passed', structure: check, text: check, raster: check },
  } as const satisfies NativePdfRedactionResult

  it('requires a publishing receipt tied to the exact accepted revision and every marked page', () => {
    const accept = (reply: NativePdfRedactionResult | null, snapshot: NativePdfSnapshot = next, affected = [10, 20]) =>
      verifiedRedactionCandidate(previous, snapshot, reply, affected)
    expect(accept(result)).toBe(result.candidate)
    expect(accept({ ...result, action: 'readPublished' })).toBe(result.candidate)
    expect(accept(null)).toBeNull()
    expect(accept({ ...result, action: 'status' })).toBeNull()
    expect(accept({ ...result, published: false })).toBeNull()
    expect(accept({ ...result, markCount: 1 })).toBeNull()
    expect(accept({ ...result, acceptedHash: outputHash })).toBeNull()
    expect(accept({ ...result, candidate: { ...result.candidate, hash } })).toBeNull()
    expect(accept({ ...result, candidate: { ...result.candidate, validatorVersion: 2 as 1 } })).toBeNull()
    expect(accept({ ...result, candidate: { ...result.candidate, affectedPages: [10] } })).toBeNull()
    expect(accept({ ...result, verification: { ...result.verification, raster: { ...check, coveredPages: 1 } } })).toBeNull()
    for (const status of ['pending', 'failed', 'unavailable', 'inconclusive'] as const) {
      expect(accept({ ...result, verification: { ...result.verification, status } })).toBeNull()
    }
    expect(accept(result, { ...next, binding: { ...next.binding, revision: 4 } })).toBeNull()
    expect(accept(result, { ...next, binding: { ...next.binding, sessionId: 2 } })).toBeNull()
    expect(accept(result, { ...next, consumedRequest: before.consumedRequest })).toBeNull()
    expect(accept(result, { ...next, scene: before.scene })).toBeNull()
    expect(accept(result, next, [10])).toBeNull()
  })

  it('keeps the download and its evidence through an equivalent accepted readback', () => {
    const candidate = verifiedRedactionCandidate(previous, next, result, [10, 20])!
    const verified = { snapshot: next, candidate }
    const readback = structuredClone(next)
    expect(redactedDownloadFor(verified, readback)).toBe(verified)
    expect(redactedDownloadFor(verified, { ...readback, outputHex: '26' })).toBeNull()
    expect(redactedDownloadFor(verified, { ...readback, outputHash: hash })).toBeNull()
    expect(redactedDownloadFor(verified, { ...readback, originStoreHash: 'b'.repeat(64) })).toBeNull()
    for (const binding of [
      { ...readback.binding, revision: 4 },
      { ...readback.binding, sessionId: 2 },
      { ...readback.binding, epoch: Array<number>(16).fill(2) },
      { ...readback.binding, originHash: outputHash },
    ]) {
      expect(redactedDownloadFor(verified, { ...readback, binding })).toBeNull()
    }
  })

  for (const locale of ['en', 'uk'] as const) {
    it(`keeps redaction unavailable without a live scrub capability in ${locale}`, () => {
      const markup = renderToStaticMarkup(createElement(PdfNativeEditPanel, {
        ...dependencies, locale, initialDocument: documentWith([]),
      }))
      expect(markup).toContain(nativePdfText(locale, 'redaction.unavailable'))
      expect(markup).toContain(nativePdfText(locale, 'viewport.unavailable'))
      expect(markup).not.toContain(nativePdfText(locale, 'viewport.retry'))
      expect(markup).not.toContain(nativePdfText(locale, 'session.redactionRecovery'))
      expect(markup).not.toContain(nativePdfText(locale, 'redaction.add'))
      expect(markup).not.toContain(nativePdfText(locale, 'redaction.apply'))
      expect(markup).not.toContain('<canvas')
      expect(markup).not.toContain(nativePdfText(locale, 'redaction.download'))
      expect(markup).not.toContain(nativePdfText(locale, 'redaction.success'))
    })
  }
})

describe('verified redaction mark settlement', () => {
  const before = { ...documentWith([]).snapshot, redactionAdmission: 'supported' } as const
  const unavailable = { ...before, redactionAdmission: 'unavailable' } as const
  const added = {
    action: 'addMark', acceptedHash: before.outputHash, markCount: 1,
    published: false, candidate: null, verification: null,
  } as const satisfies NativePdfRedactionResult

  it('accepts admission changes in either direction for the same accepted file', () => {
    expect(verifiedRedactionMarkSnapshot(before, unavailable, added, 'addMark', 1)).toBe(unavailable)
    for (const action of ['removeMark', 'clearMarks'] as const) {
      expect(verifiedRedactionMarkSnapshot(unavailable, before, { ...added, action, markCount: 0 }, action, 0)).toBe(before)
    }
  })

  it('requires an unchanged accepted file and a matching status-only mark receipt', () => {
    for (const snapshot of [null, { ...unavailable, outputHex: '26' },
      { ...unavailable, binding: { ...before.binding, revision: 3 } }, { ...unavailable, consumedRequest: 4 }]) {
      expect(verifiedRedactionMarkSnapshot(before, snapshot, added, 'addMark', 1)).toBeNull()
    }
    for (const result of [null, { ...added, published: true }, { ...added, markCount: 0 },
      { ...added, action: 'clearMarks' } as const, { ...added, acceptedHash: Array<number>(32).fill(43) }]) {
      expect(verifiedRedactionMarkSnapshot(before, unavailable, result, 'addMark', 1)).toBeNull()
    }
  })
})

describe('redaction mark geometry', () => {
  const cropBox = [125, -375, 100_625, 50_125] as const

  it('round trips zero offsets and fractional edges with the verified crop', () => {
    const origin = nativeCropBounds(['0', '0', '.5', '0,5'], cropBox)!
    const edge = nativeCropBounds(['100', '50', '.5', '.5'], cropBox)!
    expect(origin).toEqual([125, -375, 625, 125])
    expect(edge).toEqual([100_125, 49_625, 100_625, 50_125])
    expect(nativeRedactionOffsets(origin, cropBox)).toEqual([0, 0, 500, 500])
    expect(nativeRedactionOffsets(edge, cropBox)).toEqual([100_000, 50_000, 500, 500])
    expect(nativeCropBounds(['100', '50', '.501', '.5'], cropBox)).toBeNull()
    expect(nativeCropBounds(['100', '50', '.5', '.501'], cropBox)).toBeNull()
  })

  it.each([0, 90] as const)('places exact crop corners on the preview at %i°', (rotation) => {
    const origin = nativeCropBounds(['0', '0', '.5', '.5'], cropBox)!
    const edge = nativeCropBounds(['100', '50', '.5', '.5'], cropBox)!
    const first = nativeRedactionOverlay({ cropBox, rotation }, origin)
    const last = nativeRedactionOverlay({ cropBox, rotation }, edge)
    const percent = (value: string) => Number.parseFloat(value)
    const width = rotation === 0 ? 100 * 500 / 100_500 : 100 * 500 / 50_500
    const height = rotation === 0 ? 100 * 500 / 50_500 : 100 * 500 / 100_500
    expect(first.left).toBe('0%')
    expect(percent(first.top)).toBeCloseTo(rotation === 0 ? 100 - height : 0)
    expect(percent(first.width)).toBeCloseTo(width)
    expect(percent(first.height)).toBeCloseTo(height)
    expect(percent(last.left) + percent(last.width)).toBeCloseTo(100)
    expect(percent(last.top)).toBeCloseTo(rotation === 0 ? 0 : 100 - height)
  })
})

describe('text draft readback', () => {
  it('rebinds unique matches on the original page and discards ambiguous or missing targets', () => {
    const island: NativePdfTextIsland = {
      id: hash, text: 'Hello', bounds: [0, 30, 24, 54],
      profile: 'trueTypeWinAnsi', operator: 'Tj', maxAdvance: 24,
    }
    const revised = { ...island, id: Array<number>(32).fill(43) }
    const duplicate = { ...island, id: Array<number>(32).fill(44) }
    const draft = { pageId: 0, island: island.id, replacement: 'Draft', selected: true, refusal: null }
    const snapshot = documentWith([]).snapshot
    const withIslands = (islands: readonly NativePdfTextIsland[]) => ({
      ...snapshot,
      scene: { ...snapshot.scene, textPages: [
        { page: 0, islands, refusal: null },
        { page: 1, islands: [duplicate], refusal: null },
      ] },
    })

    expect(rebindNativeTextDrafts([draft], withIslands([island]), withIslands([revised])))
      .toEqual([{ ...draft, island: revised.id }])
    expect(rebindNativeTextDrafts([draft], withIslands([island]), withIslands([revised, duplicate]))).toEqual([])
    expect(rebindNativeTextDrafts([draft], withIslands([island, duplicate]), withIslands([revised]))).toEqual([])
    expect(rebindNativeTextDrafts([draft], withIslands([island]), withIslands([]))).toEqual([])
    expect(rebindNativeTextDrafts([draft], withIslands([island]), withIslands([]), new Map([[0, 1]])))
      .toEqual([{ ...draft, pageId: 1, island: duplicate.id }])
    expect(rebindNativeTextDrafts([draft], withIslands([island]), withIslands([revised]), new Map())).toEqual([])
  })
})

describe('native note workspace', () => {
  it('keeps the other pending fields whether Save or Move is accepted first', () => {
    const original = note(2)
    const pending = { note: 2, contents: 'Edited', x: '20', y: '30' }
    const edited = { ...original, contents: pending.contents }
    const moved = { ...original, rect: [10, 50, 34, 74] as const }
    const afterSave = nativeNoteDraftAfter('edit', pending, edited, geometry)
    const afterMove = nativeNoteDraftAfter('move', pending, moved, geometry)
    expect(afterSave).toEqual(pending)
    expect(afterMove).toEqual(pending)
    const final = { ...edited, rect: moved.rect }
    expect(nativeNoteDraftAfter('move', afterSave, final, geometry)).toEqual(pending)
    expect(nativeNoteDraftAfter('edit', afterMove, final, geometry)).toEqual(pending)
  })

  it('refreshes a reselected target from accepted readback and keeps other selections', () => {
    const original = note(2)
    const other = note(3)
    const before = documentWith([original, other]).snapshot
    const edited = { ...original, contents: 'Verified readback' }
    const result = documentWith([edited, other]).snapshot
    const selected = { note: 2, contents: original.contents, x: '25', y: '30' }
    const pending = {
      command: { kind: 'edit', note: 2, contents: 'Submitted' } as const,
      before, pageId: 0, draft: { ...selected, contents: 'Submitted' },
    }
    expect(nativeNoteDraftAfterAcceptance(pending, result, 0, selected))
      .toEqual({ ...selected, contents: edited.contents })
    const moved = { ...original, rect: [10, 50, 34, 74] as const }
    expect(nativeNoteDraftAfterAcceptance({
      ...pending, command: { kind: 'move', note: 2, page: 0, rect: moved.rect },
    }, documentWith([moved, other]).snapshot, 0, selected))
      .toEqual({ ...selected, x: '20', y: '30' })
    const unrelated = { ...selected, note: 3 }
    expect(nativeNoteDraftAfterAcceptance(pending, result, 0, unrelated)).toBe(unrelated)
    expect(nativeNoteDraftAfterAcceptance(pending, result, 1, selected)).toBe(selected)
  })

  it('selects the next surviving note, the preceding note, or Add after removal', () => {
    const previous = note(2)
    const target = note(3)
    const next = note(4)
    const draft = { note: 3, contents: 'Pending', x: '17', y: '28' }
    const command = { kind: 'remove', note: 3 } as const
    const afterRemoval = (before: readonly NativePdfAnnotation[], after: readonly NativePdfAnnotation[]) =>
      nativeNoteDraftAfterAcceptance({ command, before: documentWith(before).snapshot, pageId: 0, draft },
        documentWith(after).snapshot, 0, draft)
    expect(afterRemoval([previous, target, next], [previous, next]))
      .toEqual({ note: 4, contents: next.contents, x: '10', y: '10' })
    expect(afterRemoval([previous, target], [previous])?.note).toBe(2)
    expect(afterRemoval([target], [])).toBeNull()

    const pending = { command, before: documentWith([previous, target, next]).snapshot, pageId: 0, draft }
    const result = documentWith([previous, next]).snapshot
    const unrelated = { ...draft, note: 2 }
    expect(nativeNoteDraftAfterAcceptance(pending, result, 0, unrelated)).toBe(unrelated)
    expect(nativeNoteDraftAfterAcceptance(pending, result, 1, draft)).toBe(draft)
  })

  it('selects a verified added note only when its submitted draft is still active', () => {
    const draft = { note: null, contents: 'New', x: '0', y: '0' }
    const added = note(3)
    const pending = {
      command: { kind: 'add', page: 0, rect: added.rect, contents: draft.contents } as const,
      before: documentWith([note(2)]).snapshot, pageId: 0, draft,
    }
    const result = documentWith([note(2), added]).snapshot
    expect(nativeNoteDraftAfterAcceptance(pending, result, 0, draft))
      .toEqual({ note: 3, contents: added.contents, x: '10', y: '10' })
    expect(nativeNoteDraftAfterAcceptance(pending, result, 0, null)).toBeNull()
    const otherDraft = { ...draft }
    expect(nativeNoteDraftAfterAcceptance(pending, result, 0, otherDraft)).toBe(otherDraft)
  })

  it('keeps corrections and independent unseen drafts across navigation and selection', () => {
    const recovery = new NativeNoteDraftRecovery()
    const first = { note: 2, contents: 'First unseen', x: '17', y: '28' }
    const corrected = { ...first, contents: 'Corrected after recovery', x: '19' }
    const other = { ...first, note: 3, contents: 'Second unseen' }
    const newNote = { ...first, note: null, contents: 'New note' }
    expect(recovery.restoreUnseen({ pageId: 0, draft: first }, 0, first)).toBe(first)
    recovery.update(0, corrected)
    expect(recovery.restoreUnseen({ pageId: 0, draft: other }, 1, newNote)).toBe(newNote)
    expect(recovery.restoreUnseen({ pageId: 1, draft: newNote }, 1, null)).toBe(newNote)
    expect(recovery.get(0)).toBe(corrected)
    expect(recovery.get(0, 2)).toBe(corrected)
    expect(recovery.get(0, 3)).toBe(other)
    expect(recovery.get(0, null)).toBeNull()
    expect(recovery.get(1, null)).toBe(newNote)
    expect(recovery.get(1, 2)).toBeNull()
    expect(recovery.restoreUnseen({ pageId: 0, draft: other }, 0, corrected)).toBe(corrected)
    recovery.discard(0, 2)
    expect(recovery.get(0, 2)).toBeNull()
    expect(recovery.get(0, 3)).toBe(other)
  })

  it('retires only accepted fields or the removed draft, even when another note is displayed', () => {
    const recovery = new NativeNoteDraftRecovery()
    const original = note(2)
    const other = { note: 3, contents: 'Other draft', x: '10', y: '10' }
    const draft = { note: 2, contents: 'Recovered edit', x: '20', y: '30' }
    const before = documentWith([original, note(3)]).snapshot
    const edited = { ...original, contents: draft.contents }
    const moved = { ...original, rect: [10, 50, 34, 74] as const }
    const edit = { kind: 'edit', note: 2, contents: draft.contents } as const
    const move = { kind: 'move', note: 2, page: 0, rect: moved.rect } as const

    recovery.restoreUnseen({ pageId: 0, draft }, 0, draft)
    recovery.restoreUnseen({ pageId: 0, draft: other }, 0, other)
    const afterSave = documentWith([edited, note(3)]).snapshot
    recovery.accept({ command: edit, before, pageId: 0, draft }, afterSave)
    expect(recovery.get(0, 2)).toEqual(draft)
    const afterBoth = documentWith([{ ...moved, contents: draft.contents }, note(3)]).snapshot
    recovery.accept({ command: move, before: afterSave, pageId: 0, draft }, afterBoth)
    expect(recovery.get(0, 2)).toBeNull()

    recovery.restoreUnseen({ pageId: 0, draft }, 0, other)
    const afterMove = documentWith([moved, note(3)]).snapshot
    recovery.accept({ command: move, before, pageId: 0, draft }, afterMove)
    expect(recovery.get(0, 2)).toEqual(draft)
    const remove = { kind: 'remove', note: 2 } as const
    recovery.accept({ command: remove, before: afterMove, pageId: 0, draft }, documentWith([note(3)]).snapshot)
    expect(recovery.get(0, 2)).toBeNull()
    expect(recovery.get(0, 3)).toBe(other)

    recovery.accept({ command: edit, before, pageId: 0, draft: null }, afterSave)
    expect(recovery.get(0, 2)).toBeNull()
  })

  it('clears the new-note draft on acceptance and retains a new note on another page', () => {
    const recovery = new NativeNoteDraftRecovery()
    const draft = { note: null, contents: 'First new note', x: '10', y: '10' }
    const other = { ...draft, contents: 'Second new note' }
    const added = { ...note(2), contents: draft.contents }
    recovery.restoreUnseen({ pageId: 0, draft }, 0, draft)
    recovery.restoreUnseen({ pageId: 1, draft: other }, 1, other)
    recovery.accept({
      command: { kind: 'add', page: 0, rect: added.rect, contents: draft.contents },
      before: documentWith([]).snapshot, pageId: 0, draft,
    }, documentWith([added]).snapshot)
    expect(recovery.get(0, null)).toBeNull()
    expect(recovery.get(1, null)).toBe(other)
  })

  it('carries a note draft to its retained page only when the note readback still matches', () => {
    const retainedNote = note(2)
    const before = documentWith([retainedNote]).snapshot
    const original = before.scene.pages[0]!
    const moved = { ...original, id: 1, geometry: { ...geometry, rotation: 180 } }
    const recovered = new NativeNoteDraftRecovery()
    const draft = { note: 2, contents: 'Unsubmitted', x: '40', y: '50' }
    recovered.remember(0, draft)
    const after = { ...before, scene: { ...before.scene, pages: [original, moved] } }
    recovered.reindex(before, after, new Map([[0, 1]]))
    expect(recovered.get(0)).toBeNull()
    expect(recovered.get(1)).toBe(draft)
    const changed = { ...moved, annotations: [{ ...retainedNote, contents: 'Changed' }] }
    recovered.reindex(after, { ...after, scene: { ...after.scene, pages: [original, changed] } }, new Map([[1, 1]]))
    expect(recovered.get(1)).toBeNull()
  })

  it('blocks blank, composing, malformed and over-limit UTF-8 drafts', () => {
    expect(nativeNoteTextError('   ', false)).toBe('required')
    expect(nativeNoteTextError('Привіт', true)).toBe('composing')
    expect(nativeNoteTextError('\ud800', false)).toBe('invalid')
    expect(nativeNoteTextError('ї'.repeat(NATIVE_PDF_LIMITS.noteBytes / 2), false)).toBeNull()
    expect(nativeNoteTextError('ї'.repeat(NATIVE_PDF_LIMITS.noteBytes / 2) + 'a', false)).toBe('limit')
  })

  it('translates crop-relative inputs and rotated markers without clamping invalid drafts', () => {
    expect(nativeNoteRect('10', '10', geometry, [24, 24])).toEqual([0, 30, 24, 54])
    for (const value of ['', '-1', '1.5', '177', '0x1']) {
      expect(nativeNoteRect(value, '10', geometry, [24, 24])).toBeNull()
    }
    expect([0, 90, 180, 270].map((rotation) =>
      nativeMarkerPosition({ ...geometry, rotation }, [0, 30, 24, 54]),
    )).toEqual([[11, 78], [22, 11], [89, 22], [78, 89]])
  })

  it('uses a safe filename for edited output', () => {
    expect(nativeDownloadName('..\\document<1>.PDF', true)).toBe('document_1_-edited.pdf')
    expect(nativeDownloadName('contract.pdf', false)).toBe('contract.pdf')
    expect(nativeDownloadName('-edited.pdf', true)).toBe('-edited-edited.pdf')
  })

  it('has paired, parameterized messages and defaults to Ukrainian', () => {
    expect(Object.keys(nativePdfMessages.uk).sort()).toEqual(Object.keys(nativePdfMessages.en).sort())
    for (const key of Object.keys(nativePdfMessages.uk) as (keyof typeof nativePdfMessages.uk)[]) {
      expect(key.startsWith('nativePdf.')).toBe(true)
      const parameters = (message: string) => [...message.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort()
      expect(parameters(nativePdfMessages.uk[key])).toEqual(parameters(nativePdfMessages.en[key]))
    }
    expect(nativePdfText('en', 'navigation.position', { page: 2, total: 3 })).toBe('Page 2 of 3')
    for (const locale of ['uk', 'en'] as const) {
      for (const page of [1, 2]) {
        const position = { page, total: 2 }
        expect(nativePdfText(locale, 'status.draftRestored', position))
          .toContain(nativePdfText(locale, 'navigation.position', position))
      }
      expect(nativePdfText(locale, 'status.unseen')).toContain(nativePdfText(locale, 'status.unseenNoDraft'))
      expect(nativePdfText(locale, 'status.unseenNoDraft')).not.toBe(nativePdfText(locale, 'status.unseen'))
    }
    const markup = renderToStaticMarkup(createElement(PdfNativeEditPanel, dependencies))
    expect(markup).toContain('PDF ще не відкрито')
    expect(markup).toContain('aria-live="polite"')
    expect(markup).toContain('role="alert" aria-atomic="true"')
    expect(markup).toContain('class="native-file native-primary"')
    expect(markup).not.toContain('<main')
  })

  for (const locale of ['uk', 'en'] as const) {
    it(`shows the immutable original identity after reopening verified bytes in ${locale}`, () => {
      const document = documentWith([])
      const originalStoreHash = 'b'.repeat(64)
      const markup = renderToStaticMarkup(createElement(PdfNativeEditPanel, {
        ...dependencies, locale, initialDocument: { ...document, originalStoreHash },
      }))
      expect(markup).toContain(`data-origin-hash="${originalStoreHash}"`)
      expect(markup).toContain(nativePdfText(locale, 'identity.origin', { hash: originalStoreHash }))
      expect(markup).not.toContain(nativePdfText(locale, 'identity.origin', { hash: document.snapshot.originStoreHash }))
    })

    it(`renders zero, one and many accepted notes as plain text in ${locale}`, () => {
      for (const count of [0, 1, 3]) {
        const annotations = Array.from({ length: count }, (_, index) => note(index + 2))
        const markup = renderToStaticMarkup(createElement(PdfNativeEditPanel, {
          ...dependencies, locale, initialDocument: documentWith(annotations),
        }))
        expect(markup).toContain(nativePdfText(locale, 'notes.count', { count }))
        expect(markup.includes(nativePdfText(locale, 'notes.empty'))).toBe(count === 0)
        expect(markup).toContain(nativePdfText(locale, 'download.original'))
        expect(markup).toContain(nativePdfText(locale, 'download.edited'))
        expect(markup).toContain('&lt;unsafe&gt;.pdf')
        expect(markup).toContain('class="native-file"')
        expect(markup).toContain(`data-origin-hash="${'a'.repeat(64)}"`)
        expect(markup).toContain(nativePdfText(locale, 'identity.origin', { hash: 'a'.repeat(64) }))
        expect(markup).toContain(`data-output-hash="${'2a'.repeat(32)}"`)
        expect(markup).toContain('class="native-destructive"')
        expect(markup).not.toContain('<script>')
        expect(markup.split('&lt;script&gt;')).toHaveLength(count + 1)
        expect(markup.match(/data-native-note-id=/g) ?? []).toHaveLength(count)
        expect(markup).toContain('aria-hidden="true"')
        expect(markup.indexOf(nativePdfText(locale, 'note.remove.cancel')))
          .toBeLessThan(markup.lastIndexOf(nativePdfText(locale, 'note.remove')))
      }
    })
  }

  it('renders extracted text only for the accepted output and page', () => {
    const markup = renderToStaticMarkup(createElement(PdfNativeEditPanel, {
      ...dependencies,
      locale: 'en',
      initialDocument: documentWith([]),
      pageText: [
        { outputHash: '2a'.repeat(32), pageId: 0, text: [{ value: '<b>Accepted</b>', lang: 'en' }] },
        { outputHash: '0'.repeat(64), pageId: 0, text: [{ value: 'Stale output' }] },
        { outputHash: '2a'.repeat(32), pageId: 1, text: [{ value: 'Other page' }] },
      ],
    }))
    expect(markup.split('&lt;b&gt;Accepted&lt;/b&gt;')).toHaveLength(2)
    expect(markup).not.toContain('Stale output')
    expect(markup).not.toContain('Other page')
  })
})

describe('native text replacement', () => {
  const original = '<ЇAA>'
  const island = { id: hash, text: original, bounds: [20, 100, 40, 112], profile: 'identityHCidFontType2',
    operator: 'Tj', maxAdvance: 20 } as const

  it('requires a visible single line of valid Unicode within the native byte limit', () => {
    expect(nativeReplacementError('ЇA', false)).toBeNull()
    expect(nativeReplacementError('ЇA', true)).toBe('composing')
    expect(nativeReplacementError('  ', false)).toBe('required')
    for (const input of ['A\nЇ', 'A\rЇ', 'A\u2028Ї', 'A\u2029Ї', 'A\u0000', '\ud800']) {
      expect(nativeReplacementError(input, false)).toBe('invalid')
    }
    expect(nativeReplacementError('Ї'.repeat(NATIVE_PDF_LIMITS.noteBytes / 2), false)).toBeNull()
    expect(nativeReplacementError('Ї'.repeat(NATIVE_PDF_LIMITS.noteBytes / 2 + 1), false)).toBe('limit')
  })

  it('renders verified island labels and escaped source text in both languages', () => {
    const initial = documentWith([])
    const scene = { ...initial.snapshot.scene, textPages: [{ page: 0, refusal: null, islands: [island] }] }
    for (const locale of ['uk', 'en'] as const) {
      const markup = renderToStaticMarkup(createElement(PdfNativeEditPanel, {
        ...dependencies, initialDocument: { ...initial, snapshot: { ...initial.snapshot, scene } }, locale,
      }))
      expect(markup).toContain(nativePdfText(locale, 'text.select', { number: 1 }))
      expect(markup).toContain('&lt;ЇAA&gt;')
      expect(markup).toContain('aria-pressed="false"')
      expect(markup).not.toContain(nativePdfText(locale, 'text.apply'))
    }
  })

  it('shows a read-only explanation for missing capability and every stable refusal', () => {
    const initial = documentWith([])
    for (const [code, reason] of [
      ['FLOW_PDF_NATIVE_TEXT_FONT', 'reason.font'],
      ['FLOW_PDF_NATIVE_TEXT_MAPPING', 'reason.mapping'],
      ['FLOW_PDF_NATIVE_TEXT_LAYOUT', 'reason.layout'],
      ['FLOW_PDF_NATIVE_TEXT_OWNERSHIP', 'reason.references'],
      ['FLOW_PDF_NATIVE_TEXT_OVERFLOW', 'reason.overflow'],
      [null, 'text.unavailable'],
    ] as const) {
      for (const locale of ['uk', 'en'] as const) {
        const scene = code === null ? initial.snapshot.scene : {
          ...initial.snapshot.scene, textPages: [{ page: 0, refusal: code, islands: [] }],
        }
        const markup = renderToStaticMarkup(createElement(PdfNativeEditPanel, {
          ...dependencies, initialDocument: { ...initial, snapshot: { ...initial.snapshot, scene } }, locale,
        }))
        expect(markup).toContain(nativePdfText(locale, reason))
        const description = markup.match(/<h3[^>]+aria-describedby="([^"]+)"[^>]*>/u)?.[1]
        expect(description).toBeDefined()
        expect(markup).toContain(`<p id="${description}" class="native-help native-text-feedback">`)
        expect(markup).not.toContain(nativePdfText(locale, 'text.apply'))
      }
    }
  })
})

describe('native object tools', () => {
  it('converts page-local points and rejects incomplete, excessive, or off-page geometry', () => {
    const pageBounds = [-10, 20, 190, 120] as const
    expect(nativeObjectBounds(['1.125', '2', '40', '30'], pageBounds)).toEqual([-8875, 22000, 31125, 52000])
    expect(nativeObjectBounds(['1,125', '2', '40', '30'], pageBounds)).toEqual([-8875, 22000, 31125, 52000])
    for (const draft of [
      ['', '2', '40', '30'], ['0', '0', '0', '30'], ['-1', '2', '40', '30'],
      ['1.0001', '2', '40', '30'], ['180', '2', '40', '30'], ['1,', '2', '40', '30'],
    ] as const) expect(nativeObjectBounds(draft, pageBounds)).toBeNull()
  })

  it('rebinds the exact invocation slot through verified insertions and removals', () => {
    const first = { id: hash, kind: 'graphic', profile: 'rectangle', bounds: [0, 30000, 24000, 54000],
      operations: ['replace', 'move', 'remove'] } as const
    const second = { ...first, id: hash.map(() => 43) }
    const inserted = { ...first, id: hash.map(() => 44) }
    const before = documentWith([]).snapshot
    const withObjects = (objects: readonly typeof first[]) => ({
      ...before, scene: { ...before.scene, objectPages: [{ page: 0, canAdd: true, refusal: null, objects }] },
    })
    expect(rebindNativeObject(withObjects([first]), withObjects([second, inserted]), 0, first,
      { kind: 'addImage', page: 0, bounds: first.bounds, assetHex: '00' })).toBe(second)
    expect(rebindNativeObject(withObjects([first, second]), withObjects([inserted]), 0, second,
      { kind: 'removeObject', target: { page: 0, id: first.id, profile: first.profile } })).toBe(inserted)
    expect(rebindNativeObject(withObjects([first]), withObjects([]), 0, first,
      { kind: 'removeObject', target: { page: 0, id: first.id, profile: first.profile } })).toBeUndefined()
  })

  it('keeps a recovered image draft across an unrelated revision only for a matching invocation', () => {
    const image = { id: hash, kind: 'image', profile: 'rasterGray8', bounds: [0, 0, 24000, 24000],
      operations: ['replace', 'move', 'remove'] } as const
    const revised = { ...image, id: hash.map(() => 43) }
    const snapshot = documentWith([]).snapshot
    const withObjects = (objects: NativePdfObjectPage['objects'], canAdd = true) => ({
      ...snapshot, scene: { ...snapshot.scene, objectPages: [{ page: 0, canAdd, refusal: null, objects }] },
    })
    const before = withObjects([image])
    const after = withObjects([revised])
    const draft = { profile: 'image', coordinates: ['1', '2', '30', '40'], color: '#334155', stroke: '1',
      asset: { status: 'ready', name: 'draft.png', hex: '12' } } as const
    const saved = { pageId: 0, target: { page: 0, id: image.id, profile: image.profile }, draft }
    const unrelated = { kind: 'add', page: 1, contents: 'Page two note', rect: [0, 0, 24, 24] } as const
    const rebound = rebindRecoveredObjectDraft(saved, before, after, unrelated)
    expect(rebound?.target?.id).toBe(revised.id)
    expect(rebound?.draft).toBe(draft)
    expect(rebindRecoveredObjectDraft(saved, before, withObjects([]), unrelated)).toBeNull()
    expect(rebindRecoveredObjectDraft(saved, before, withObjects([{ ...revised, bounds: [1, 0, 24000, 24000] }]), unrelated)).toBeNull()
    expect(rebindRecoveredObjectDraft(saved, before, after,
      { kind: 'replaceImage', target: saved.target, assetHex: '12' })).toBeNull()

    const newObject = { ...saved, target: null }
    expect(rebindRecoveredObjectDraft(newObject, before, after, unrelated)).toBe(newObject)
    expect(rebindRecoveredObjectDraft(newObject, before, withObjects([revised], false), unrelated)).toBeNull()

    const pageMove = { kind: 'reorderPage', target: { page: 0, stableId: 8 }, position: 1 } as const
    const moved = { ...after, scene: { ...after.scene, objectPages: [
      { page: 0, canAdd: true, refusal: null, objects: [image] },
      { page: 1, canAdd: true, refusal: null, objects: [revised] },
    ] } }
    expect(rebindRecoveredObjectDraft(saved, before, moved, pageMove, 1))
      .toEqual({ ...saved, pageId: 1, target: { ...saved.target, page: 1, id: revised.id } })
  })

  it('renders only admitted profile controls and associates each refusal with the object region', () => {
    for (const locale of ['en', 'uk'] as const) {
      const document = documentWith([])
      const markup = (refusal: NativePdfObjectPage['refusal']) => renderToStaticMarkup(createElement(PdfNativeEditPanel, {
        ...dependencies, locale, initialDocument: { ...document, snapshot: { ...document.snapshot,
          scene: { ...document.snapshot.scene, objectPages: [{ page: 0, objects: [], canAdd: !refusal, refusal }] } } },
      }))
      const admitted = markup(null)
      for (const message of ['object.profile.image', 'object.profile.line', 'object.profile.rectangle',
        'object.profile.polygon', 'object.geometry.help', 'object.file'] as const) {
        expect(admitted).toContain(nativePdfText(locale, message))
      }
      for (const [code, message] of [
        ['FLOW_PDF_NATIVE_OBJECT_STATE', 'object.reason.state'],
        ['FLOW_PDF_NATIVE_OBJECT_GEOMETRY', 'object.reason.geometry'],
        ['FLOW_PDF_NATIVE_OBJECT_ASSET', 'object.reason.asset'],
        ['FLOW_PDF_NATIVE_OBJECT_PROFILE', 'object.reason.profile'],
      ] as const) {
        const refused = markup(code)
        const description = refused.match(/class="native-stack native-object-tools"[^>]+aria-describedby="([^"]+)"/u)?.[1]
        expect(description).toBeDefined()
        expect(refused).toContain(`<p id="${description}" class="native-help">${nativePdfText(locale, message)}</p>`)
        expect(refused).not.toContain(nativePdfText(locale, 'object.profile.polygon'))
      }
    }
  })
})

describe('native page management', () => {
  for (const locale of ['en', 'uk'] as const) {
    it(`shows accepted page bounds and explains final-page deletion in ${locale}`, () => {
      const initial = documentWith([])
      const markup = renderToStaticMarkup(createElement(PdfNativeEditPanel, {
        ...dependencies, locale, initialDocument: {
          ...initial,
          snapshot: { ...initial.snapshot, pageState: { order: [42], selected: 42, canDelete: false, maxPosition: 0 } },
        },
      }))
      expect(markup).toMatch(/aria-describedby="([^"]+-final-page)"[^>]*>/)
      const reasonId = markup.match(/aria-describedby="([^"]+-final-page)"/)?.[1]
      expect(markup).toContain(`id="${reasonId}"`)
      expect(markup).toContain(nativePdfText(locale, 'page.final'))
      expect(markup).toContain(nativePdfText(locale, 'navigation.position', { page: 1, total: 1 }))
      expect(markup).toContain(nativePdfText(locale, 'page.position'))
      for (const action of ['navigation.previous', 'navigation.next', 'page.reorder', 'page.earlier', 'page.later', 'page.delete'] as const) {
        const label = nativePdfText(locale, action)
        expect(markup.match(/<button[^>]* disabled=""[^>]*>[^<]*<\/button>/gu))
          .toEqual(expect.arrayContaining([expect.stringContaining(`>${label}</button>`)]))
      }
    })
  }
})

it('renders the Rust-selected stable page after its position changes', () => {
  const initial = documentWith([])
  const first = initial.snapshot.scene.pages[0]!
  const snapshot = {
    ...initial.snapshot,
    pageState: { order: [86, 42], selected: 42, canDelete: true, maxPosition: 1 },
    scene: { ...initial.snapshot.scene, pages: [first, { ...first, id: 1, annotations: [note(2)] }] },
  }
  const markup = renderToStaticMarkup(createElement(PdfNativeEditPanel, {
    ...dependencies, locale: 'en', initialDocument: { ...initial, snapshot },
  }))
  expect(markup).toContain('Page 2 of 2')
  expect(markup).toContain('data-native-note-id="2"')
  expect(markup).not.toContain(nativePdfText('en', 'page.final'))
})

describe('native field drafts', () => {
  const field: NativePdfFieldView = {
    id: hash, object: ref, page: 0, kind: 'text', name: 'recipient',
    value: 'Ї', checked: null, rect: [0, 30, 140, 54], canEditAppearance: true, refusal: null,
  }
  const readOnlyField: NativePdfFieldView = {
    ...field, kind: null, name: 'Consent form', value: null, canEditAppearance: false,
    refusal: 'FLOW_PDF_NATIVE_FIELD_PROFILE',
  }
  const initialDocument = documentWith([])
  const binding = { ...initialDocument.snapshot.binding, revision: 0 }
  const decodeFields = (fields: readonly NativePdfFieldView[]) => readNativeResponse(JSON.stringify({
    protocolVersion: NATIVE_PDF_PROTOCOL_VERSION, operation: 'open', requestId: 1, status: 'opened',
    binding, originStoreHash: initialDocument.snapshot.originStoreHash,
    code: null, limits: NATIVE_PDF_LIMITS, resultHash: 'b'.repeat(64),
    snapshot: {
      ...initialDocument.snapshot, binding, consumedRequest: 0, fields, redactionAdmission: 'unavailable',
      pageState: { order: [1], selected: 1, canDelete: false, maxPosition: 0 },
    },
  }))

  it('checks whole-point geometry against the selected page crop', () => {
    expect(nativeFieldBounds(['10', '10', '140', '24'], geometry.bounds)).toEqual(field.rect)
    expect(nativeFieldBounds(['', '10', '140', '24'], geometry.bounds)).toBeNull()
    expect(nativeFieldBounds(['10', '10', '7', '24'], geometry.bounds)).toBeNull()
    expect(nativeFieldBounds(['10.5', '10', '140', '24'], geometry.bounds)).toBeNull()
    expect(nativeFieldBounds(['10', '10', '200', '24'], geometry.bounds)).toBeNull()
  })

  it('rebinds only the same verified field across a page operation', () => {
    const next = { ...field, id: hash.map(() => 43), page: 1 }
    expect(rebindNativeField(field, [next], 1)).toBe(next)
    expect(rebindNativeField(field, [next, { ...next, id: hash }], 1)).toBeUndefined()
    expect(rebindNativeField(field, [next], 0)).toBeUndefined()
    expect(rebindNativeField(field, [{ ...next, object: { ...ref, objectNumber: 2 } }], 1)).toBeUndefined()
    expect(rebindNativeField(field, [{ ...next, value: 'Different' }], 1)).toBeUndefined()
    expect(rebindNativeField(field, [{ ...next, canEditAppearance: false }], 1)).toBeUndefined()
  })

  it('rebinds a renumbered field only with unique verified identity on the retained page', () => {
    const next = { ...field, id: hash.map(() => 43), object: { ...ref, objectNumber: 2 }, page: 1 }
    const other = { ...next, object: { ...ref, objectNumber: 3 }, value: 'Different' }
    expect(rebindNativeField(field, [next, { ...other, page: 2 }], 1, [field])).toBe(next)
    expect(rebindNativeField(field, [next, other], 1, [field])).toBeUndefined()
    expect(rebindNativeField(field, [{ ...next, object: ref }, other], 1, [field])).toBeUndefined()
    expect(rebindNativeField(field, [next], 1, [field, { ...other, page: 0 }])).toBeUndefined()
    expect(rebindNativeField(field, [other], 1, [field])).toBeUndefined()
    expect(rebindNativeField(field, [next], 0, [field])).toBeUndefined()
    const displayOnly = { ...readOnlyField, name: field.name }
    expect(rebindNativeField(field, [next, { ...displayOnly, page: 1 }], 1, [field, displayOnly])).toBe(next)
  })

  it('rebinds a survivor after removing a sibling and rejects changed accepted state', () => {
    const sibling = { ...field, name: 'sender' }
    const before = { ...field, id: hash.map(() => 2), object: { ...ref, objectNumber: 2 } }
    const after = { ...field, id: hash.map(() => 3) }
    expect(rebindNativeField(before, [after], 0, [sibling, before])).toBe(after)
    expect(rebindNativeField(before, [{ ...after, value: 'Different' }], 0, [sibling, before])).toBeUndefined()
    expect(rebindNativeField(before, [{ ...after, canEditAppearance: false }], 0, [sibling, before])).toBeUndefined()
  })

  it('excludes display-only fields from draft rebinding', () => {
    const revised = { ...readOnlyField, id: hash.map(() => 43), page: 1 }
    expect(rebindNativeField(readOnlyField, [revised], 1, [readOnlyField])).toBeUndefined()
    const sameName = { ...revised, name: field.name }
    expect(rebindNativeField(field, [sameName], 1, [field])).toBeUndefined()
  })

  it('decodes and displays distinct read-only widget labels and refusal reasons', () => {
    const widgets = [
      readOnlyField,
      { ...readOnlyField, name: 'Згода', id: hash.map(() => 43), object: { ...ref, objectNumber: 2 } },
      {
        ...readOnlyField, name: null, id: hash.map(() => 44), object: { ...ref, objectNumber: 3 },
        refusal: 'FLOW_PDF_NATIVE_FIELD_APPEARANCE',
      },
    ] satisfies NativePdfFieldView[]
    const response = decodeFields(widgets)
    if (response?.status !== 'opened') throw Error('Expected a decoded open response')
    expect(response.snapshot.fields).toEqual(widgets)
    for (const locale of ['uk', 'en'] as const) {
      const html = renderToStaticMarkup(createElement(PdfNativeEditPanel, {
        ...dependencies, locale, initialDocument: { ...initialDocument, snapshot: response.snapshot },
      }))
      for (const name of ['Consent form', 'Згода']) {
        expect(html).toContain(nativePdfText(locale, 'field.onPage', { name, page: 1 }))
      }
      expect(html).toContain(nativePdfText(locale, 'field.widget', { object: 3, generation: 0 }))
      expect(html).toContain(nativePdfText(locale, 'field.reason.profile'))
      expect(html).toContain(nativePdfText(locale, 'field.reason.appearance'))
      for (const action of ['field.save', 'field.move', 'field.remove'] as const) {
        expect(html).not.toContain(nativePdfText(locale, action))
      }
    }
  })

  it('enforces display label bounds and keeps the stricter editable field name contract', () => {
    expect(decodeFields([{ ...readOnlyField, name: 'Ї'.repeat(32) }])?.status).toBe('opened')
    for (const name of ['', 'Ї'.repeat(33), 'name\u0000', '\uD800']) {
      expect(decodeFields([{ ...readOnlyField, name }])).toBeNull()
    }
    expect(decodeFields([{ ...readOnlyField, checked: true }])).toBeNull()
    expect(decodeFields([{ ...field, name: readOnlyField.name }])).toBeNull()
    const add = { kind: 'addTextField', page: 0, rect: field.rect, name: field.name, value: '' }
    expect(isNativeFieldCommand(add)).toBe(true)
    expect(isNativeFieldCommand({ ...add, name: readOnlyField.name })).toBe(false)
  })
})
