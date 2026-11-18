import { expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'

import '../src/styles.css'
import '../src/editor/editor.css'
import { PdfSourceStore, hashPdfSourceBytes } from '../persistence/pdf-source-store.js'
import { mountEditorApp } from '../src/editor/editor-app.js'
import { createNativePdfRuntime } from '../src/layout/layout-runtime.js'
import { nativePdfText, type NativePdfMessage } from '../src/pdf/pdf-native-edit-messages.js'
import type { NativePdfWorkerRequest } from '../src/pdf/pdf-native-edit-protocol.js'

type Profile = 'admitted' | 'ambiguous' | 'clipped' | 'unembedded'

function textPdf(font: Uint8Array<ArrayBuffer>, profile: Profile): Uint8Array<ArrayBuffer> {
  const encoder = new TextEncoder()
  const parts: Uint8Array<ArrayBuffer>[] = []
  let offset = 0
  function append(value: string | Uint8Array<ArrayBuffer>) {
    const bytes = typeof value === 'string' ? encoder.encode(value) : value
    parts.push(bytes)
    offset += bytes.length
  }

  const offsets: number[] = []
  function object(content: string | Uint8Array<ArrayBuffer>, streamAttributes?: string) {
    offsets.push(offset)
    append(`${offsets.length} 0 obj\n`)
    if (streamAttributes !== undefined) append(`<< /Length ${content.length}${streamAttributes} >>\nstream\n`)
    append(content)
    if (streamAttributes !== undefined) append('\nendstream')
    append('\nendobj\n')
  }

  append('%PDF-1.7\n')
  object('<< /Type /Catalog /Pages 2 0 R >>')
  object('<< /Type /Pages /Kids [3 0 R] /Count 1 >>')
  object('<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 420] /Resources << /Font << /Body 5 0 R >> >> /Contents 4 0 R >>')
  const clip = profile === 'clipped' ? '0 0 200 200 re W n\n' : ''
  object(`${clip}BT /Body 12 Tf 30 140 Td <000200010001> Tj ET\nBT /Body 12 Tf 30 180 Td <00010001> Tj ET`, '')
  object('<< /Type /Font /Subtype /Type0 /BaseFont /NotoSans-Regular /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>')
  const ambiguous = profile === 'ambiguous'
  object(`<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NotoSans-Regular /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /W [1 [639 339${ambiguous ? ' 639' : ''}]] /FontDescriptor 8 0 R /CIDToGIDMap 10 0 R >>`)
  object(`/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapName /BodyMap def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
${ambiguous ? 3 : 2} beginbfchar
<0001> <0041>
<0002> <0407>
${ambiguous ? '<0003> <0041>\n' : ''}endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end end`, '')
  object(`<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32${profile === 'unembedded' ? '' : ' /FontFile2 9 0 R'} >>`)
  object(font, ` /Length1 ${font.length}`)
  object(ambiguous ? Uint8Array.of(0, 0, 0, 36, 1, 160, 0, 36) : Uint8Array.of(0, 0, 0, 36, 1, 160), '')

  const xref = offset
  const size = offsets.length + 1
  append(`xref\n0 ${size}\n0000000000 65535 f \n`)
  for (const start of offsets) append(`${String(start).padStart(10, '0')} 00000 n \n`)
  append(`trailer\n<< /Root 1 0 R /Size ${size} >>\nstartxref\n${xref}\n%%EOF\n`)
  const pdf = new Uint8Array(offset)
  let start = 0
  for (const part of parts) {
    pdf.set(part, start)
    start += part.length
  }
  return pdf
}

for (const locale of ['uk', 'en'] as const) {
  test(`native text replacement and refusals use verified PDF bytes in ${locale}`, async () => {
    await page.viewport(320, 900)
    const response = await fetch(new URL('../../crates/flow-core/data/NotoSans-Regular.ttf', import.meta.url))
    expect(response.ok).toBe(true)
    const font = new Uint8Array(await response.arrayBuffer())
    const root = document.createElement('div')
    document.body.append(root)
    const lifecycle = new AbortController()
    const store = new PdfSourceStore(`native-text-${crypto.randomUUID()}`)
    const nativePdf = createNativePdfRuntime(undefined, store)
    const postMessage = Worker.prototype.postMessage
    let pause = false
    let resume: (() => void) | undefined
    const worker = vi.spyOn(Worker.prototype, 'postMessage').mockImplementation(
      function (this: Worker, ...args: Parameters<Worker['postMessage']>) {
        const request = (args[0] as Partial<NativePdfWorkerRequest> | undefined)?.request
        if (pause && request?.operation === 'command') {
          pause = false
          resume = () => Reflect.apply(postMessage, this, args)
          return
        }
        Reflect.apply(postMessage, this, args)
      },
    )
    const downloads: { filename: string; bytes: Promise<Uint8Array<ArrayBuffer>> }[] = []
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      downloads.push({
        filename: this.download,
        bytes: fetch(this.href).then(async (result) => new Uint8Array(await result.arrayBuffer())),
      })
    })
    const t = (key: NativePdfMessage, values?: Record<string, string | number>) => nativePdfText(locale, key, values)

    try {
      const controller = await mountEditorApp(root, { locale, databaseName: `native-text-${crypto.randomUUID()}` },
        {}, undefined, { nativePdf, signal: lifecycle.signal })
      const semantic = controller.snapshot().accepted
      const panel = page.getByRole('region', { name: t('title'), exact: true })
      const button = (key: NativePdfMessage) => panel.getByRole('button', { name: t(key), exact: true })
      const island = (number: number) => panel.getByRole('button', { name: t('text.select', { number }), exact: true })
      const original = panel.getByRole('textbox', { name: t('text.original') })
      const replacement = panel.getByRole('textbox', { name: t('text.replacement') })
      const edited = button('download.edited')
      const apply = button('text.apply')
      const outputHash = () => edited.element().getAttribute('data-output-hash')
      const countCommands = () => worker.mock.calls.filter(([message]) =>
        (message as Partial<NativePdfWorkerRequest> | undefined)?.request?.operation === 'command').length
      async function activate(element: Element) {
        if (!(element instanceof HTMLElement)) throw new Error('Missing keyboard target')
        element.focus()
        expect(document.activeElement).toBe(element)
        await userEvent.keyboard('{Enter}')
      }
      async function open(bytes: Uint8Array<ArrayBuffer>, filename: string) {
        const input = root.querySelector<HTMLInputElement>('[data-pdf-native-edit] input[type=file]')
        if (!input) throw new Error('Missing native file input')
        const transfer = new DataTransfer()
        transfer.items.add(new File([bytes], filename, { type: 'application/pdf' }))
        input.files = transfer.files
        input.dispatchEvent(new Event('change', { bubbles: true }))
        await expect.element(panel.getByText(filename, { exact: true })).toBeVisible()
      }
      async function download(key: 'download.original' | 'download.edited') {
        const index = downloads.length
        await activate(button(key).element())
        await expect.poll(() => downloads.length).toBe(index + 1)
        return downloads[index]!
      }

      for (const [profile, reason] of [
        ['ambiguous', 'reason.mapping'], ['clipped', 'reason.layout'], ['unembedded', 'reason.font'],
      ] as const) {
        const unsupported = textPdf(font, profile)
        await open(unsupported, `${profile}.pdf`)
        await expect.element(panel.getByText(t(reason), { exact: false })).toBeVisible()
        await expect.element(apply).not.toBeInTheDocument()
        await expect.element(edited).toBeDisabled()
        expect(await (await download('download.original')).bytes).toEqual(unsupported)
      }
      expect(countCommands()).toBe(0)

      const source = textPdf(font, 'admitted')
      const origin = (await hashPdfSourceBytes(source)).slice('sha256:v1:'.length)
      await open(source, 'contracts.pdf')
      const initialHash = outputHash()
      await activate(island(1).element())
      await expect.element(original).toHaveValue('ЇAA')
      await expect.element(original).toHaveAttribute('readonly')
      expect(document.activeElement).toBe(replacement.element())
      await expect.element(apply).toBeDisabled()

      replacement.element().dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
      await replacement.fill('ЇA')
      await userEvent.keyboard('{Enter}')
      await expect.element(apply).toBeDisabled()
      expect(countCommands()).toBe(0)
      replacement.element().dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: 'ЇA' }))
      await expect.element(apply).toBeEnabled()
      const beforePendingDownload = downloads.length
      pause = true
      try {
        await activate(apply.element())
        await expect.poll(() => resume !== undefined).toBe(true)
        await expect.element(edited).toBeDisabled()
        await expect.element(original).toHaveValue('ЇAA')
        expect(outputHash()).toBe(initialHash)
        expect(downloads).toHaveLength(beforePendingDownload)
      } finally {
        resume?.()
      }
      await expect.element(panel.getByRole('status')).toHaveTextContent(t('status.accepted', { revision: 1 }))
      expect(outputHash()).not.toBe(initialHash)
      expect(document.activeElement).toBe(island(1).element())
      await activate(island(1).element())
      await expect.element(original).toHaveValue('ЇA')

      const first = await download('download.edited')
      expect(first.filename).toBe('contracts-edited.pdf')
      const acceptedHash = outputHash()
      for (const [draft, reason] of [['Z', 'reason.mapping'], ['Ї' + 'A'.repeat(120), 'reason.overflow']] as const) {
        await replacement.fill(draft)
        const before = countCommands()
        await activate(apply.element())
        await expect.element(panel.getByRole('alert')).toHaveTextContent(t(reason))
        expect(replacement.element().getAttribute('aria-describedby')?.split(' ').some((id) =>
          document.getElementById(id)?.textContent?.includes(t(reason)))).toBe(true)
        expect(panel.element().scrollWidth).toBeLessThanOrEqual(panel.element().clientWidth + 1)
        expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(innerWidth + 1)
        expect(countCommands()).toBe(before + 1)
        await expect.element(replacement).toHaveValue(draft)
        await expect.element(original).toHaveValue('ЇA')
        await expect.element(edited).toBeEnabled()
        expect(outputHash()).toBe(acceptedHash)
        expect(button('download.original').element().getAttribute('data-origin-hash')).toBe(origin)
        expect(await (await download('download.edited')).bytes).toEqual(await first.bytes)
      }
      const panelElement = panel.element()
      expect(panelElement.scrollWidth).toBeLessThanOrEqual(panelElement.clientWidth + 1)
      for (const control of [island(1), original, replacement, apply]) {
        const { width, height } = control.element().getBoundingClientRect()
        expect(width).toBeGreaterThanOrEqual(44)
        expect(height).toBeGreaterThanOrEqual(44)
      }

      expect(await (await download('download.original')).bytes).toEqual(source)
      expect(await store.get(origin)).toEqual(source)

      await open(await first.bytes, first.filename)
      await activate(island(1).element())
      await expect.element(original).toHaveValue('ЇA')
      await activate(island(2).element())
      await expect.element(original).toHaveValue('AA')
      await expect.element(edited).toBeDisabled()
      await replacement.fill('A')
      await activate(apply.element())
      await expect.element(panel.getByRole('status')).toHaveTextContent(t('status.accepted', { revision: 1 }))
      const derived = await download('download.edited')

      await open(await derived.bytes, derived.filename)
      await activate(island(1).element())
      await expect.element(original).toHaveValue('ЇA')
      await activate(island(2).element())
      await expect.element(original).toHaveValue('A')
      await expect.element(edited).toBeDisabled()

      expect(countCommands()).toBe(4)
      expect(controller.snapshot().accepted).toBe(semantic)
    } finally {
      lifecycle.abort()
      nativePdf.dispose()
      worker.mockRestore()
      click.mockRestore()
      root.remove()
    }
  }, 60_000)
}
