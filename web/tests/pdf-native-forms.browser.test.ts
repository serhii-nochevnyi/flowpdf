import { expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'

import init from '../generated/flow_wasm.js'
import '../src/styles.css'
import '../src/editor/editor.css'
import { hashPdfSourceBytes, PdfSourceStore } from '../persistence/pdf-source-store.js'
import { mountEditorApp } from '../src/editor/editor-app.js'
import { createNativePdfRuntime } from '../src/layout/layout-runtime.js'
import { nativePdfText, type NativePdfMessage } from '../src/pdf/pdf-native-edit-messages.js'
import { nativeHexBytes } from '../src/pdf/pdf-native-edit-protocol.js'

function pdfFixture(profile: 'plain' | 'embedded' | 'unsupported' | 'unsupportedFont', font?: { bytes: Uint8Array<ArrayBuffer>; length: number }) {
  const encoder = new TextEncoder()
  const buffers: Uint8Array<ArrayBuffer>[] = []
  let length = 0
  function append(value: string | Uint8Array<ArrayBuffer>) {
    const bytes = typeof value === 'string' ? encoder.encode(value) : value
    buffers.push(bytes)
    length += bytes.length
  }
  const offsets: number[] = []
  function object(value: string | Uint8Array<ArrayBuffer>, entries?: string) {
    offsets.push(length)
    append(`${offsets.length} 0 obj\n`)
    if (entries !== undefined) append(`<< /Length ${value.length} ${entries} >>\nstream\n`)
    append(value)
    if (entries !== undefined) append('\nendstream')
    append('\nendobj\n')
  }

  const hasForm = profile !== 'plain'
  const hasWidget = profile === 'embedded' || profile === 'unsupported'
  append('%PDF-1.7\n')
  object(`<< /Type /Catalog /Pages 2 0 R${hasForm ? ' /AcroForm 5 0 R' : ''} >>`)
  object('<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 500] >>')
  object(`<< /Type /Page /Parent 2 0 R /Resources <<>>${hasWidget ? ' /Annots [6 0 R]' : ''} >>`)
  object('<< /Type /Page /Parent 2 0 R /Resources <<>> >>')
  if (profile === 'unsupported') {
    object('<< /Fields [6 0 R] /NeedAppearances true >>')
    object('<< /Type /Annot /Subtype /Widget /FT /Ch /Ff 131072 /T (region) /Opt [(North) (South)] /V (North) /P 3 0 R /Rect [10 10 150 40] >>')
  } else if (profile === 'unsupportedFont') {
    object('<< /Fields [] /DR << /Font << /FlowField 6 0 R >> >> >>')
    object('<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>')
  } else if (profile === 'embedded') {
    if (!font) throw Error('Missing embedded font')
    object('<< /Fields [6 0 R] /DR << /Font << /FlowField 7 0 R >> >> >>')
    object('<< /Type /Annot /Subtype /Widget /FT /Tx /T (recipient) /V () /DV () /P 3 0 R /Rect [10 10 150 40] /AP << /N 13 0 R >> >>')
    object('<< /Type /Font /Subtype /Type0 /BaseFont /NotoSans-Regular /Encoding /Identity-H /DescendantFonts [8 0 R] /ToUnicode 9 0 R >>')
    object('<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NotoSans-Regular /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /W [1 [339]] /FontDescriptor 10 0 R /CIDToGIDMap 12 0 R >>')
    object(`/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapType 2 def
/CMapName /FieldMap def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
1 beginbfchar
<0001> <0407>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end end`, '')
    object('<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 /FontFile2 11 0 R >>')
    object(font.bytes, `/Filter /FlateDecode /Length1 ${font.length}`)
    object(Uint8Array.of(0, 0, 1, 160), '')
    object('q 1 g 0 0 140 30 re f 0 G 1 w 1 1 138 28 re S\nBT /FlowField 10 Tf 0 g 2 4 Td <> Tj ET\nQ\n',
      '/Type /XObject /Subtype /Form /BBox [0 0 140 30] /Resources << /Font << /FlowField 7 0 R >> >>')
  }

  const xref = length
  const size = offsets.length + 1
  append(`xref\n0 ${size}\n0000000000 65535 f \n`)
  for (const offset of offsets) append(`${String(offset).padStart(10, '0')} 00000 n \n`)
  append(`trailer\n<< /Root 1 0 R /Size ${size} >>\nstartxref\n${xref}\n%%EOF\n`)
  const bytes = new Uint8Array(length)
  let offset = 0
  for (const buffer of buffers) {
    bytes.set(buffer, offset)
    offset += buffer.length
  }
  return bytes
}

async function activate(element: Element, key = '{Enter}') {
  if (!(element instanceof HTMLElement)) throw Error('Missing keyboard target')
  element.focus()
  expect(document.activeElement).toBe(element)
  await userEvent.keyboard(key)
}

for (const locale of ['en', 'uk'] as const) {
  test(`native PDF fields keep verified values, appearances and page ownership in ${locale}`, async () => {
    await page.viewport(320, 900)
    await init()
    const root = document.createElement('div')
    document.body.append(root)
    const lifecycle = new AbortController()
    const store = new PdfSourceStore(`native-fields-${crypto.randomUUID()}`)
    const nativePdf = createNativePdfRuntime(undefined, store)
    const downloads: Promise<Uint8Array<ArrayBuffer>>[] = []
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      downloads.push(fetch(this.href).then(async (response) => new Uint8Array(await response.arrayBuffer())))
    })
    const t = (key: NativePdfMessage, parameters?: Record<string, string | number>) => nativePdfText(locale, key, parameters)
    const longName = 'recipient_' + 'a'.repeat(54)
    let revision = 0

    try {
      await mountEditorApp(root, { locale, databaseName: `native-fields-${crypto.randomUUID()}` },
        {}, undefined, { nativePdf, signal: lifecycle.signal })
      const panel = page.getByRole('region', { name: t('title'), exact: true })
      const fields = panel.getByRole('region', { name: t('field.title'), exact: true })
      const button = (key: NativePdfMessage) => panel.getByRole('button', { name: t(key), exact: true })
      const select = (key: NativePdfMessage) => panel.getByRole('combobox', { name: t(key), exact: true })
      const input = (key: NativePdfMessage) => fields.getByRole('textbox', { name: t(key), exact: true })
      const number = (key: NativePdfMessage) => fields.getByRole('spinbutton', { name: t(key), exact: true })
      const status = panel.getByRole('status')
      const fieldDialog = panel.getByRole('dialog', { name: t('field.remove'), exact: true })

      async function open(bytes: Uint8Array<ArrayBuffer>, name: string) {
        const picker = root.querySelector('[data-pdf-native-edit] input[type=file][accept*="application/pdf"]')
        if (!(picker instanceof HTMLInputElement)) throw Error('Missing file input')
        const transfer = new DataTransfer()
        transfer.items.add(new File([bytes], name, { type: 'application/pdf' }))
        picker.files = transfer.files
        picker.dispatchEvent(new Event('change', { bubbles: true }))
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

      async function getDownload(key: 'download.edited' | 'download.original' = 'download.edited') {
        const index = downloads.length
        await activate(button(key).element())
        await expect.poll(() => downloads.length).toBe(index + 1)
        return downloads[index]!
      }

      async function reopen() {
        const bytes = await getDownload()
        const reader = nativePdf.createCoordinator()
        try {
          const hash = (await hashPdfSourceBytes(bytes)).slice('sha256:v1:'.length)
          const snapshot = await reader.open({ bytes, originStoreHash: hash })
          expect(nativeHexBytes(snapshot.outputHex)).toEqual(bytes)
          return {
            bytes, pdf: new TextDecoder().decode(bytes), pages: snapshot.scene.pages, fields: snapshot.fields ?? [],
          }
        } finally {
          reader.dispose()
        }
      }

      async function mutate(key: NativePdfMessage, pageChange?: { page: number; total: number }) {
        await expect.element(button(key)).toBeEnabled()
        await activate(button(key).element())
        await expect.element(status).toHaveTextContent(t(pageChange ? 'page.accepted' : 'status.accepted', {
          ...pageChange, revision: ++revision,
        }))
      }

      async function confirmRemoval(name: string, number: number) {
        await activate(button('field.remove').element())
        await expect.element(fieldDialog).toBeVisible()
        await expect.element(fieldDialog.getByText(t('field.onPage', { name, page: number }), { exact: true })).toBeVisible()
        await expect.element(fieldDialog.getByText(t('field.removeBody'), { exact: true })).toBeVisible()
        expect(document.activeElement).toBe(fieldDialog.getByRole('button', { name: t('field.keep'), exact: true }).element())
      }

      const plain = pdfFixture('plain')
      await open(plain, 'plain-fields.pdf')
      await expect.element(button('field.add')).toBeDisabled()
      await input('field.name').fill(longName)
      await input('field.value').fill('Alice')
      await mutate('field.add')
      expect(document.activeElement).toBe(select('field.select').element())
      await expect.element(fields.getByText(t('field.owned'), { exact: true })).toBeVisible()
      const originalTextKey = (select('field.select').element() as HTMLSelectElement).value

      const value = input('field.value').element()
      value.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
      await input('field.value').fill('Bob')
      await expect.element(button('field.save')).toBeDisabled()
      await userEvent.keyboard('{Enter}')
      await expect.element(fields.getByText(t('field.accepted', { value: 'Alice' }), { exact: true })).toBeVisible()
      value.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: 'Bob' }))
      await mutate('field.save')
      expect(document.activeElement).toBe(button('field.save').element())

      await choose('field.page', '1')
      await number('field.x').fill('30')
      await number('field.y').fill('40')
      await input('field.value').fill('Carol')
      await mutate('field.move')
      expect(document.activeElement).toBe(button('field.move').element())
      await expect.element(input('field.value')).toHaveValue('Carol')
      await expect.element(fields.getByText(t('field.accepted', { value: 'Bob' }), { exact: true })).toBeVisible()
      await mutate('field.save')

      await choose('field.select', '')
      await choose('field.type', 'checkbox')
      await input('field.name').fill('approved')
      const checked = fields.getByRole('checkbox', { name: t('field.checked'), exact: true })
      await mutate('field.add')
      await expect.element(checked).not.toBeChecked()
      const unchecked = await reopen()
      expect(unchecked.fields).toContainEqual(expect.objectContaining({ name: 'approved', checked: false, canEditAppearance: true }))
      expect(unchecked.pdf).toMatch(/\/AS\s+\/Off\b/u)
      expect(unchecked.pdf).toMatch(/\/V\s+\/Off\b/u)
      await activate(checked.element(), ' ')
      await mutate('field.save')
      await choose('field.page', '1')
      await number('field.x').fill('60')
      await number('field.y').fill('80')
      await number('field.width').fill('24')
      await mutate('field.move')

      const first = await reopen()
      expect(first.fields).toMatchObject([
        { name: longName, page: 1, value: 'Carol', rect: [30, 40, 150, 64], canEditAppearance: true },
        { name: 'approved', page: 1, checked: true, rect: [60, 80, 84, 104], canEditAppearance: true },
      ])
      expect(first.pdf).toContain('<4361726F6C> Tj')
      expect(first.pdf).toMatch(/\/AS\s+\/Yes\b/u)
      expect(first.pdf).toMatch(/\/V\s+\/Yes\b/u)
      expect(first.pdf).toMatch(/\/AP\s*<<\s*\/N\s*<<\s*\/Off\s+\d+ 0 R\s+\/Yes\s+\d+ 0 R/u)
      expect(await getDownload('download.original')).toEqual(plain)

      await choose('navigation.select', '1')
      await choose('field.select', originalTextKey)
      await input('field.value').fill('Draft')
      await confirmRemoval(longName, 2)
      await userEvent.keyboard('{Escape}')
      expect(document.activeElement).toBe(button('field.remove').element())
      await mutate('page.duplicate', { page: 3, total: 3 })
      const rotate = button('page.right').element()
      const statusElement = status.element()
      await confirmRemoval(longName, 2)
      if (!(rotate instanceof HTMLButtonElement)) throw Error('Missing rotate action')
      rotate.click()
      const rotatedStatus = t('page.accepted', { page: 3, total: 3, revision: ++revision })
      await expect.poll(() => statusElement.textContent).toContain(rotatedStatus)
      await activate(fieldDialog.getByRole('button', { name: t('field.remove'), exact: true }).element())
      await expect.element(panel.getByRole('alert')).toHaveTextContent(t('error.stale'))
      expect(document.activeElement).toBe(button('field.remove').element())
      await expect.element(input('field.value')).toHaveValue('Draft')
      await expect.element(fields.getByText(t('field.accepted', { value: 'Carol' }), { exact: true })).toBeVisible()
      const duplicated = await reopen()
      expect(duplicated.fields).toHaveLength(4)
      const copiedText = duplicated.fields.find((field) => field.page === 2 && field.kind === 'text')!
      const copiedCheck = duplicated.fields.find((field) => field.page === 2 && field.kind === 'checkbox')!
      expect(copiedText.name).not.toBe(longName)
      expect(copiedCheck).toMatchObject({ checked: true, canEditAppearance: true })
      await choose('field.select', `${copiedText.object.objectNumber}:${copiedText.object.generation}`)
      await input('field.value').fill('Copy')
      await mutate('field.save')
      const changedCopy = await reopen()
      expect(changedCopy.fields).toEqual(expect.arrayContaining([
        expect.objectContaining({ name: longName, page: 1, value: 'Carol' }),
        expect.objectContaining({ name: copiedText.name, page: 2, value: 'Copy' }),
      ]))

      await choose('field.select', originalTextKey)
      await expect.element(input('field.value')).toHaveValue('Draft')
      await choose('field.select', `${copiedCheck.object.objectNumber}:${copiedCheck.object.generation}`)
      await confirmRemoval(copiedCheck.name!, 3)
      await activate(fieldDialog.getByRole('button', { name: t('field.remove'), exact: true }).element())
      await expect.element(status).toHaveTextContent(t('status.accepted', { revision: ++revision }))
      expect(document.activeElement).toBe(select('field.select').element())
      expect((await reopen()).fields.map((field) => field.name)).not.toContain(copiedCheck.name)

      await activate(button('page.delete').element())
      const pageDialog = panel.getByRole('dialog', { name: t('page.confirm'), exact: true })
      await activate(pageDialog.getByRole('button', { name: t('page.delete'), exact: true }).element())
      await expect.element(status).toHaveTextContent(t('page.accepted', { page: 2, total: 2, revision: ++revision }))
      const finalPlain = await reopen()
      expect(finalPlain.pages).toHaveLength(2)
      expect(finalPlain.fields.map((field) => [field.name, field.page])).toEqual([[longName, 1], ['approved', 1]])
      const retainedText = finalPlain.fields.find((field) => field.name === longName)!
      await choose('field.select', `${retainedText.object.objectNumber}:${retainedText.object.generation}`)
      await expect.element(fields.getByRole('paragraph').filter({ hasText: t('field.onPage', { name: longName, page: 2 }) })).toBeVisible()
      expect(fields.element().scrollWidth).toBeLessThanOrEqual(fields.element().clientWidth + 1)
      expect(panel.element().scrollWidth).toBeLessThanOrEqual(panel.element().clientWidth + 1)
      await input('field.value').fill('Retained draft')
      await choose('navigation.select', '0')
      await activate(button('page.delete').element())
      await activate(pageDialog.getByRole('button', { name: t('page.delete'), exact: true }).element())
      await expect.element(status).toHaveTextContent(t('page.accepted', { page: 1, total: 1, revision: ++revision }))
      const afterBlankDeletion = await reopen()
      const renumberedText = afterBlankDeletion.fields.find((field) => field.name === longName)!
      expect(renumberedText.object).not.toEqual(retainedText.object)
      expect(renumberedText).toMatchObject({ page: 0, value: 'Carol', canEditAppearance: true })
      await expect.element(select('field.select')).toHaveValue(`${renumberedText.object.objectNumber}:${renumberedText.object.generation}`)
      await expect.element(input('field.value')).toHaveValue('Retained draft')
      await expect.element(fields.getByText(t('field.accepted', { value: 'Carol' }), { exact: true })).toBeVisible()

      const retainedCheck = afterBlankDeletion.fields.find((field) => field.name === 'approved')!
      await choose('field.select', `${retainedCheck.object.objectNumber}:${retainedCheck.object.generation}`)
      await confirmRemoval('approved', 1)
      await activate(fieldDialog.getByRole('button', { name: t('field.remove'), exact: true }).element())
      await expect.element(status).toHaveTextContent(t('status.accepted', { revision: ++revision }))
      const onlyText = (await reopen()).fields
      expect(onlyText).toMatchObject([{ name: longName, value: 'Carol', page: 0 }])
      await input('field.name').fill('staged')
      await input('field.value').fill('Unsubmitted')
      await choose('field.select', `${onlyText[0]!.object.objectNumber}:${onlyText[0]!.object.generation}`)
      await confirmRemoval(longName, 1)
      await activate(fieldDialog.getByRole('button', { name: t('field.remove'), exact: true }).element())
      await expect.element(status).toHaveTextContent(t('status.accepted', { revision: ++revision }))
      await expect.element(select('field.select')).toHaveValue('')
      expect(document.activeElement).toBe(select('field.select').element())
      await expect.element(input('field.name')).toHaveValue('staged')
      await expect.element(input('field.value')).toHaveValue('Unsubmitted')
      expect((await reopen()).fields).toEqual([])

      const fontResponse = await fetch(new URL('../../crates/flow-core/data/NotoSans-Regular.ttf', import.meta.url))
      expect(fontResponse.ok).toBe(true)
      const fontBytes = await fontResponse.arrayBuffer()
      const compressed = new Blob([fontBytes]).stream().pipeThrough(new CompressionStream('deflate'))
      const embedded = pdfFixture('embedded', {
        bytes: new Uint8Array(await new Response(compressed).arrayBuffer()), length: fontBytes.byteLength,
      })
      await open(embedded, 'embedded-font.pdf')
      await choose('field.select', '6:0')
      await input('field.value').fill('Ї')
      await mutate('field.save')
      const unicode = await reopen()
      expect(unicode.fields).toMatchObject([{ name: 'recipient', value: 'Ї', canEditAppearance: true }])
      expect(unicode.pdf).toContain('<0001> Tj')
      await input('field.value').fill('Ж')
      await activate(button('field.save').element())
      await expect.element(panel.getByRole('alert')).toHaveTextContent('FLOW_PDF_NATIVE_FIELD_GLYPH')
      await expect.element(fields.getByText(t('field.reason.glyph'), { exact: true })).toBeVisible()
      await expect.element(input('field.value')).toHaveValue('Ж')
      await expect.element(fields.getByText(t('field.accepted', { value: 'Ї' }), { exact: true })).toBeVisible()
      expect(await getDownload()).toEqual(unicode.bytes)
      expect(await getDownload('download.original')).toEqual(embedded)
      await activate(button('field.cancel').element())
      await expect.element(input('field.value')).toHaveValue('Ї')

      await open(pdfFixture('unsupportedFont'), 'unsupported-font.pdf')
      await choose('field.type', 'checkbox')
      await input('field.name').fill('approved')
      await mutate('field.add')
      const beforeAppearanceRefusal = await getDownload()
      await choose('field.select', '')
      await input('field.name').fill('recipient')
      await input('field.value').fill('Alice')
      await activate(button('field.add').element())
      await expect.element(panel.getByRole('alert')).toHaveTextContent('FLOW_PDF_NATIVE_FIELD_APPEARANCE')
      await expect.element(fields.getByText(t('field.reason.appearance'), { exact: true })).toBeVisible()
      expect(await getDownload()).toEqual(beforeAppearanceRefusal)

      const unsupported = pdfFixture('unsupported')
      await open(unsupported, 'read-only-widget.pdf')
      await choose('field.select', '6:0:0')
      await expect.element(fields.getByText(t('field.readOnly', { reason: t('field.reason.profile') }), { exact: true })).toBeVisible()
      expect(fields.getByRole('button', { name: t('field.save'), exact: true }).query()).toBeNull()
      expect(fields.getByRole('button', { name: t('field.remove'), exact: true }).query()).toBeNull()
      await choose('field.select', '')
      await input('field.name').fill('new_field')
      await activate(button('field.add').element())
      await expect.element(panel.getByRole('alert')).toHaveTextContent('FLOW_PDF_NATIVE_FIELD_PROFILE')
      await expect.element(input('field.name')).toHaveValue('new_field')
      await expect.element(button('download.edited')).toBeDisabled()
      expect(await getDownload('download.original')).toEqual(unsupported)
    } finally {
      lifecycle.abort()
      click.mockRestore()
      nativePdf.dispose()
      store.close()
      root.remove()
    }
  }, 60_000)
}
