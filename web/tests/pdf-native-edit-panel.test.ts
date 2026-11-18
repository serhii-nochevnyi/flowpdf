import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import {
  NativeNoteDraftRecovery,
  PdfNativeEditPanel,
  nativeDownloadName,
  nativeMarkerPosition,
  nativeNoteDraftAfter,
  nativeNoteDraftAfterAcceptance,
  nativeNoteRect,
  nativeNoteTextError,
  nativeReplacementError,
  type NativePdfAcceptedDocument,
} from '../src/pdf/pdf-native-edit-panel.js'
import { nativePdfMessages, nativePdfText } from '../src/pdf/pdf-native-edit-messages.js'
import { NATIVE_PDF_LIMITS, type NativePdfAnnotation } from '../src/pdf/pdf-native-edit-protocol.js'

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
