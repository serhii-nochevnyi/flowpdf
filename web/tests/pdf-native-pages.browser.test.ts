import { expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'

import init from '../generated/flow_wasm.js'
import '../src/styles.css'
import '../src/editor/editor.css'
import { hashPdfSourceBytes, PdfSourceStore } from '../persistence/pdf-source-store.js'
import { mountEditorApp } from '../src/editor/editor-app.js'
import { createNativePdfRuntime } from '../src/layout/layout-runtime.js'
import { nativePdfText, type NativePdfMessage } from '../src/pdf/pdf-native-edit-messages.js'
import { nativeHexBytes, type NativePdfWorkerRequest } from '../src/pdf/pdf-native-edit-protocol.js'

function pagePdf(count = 2, destination = false, font?: { bytes: Uint8Array<ArrayBuffer>; length: number }) {
  const kids = Array.from({ length: count }, (_, index) => `${index + 3} 0 R`).join(' ')
  const note = count + 3
  const content = note + 1
  const resources = font ? `/Resources << /Font << /Body ${content + 1} 0 R >> >> /Contents ${content} 0 R` : '/Resources <<>>'
  const objects = [
    `<< /Type /Catalog /Pages 2 0 R${destination ? ' /Dests << /Permanent [3 0 R /Fit] >>' : ''} >>`,
    `<< /Type /Pages /Kids [${kids}] /Count ${count} >>`,
    ...Array.from({ length: count }, (_, index) => index === 0
      ? `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 420] /CropBox [10 20 290 390] /Rotate 90 ${resources} /Annots [${note} 0 R] >>`
      : `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 450 600] /Resources <<>>${font && index === 1 ? ` /Contents ${content + 7} 0 R` : ''} >>`),
    '<< /Type /Annot /Subtype /Text /P 3 0 R /Rect [34 56 58 80] /Contents (Source note) >>',
  ]
  const offsets: number[] = []
  const parts: Uint8Array<ArrayBuffer>[] = []
  const encoder = new TextEncoder()
  let length = 0
  function append(value: string | Uint8Array<ArrayBuffer>) {
    const bytes = typeof value === 'string' ? encoder.encode(value) : value
    parts.push(bytes)
    length += bytes.length
  }
  function object(value: string | Uint8Array<ArrayBuffer>, attributes?: string) {
    offsets.push(length)
    append(`${offsets.length} 0 obj\n`)
    if (attributes !== undefined) append(`<< /Length ${value.length}${attributes} >>\nstream\n`)
    append(value)
    if (attributes !== undefined) append('\nendstream')
    append('\nendobj\n')
  }
  append('%PDF-1.7\n')
  for (const value of objects) object(value)
  if (font) {
    object('BT /Body 12 Tf 30 140 Td <000200010001> Tj ET', '')
    object(`<< /Type /Font /Subtype /Type0 /BaseFont /NotoSans-Regular /Encoding /Identity-H /DescendantFonts [${content + 2} 0 R] /ToUnicode ${content + 3} 0 R >>`)
    object(`<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NotoSans-Regular /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /W [1 [639 339]] /FontDescriptor ${content + 4} 0 R /CIDToGIDMap ${content + 6} 0 R >>`)
    object(`/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapName /BodyMap def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
2 beginbfchar
<0001> <0041>
<0002> <0407>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end end`, '')
    object(`<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 /FontFile2 ${content + 5} 0 R >>`)
    object(font.bytes, ` /Filter /FlateDecode /Length1 ${font.length}`)
    object(Uint8Array.of(0, 0, 0, 36, 1, 160), '')
    object('50 65 20 25 re f', '')
  }
  const xref = length
  const size = offsets.length + 1
  append(`xref\n0 ${size}\n0000000000 65535 f \n`)
  for (const offset of offsets) append(`${String(offset).padStart(10, '0')} 00000 n \n`)
  append(`trailer\n<< /Root 1 0 R /Size ${size} >>\nstartxref\n${xref}\n%%EOF\n`)
  const pdf = new Uint8Array(length)
  let offset = 0
  for (const part of parts) {
    pdf.set(part, offset)
    offset += part.length
  }
  return pdf
}

async function activate(element: Element) {
  if (!(element instanceof HTMLElement)) throw Error('Missing keyboard target')
  element.focus()
  expect(document.activeElement).toBe(element)
  await userEvent.keyboard('{Enter}')
}

for (const locale of ['en', 'uk'] as const) {
  test(`native page changes and reference refusal through the keyboard in ${locale}`, async () => {
    await page.viewport(320, 900)
    await init()
    const fontResponse = await fetch(new URL('../../crates/flow-core/data/NotoSans-Regular.ttf', import.meta.url))
    expect(fontResponse.ok).toBe(true)
    const fontBytes = await fontResponse.arrayBuffer()
    const compressed = new Blob([fontBytes]).stream().pipeThrough(new CompressionStream('deflate'))
    const font = { bytes: new Uint8Array(await new Response(compressed).arrayBuffer()), length: fontBytes.byteLength }
    const root = document.createElement('div')
    document.body.append(root)
    const lifecycle = new AbortController()
    const store = new PdfSourceStore(`native-pages-${crypto.randomUUID()}`)
    const nativePdf = createNativePdfRuntime(undefined, store)
    const downloads: Promise<Uint8Array<ArrayBuffer>>[] = []
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      downloads.push(fetch(this.href).then(async (response) => new Uint8Array(await response.arrayBuffer())))
    })
    let pauseKind: 'deletePage' | 'insertBlankPage' | 'rotatePage' | undefined
    let unseenRotation = false
    let failProjection = false
    const held: { resume?: () => void } = {}
    function release() {
      const resume = held.resume
      delete held.resume
      resume?.()
    }
    const postMessage = Worker.prototype.postMessage
    const worker = vi.spyOn(Worker.prototype, 'postMessage').mockImplementation(
      function (this: Worker, ...args: Parameters<Worker['postMessage']>) {
        const envelope = args[0] as Partial<NativePdfWorkerRequest> | undefined
        const request = envelope?.request
        if (unseenRotation && envelope && request?.operation === 'command' && request.command.kind === 'rotatePage') {
          unseenRotation = false
          queueMicrotask(() => this.dispatchEvent(new MessageEvent('message', { data: { dispatchId: envelope.dispatchId } })))
          return
        }
        if (failProjection && envelope && request?.operation === 'snapshot') {
          failProjection = false
          held.resume = () => {
            this.dispatchEvent(new MessageEvent('message', { data: { dispatchId: envelope.dispatchId } }))
          }
          return
        }
        if (pauseKind && request?.operation === 'command' && request.command.kind === pauseKind) {
          pauseKind = undefined
          held.resume = () => Reflect.apply(postMessage, this, args)
          return
        }
        Reflect.apply(postMessage, this, args)
      },
    )
    const t = (key: NativePdfMessage, parameters?: Record<string, string | number>) => nativePdfText(locale, key, parameters)

    try {
      await mountEditorApp(root, { locale, databaseName: `native-pages-${crypto.randomUUID()}` },
        {}, undefined, { nativePdf, signal: lifecycle.signal })
      const panel = page.getByRole('region', { name: t('title'), exact: true })
      const button = (key: NativePdfMessage) => panel.getByRole('button', { name: t(key), exact: true })
      const select = (key: NativePdfMessage) => panel.getByRole('combobox', { name: t(key), exact: true })
      const selector = select('navigation.select')
      const status = panel.getByRole('status')
      const dialog = panel.getByRole('dialog', { name: t('page.confirm'), exact: true })
      const edited = button('download.edited')
      let revision = 0

      async function open(bytes: Uint8Array<ArrayBuffer>, name: string) {
        const inputs = root.querySelectorAll('[data-pdf-native-edit] input[type=file][accept*="application/pdf"]')
        expect(inputs).toHaveLength(1)
        const input = inputs[0]
        if (!(input instanceof HTMLInputElement)) throw Error('Missing file input')
        const transfer = new DataTransfer()
        transfer.items.add(new File([bytes], name, { type: 'application/pdf' }))
        input.files = transfer.files
        input.dispatchEvent(new Event('change', { bubbles: true }))
        await expect.element(panel.getByText(name, { exact: true })).toBeVisible()
        revision = 0
      }

      async function choose(key: NativePdfMessage, value: string) {
        const control = select(key).element()
        if (!(control instanceof HTMLSelectElement)) throw Error('Missing native select')
        control.focus()
        expect(document.activeElement).toBe(control)
        const option = Array.from(control.options).find((entry) => entry.value === value)
        if (!option) throw Error('Missing native select option')
        await select(key).selectOptions(option)
        await expect.element(select(key)).toHaveValue(value)
      }

      async function download() {
        const index = downloads.length
        await activate(edited.element())
        await expect.poll(() => downloads.length).toBe(index + 1)
        return downloads[index]!
      }

      async function readSnapshot() {
        const bytes = await download()
        const reader = nativePdf.createCoordinator()
        try {
          const hash = (await hashPdfSourceBytes(bytes)).slice('sha256:v1:'.length)
          const snapshot = await reader.open({ bytes, originStoreHash: hash })
          expect(nativeHexBytes(snapshot.outputHex)).toEqual(bytes)
          return snapshot
        } finally {
          reader.dispose()
        }
      }

      async function readBack() {
        return (await readSnapshot()).scene.pages
      }

      async function accepted(position: number, total: number) {
        await expect.element(status).toHaveTextContent(t('page.accepted', { page: position, total, revision: ++revision }))
        await expect.element(selector).toHaveValue(String(position - 1))
      }

      async function change(key: NativePdfMessage, position: number, total: number) {
        await activate(button(key).element())
        await accepted(position, total)
      }

      async function confirmDelete(position: number, total: number) {
        await activate(button('page.delete').element())
        await expect.element(dialog).toBeVisible()
        await expect.element(dialog.getByText(t('navigation.position', { page: position, total }), { exact: true })).toBeVisible()
        await expect.element(dialog.getByText(t('page.deleteBody'), { exact: true })).toBeVisible()
        expect(document.activeElement).toBe(dialog.getByRole('button', { name: t('page.keep'), exact: true }).element())
      }

      await expect.element(selector).toBeDisabled()
      await expect.element(select('navigation.zoom')).toBeDisabled()
      await expect.element(button('navigation.fit')).toBeDisabled()
      await open(pagePdf(2, false, font), 'pages.pdf')
      await expect.element(button('navigation.previous')).toBeDisabled()
      failProjection = true
      await activate(button('navigation.next').element())
      await expect.poll(() => held.resume).toBeTypeOf('function')
      await expect.element(selector).toBeDisabled()
      await expect.element(button('navigation.fit')).toBeDisabled()
      await expect.element(status).toHaveTextContent(t('navigation.pending', { page: 2 }))
      await expect.element(panel.getByText('Source note', { exact: true })).toBeVisible()
      release()
      await expect.element(status).toHaveTextContent(t('navigation.error', { page: 2 }))
      await expect.element(selector).toHaveValue('0')
      await activate(button('navigation.retry').element())
      await expect.element(selector).toHaveValue('1')
      await expect.element(button('navigation.next')).toBeDisabled()
      await activate(button('navigation.previous').element())
      await expect.element(selector).toHaveValue('0')

      await choose('navigation.zoom', '200')
      await activate(button('navigation.fit').element())
      await expect.element(select('navigation.zoom')).toHaveValue('0')
      await choose('page.placement', 'before')
      failProjection = true
      await activate(button('navigation.next').element())
      await expect.poll(() => held.resume).toBeTypeOf('function')
      const finishSupersededProjection = held.resume
      delete held.resume
      pauseKind = 'insertBlankPage'
      await activate(button('page.insert').element())
      await expect.poll(() => held.resume).toBeTypeOf('function')
      await expect.element(selector).toBeDisabled()
      await expect.element(selector).toHaveValue('0')
      await expect.element(panel.getByText('Source note', { exact: true })).toBeVisible()
      finishSupersededProjection?.()
      await expect.element(status).toHaveTextContent(t('status.pending'))
      release()
      await accepted(1, 3)
      const inserted = await readBack()
      expect(inserted.map(({ geometry }) => geometry?.bounds)).toEqual([
        [10, 20, 290, 390], [10, 20, 290, 390], [0, 0, 450, 600],
      ])
      expect(inserted.map(({ annotations }) => annotations.length)).toEqual([0, 1, 0])

      await activate(button('navigation.next').element())
      await expect.element(selector).toHaveValue('1')
      const noteList = panel.getByRole('list', { name: t('notes.title'), exact: true })
      const noteDraft = panel.getByRole('textbox', { name: t('note.contents'), exact: true })
      const replacement = panel.getByRole('textbox', { name: t('text.replacement'), exact: true })
      const island = panel.getByRole('button', { name: t('text.select', { number: 1 }), exact: true })
      const objectSelect = select('object.title')
      const objectX = panel.getByRole('textbox', { name: t('object.x'), exact: true })
      const rotate = button('page.right')
      await activate(noteList.getByRole('button').element())
      await noteDraft.fill('Source draft')
      await expect.element(panel.getByRole('spinbutton', { name: t('geometry.x'), exact: true })).toHaveValue(24)
      await expect.element(panel.getByRole('spinbutton', { name: t('geometry.y'), exact: true })).toHaveValue(36)
      await expect.element(panel.getByText(t('geometry.help'), { exact: true })).toBeVisible()
      await activate(island.element())
      await replacement.fill('AA')

      failProjection = true
      await activate(button('navigation.next').element())
      await expect.poll(() => held.resume).toBeTypeOf('function')
      await expect.element(noteDraft).toBeDisabled()
      await expect.element(replacement).toBeDisabled()
      await expect.element(edited).toBeEnabled()
      release()
      await expect.element(status).toHaveTextContent(t('navigation.error', { page: 3 }))
      await expect.element(noteDraft).toBeEnabled()
      await expect.element(replacement).toBeEnabled()
      await expect.element(noteDraft).toHaveValue('Source draft')
      await expect.element(replacement).toHaveValue('AA')
      await activate(button('navigation.retry').element())
      await expect.element(selector).toHaveValue('2')
      await expect.element(selector).toHaveFocus()
      await activate(button('navigation.previous').element())
      await expect.element(selector).toHaveValue('1')
      await expect.element(noteDraft).toHaveValue('Source draft')
      await expect.element(replacement).toHaveValue('AA')

      pauseKind = 'rotatePage'
      await activate(rotate.element())
      await expect.poll(() => held.resume).toBeTypeOf('function')
      await expect.element(rotate).toBeDisabled()
      await expect.element(status).toHaveTextContent(t('status.pending'))
      release()
      await accepted(2, 3)
      await expect.element(rotate).toHaveFocus()
      await expect.element(replacement).toHaveValue('AA')

      unseenRotation = true
      await userEvent.keyboard('{Enter}')
      const reconcile = button('session.reconcile')
      await expect.element(reconcile).toHaveFocus()
      await userEvent.keyboard('{Enter}')
      await expect.element(status).toHaveTextContent(t('status.unseenNoDraft'))
      await expect.element(rotate).toHaveFocus()
      await userEvent.keyboard('{Enter}')
      await accepted(2, 3)
      await expect.element(rotate).toHaveFocus()
      expect((await readBack()).map(({ geometry }) => geometry?.rotation)).toEqual([90, 270, 0])
      await change('page.left', 2, 3)
      await change('page.left', 2, 3)
      expect((await readBack())[1]?.geometry?.rotation).toBe(90)

      await choose('page.position', '2')
      await change('page.reorder', 3, 3)
      expect((await readBack()).map(({ annotations }) => annotations.length)).toEqual([0, 0, 1])
      await change('page.earlier', 2, 3)
      await change('page.later', 3, 3)
      await expect.element(noteDraft).toHaveValue('Source draft')
      await expect.element(replacement).toHaveValue('AA')
      await change('page.duplicate', 4, 4)
      const duplicateSnapshot = await readSnapshot()
      const duplicated = duplicateSnapshot.scene.pages
      expect(duplicated.map(({ geometry }) => geometry?.bounds)).toEqual([
        [10, 20, 290, 390], [0, 0, 450, 600], [10, 20, 290, 390], [10, 20, 290, 390],
      ])
      for (const index of [2, 3]) {
        expect(duplicated[index]?.annotations).toMatchObject([{ contents: 'Source note', rect: [34, 56, 58, 80] }])
        expect(duplicateSnapshot.scene.textPages?.find(({ page }) => page === index)?.refusal)
          .toBe('FLOW_PDF_NATIVE_TEXT_OWNERSHIP')
      }
      expect(new TextDecoder().decode(nativeHexBytes(duplicateSnapshot.outputHex))).toContain('<000200010001> Tj')
      expect(duplicated[2]?.annotations[0]?.object).not.toEqual(duplicated[3]?.annotations[0]?.object)
      await expect.element(noteDraft).not.toBeInTheDocument()
      await expect.element(replacement).not.toBeInTheDocument()
      await activate(button('navigation.previous').element())
      await expect.element(selector).toHaveValue('2')
      await expect.element(noteDraft).toHaveValue('Source draft')
      await expect.element(replacement).not.toBeInTheDocument()
      await activate(button('navigation.next').element())
      await expect.element(selector).toHaveValue('3')
      await activate(noteList.getByRole('button').element())
      await expect.element(noteDraft).toHaveValue('Source note')
      await noteDraft.fill('Copied note')
      await activate(button('note.save').element())
      await expect.element(status).toHaveTextContent(t('status.accepted', { revision: ++revision }))
      const editedCopy = await readBack()
      expect(editedCopy[2]?.annotations).toMatchObject([{ contents: 'Source note' }])
      expect(editedCopy[3]?.annotations).toMatchObject([{ contents: 'Copied note', rect: [34, 56, 58, 80] }])
      const x = panel.getByRole('spinbutton', { name: t('geometry.x'), exact: true })
      await x.fill('50')
      await activate(button('note.move').element())
      await expect.element(status).toHaveTextContent(t('status.accepted', { revision: ++revision }))
      const movedCopy = await readBack()
      expect(movedCopy[2]?.annotations).toMatchObject([{ contents: 'Source note', rect: [34, 56, 58, 80] }])
      expect(movedCopy[3]?.geometry?.rotation).toBe(90)
      expect(movedCopy[3]?.annotations).toMatchObject([{ contents: 'Copied note', rect: [60, 56, 84, 80] }])

      await confirmDelete(4, 4)
      await userEvent.keyboard('{Escape}')
      await expect.element(dialog).not.toBeInTheDocument()
      expect(document.activeElement).toBe(button('page.delete').element())
      await confirmDelete(4, 4)
      await userEvent.keyboard('{Tab}{Enter}')
      await accepted(3, 3)
      expect(document.activeElement).toBe(selector.element())
      const restored = await readSnapshot()
      expect(restored.scene.pages[2]?.annotations).toMatchObject([{ contents: 'Source note' }])
      expect(new TextDecoder().decode(nativeHexBytes(restored.outputHex))).toContain('<000200010001> Tj')
      await expect.element(replacement).toHaveValue('AA')

      await activate(button('navigation.previous').element())
      await expect.element(selector).toHaveValue('1')
      const objectControl = objectSelect.element() as HTMLSelectElement
      await choose('object.title', objectControl.options[1]!.value)
      await expect.element(objectX).toHaveValue('50')
      await objectX.fill('47')
      await change('page.right', 2, 3)
      await expect.element(objectX).toHaveValue('47')
      await change('page.duplicate', 3, 4)
      const duplicateGraphic = await readSnapshot()
      for (const index of [1, 2]) {
        expect(duplicateGraphic.scene.objectPages?.find(({ page }) => page === index)?.objects).toMatchObject([
          { bounds: [50_000, 65_000, 70_000, 90_000] },
        ])
      }
      await expect.element(objectX).toHaveValue('')
      await activate(button('navigation.previous').element())
      await expect.element(selector).toHaveValue('1')
      await expect.element(objectX).toHaveValue('47')
      await change('page.earlier', 1, 4)
      await expect.element(objectX).toHaveValue('47')
      expect((objectSelect.element() as HTMLSelectElement).selectedIndex).toBe(1)

      await open(pagePdf(1), 'final.pdf')
      await expect.element(button('page.delete')).toBeDisabled()
      await expect.element(button('page.delete')).toHaveAccessibleDescription(t('page.final'))
      await choose('page.placement', 'after')
      await change('page.insert', 2, 2)
      expect((await readBack()).map(({ annotations }) => annotations.length)).toEqual([1, 0])

      await open(pagePdf(2, true), 'destination.pdf')
      await change('page.right', 1, 2)
      const safeBytes = await download()
      await activate(noteList.getByRole('button').element())
      const draft = panel.getByRole('textbox', { name: t('note.contents'), exact: true })
      await draft.fill('Unsubmitted draft')
      await confirmDelete(1, 2)
      pauseKind = 'deletePage'
      await userEvent.keyboard('{Tab}{Enter}')
      await expect.poll(() => held.resume).toBeTypeOf('function')
      await expect.element(selector).toBeDisabled()
      release()
      await expect.element(panel.getByRole('alert')).toHaveTextContent(t('page.unsupported'))
      await expect.element(selector).toHaveValue('0')
      await expect.element(selector).toHaveFocus()
      await expect.element(draft).toHaveValue('Unsubmitted draft')
      await expect.element(button('navigation.next')).toBeEnabled()
      expect(await download()).toEqual(safeBytes)
      await activate(button('navigation.next').element())
      await expect.element(selector).toHaveValue('1')

      await open(pagePdf(40), 'long.pdf')
      const pageSelect = selector.element()
      if (!(pageSelect instanceof HTMLSelectElement)) throw Error('Missing page select')
      expect(pageSelect.options).toHaveLength(40)
      expect(pageSelect.getBoundingClientRect().height).toBeGreaterThanOrEqual(44)
      expect(pageSelect.getBoundingClientRect().height).toBeLessThanOrEqual(320)
      await choose('navigation.select', '39')
      await expect.element(button('navigation.next')).toBeDisabled()
      expect(panel.element().scrollWidth).toBeLessThanOrEqual(panel.element().clientWidth + 1)
    } finally {
      release()
      worker.mockRestore()
      click.mockRestore()
      lifecycle.abort()
      root.remove()
      nativePdf.dispose()
    }
  }, 60_000)
}
