import { expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'

import '../src/styles.css'
import '../src/editor/editor.css'
import init, {
  native_pdf_open, native_pdf_close, native_pdf_command, verify_native_pdf_response,
} from '../generated/flow_wasm.js'
import { hashPdfSourceBytes, PdfSourceStore } from '../persistence/pdf-source-store.js'
import { mountEditorApp } from '../src/editor/editor-app.js'
import { createNativePdfRuntime } from '../src/layout/layout-runtime.js'
import { nativePdfText, type NativePdfLocale, type NativePdfMessage } from '../src/pdf/pdf-native-edit-messages.js'
import {
  nativeBytesHex, readNativeResponse,
  type NativePdfSnapshot, type NativePdfWorkerRequest,
} from '../src/pdf/pdf-native-edit-protocol.js'

function classicPdf(): Uint8Array<ArrayBuffer> {
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>',
  ]
  let pdf = '%PDF-1.7\n'
  const offsets = objects.map((object, i) => {
    const offset = pdf.length
    pdf += `${i + 1} 0 obj\n${object}\nendobj\n`
    return offset
  })
  const xref = pdf.length
  pdf += 'xref\n0 4\n0000000000 65535 f \n'
  for (const offset of offsets) pdf += `${String(offset).padStart(10, '0')} 00000 n \n`
  pdf += `trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`
  return new TextEncoder().encode(pdf)
}

function rustReply(json: string) {
  expect(verify_native_pdf_response(json)).toBe(true)
  const response = readNativeResponse(json)
  if (!response) throw new Error('Rust response failed decoding')
  return response
}

async function readPdf(bytes: Uint8Array, checkRefusals = false): Promise<NativePdfSnapshot> {
  const originStoreHash = (await hashPdfSourceBytes(bytes)).slice('sha256:v1:'.length)
  const opened = rustReply(native_pdf_open(JSON.stringify({
    operation: 'open', protocolVersion: 1, requestId: 1, originStoreHash,
    epoch: Array.from(crypto.getRandomValues(new Uint8Array(16))), bytesHex: nativeBytesHex(bytes),
  })))
  if (opened.status !== 'opened') throw new Error(`Rust could not reopen downloaded PDF: ${opened.status}`)
  const snapshot = opened.snapshot
  const identity = { protocolVersion: 1, originStoreHash, binding: snapshot.binding }
  try {
    if (checkRefusals) {
      const command = {
        ...identity, operation: 'command', requestId: 2, outputHash: snapshot.outputHash,
        command: { kind: 'add', page: 0, rect: [1, 2, 25, 26], contents: 'Discard this candidate' },
      }
      for (const refused of [
        { ...command, originStoreHash: '00'.repeat(32) },
        { ...command, binding: { ...identity.binding, revision: 99 } },
        { ...command, command: { ...command.command, page: 99 } },
      ]) {
        const reply = rustReply(native_pdf_command(JSON.stringify(refused)))
        expect(reply.status).toBe('failed')
        expect(reply.snapshot).toBeNull()
      }
    }
    return snapshot
  } finally {
    expect(rustReply(native_pdf_close(JSON.stringify({
      ...identity, operation: 'close', requestId: 3,
    }))).status).toBe('closed')
  }
}

function dispatchFile(input: HTMLInputElement, bytes: Uint8Array<ArrayBuffer>, filename: string, wait?: Promise<void>) {
  const file = new File([bytes], filename, { type: 'application/pdf' })
  if (wait) {
    const read = file.arrayBuffer.bind(file)
    file.arrayBuffer = async () => {
      await wait
      return read()
    }
  }
  Object.defineProperty(input, 'files', { configurable: true, value: [file] })
  input.dispatchEvent(new Event('change', { bubbles: true }))
}

async function activate(button: HTMLElement) {
  button.focus()
  expect(document.activeElement).toBe(button)
  await userEvent.keyboard('{Enter}')
}

function required<T extends Element>(parent: ParentNode, selector: string): T {
  const element = parent.querySelector<T>(selector)
  if (!element) throw new Error(`Missing element ${selector}`)
  return element
}

function checkResponsive(panel: HTMLElement) {
  expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(innerWidth + 1)
  expect(panel.scrollWidth).toBeLessThanOrEqual(panel.clientWidth + 1)
  for (const control of panel.querySelectorAll<HTMLElement>('button, input, select, textarea, summary')) {
    if (control.getClientRects().length === 0) continue
    const target = control.getBoundingClientRect()
    expect(target.width).toBeGreaterThanOrEqual(44)
    expect(target.height).toBeGreaterThanOrEqual(44)
  }
}

