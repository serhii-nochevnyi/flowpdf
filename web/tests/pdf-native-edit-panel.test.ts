import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import {
  PdfNativeEditPanel,
  nativeDownloadName,
  nativeMarkerPosition,
  nativeNoteRect,
  nativeNoteTextError,
  type NativePdfAcceptedDocument,
} from '../src/pdf/pdf-native-edit-panel.js'
import { nativePdfMessages, nativePdfText } from '../src/pdf/pdf-native-edit-messages.js'
import { NATIVE_PDF_LIMITS, type NativePdfAnnotation } from '../src/pdf/pdf-native-edit-protocol.js'

const geometry = { bounds: [-10, 20, 190, 120], rotation: 90 } as const
const hash = Array<number>(32).fill(42)
const ref = { objectNumber: 1, generation: 0 }
const note = (id: number): NativePdfAnnotation => ({
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
        expect(markup).toContain(nativePdfText(locale, 'download.original'))
        expect(markup).toContain(nativePdfText(locale, 'download.edited'))
        expect(markup).toContain('&lt;unsafe&gt;.pdf')
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
