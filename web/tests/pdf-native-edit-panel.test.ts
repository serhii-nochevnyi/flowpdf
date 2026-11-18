import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import {
  PdfNativeEditPanel,
  nativeDownloadName,
  nativeMarkerPosition,
  nativeNoteDraftAfter,
  nativeNoteDraftAfterAcceptance,
  nativeNoteDraftForPage,
  nativeNoteRect,
  nativeNoteTextError,
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

    const removal = { ...pending, command: { kind: 'remove', note: 2 } as const }
    const removed = documentWith([other]).snapshot
    expect(nativeNoteDraftAfterAcceptance(removal, removed, 0, selected)).toBeNull()
    expect(nativeNoteDraftAfterAcceptance(removal, removed, 0, unrelated)).toBe(unrelated)
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

  it('recovers an unseen command draft only on its original page', () => {
    for (const id of [null, 2]) {
      const draft = { note: id, contents: 'Unseen', x: '17', y: '28' }
      const pending = { pageId: 1, draft }
      expect(nativeNoteDraftForPage(pending, 0)).toBeNull()
      expect(nativeNoteDraftForPage(pending, 1)).toBe(draft)
      expect(nativeNoteDraftForPage(pending, 2)).toBeNull()
      expect(nativeNoteDraftForPage(pending, 1)).toBe(draft)
    }
    expect(nativeNoteDraftForPage(null, 1)).toBeNull()
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