for (const locale of ['uk', 'en'] satisfies NativePdfLocale[]) {
  test(`native note downloads round-trip through Chromium, the production worker and Rust in ${locale}`, async () => {
    await init()
    const initialWidth = locale === 'uk' ? 320 : 1280
    await page.viewport(initialWidth, 900)
    const root = document.createElement('div')
    const shortcutBoundary = document.createElement('div')
    shortcutBoundary.append(root)
    document.body.append(shortcutBoundary)
    const lifecycle = new AbortController()
    const store = new PdfSourceStore(`flowpdf-native-tracer-${crypto.randomUUID()}`)
    const nativePdf = createNativePdfRuntime(undefined, store)
    const postMessage = Worker.prototype.postMessage
    let pauseNextCommand = false
    let resumeCommand: (() => void) | undefined
    const post = vi.spyOn(Worker.prototype, 'postMessage').mockImplementation(
      function (this: Worker, ...args: Parameters<Worker['postMessage']>) {
        const [message] = args
        if (pauseNextCommand &&
          (message as Partial<NativePdfWorkerRequest> | undefined)?.request?.operation === 'command') {
          pauseNextCommand = false
          resumeCommand = () => Reflect.apply(postMessage, this, args)
          return
        }
        Reflect.apply(postMessage, this, args)
      },
    )
    const downloads: { filename: string; bytes: Promise<Uint8Array<ArrayBuffer>> }[] = []
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      expect(this.href).toMatch(/^blob:/)
      downloads.push({
        filename: this.download,
        bytes: fetch(this.href).then(async (response) => {
          expect(response.headers.get('Content-Type')).toBe('application/pdf')
          return new Uint8Array(await response.arrayBuffer())
        }),
      })
    })
    const t = (key: NativePdfMessage, parameters?: Record<string, string | number>) => nativePdfText(locale, key, parameters)
    try {
      const controller = await mountEditorApp(root, { locale, databaseName: `native-tracer-${crypto.randomUUID()}` },
        {}, undefined, { nativePdf, signal: lifecycle.signal })
      const semanticDocument = controller.snapshot().accepted
      const panelLocator = page.getByRole('region', { name: t('title'), exact: true })
      await expect.element(panelLocator).toHaveAttribute('lang', locale)
      const panel = required<HTMLElement>(root, '[data-pdf-native-edit]')
      const control = (key: NativePdfMessage) => panelLocator.getByRole('button', { name: t(key), exact: true })
      const button = (key: NativePdfMessage) => control(key).element() as HTMLButtonElement
      const input = required<HTMLInputElement>(panel, 'input[type=file]')
      const edited = button('download.edited')
      const original = button('download.original')
      const notes = () => [...panel.querySelectorAll<HTMLButtonElement>('[data-native-note-id]')]
      const status = required<HTMLElement>(panel, '[role=status]')
      const initial = classicPdf()
      const sourceHash = (await hashPdfSourceBytes(initial)).slice('sha256:v1:'.length)
      const filename = `contract-${'довгий'.repeat(26)}.pdf`
      const firstText = `Узгодити пункт договору ${'Додаток'.repeat(30)}`
      let semanticShortcut = false
      shortcutBoundary.addEventListener('keydown', (event) => {
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z') semanticShortcut = true
      })

      await expect.element(control('download.edited')).toBeDisabled()
      dispatchFile(input, initial, filename)
      await expect.element(control('note.add')).toBeEnabled()
      expect(notes()).toHaveLength(0)
      expect(original.dataset.originHash).toBe(sourceHash)
      const originOutputHash = edited.dataset.outputHash
      expect(originOutputHash).toMatch(/^[a-f0-9]{64}$/)
      const hashes = new Set([originOutputHash])

      async function readEdited(revision: number) {
        await expect.poll(() => status.textContent, { timeout: 10_000 })
          .toContain(t(revision < 5 ? 'status.accepted' : 'note.removed', { revision }))
        const before = downloads.length
        await activate(edited)
        await expect.poll(() => downloads.length).toBe(before + 1)
        const downloaded = downloads[before]!
        expect(downloaded.filename).toMatch(/-edited\.pdf$/)
        const bytes = await downloaded.bytes
        expect(bytes).not.toEqual(initial)
        const reopened = await readPdf(bytes, revision === 1)
        expect(reopened.outputHex).toBe(nativeBytesHex(bytes))
        const outputHash = nativeBytesHex(Uint8Array.from(reopened.outputHash))
        expect(outputHash).toBe(edited.dataset.outputHash)
        expect(hashes.has(outputHash)).toBe(false)
        hashes.add(outputHash)
        expect(original.dataset.originHash).toBe(sourceHash)
        return reopened.scene.pages[0]!.annotations
      }

      await activate(button('note.add'))
      const contents = required<HTMLTextAreaElement>(panel, 'textarea')
      expect(document.activeElement).toBe(contents)
      contents.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
      await panelLocator.getByRole('textbox', { name: t('note.contents') }).fill(firstText)
      await expect.element(control('note.add')).toBeDisabled()
      contents.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: firstText }))
      await expect.element(control('note.add')).toBeEnabled()
      await activate(button('note.add'))
      const [first] = await readEdited(1)
      expect(first).toMatchObject({ kind: 'plainNote', contents: firstText, rect: [0, 0, 24, 24] })
      const firstButton = notes()[0]!
      const firstId = firstButton.dataset.nativeNoteId
      expect(firstButton.querySelector('.native-note-text')?.textContent).toBe(firstText)
      expect(panel.querySelectorAll('.native-note-text')).toHaveLength(1)
      expect(panel.querySelector('.native-paper')?.getAttribute('aria-hidden')).toBe('true')
      expect(document.activeElement).toBe(edited)

      await activate(button('note.add'))
      expect(document.activeElement).toBe(required(panel, 'textarea'))
      await userEvent.keyboard('Confirm delivery date')
      const firstAcceptedHash = edited.dataset.outputHash
      const beforePending = downloads.length
      pauseNextCommand = true
      await activate(button('note.add'))
      try {
        await expect.poll(() => resumeCommand !== undefined).toBe(true)
        await expect.element(control('download.edited')).toBeDisabled()
        expect(input.disabled).toBe(true)
        expect(edited.dataset.outputHash).toBe(firstAcceptedHash)
        expect(notes()).toHaveLength(1)
        expect(downloads).toHaveLength(beforePending)
      } finally {
        resumeCommand?.()
      }
      const twoNotes = await readEdited(2)
      expect(twoNotes).toHaveLength(2)
      expect(twoNotes).toContainEqual(first)
      expect(twoNotes).toEqual(expect.arrayContaining([expect.objectContaining({ contents: 'Confirm delivery date' })]))

      for (const width of [320, 1280]) {
        await page.viewport(width, 900)
        checkResponsive(panel)
      }
      await page.viewport(initialWidth, 900)

      await activate(firstButton)
      expect(required<HTMLTextAreaElement>(panel, 'textarea').value).toBe(firstText)
      await userEvent.keyboard('{Control>}z{/Control}')
      expect(semanticShortcut).toBe(false)
      const x = panelLocator.getByRole('spinbutton', { name: t('geometry.x') })
      await x.fill('999')
      const beforeInvalid = post.mock.calls.length
      const beforeInvalidHash = edited.dataset.outputHash
      await activate(button('note.move'))
      expect(x.element()).toBe(document.activeElement)
      expect(edited.dataset.outputHash).toBe(beforeInvalidHash)
      expect(post.mock.calls).toHaveLength(beforeInvalid)
      await x.fill('48')
      await panelLocator.getByRole('spinbutton', { name: t('geometry.y') }).fill('64')
      await activate(button('note.move'))
      const moved = await readEdited(3)
      expect(moved).toEqual(expect.arrayContaining([expect.objectContaining({
        kind: 'plainNote', contents: firstText, rect: [48, 64, 72, 88],
      })]))

      expect(notes()[0]?.dataset.nativeNoteId).toBe(firstId)
      await panelLocator.getByRole('textbox', { name: t('note.contents') }).fill('Погоджено / Approved')
      await activate(button('note.save'))
      const saved = await readEdited(4)
      expect(saved).toEqual(expect.arrayContaining([expect.objectContaining({
        kind: 'plainNote', rect: [48, 64, 72, 88], contents: 'Погоджено / Approved',
      })]))

      expect(notes()[0]?.dataset.nativeNoteId).toBe(firstId)
      const remove = button('note.remove')
      await activate(remove)
      const dialog = required<HTMLDialogElement>(panel, 'dialog')
      expect(dialog.open).toBe(true)
      expect(document.activeElement?.textContent).toBe(t('note.remove.cancel'))
      await userEvent.keyboard('{Escape}')
      await expect.poll(() => dialog.open).toBe(false)
      expect(document.activeElement).toBe(remove)
      await activate(remove)
      await userEvent.keyboard('{Enter}')
      await expect.poll(() => dialog.open).toBe(false)
      expect(document.activeElement).toBe(remove)
      await activate(remove)
      await userEvent.keyboard('{Tab}{Enter}')
      await expect.poll(() => notes().length).toBe(1)
      expect(document.activeElement).toBe(notes()[0])
      const remaining = await readEdited(5)
      expect(remaining).toHaveLength(1)
      expect(remaining[0]).toMatchObject({ contents: 'Confirm delivery date' })

      const lastAcceptedDownload = downloads.at(-1)!
      const savedHash = edited.dataset.outputHash
      let finishRead!: () => void
      const wait = new Promise<void>((resolve) => { finishRead = resolve })
      dispatchFile(input, new TextEncoder().encode('invalid PDF'), 'invalid.pdf', wait)
      try {
        await expect.element(control('download.edited')).toBeDisabled()
        expect(input.disabled).toBe(true)
        expect(panel.textContent).toContain(t('status.previousView'))
        expect(edited.dataset.outputHash).toBe(savedHash)
        expect(notes()).toHaveLength(1)
      } finally {
        finishRead()
      }
      await expect.element(control('download.edited')).toBeEnabled()
      const importAlert = required(panel, '[role=alert]').textContent
      expect(importAlert).toContain('FLOW_PDF_NATIVE_IMPORT_REFUSED')
      expect(importAlert).toContain(t('error.importRefused'))
      expect(edited.dataset.outputHash).toBe(savedHash)
      expect(notes()).toHaveLength(1)

      const previous = downloads.length
      await activate(original)
      await expect.poll(() => downloads.length).toBe(previous + 1)
      expect(downloads[previous]!.filename).not.toMatch(/-edited\.pdf$/)
      expect(await downloads[previous]!.bytes).toEqual(initial)
      expect(await store.get(sourceHash)).toEqual(initial)

      await activate(notes()[0]!)
      const draft = 'An unsaved draft must never be replayed.'
      await panelLocator.getByRole('textbox', { name: t('note.contents') }).fill(draft)
      const workerIndex = post.mock.calls.findIndex(([message]) =>
        (message as Partial<NativePdfWorkerRequest> | undefined)?.request?.operation === 'command')
      expect(workerIndex).toBeGreaterThanOrEqual(0)
      const worker = post.mock.contexts[workerIndex] as Worker
      const onMessageError = worker.onmessageerror
      expect(onMessageError).toBeTypeOf('function')
      onMessageError?.call(worker, new MessageEvent('messageerror'))
      await expect.element(control('session.reopen'), { timeout: 5_000 }).toBeEnabled()
      expect(edited.disabled).toBe(false)
      expect(edited.dataset.outputHash).toBe(savedHash)
      expect(panel.textContent).toContain(t('output.last', { revision: 5 }))
      expect(required<HTMLTextAreaElement>(panel, 'textarea').readOnly).toBe(true)
      expect(button('note.save').disabled).toBe(true)
      const beforeLastVerified = downloads.length
      await activate(edited)
      await expect.poll(() => downloads.length).toBe(beforeLastVerified + 1)
      expect(await downloads[beforeLastVerified]!.bytes).toEqual(await lastAcceptedDownload.bytes)

      await activate(button('session.reopen'))
      await expect.element(control('note.add')).toBeEnabled()
      expect(edited.disabled).toBe(true)
      expect(edited.dataset.outputHash).toBe(originOutputHash)
      expect(notes()).toHaveLength(0)
      expect(panel.querySelector('textarea')).toBeNull()
      expect(post.mock.calls.filter(([message]) =>
        (message as Partial<NativePdfWorkerRequest> | undefined)?.request?.operation === 'command')).toHaveLength(5)
      expect(controller.snapshot().accepted).toBe(semanticDocument)
    } finally {
      lifecycle.abort()
      nativePdf.dispose()
      post.mockRestore()
      click.mockRestore()
      shortcutBoundary.remove()
    }
  }, 60_000)
}
