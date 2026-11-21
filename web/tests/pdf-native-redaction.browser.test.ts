import { expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'

import init, { native_pdf_close, native_pdf_open, read_pdf, verify_native_pdf_response, verify_pdf_reader_response } from '../generated/flow_wasm.js'
import '../src/styles.css'
import '../src/editor/editor.css'
import { mountEditorApp } from '../src/editor/editor-app.js'
import { createNativePdfRuntime } from '../src/layout/layout-runtime.js'
import { hashPdfSourceBytes, PdfSourceStore } from '../persistence/pdf-source-store.js'
import { nativePdfText, type NativePdfMessage } from '../src/pdf/pdf-native-edit-messages.js'
import {
  isNativeWorkerResponse, matchesNativeResponse, nativeBytesHex, nativeDispatchId, nativeHexBytes, readNativeResponse,
  NATIVE_PDF_PROTOCOL_VERSION,
  type NativePdfRedactionResult, type NativePdfRequest, type NativePdfWorkerRequest, type NativePdfWorkerResponse,
} from '../src/pdf/pdf-native-edit-protocol.js'
import { NativePdfEditCoordinator, type NativePdfWorkerPort } from '../src/pdf/pdf-native-edit-worker.js'
import { PDF_READER_PROTOCOL_VERSION, type PdfReaderResultDto } from '../src/pdf/pdf-protocol.js'

const privateNote = 'Private margin note'
const publicNote = 'Retain this note'
const metadata = 'Private document title'

async function redactionFixture(unsupported = false) {
  const response = await fetch(new URL('../../crates/flow-core/data/NotoSans-Regular.ttf', import.meta.url))
  if (!response.ok) throw Error('Fixture font unavailable')
  const font = await response.arrayBuffer()
  const compressedFont = new Uint8Array(await new Response(
    new Blob([font]).stream().pipeThrough(new CompressionStream('deflate')),
  ).arrayBuffer())
  const objects: BlobPart[] = []
  const offsets: number[] = []
  let length = 0
  function append(part: string | Uint8Array<ArrayBuffer>) {
    objects.push(part)
    length += part.length
  }
  function object(body: string | Uint8Array<ArrayBuffer>, streamEntries?: string) {
    offsets.push(length)
    append(`${offsets.length} 0 obj\n`)
    if (streamEntries !== undefined) append(`<< /Length ${body.length} ${streamEntries} >>\nstream\n`)
    append(body)
    if (streamEntries !== undefined) append('\nendstream')
    append('\nendobj\n')
  }

  append('%PDF-1.7\n')
  object('<< /Type /Catalog /Pages 2 0 R /Metadata 14 0 R >>')
  object('<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 /MediaBox [0 0 160 140] >>')
  object('<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Annots [7 0 R 8 0 R] /Resources << /XObject << /Sample 9 0 R >> /Font << /Body 10 0 R >> >> >>')
  object([
    ...(unsupported ? ['0 0 150 130 re W n'] : []),
    'BT /Body 12 Tf 25 75 Td <00020002> Tj ET',
    'BT /Body 12 Tf 105 105 Td <00010001> Tj ET',
    '1 0 0 rg 25 25 10 10 re f',
    '0 0 1 RG 0.04 w 40 52.06 m 54 52.06 l S',
    '0 g 100 25 10 10 re f',
    'q 40 0 0 10 60 35 cm /Sample Do Q',
  ].join('\n'), '')
  object('<< /Type /Page /Parent 2 0 R /Contents 6 0 R /Resources <<>> >>')
  object('0 0 1 rg 25 25 10 10 re f\n0 g 105 25 10 10 re f', '')
  object(`<< /Type /Annot /Subtype /Text /Rect [24 42 28 46] /P 3 0 R /Contents (${privateNote}) >>`)
  object(`<< /Type /Annot /Subtype /Text /Rect [115 45 125 55] /P 3 0 R /Contents (${publicNote}) >>`)
  object(Uint8Array.of(211, 88), '/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8')
  object('<< /Type /Font /Subtype /Type0 /BaseFont /NotoSans-Regular /Encoding /Identity-H /DescendantFonts [11 0 R] /ToUnicode 12 0 R >>')
  object('<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NotoSans-Regular /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /W [1 [639 339]] /FontDescriptor 13 0 R /CIDToGIDMap 16 0 R >>')
  object(`/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapType 2 def
/CMapName /FixtureMap def
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
  object('<< /Type /FontDescriptor /FontName /NotoSans-Regular /Flags 32 /FontFile2 17 0 R >>')
  object('<xmp>Private catalog metadata</xmp>', '/Type /Metadata /Subtype /XML')
  object(`<< /Title (${metadata}) >>`)
  object(Uint8Array.of(0, 0, 0, 36, 1, 160), '')
  object(compressedFont, `/Filter /FlateDecode /Length1 ${font.byteLength}`)
  const xref = length
  append(`xref\n0 ${offsets.length + 1}\n0000000000 65535 f \n`)
  for (const offset of offsets) append(`${String(offset).padStart(10, '0')} 00000 n \n`)
  append(`trailer\n<< /Root 1 0 R /Info 15 0 R /Size ${offsets.length + 1} >>\nstartxref\n${xref}\n%%EOF\n`)
  return new Uint8Array(await new Blob(objects).arrayBuffer())
}

type Action = NativePdfRedactionResult['action']
type Fault = 'failed' | 'unavailable' | 'inconclusive' | 'hash' | 'version' | 'coverage' | 'missing' | 'lostPublish' | null
const refusedLane = { failed: 'structure', unavailable: 'text', inconclusive: 'raster' } as const

function controlledWorker() {
  const worker = new Worker(new URL('../src/pdf/pdf-native-edit-worker-entry.ts', import.meta.url), { type: 'module' })
  const requests: Action[] = []
  const previewPages: number[] = []
  const dispatched = new Map<number, NativePdfRequest>()
  const rawReplies: unknown[] = []
  const syntheticRefusals = new Set<string>()
  const rawReadDispatches = new Set<number>()
  let nextRawDispatchId = 0xffffffff
  async function readRaw(request: Extract<NativePdfRequest, { operation: 'snapshot' | 'preview' }>) {
    const dispatchId = nextRawDispatchId--
    rawReadDispatches.add(dispatchId)
    port.postMessage({ dispatchId, request })
    const received = () => rawReplies.find((message) => nativeDispatchId(message) === dispatchId)
    await expect.poll(received).toBeDefined()
    const message = received()
    if (!isNativeWorkerResponse(message) || message.type !== 'response') throw Error('Missing raw readback response')
    expect(verify_native_pdf_response(message.json)).toBe(true)
    const reply = readNativeResponse(message.json)
    if (reply === null) throw Error('Invalid raw readback payload')
    expect(matchesNativeResponse(request, reply)).toBe(true)
    return reply
  }
  const control = {
    fault: null as Fault,
    refusePreview: false,
    dropMarkReplies: 0,
    hold: null as Action | null,
    resume: undefined as (() => void) | undefined,
    requests, previewPages, dispatched, rawReplies, readRaw,
    die: () => port.onerror?.(new ErrorEvent('error')),
  }
  const port: NativePdfWorkerPort = {
    onmessage: null, onerror: null, onmessageerror: null,
    postMessage(message: NativePdfWorkerRequest) {
      if (message.request.operation === 'preview') previewPages.push(message.request.pageId)
      const action = message.request.operation === 'redaction' ? message.request.action.kind : null
      if (action) requests.push(action)
      const dispatch = () => {
        dispatched.set(message.dispatchId, message.request)
        worker.postMessage(message)
      }
      if (action !== null && action === control.hold) {
        control.hold = null
        control.resume = dispatch
      } else dispatch()
    },
    terminate: () => worker.terminate(),
  }
  worker.onerror = (event) => port.onerror?.(event)
  worker.onmessageerror = (event) => port.onmessageerror?.(event)
  worker.onmessage = ({ data }: MessageEvent<NativePdfWorkerResponse>) => {
    rawReplies.push(data)
    if (rawReadDispatches.has(data.dispatchId)) return
    if (data.type !== 'response') {
      port.onmessage?.(new MessageEvent('message', { data }))
      return
    }
    const reply = readNativeResponse(data.json)
    if (reply?.status === 'preview' && control.refusePreview) {
      const refusal: NativePdfWorkerResponse = {
        dispatchId: data.dispatchId, type: 'failed', code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID',
      }
      port.onmessage?.(new MessageEvent('message', { data: refusal }))
      return
    }
    if (reply?.operation === 'redaction' && reply.status !== 'failed') {
      const result = reply.redaction
      if (result.action === 'addMark' && control.dropMarkReplies > 0) {
        control.dropMarkReplies -= 1
        return
      }
      if (result.action === 'verify' && control.fault && control.fault !== 'lostPublish') {
        const fault = control.fault
        control.fault = null
        if (fault === 'missing') return
        const candidate = result.candidate!
        const verification = result.verification!
        const negative = fault === 'failed' || fault === 'unavailable' || fault === 'inconclusive'
        const json = JSON.stringify({
          ...reply, redaction: {
            ...result,
            candidate: fault === 'hash' ? { ...candidate, hash: candidate.hash.map((byte) => byte ^ 1) }
              : fault === 'version' ? { ...candidate, validatorVersion: 2 } : candidate,
            verification: negative ? { ...verification, status: fault, [refusedLane[fault]]: { status: fault, coveredPages: 0 } }
              : fault === 'coverage' ? { ...verification, raster: { ...verification.raster, coveredPages: 0 } } : verification,
          },
        })
        // @contract: Only refusal cases and malformed responses use the synthetic verifier seam.
        syntheticRefusals.add(json)
        port.onmessage?.(new MessageEvent('message', { data: { ...data, json } }))
        return
      }
      if (result.action === 'publish' && control.fault === 'lostPublish') {
        control.fault = null
        return
      }
    }
    port.onmessage?.(new MessageEvent('message', { data }))
  }
  const coordinator = new NativePdfEditCoordinator(port, (json) => syntheticRefusals.delete(json) || verify_native_pdf_response(json), 3_500)
  return { coordinator, control }
}

function rawRedactionReplies({ dispatched, rawReplies }: ReturnType<typeof controlledWorker>['control']) {
  const seen = new Set<number>()
  const redactionReplies = rawReplies.flatMap((message) => {
    if (!isNativeWorkerResponse(message)) throw Error('Invalid raw worker envelope')
    const { dispatchId } = message
    const request = dispatched.get(dispatchId)
    if (!request || seen.has(dispatchId)) throw Error(`Unexpected worker reply for dispatch ${dispatchId}`)
    seen.add(dispatchId)
    if (request.operation !== 'redaction') return []
    const reply = message.type === 'response' ? readNativeResponse(message.json) : null
    if ((reply === null && message.type === 'response') || (reply !== null && reply.operation !== 'redaction')) {
      throw Error(`Invalid redaction reply for dispatch ${dispatchId}`)
    }
    if (reply !== null) expect(matchesNativeResponse(request, reply)).toBe(true)
    return [{ message, request, reply }]
  })
  for (const [dispatchId, request] of dispatched) {
    if (request.operation === 'redaction') expect(seen.has(dispatchId), `Missing redaction reply for dispatch ${dispatchId}`).toBe(true)
  }
  return redactionReplies
}

function assertUnpublishedReplies(replies: ReturnType<typeof rawRedactionReplies>) {
  for (const { message } of replies) {
    if (message.type === 'failed') {
      expect(Object.keys(message).sort()).toEqual(['code', 'dispatchId', 'type'])
      continue
    }
    expect(Object.keys(message).sort()).toEqual(['dispatchId', 'json', 'type'])
    const payload = JSON.parse(message.json)
    expect(payload).toMatchObject({ operation: 'redaction', snapshot: null })
    const keys = [
      'binding', 'code', 'limits', 'operation', 'originStoreHash', 'protocolVersion',
      'requestId', 'resultHash', 'snapshot', 'status',
    ]
    if (payload.status !== 'failed') {
      expect(payload.status).toBe('redaction')
      expect(payload.redaction.published).toBe(false)
      expect(Object.keys(payload.redaction).sort()).toEqual([
        'acceptedHash', 'action', 'candidate', 'markCount', 'published', 'verification',
      ])
      keys.push('redaction')
    }
    expect(Object.keys(payload).sort()).toEqual(keys.sort())
  }
}

async function activate(element: Element, key = '{Enter}') {
  if (!(element instanceof HTMLElement)) throw Error('Missing keyboard target')
  element.focus()
  expect(document.activeElement).toBe(element)
  await userEvent.keyboard(key)
}

function readScene(bytes: Uint8Array) {
  const json = read_pdf(JSON.stringify({
    protocolVersion: PDF_READER_PROTOCOL_VERSION, requestId: crypto.randomUUID(),
    bytesHex: nativeBytesHex(bytes), firstPage: 0, pageCount: 2,
  }))
  expect(verify_pdf_reader_response(json)).toBe(true)
  return (JSON.parse(json) as { result: PdfReaderResultDto }).result.scene
}

async function reopenOutput(bytes: Uint8Array) {
  const originStoreHash = (await hashPdfSourceBytes(bytes)).slice('sha256:v1:'.length)
  const request = { protocolVersion: 1, requestId: 1, originStoreHash, epoch: [...crypto.getRandomValues(new Uint8Array(16))] }
  const json = native_pdf_open(JSON.stringify({ ...request, operation: 'open', bytesHex: nativeBytesHex(bytes) }))
  expect(verify_native_pdf_response(json)).toBe(true)
  const reply = readNativeResponse(json)
  if (reply?.status !== 'opened') throw Error('Published bytes did not reopen')
  expect(nativeHexBytes(reply.snapshot.outputHex)).toEqual(bytes)
  native_pdf_close(JSON.stringify({
    protocolVersion: 1, requestId: 2, operation: 'close', originStoreHash, binding: reply.binding,
  }))
  return reply.snapshot
}

const ordinaryNote = 'Ordinary prior note'
const retainedDraft = 'Unsubmitted note draft'

for (const locale of ['en', 'uk'] as const) {
  test(`native redaction publishes only the exact locally verified PDF in ${locale}`, async () => {
    await page.viewport(320, 900)
    await init()
    const source = await redactionFixture()
    const root = document.createElement('div')
    document.body.append(root)
    const lifecycle = new AbortController()
    const store = new PdfSourceStore(`native-redaction-${crypto.randomUUID()}`)
    const nativePdf = createNativePdfRuntime(undefined, store)
    const put = vi.spyOn(store, 'put')
    const clipboard = vi.spyOn(Clipboard.prototype, 'writeText')
    const clipboardItems = vi.spyOn(Clipboard.prototype, 'write')
    const downloads: { name: string; bytes: Promise<Uint8Array<ArrayBuffer>> }[] = []
    const urls = vi.spyOn(URL, 'createObjectURL')
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      downloads.push({ name: this.download, bytes: fetch(this.href).then(async (r) => new Uint8Array(await r.arrayBuffer())) })
    })
    let lane = controlledWorker()
    const t = (key: NativePdfMessage, values?: Record<string, string | number>) => nativePdfText(locale, key, values)
    try {
      const editor = await mountEditorApp(root, { locale, databaseName: `redaction-editor-${crypto.randomUUID()}` },
        {}, undefined, { nativePdf: { ...nativePdf, createCoordinator: () => lane.coordinator }, signal: lifecycle.signal })
      const semantic = editor.snapshot().accepted
      const panel = page.getByRole('region', { name: t('title'), exact: true })
      const redaction = panel.getByRole('region', { name: t('family.redaction'), exact: true })
      const projection = panel.getByRole('region', { name: t('projection.label'), exact: true })
      const retiredPageText = 'ЇЇ'
      const retiredText = projection.getByText(retiredPageText, { exact: true })
      const retainedText = projection.getByText('AA', { exact: true })
      const button = (key: NativePdfMessage) => panel.getByRole('button', { name: t(key), exact: true })
      const redactionButton = (key: NativePdfMessage) => redaction.getByRole('button', { name: t(key), exact: true })
      const text = (key: NativePdfMessage) => redaction.getByRole('textbox', { name: t(key), exact: true })
      const status = panel.getByRole('status')
      const alert = panel.getByRole('alert')
      const check = (key: 'internal' | 'structure' | 'text' | 'raster') => redaction.getByText(t(`evidence.${key}`), { exact: true })
      const prior = button('redaction.downloadPrior')
      const newOutput = () => button('redaction.download').query()
      const marksOnPage = () => root.querySelectorAll('.native-redaction-mark')
      const visibleRaster = () => root.querySelector<HTMLCanvasElement>('.native-page-raster:not([hidden]) canvas')
      const pixelAt = (x: number, y: number) => {
        const pixels = visibleRaster()?.getContext('2d')?.getImageData(x, y, 1, 1).data
        return pixels && Array.from(pixels)
      }
      const paintAtMark = () => pixelAt(60, 220)
      const fineRuleAtMark = () => pixelAt(92, 175)
      const acceptedScope = () => lane.coordinator.snapshot().accepted
      const putOriginal = async (bytes: Uint8Array<ArrayBuffer>) => {
        const picker = root.querySelector<HTMLInputElement>('[data-pdf-native-edit] input[type=file][accept*="application/pdf"]')
        if (!picker) throw Error('Missing native PDF picker')
        const transfer = new DataTransfer()
        transfer.items.add(new File([bytes], 'contract.pdf', { type: 'application/pdf' }))
        picker.files = transfer.files
        picker.dispatchEvent(new Event('change', { bubbles: true }))
        const hash = (await hashPdfSourceBytes(bytes)).slice('sha256:v1:'.length)
        await expect.element(button('download.original')).toHaveAttribute('data-origin-hash', hash)
        await expect.element(panel.getByText('contract.pdf', { exact: true })).toBeVisible()
      }
      const getDownload = async (key: NativePdfMessage) => {
        const index = downloads.length
        await activate(button(key).element())
        await expect.poll(() => downloads.length).toBe(index + 1)
        return { name: downloads[index]!.name, bytes: await downloads[index]!.bytes }
      }
      const fillMark = async (x: number, y: number, width: number, height: number) => {
        await text('geometry.x').fill(String(x))
        await text('geometry.y').fill(String(y))
        await text('geometry.width').fill(String(width))
        await text('geometry.height').fill(String(height))
      }
      const addMark = async (count: number) => {
        const add = redactionButton('redaction.add')
        await expect.element(add, { timeout: 5_000 }).toBeEnabled()
        await activate(add.element())
        await expect.poll(() => marksOnPage().length).toBe(count)
      }
      const openConfirmation = async (kind: 'clear' | 'apply', pages: string, count = 2) => {
        const trigger = redactionButton(kind === 'clear' ? 'redaction.clear' : 'redaction.apply')
        await expect.element(trigger).toBeEnabled()
        await activate(trigger.element())
        const dialog = panel.getByRole('dialog', { name: t(kind === 'clear' ? 'redaction.clear.heading' : 'redaction.confirm.heading'), exact: true })
        await expect.element(dialog).toBeVisible()
        await expect.element(dialog.getByText(t(kind === 'clear' ? 'redaction.clear.body' : 'redaction.confirm.body'), { exact: true })).toBeVisible()
        await expect.element(dialog.getByText(t('redaction.markSummary', { count, pages }), { exact: true })).toBeVisible()
        expect(document.activeElement).toBe(dialog.getByRole('button', { name: t('redaction.keep'), exact: true }).element())
        return dialog.getByRole('button', { name: t(kind === 'clear' ? 'redaction.clear' : 'redaction.apply'), exact: true })
      }
      const runApply = async () => {
        const pages = new Intl.ListFormat(locale).format(['1', '2'])
        await activate((await openConfirmation('apply', pages)).element())
      }

      const pageSelect = panel.getByRole('combobox', { name: t('navigation.select'), exact: true })
      const selectPage = async (index: number) => {
        const option = (pageSelect.element() as HTMLSelectElement).options[index]!
        const value = option.value
        await pageSelect.selectOptions(option)
        await expect.element(pageSelect).toHaveValue(value)
      }
      const assertPreviewRefusal = async () => {
        await expect.element(panel.getByText(t('viewport.unavailable'), { exact: true })).toBeVisible()
        expect(visibleRaster()).toBeNull()
        expect(redactionButton('redaction.add').query()).toBeNull()
        expect(redactionButton('redaction.apply').query()).toBeNull()
      }

      lane.control.refusePreview = true
      await putOriginal(source)
      expect(acceptedScope()?.redactionAdmission).toBe('supported')
      await assertPreviewRefusal()
      await expect.element(button('note.add')).toBeEnabled()
      expect((await getDownload('download.original')).bytes).toEqual(source)
      const notes = panel.getByRole('textbox', { name: t('note.contents'), exact: true })
      await activate(button('note.add').element())
      await notes.fill(ordinaryNote)
      await panel.getByRole('spinbutton', { name: t('geometry.x'), exact: true }).fill('120')
      await panel.getByRole('spinbutton', { name: t('geometry.y'), exact: true }).fill('10')
      await activate(button('note.add').element())
      await expect.element(status).toHaveTextContent(t('status.accepted', { revision: 1 }))
      await assertPreviewRefusal()
      lane.control.refusePreview = false
      await activate(button('viewport.retry').element())
      await expect.poll(() => visibleRaster()?.dataset.nativePreviewPage).toBe(String(acceptedScope()!.pageState.order[0]))
      expect(button('viewport.retry').query()).toBeNull()
      await activate(button('note.add').element())
      await notes.fill(retainedDraft)
      const ordinary = acceptedScope()!
      const ordinaryBytes = nativeHexBytes(ordinary.outputHex)
      const ordinaryHash = nativeBytesHex(Uint8Array.from(ordinary.outputHash))
      await expect.poll(() => visibleRaster()?.dataset.nativePreviewHash).toBe(ordinaryHash)
      expect(visibleRaster()?.dataset.nativePreviewRevision).toBe('1')
      expect(visibleRaster()?.dataset.nativePreviewPage).toBe(String(ordinary.pageState.order[0]))
      expect(paintAtMark()).toEqual([255, 0, 0, 255])
      expect(fineRuleAtMark()).toEqual([128, 128, 255, 255])
      expect(pixelAt(92, 174)).toEqual([255, 255, 255, 255])

      await fillMark(20, 20, 0, 80)
      await expect.element(redactionButton('redaction.add')).toBeDisabled()
      await fillMark(20, 20, 59, 80)
      const x = text('geometry.x').element()
      x.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
      await expect.element(text('geometry.x')).toBeEnabled()
      await expect.element(redactionButton('redaction.add')).toBeDisabled()
      x.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '20' }))
      await addMark(1)
      await expect.element(redaction.getByText(t('redaction.marked'), { exact: true })).toBeVisible()
      expect(getComputedStyle(marksOnPage()[0]!).borderTopStyle).toBe('dashed')
      const canvas = visibleRaster()!
      const rasterBounds = canvas.getBoundingClientRect()
      const markBounds = marksOnPage()[0]!.getBoundingClientRect()
      const paintedX = rasterBounds.left + rasterBounds.width * (92.5 / canvas.width)
      const paintedY = rasterBounds.top + rasterBounds.height * (175.5 / canvas.height)
      expect(paintedX).toBeGreaterThan(markBounds.left)
      expect(paintedX).toBeLessThan(markBounds.right)
      expect(paintedY).toBeGreaterThan(markBounds.top)
      expect(paintedY).toBeLessThan(markBounds.bottom)
      expect(paintAtMark()).toEqual([255, 0, 0, 255])
      expect(fineRuleAtMark()).toEqual([128, 128, 255, 255])
      expect(newOutput()).toBeNull()
      expect(acceptedScope()).toBe(ordinary)
      const preRedaction = await getDownload('redaction.downloadPrior')
      expect(preRedaction.name).toBe('contract-pre-redaction.pdf')
      expect(preRedaction.bytes).toEqual(ordinaryBytes)

      lane.control.refusePreview = true
      await selectPage(1)
      await assertPreviewRefusal()
      await expect.element(redaction.getByText(t('redaction.marked'), { exact: true })).toBeVisible()
      await expect.element(redactionButton('redaction.clear')).toBeEnabled()
      expect(lane.control.requests).toEqual(['addMark'])
      lane.control.refusePreview = false
      await activate(button('viewport.retry').element())
      await expect.poll(() => visibleRaster()?.dataset.nativePreviewPage).toBe(String(ordinary.pageState.order[1]))
      await expect.element(redactionButton('redaction.apply')).toBeEnabled()
      await selectPage(0)
      await expect.poll(() => visibleRaster()?.dataset.nativePreviewHash).toBe(ordinaryHash)
      await expect.poll(() => visibleRaster()?.dataset.nativePreviewPage).toBe(String(ordinary.pageState.order[0]))
      await expect.element(redactionButton('redaction.apply')).toBeEnabled()

      await fillMark(120, 65, 15, 15)
      await addMark(2)
      await openConfirmation('clear', '1')
      await userEvent.keyboard('{Escape}')
      expect(document.activeElement).toBe(redactionButton('redaction.clear').element())
      expect(marksOnPage()).toHaveLength(2)
      await activate((await openConfirmation('clear', '1')).element())
      await expect.poll(() => marksOnPage().length).toBe(0)
      await expect.element(status).toHaveTextContent(t('redaction.empty'))
      await expect.element(status).toHaveAttribute('data-native-redaction-status', 'neutral')
      expect(acceptedScope()).toBe(ordinary)

      await fillMark(20, 20, 59, 80)
      await addMark(1)
      await activate(redactionButton('redaction.removeMark').element())
      await expect.poll(() => marksOnPage().length).toBe(0)
      await fillMark(20, 20, 59, 80)
      lane.control.dropMarkReplies = 2
      await activate(redactionButton('redaction.add').element())
      const checkResult = button('session.reconcile')
      await expect.element(checkResult, { timeout: 6_000 }).toBeVisible()
      const previewCount = lane.control.previewPages.length
      await expect.element(pageSelect).toBeEnabled()
      const pageSelector = pageSelect.element()
      pageSelector.focus()
      expect(document.activeElement).toBe(pageSelector)
      await selectPage(1)
      await expect.element(panel.getByText(t('navigation.position', { page: 2, total: 2 }), { exact: true }).first()).toBeVisible()
      await assertPreviewRefusal()
      expect(lane.control.previewPages).toHaveLength(previewCount)
      await activate(checkResult.element())
      await expect.element(button('session.reopen'), { timeout: 6_000 }).toBeEnabled()
      await expect.element(alert).toHaveTextContent(t('session.redactionRecovery'))
      expect(acceptedScope()).toBe(ordinary)
      expect(lane.coordinator.outputBytes()).toBeNull()
      expect(newOutput()).toBeNull()
      await assertPreviewRefusal()
      await activate(checkResult.element())
      await expect.poll(() => visibleRaster()?.dataset.nativePreviewPage).toBe(String(ordinary.pageState.order[1]))
      expect(visibleRaster()?.dataset.nativePreviewRevision).toBe('1')
      expect(visibleRaster()?.dataset.nativePreviewHash).toBe(ordinaryHash)
      expect(lane.control.previewPages.slice(previewCount)).toEqual([ordinary.pageState.order[1]])
      expect(button('session.reopen').query()).toBeNull()
      await expect.element(redactionButton('redaction.apply')).toBeEnabled()
      expect(paintAtMark()).toEqual([0, 0, 255, 255])
      await expect.element(status).toHaveAttribute('data-native-redaction-status', 'neutral')
      await fillMark(20, 20, 20, 20)
      await addMark(1)
      await selectPage(0)
      await expect.element(notes).toHaveValue(retainedDraft)

      await openConfirmation('apply', new Intl.ListFormat(locale).format(['1', '2']))
      await userEvent.keyboard('{Enter}')
      expect(document.activeElement).toBe(redactionButton('redaction.apply').element())
      expect(lane.control.requests).not.toContain('stage')
      await expect.element(retiredText).toBeVisible()
      await expect.element(retainedText).toBeVisible()

      const checkpoint = () => ({ downloads: downloads.length, urls: urls.mock.calls.length })
      async function assertQuarantine(previous: ReturnType<typeof checkpoint>) {
        expect(newOutput()).toBeNull()
        expect(downloads).toHaveLength(previous.downloads)
        expect(urls).toHaveBeenCalledTimes(previous.urls)
        expect(clipboard).not.toHaveBeenCalled()
        expect(clipboardItems).not.toHaveBeenCalled()
        expect(put).toHaveBeenCalledExactlyOnceWith(ordinary.originStoreHash, source)
        expect(editor.snapshot().accepted).toBe(semantic)
        expect(acceptedScope()).toBe(ordinary)
        const exposed = lane.coordinator.outputBytes()
        expect(exposed === null || nativeBytesHex(exposed) === ordinary.outputHex).toBe(true)
        expect(marksOnPage()).toHaveLength(1)
        expect(visibleRaster()?.dataset.nativePreviewHash).toBe(ordinaryHash)
        expect(paintAtMark()).toEqual([255, 0, 0, 255])
        await expect.element(panel.getByText(privateNote, { exact: true })).toBeVisible()
        await expect.element(notes).toHaveValue(retainedDraft)
        expect(prior.element().getAttribute('data-output-hash')).toBe(nativeBytesHex(Uint8Array.from(ordinary.outputHash)))
        expect(status.element().textContent).not.toContain(t('redaction.success'))
      }
      async function assertCheck(key: 'internal' | 'structure' | 'text' | 'raster', value: NativePdfMessage) {
        await expect.poll(() => check(key).query()?.nextElementSibling?.textContent).toBe(t(value))
      }

      lane.control.hold = 'verify'
      const cancelling = checkpoint()
      await runApply()
      await expect.poll(() => lane.control.resume).toBeTypeOf('function')
      await expect.element(status).toHaveTextContent(t('redaction.pending'))
      await assertCheck('internal', 'evidence.passed')
      for (const key of ['structure', 'text', 'raster'] as const) await assertCheck(key, 'evidence.pending')
      await assertQuarantine(cancelling)
      const cancel = redactionButton('redaction.cancel')
      await expect.element(cancel).toHaveFocus()
      await userEvent.keyboard('{Enter}')
      lane.control.resume!()
      await expect.element(alert).toHaveTextContent(t('redaction.cancelled'))
      expect(lane.control.requests).toContain('cancel')
      expect(lane.control.requests).not.toContain('publish')
      await assertQuarantine(cancelling)

      for (const fault of ['failed', 'unavailable', 'inconclusive', 'hash', 'version', 'coverage', 'missing'] as const) {
        lane.control.fault = fault
        const evidence = checkpoint()
        await runApply()
        await expect.element(alert, { timeout: 6_000 }).toHaveTextContent(t('redaction.noOutput', {
          reason: t(fault === 'failed' ? 'redaction.failed' : fault === 'unavailable' ? 'redaction.unavailable' : 'redaction.inconclusive'),
        }))
        if (fault === 'failed' || fault === 'unavailable' || fault === 'inconclusive') {
          await assertCheck(refusedLane[fault], `evidence.${fault}`)
        } else {
          for (const key of ['structure', 'text', 'raster'] as const) await assertCheck(key, 'evidence.inconclusive')
        }
        await assertQuarantine(evidence)
        if (fault === 'hash' || fault === 'version' || fault === 'coverage' || fault === 'missing') {
          await activate(button('session.reconcile').element())
          await expect.poll(() => lane.coordinator.snapshot().phase).toBe('ready')
          await assertQuarantine(evidence)
        }
        expect(lane.control.requests).not.toContain('publish')
        await expect.element(panel.getByText(t('redaction.prior', { revision: 1 }), { exact: true })).toBeVisible()
      }
      expect((await getDownload('redaction.downloadPrior')).bytes).toEqual(ordinaryBytes)

      const readbackIdentity = {
        protocolVersion: NATIVE_PDF_PROTOCOL_VERSION,
        requestId: 0xffffffff,
        binding: ordinary.binding,
        originStoreHash: ordinary.originStoreHash,
      } as const
      const snapshotRequest = { ...readbackIdentity, operation: 'snapshot' } as const
      const previewRequest = {
        ...readbackIdentity, operation: 'preview', outputHash: ordinary.outputHash, pageId: ordinary.pageState.order[0]!,
      } as const
      const priorSnapshot = await lane.control.readRaw(snapshotRequest)
      expect(priorSnapshot).toMatchObject({ status: 'unseen' })
      if (priorSnapshot.operation !== 'snapshot' || priorSnapshot.status === 'failed') throw Error('Missing accepted snapshot')
      expect(priorSnapshot.snapshot).toEqual(ordinary)
      const priorPreview = await lane.control.readRaw(previewRequest)
      expect(priorPreview).toMatchObject({ status: 'preview', snapshot: null })

      lane.control.hold = 'publish'
      lane.control.resume = undefined
      if (locale === 'uk') lane.control.fault = 'lostPublish'
      const beforePublish = checkpoint()
      await runApply()
      await expect.poll(() => lane.control.resume).toBeTypeOf('function')
      await assertQuarantine(beforePublish)
      const assertTextQuarantine = async () => {
        const unavailable = locale === 'uk' ? 'Вилучення тексту для цієї сторінки недоступне.'
          : 'Text extraction is unavailable for this page.'
        await expect.element(projection.getByText(unavailable, { exact: true })).toBeVisible()
        await expect.element(projection).toHaveAttribute('aria-busy', 'false')
        expect(projection.element().textContent).toBe(unavailable)
      }
      await assertTextQuarantine()
      for (const key of ['internal', 'structure', 'text', 'raster'] as const) await assertCheck(key, 'evidence.passed')
      await expect.element(redactionButton('redaction.cancel')).toBeDisabled()
      const unpublished = rawRedactionReplies(lane.control)
      assertUnpublishedReplies(unpublished)
      const verified = unpublished.at(-1)!
      expect(verified.request.action.kind).toBe('verify')
      expect(verified.reply).toMatchObject({ status: 'redaction', redaction: { verification: { status: 'passed' } } })
      expect(await lane.control.readRaw(snapshotRequest)).toEqual(priorSnapshot)
      expect(await lane.control.readRaw(previewRequest)).toEqual(priorPreview)
      expect(lane.control.resume).toBeTypeOf('function')
      lane.control.resume!()
      if (locale === 'uk') {
        await expect.element(button('session.reconcile'), { timeout: 6_000 }).toBeVisible()
        await assertQuarantine(beforePublish)
        await assertTextQuarantine()
        await activate(button('session.reconcile').element())
      }
      await expect.element(status).toHaveTextContent(t('redaction.success'))
      await expect.element(retainedText).toBeVisible()
      await expect.element(projection).toHaveAttribute('aria-busy', 'false')
      expect(projection.element().textContent).not.toContain(retiredPageText)
      expect(status.element().hasAttribute('data-native-redaction-status')).toBe(false)
      const publication = rawRedactionReplies(lane.control).slice(unpublished.length)
      expect(publication.map(({ request }) => request.action.kind)).toEqual(
        locale === 'uk' ? ['publish', 'status', 'readPublished'] : ['publish'],
      )
      const published = publication[0]?.reply
      const accepted = publication.at(-1)?.reply
      if (published?.status !== 'published' || accepted?.status !== 'published') throw Error('Missing published worker snapshot')
      for (const { request } of publication) {
        expect(request).toEqual({
          ...verified.request, action: { kind: request.action.kind, candidate: published.redaction.candidate },
        })
      }
      if (locale === 'uk') {
        expect(publication[1]?.reply).toMatchObject({
          status: 'redaction', snapshot: null, redaction: { action: 'status', published: true },
        })
      }
      expect(accepted.snapshot).toEqual(published.snapshot)
      const affectedPages = ordinary.pageState.order
      expect(accepted.redaction.candidate?.affectedPages).toEqual(affectedPages)
      expect(accepted.redaction.verification).toEqual({
        status: 'passed', structure: { status: 'passed', coveredPages: 2 },
        text: { status: 'passed', coveredPages: 2 }, raster: { status: 'passed', coveredPages: 2 },
      })
      expect(acceptedScope()?.binding.revision).toBe(2)
      expect(marksOnPage()).toHaveLength(0)
      expect(panel.getByText(privateNote, { exact: true }).query()).toBeNull()
      expect(notes.query()).toBeNull()
      await expect.element(panel.getByText(publicNote, { exact: true })).toBeVisible()
      await expect.element(panel.getByText(ordinaryNote, { exact: true })).toBeVisible()
      expect(prior.query()).toBeNull()
      const result = await getDownload('redaction.download')
      expect(result.name).toBe('contract-redacted.pdf')
      expect(nativeBytesHex(result.bytes)).toBe(accepted.snapshot.outputHex)
      const reopened = await reopenOutput(result.bytes)
      const hash = nativeBytesHex(Uint8Array.from(reopened.outputHash))
      expect(hash).toBe(nativeBytesHex(Uint8Array.from(accepted.redaction.candidate!.hash)))
      expect(button('redaction.download').element().getAttribute('data-output-hash')).toBe(hash)
      expect(button('redaction.download').element().getAttribute('data-validator-version')).toBe('1')
      await expect.poll(() => visibleRaster()?.dataset.nativePreviewHash).toBe(hash)
      expect(visibleRaster()?.dataset.nativePreviewRevision).toBe('2')
      expect(paintAtMark()).toEqual([255, 255, 255, 255])
      expect(fineRuleAtMark()).toEqual([255, 255, 255, 255])
      await expect.element(panel.getByText(t('redaction.outputIdentity', { revision: 2, hash }), { exact: true })).toBeVisible()
      expect(reopened.scene.pages.flatMap((page) => page.annotations.flatMap((entry) =>
        entry.kind === 'plainNote' ? [entry.contents] : [],
      ))).toEqual([publicNote, ordinaryNote])
      const scene = readScene(result.bytes)
      const first = scene.pages[0]!.elements
      expect(first.flatMap((entry) => entry.kind === 'text' ? [entry.value.text] : [])).toEqual(['AA'])
      expect(first.flatMap((entry) => entry.kind === 'image' ? [entry.value.dataHex] : [])).toEqual(['0058'])
      const serialized = new TextDecoder().decode(result.bytes)
      for (const marker of ['/Info', '/Metadata', metadata, 'Private catalog metadata']) expect(serialized).not.toContain(marker)
      expect((await getDownload('download.original')).bytes).toEqual(source)
      const original = readScene(source)
      expect(original.pages[0]!.elements.flatMap((entry) => entry.kind === 'text' ? [entry.value.text] : [])).toEqual(['ЇЇ', 'AA'])
      expect(original.pages[0]!.elements.flatMap((entry) => entry.kind === 'image' ? [entry.value.dataHex] : [])).toEqual(['d358'])
      for (const [index, before] of original.pages.entries()) {
        const paths = before.elements.flatMap((entry) => entry.kind === 'path' ? [entry.value.rect] : [])
        const remaining = scene.pages[index]!.elements.flatMap((entry) => entry.kind === 'path' ? [entry.value.rect] : [])
        expect(paths).toHaveLength(index === 0 ? 3 : 2)
        expect(remaining).toEqual(paths.slice(-1))
      }
      expect(panel.element().scrollWidth).toBeLessThanOrEqual(panel.element().clientWidth)

      lane = controlledWorker()
      await putOriginal(await redactionFixture(true))
      await expect.element(redaction.getByText(t('redaction.documentUnavailable'), { exact: true })).toBeVisible()
      await expect.element(panel.getByText(t('viewport.unavailable'), { exact: true })).toBeVisible()
      expect(visibleRaster()).toBeNull()
      expect(redactionButton('redaction.add').query()).toBeNull()
      expect(redactionButton('redaction.apply').query()).toBeNull()
      expect(marksOnPage()).toHaveLength(0)
      expect(newOutput()).toBeNull()
      await expect.element(button('note.add')).toBeEnabled()

      for (const held of ['stage', 'verify'] as const) {
        lane = controlledWorker()
        await putOriginal(source)
        await fillMark(20, 20, 59, 80)
        await addMark(1)
        const refused = checkpoint()
        const sourceWrites = put.mock.calls.length
        const beforeLoss = acceptedScope()
        lane.control.hold = held
        await activate((await openConfirmation('apply', '1', 1)).element())
        await expect.poll(() => lane.control.resume).toBeTypeOf('function')
        await expect.element(status).toHaveTextContent(t(held === 'stage' ? 'redaction.staging' : 'redaction.pending'))
        if (held === 'stage') {
          expect(panel.element().querySelector('[data-redaction-check]')).toBeNull()
        } else {
          await assertCheck('internal', 'evidence.passed')
          for (const key of ['structure', 'text', 'raster'] as const) await assertCheck(key, 'evidence.pending')
        }
        lane.control.die()
        await expect.element(button('session.reopen')).toBeVisible()
        await expect.element(alert).toHaveTextContent(t('redaction.noOutput', {
          reason: t(held === 'stage' ? 'redaction.stageUnavailable' : 'redaction.inconclusive'),
        }))
        if (held === 'stage') {
          expect(panel.element().querySelector('[data-redaction-check]')).toBeNull()
        } else {
          await assertCheck('internal', 'evidence.passed')
          for (const key of ['structure', 'text', 'raster'] as const) await assertCheck(key, 'evidence.inconclusive')
        }
        await expect.element(redaction.getByText(t('redaction.marked'), { exact: true })).toBeVisible()
        expect(visibleRaster()).not.toBeNull()
        expect(redactionButton('redaction.apply').query()).toBeNull()
        expect(newOutput()).toBeNull()
        expect(marksOnPage()).toHaveLength(1)
        expect(acceptedScope()).toBe(beforeLoss)
        expect(downloads).toHaveLength(refused.downloads)
        expect(urls).toHaveBeenCalledTimes(refused.urls)
        expect(put).toHaveBeenCalledTimes(sourceWrites)
        expect(lane.control.requests).not.toContain('publish')
        assertUnpublishedReplies(rawRedactionReplies(lane.control))
      }

      lane = controlledWorker()
      await putOriginal(source)
      await fillMark(20, 20, 59, 80)
      await addMark(1)
      expect(panel.element().querySelector('[data-redaction-check]')).toBeNull()
      const pendingConfirmation = await openConfirmation(locale === 'en' ? 'clear' : 'apply', '1', 1)
      await expect.element(pendingConfirmation).toBeEnabled()
      lane.control.die()
      const reopen = button('session.reopen')
      await expect.element(reopen).toHaveFocus()
      expect(panel.element().querySelector('dialog[open]')).toBeNull()
      expect(marksOnPage()).toHaveLength(1)
      expect(visibleRaster()).not.toBeNull()
      expect(lane.control.requests).toEqual(['addMark'])
      assertUnpublishedReplies(rawRedactionReplies(lane.control))
      lane = controlledWorker()
      await reopen.click()
      await expect.element(status).toHaveTextContent(t('status.reopened'))
      expect(marksOnPage()).toHaveLength(0)
      expect(clipboard).not.toHaveBeenCalled()
      expect(clipboardItems).not.toHaveBeenCalled()
    } finally {
      lifecycle.abort()
      lane.coordinator.dispose()
      nativePdf.dispose()
      root.remove()
      vi.restoreAllMocks()
    }
  }, 90_000)
}
