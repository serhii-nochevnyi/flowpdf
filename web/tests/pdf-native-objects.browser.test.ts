import { expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'

import init from '../generated/flow_wasm.js'
import '../src/styles.css'
import '../src/editor/editor.css'
import { PdfSourceStore, hashPdfSourceBytes } from '../persistence/pdf-source-store.js'
import { mountEditorApp } from '../src/editor/editor-app.js'
import { createNativePdfRuntime } from '../src/layout/layout-runtime.js'
import { nativePdfText, type NativePdfMessage } from '../src/pdf/pdf-native-edit-messages.js'
import { NATIVE_PDF_LIMITS, nativeHexBytes, type NativePdfObjectPage, type NativePdfGraphic, type NativePdfRect, type NativePdfWorkerRequest } from '../src/pdf/pdf-native-edit-protocol.js'

function objectPdf(transform = '30 0 0 40 100 120', clip = '') {
  const encoder = new TextEncoder()
  const stream = (data: string, attributes = '') => `<< /Length ${data.length}${attributes} >>\nstream\n${data}\nendstream`
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 420] /Resources 5 0 R /Contents 6 0 R >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 420] /Resources 5 0 R /Contents 7 0 R >>',
    '<< /XObject << /Shared 8 0 R >> >>',
    stream(`q ${clip}${transform} cm /Shared Do Q\n20 30 10 12 re f\n`),
    stream('q 15 0 0 18 90 90 cm /Shared Do Q\n10 12 24 26 re f\n'),
    stream('UVW', ' /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB'),
  ]
  let pdf = '%PDF-1.7\n'
  const offsets = objects.map((object, index) => {
    const offset = pdf.length
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`
    return offset
  })
  const xref = pdf.length
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`
  pdf += offsets.map((offset) => `${String(offset).padStart(10, '0')} 00000 n \n`).join('')
  return encoder.encode(`${pdf}trailer\n<< /Root 1 0 R /Size ${objects.length + 1} >>\nstartxref\n${xref}\n%%EOF\n`)
}

const grayPng = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR4nGPoAAAAigCJ/xNBzgAAAABJRU5ErkJggg=='),
  (character) => character.charCodeAt(0))

function chooseFile(input: Element, file: File) {
  if (!(input instanceof HTMLInputElement)) throw Error('Missing file input')
  const transfer = new DataTransfer()
  transfer.items.add(file)
  input.files = transfer.files
  input.dispatchEvent(new Event('change', { bubbles: true }))
}

function signatures(projection: NativePdfObjectPage | undefined) {
  return projection?.objects.map(({ kind, profile, bounds }) => ({ kind, profile, bounds }))
}

