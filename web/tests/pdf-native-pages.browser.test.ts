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

function pagePdf(count = 2, destination = false) {
  const kids = Array.from({ length: count }, (_, index) => `${index + 3} 0 R`).join(' ')
  const note = count + 3
  const objects = [
    `<< /Type /Catalog /Pages 2 0 R${destination ? ' /Dests << /Permanent [3 0 R /Fit] >>' : ''} >>`,
    `<< /Type /Pages /Kids [${kids}] /Count ${count} >>`,
    ...Array.from({ length: count }, (_, index) => index === 0
      ? `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 420] /CropBox [10 20 290 390] /Rotate 90 /Resources <<>> /Annots [${note} 0 R] >>`
      : '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 450 600] /Resources <<>> >>'),
    '<< /Type /Annot /Subtype /Text /P 3 0 R /Rect [34 56 58 80] /Contents (Source note) >>',
  ]
  let pdf = '%PDF-1.7\n'
  const offsets = objects.map((object, index) => {
    const offset = pdf.length
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`
    return offset
  })
  const xref = pdf.length
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`
  for (const offset of offsets) pdf += `${String(offset).padStart(10, '0')} 00000 n \n`
  return new TextEncoder().encode(`${pdf}trailer\n<< /Root 1 0 R /Size ${objects.length + 1} >>\nstartxref\n${xref}\n%%EOF\n`)
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
    const root = document.createElement('div')
    document.body.append(root)
    const lifecycle = new AbortController()
    const store = new PdfSourceStore(`native-pages-${crypto.randomUUID()}`)
    const nativePdf = createNativePdfRuntime(undefined, store)
    const downloads: Promise<Uint8Array<ArrayBuffer>>[] = []
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      downloads.push(fetch(this.href).then(async (response) => new Uint8Array(await response.arrayBuffer())))
    })
    let pauseKind: 'deletePage' | 'insertBlankPage' | undefined
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

      async function readBack() {
        const bytes = await download()
        const reader = nativePdf.createCoordinator()
        try {
          const hash = (await hashPdfSourceBytes(bytes)).slice('sha256:v1:'.length)
          const snapshot = await reader.open({ bytes, originStoreHash: hash })
          expect(nativeHexBytes(snapshot.outputHex)).toEqual(bytes)
          return snapshot.scene.pages
        } finally {
          reader.dispose()
        }
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
      await open(pagePdf(), 'pages.pdf')
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
      await change('page.right', 2, 3)
      expect((await readBack()).map(({ geometry }) => geometry?.rotation)).toEqual([90, 180, 0])
      await change('page.left', 2, 3)
      expect((await readBack())[1]?.geometry?.rotation).toBe(90)
      const noteList = panel.getByRole('list', { name: t('notes.title'), exact: true })
      await activate(noteList.getByRole('button').element())
      await expect.element(panel.getByRole('spinbutton', { name: t('geometry.x'), exact: true })).toHaveValue(24)
      await expect.element(panel.getByRole('spinbutton', { name: t('geometry.y'), exact: true })).toHaveValue(36)
      await expect.element(panel.getByText(t('geometry.help'), { exact: true })).toBeVisible()

      await choose('page.position', '2')
      await change('page.reorder', 3, 3)
      expect((await readBack()).map(({ annotations }) => annotations.length)).toEqual([0, 0, 1])
      await change('page.earlier', 2, 3)
      await change('page.later', 3, 3)
      await change('page.duplicate', 4, 4)
      const duplicated = await readBack()
      expect(duplicated.map(({ geometry }) => geometry?.bounds)).toEqual([
        [10, 20, 290, 390], [0, 0, 450, 600], [10, 20, 290, 390], [10, 20, 290, 390],
      ])
      for (const index of [2, 3]) {
        expect(duplicated[index]?.annotations).toMatchObject([{ contents: 'Source note', rect: [34, 56, 58, 80] }])
      }
      expect(duplicated[2]?.annotations[0]?.object).not.toEqual(duplicated[3]?.annotations[0]?.object)
      await activate(noteList.getByRole('button').element())
      await panel.getByRole('textbox', { name: t('note.contents'), exact: true }).fill('Copied note')
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
      expect((await readBack())[2]?.annotations).toMatchObject([{ contents: 'Source note' }])

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