for (const locale of ['uk', 'en'] as const) {
  test(`native image and graphic commands preserve the original and shared sibling in ${locale}`, async () => {
    await page.viewport(320, 900)
    await init()
    const root = document.createElement('div')
    document.body.append(root)
    const lifecycle = new AbortController()
    const store = new PdfSourceStore(`native-objects-${crypto.randomUUID()}`)
    const nativePdf = createNativePdfRuntime(undefined, store)
    const downloads: Promise<Uint8Array<ArrayBuffer>>[] = []
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      downloads.push(fetch(this.href).then(async (response) => new Uint8Array(await response.arrayBuffer())))
    })
    let staleId: readonly number[] | undefined
    const postMessage = Worker.prototype.postMessage
    const worker = vi.spyOn(Worker.prototype, 'postMessage').mockImplementation(
      function (this: Worker, ...args: Parameters<Worker['postMessage']>) {
        const envelope = args[0] as Partial<NativePdfWorkerRequest> | undefined
        const request = envelope?.request
        if (staleId && request?.operation === 'command' && 'target' in request.command) {
          args[0] = { ...envelope, request: { ...request,
            command: { ...request.command, target: { ...request.command.target, id: staleId } },
          } }
          staleId = undefined
        }
        Reflect.apply(postMessage, this, args)
      },
    )
    const t = (key: NativePdfMessage, params?: Record<string, string | number>) => nativePdfText(locale, key, params)

    try {
      const controller = await mountEditorApp(root, { locale, databaseName: `native-objects-${crypto.randomUUID()}` },
        {}, undefined, { nativePdf, signal: lifecycle.signal })
      const semantic = controller.snapshot().accepted
      const panel = page.getByRole('region', { name: t('title'), exact: true })
      const objects = panel.getByRole('region', { name: t('object.title') })
      const selector = objects.getByRole('combobox', { name: t('object.title') })
      const profile = objects.getByRole('combobox', { name: t('object.profile.label') })
      const asset = () => objects.getByLabelText(t('object.file'), { exact: true }).element()
      const button = (key: NativePdfMessage) => panel.getByRole('button', { name: t(key), exact: true })
      const edited = button('download.edited')
      const outputHash = () => edited.element().getAttribute('data-output-hash')
      const commandCount = () => worker.mock.calls.filter(([message]) =>
        (message as Partial<NativePdfWorkerRequest> | undefined)?.request?.operation === 'command').length

      async function open(bytes: Uint8Array<ArrayBuffer>, name: string) {
        const input = root.querySelector('[data-pdf-native-edit] input[type=file]')
        if (!input) throw Error('Missing source file input')
        chooseFile(input, new File([bytes], name, { type: 'application/pdf' }))
        await expect.element(panel.getByText(name, { exact: true })).toBeVisible()
      }

      async function activate(element: Element) {
        if (!(element instanceof HTMLElement)) throw Error('Missing keyboard target')
        element.focus()
        expect(document.activeElement).toBe(element)
        await userEvent.keyboard('{Enter}')
      }

      async function geometry(values: readonly string[]) {
        for (const [index, key] of (['object.x', 'object.y', 'object.width', 'object.height'] as const).entries()) {
          const input = objects.getByRole('textbox', { name: t(key), exact: true }).element() as HTMLInputElement
          input.focus()
          input.select()
          await userEvent.keyboard(values[index]!)
        }
      }

      async function download(key: 'download.original' | 'download.edited') {
        const index = downloads.length
        await activate(button(key).element())
        await expect.poll(() => downloads.length).toBe(index + 1)
        return downloads[index]!
      }

      async function inspect(bytes: Uint8Array<ArrayBuffer>) {
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

      async function accepted(action: NativePdfMessage, expected: ReturnType<typeof signatures>, selectedIndex?: number) {
        const prior = outputHash()
        const watched = downloads.length
        if (action === 'object.remove') {
          const dialog = panel.getByRole('dialog', { name: t('object.remove') })
          await activate(dialog.getByRole('button', { name: t('object.remove') }).element())
        } else await activate(button(action).element())
        await expect.poll(outputHash).not.toBe(prior)
        expect(downloads).toHaveLength(watched)
        const bytes = await download('download.edited')
        const snapshot = await inspect(bytes)
        const objectPage = snapshot.scene.objectPages?.[0]
        expect(objectPage?.refusal).toBeNull()
        expect(signatures(objectPage)).toEqual(expected)
        expect(signatures(snapshot.scene.objectPages?.[1])).toEqual(sibling)
        const serialized = new TextDecoder().decode(bytes)
        expect(serialized).toContain('q 15 0 0 18 90 90 cm /Shared Do Q')
        expect(serialized).toContain('10 12 24 26 re f')
        expect(serialized).toMatch(/stream\r?\nUVW\r?\nendstream/u)
        expect(new Set(objectPage?.objects.map(({ id }) => id.join(','))).size).toBe(expected?.length)
        if (selectedIndex !== undefined) {
          expect((selector.element() as HTMLSelectElement).selectedIndex).toBe(selectedIndex + 1)
        }
      }

      await open(objectPdf('30 1 0 40 100 120'), 'skew.pdf')
      await expect.element(objects.getByText(t('object.reason.geometry'))).toBeVisible()
      expect(objects.getByRole('button', { name: t('object.add') }).query()).toBeNull()
      expect(await download('download.original')).toEqual(objectPdf('30 1 0 40 100 120'))

      const clipped = objectPdf(undefined, '0 0 110 140 re W n ')
      await open(clipped, 'clipped.pdf')
      await expect.element(objects.getByText(t('object.reason.state'))).toBeVisible()
      expect(selector.query()).toBeNull()
      expect(await download('download.original')).toEqual(clipped)

      const source = objectPdf()
      await open(source, 'shared.pdf')
      const original = await inspect(source)
      const sibling = signatures(original.scene.objectPages?.[1])
      const [baseImage, baseGraphic] = signatures(original.scene.objectPages?.[0]) ?? []
      expect(baseImage?.profile).toBe('rasterRgb8')
      expect(baseGraphic?.profile).toBe('rectangle')
      const image = (bounds: NativePdfRect) => ({ kind: 'image', profile: 'rasterGray8', bounds } as const)
      const graphic = (shape: NativePdfGraphic['shape'], bounds: NativePdfRect) => ({ kind: 'graphic', profile: shape, bounds } as const)
      if (!baseImage || !baseGraphic) throw Error('Missing fixture objects')
      expect(root.scrollWidth).toBeLessThanOrEqual(320)
      expect(button('object.add').element().getBoundingClientRect().height).toBeGreaterThanOrEqual(44)
      expect(selector.element().getBoundingClientRect().height).toBeGreaterThanOrEqual(44)
      expect(objects.getByRole('textbox', { name: t('object.x') }).element().getBoundingClientRect().height).toBeGreaterThanOrEqual(44)
      const before = commandCount()
      await geometry(['310', '20', '30', '40'])
      await expect.element(objects.getByText(t('object.geometry.invalid'))).toBeVisible()
      await expect.element(button('object.add')).toBeDisabled()
      await geometry(['10', '20', '30', '40'])
      chooseFile(asset(), new File([new Uint8Array(NATIVE_PDF_LIMITS.imageBytes + 1)], 'large.png'))
      await expect.element(objects.getByText(t('object.file.limit'), { exact: false })).toBeVisible()
      let finishRead!: (bytes: ArrayBuffer) => void
      const corrupt = new File([grayPng], '<invalid>.png', { type: 'image/png' })
      vi.spyOn(corrupt, 'arrayBuffer').mockReturnValue(new Promise((resolve) => { finishRead = resolve }))
      chooseFile(asset(), corrupt)
      await expect.element(objects.getByText(t('object.file.loading'), { exact: false })).toBeVisible()
      await expect.element(button('object.add')).toBeDisabled()
      finishRead(grayPng.slice(0, 33).buffer)
      await expect.element(objects.getByText(t('object.file.failed'), { exact: false })).toBeVisible()
      await expect.element(objects.getByText('<invalid>.png', { exact: true })).toBeVisible()
      expect(commandCount()).toBe(before)

      await activate(button('object.cancel').element())
      await expect.element(objects.getByRole('textbox', { name: t('object.x') })).toHaveValue('')
      await geometry(['10', '20', '30', '40'])
      chooseFile(asset(), new File([grayPng], 'gray.png', { type: 'image/png' }))
      await expect.element(objects.getByText(t('object.file.ready'), { exact: false })).toBeVisible()
      const addedImage = image([10000, 20000, 40000, 60000])
      await accepted('object.add', [addedImage, baseImage, baseGraphic], 0)

      await selector.selectOptions((selector.element() as HTMLSelectElement).options[2]!.value)
      const originalTarget = (selector.element() as HTMLSelectElement).value
      chooseFile(asset(), new File([grayPng], 'replacement.png', { type: 'image/png' }))
      await expect.element(objects.getByText(t('object.file.ready'), { exact: false })).toBeVisible()
      await accepted('object.replace', [addedImage, image([...baseImage.bounds]), baseGraphic], 1)
      await geometry([locale === 'uk' ? '105,125' : '105.125', '125', '30', '40'])
      await accepted('object.move', [addedImage, image([105125, 125000, 135125, 165000]), baseGraphic], 1)
      const beforeRefusal = await download('download.edited')
      const beforeRefusalHash = outputHash()
      expect((selector.element() as HTMLSelectElement).value).not.toBe(originalTarget)
      staleId = Array.from(nativeHexBytes(originalTarget))
      await geometry(['110', '125', '30', '40'])
      await activate(button('object.move').element())
      await expect.element(panel.getByRole('alert')).toHaveTextContent(t('error.stale'))
      expect(outputHash()).toBe(beforeRefusalHash)
      await expect.element(objects.getByRole('textbox', { name: t('object.x') })).toHaveValue('110')
      expect(await download('download.edited')).toEqual(beforeRefusal)

      async function remove(expected: ReturnType<typeof signatures>) {
        const trigger = button('object.remove').element()
        const dialog = panel.getByRole('dialog', { name: t('object.remove') })
        await activate(trigger)
        await expect.element(dialog.getByRole('button', { name: t('note.remove.cancel') })).toHaveFocus()
        const selected = (selector.element() as HTMLSelectElement).selectedOptions[0]?.textContent?.trim()
        expect(dialog.element().textContent).toContain(selected)
        const beforeCancel = outputHash()
        const commandsBeforeCancel = commandCount()
        await userEvent.keyboard('{Escape}')
        expect(document.activeElement).toBe(trigger)
        expect(outputHash()).toBe(beforeCancel)
        expect(commandCount()).toBe(commandsBeforeCancel)
        await activate(trigger)
        await accepted('object.remove', expected)
      }

      await remove([addedImage, baseGraphic])
      await profile.selectOptions('line')
      await geometry(['70', '20', '20', '30'])
      await expect.element(objects.getByText(t('object.graphic.line.help'))).toBeVisible()
      const stroke = objects.getByRole('textbox', { name: t('object.stroke') }).element() as HTMLInputElement
      stroke.focus()
      stroke.select()
      await userEvent.keyboard('32')
      await expect.element(objects.getByText(t('object.stroke.invalid'))).toBeVisible()
      await expect.element(button('object.add')).toBeDisabled()
      await profile.selectOptions('rectangle')
      const rectangle = graphic('rectangle', [70000, 20000, 90000, 50000])
      await accepted('object.add', [rectangle, addedImage, baseGraphic], 0)
      await profile.selectOptions('polygon')
      const polygon = graphic('polygon', rectangle.bounds)
      await accepted('object.replace', [polygon, addedImage, baseGraphic], 0)
      await geometry(['75', '22', '20', '30'])
      await accepted('object.move', [graphic('polygon', [75000, 22000, 95000, 52000]), addedImage, baseGraphic], 0)
      await remove([addedImage, baseGraphic])
      expect(await download('download.original')).toEqual(source)
      expect(controller.snapshot().accepted).toBe(semantic)
    } finally {
      lifecycle.abort()
      nativePdf.dispose()
      click.mockRestore()
      worker.mockRestore()
      root.remove()
    }
  }, 60_000)
}
