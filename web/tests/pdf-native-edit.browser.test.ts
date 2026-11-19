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
  nativeBytesHex, nativeHexBytes, readNativeResponse,
  type NativePdfSnapshot, type NativePdfWorkerRequest, type NativePdfWorkerResponse,
} from '../src/pdf/pdf-native-edit-protocol.js'
import type { PdfReaderWorkerInboundMessage } from '../src/pdf/pdf-protocol.js'

function classicPdf(objects = [
  '<< /Type /Catalog /Pages 2 0 R >>',
  '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
  '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>',
]): Uint8Array<ArrayBuffer> {
  let pdf = '%PDF-1.7\n'
  const offsets = objects.map((object, i) => {
    const offset = pdf.length
    pdf += `${i + 1} 0 obj\n${object}\nendobj\n`
    return offset
  })
  const xref = pdf.length
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`
  for (const offset of offsets) pdf += `${String(offset).padStart(10, '0')} 00000 n \n`
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`
  return new TextEncoder().encode(pdf)
}

function readOnlyTextPdf(first: string, second: string): Uint8Array<ArrayBuffer> {
  const sheet = (stream: number) =>
    `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents ${stream} 0 R >>`
  const content = (value: string) => {
    const data = `BT /F1 12 Tf 72 650 Td (${value}) Tj ET\n`
    return `<< /Length ${data.length} >>\nstream\n${data}endstream`
  }
  return classicPdf([
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>',
    sheet(6), sheet(7),
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>',
    content(first), content(second),
  ])
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
  test(`reads accessible text from the exact accepted PDF and selected page in ${locale}`, async () => {
    await init()
    const root = document.createElement('div')
    document.body.append(root)
    const lifecycle = new AbortController()
    const store = new PdfSourceStore(`flowpdf-native-readback-${crypto.randomUUID()}`)
    const nativePdf = createNativePdfRuntime(undefined, store)
    const firstText = '<First page text>'
    const secondText = 'Second page text'
    const secondReadback = `${secondText}\uFFFD`
    const wrongText = 'Different PDF text'
    const source = readOnlyTextPdf(firstText, `${secondText}\\351`)
    const wrongSource = readOnlyTextPdf(firstText, wrongText)
    let holdNextRead = true
    let staleReply: (() => void) | undefined
    let substituteBytes = false
    const postMessage = Worker.prototype.postMessage
    const post = vi.spyOn(Worker.prototype, 'postMessage').mockImplementation(
      function (this: Worker, ...args: Parameters<Worker['postMessage']>) {
        const message = args[0] as PdfReaderWorkerInboundMessage
        if (message.type === 'read') {
          if (holdNextRead) {
            holdNextRead = false
            const receive = this.onmessage
            this.onmessage = (event) => {
              this.onmessage = receive
              staleReply = () => receive?.call(this, event)
            }
          } else if (substituteBytes) {
            substituteBytes = false
            postMessage.call(this, {
              ...message, request: { ...message.request, bytesHex: nativeBytesHex(wrongSource) },
            })
            return
          }
        }
        Reflect.apply(postMessage, this, args)
      },
    )
    const t = (key: NativePdfMessage) => nativePdfText(locale, key)
    try {
      await mountEditorApp(root, { locale, databaseName: `native-readback-${crypto.randomUUID()}` },
        {}, undefined, { nativePdf, signal: lifecycle.signal })
      const workspace = page.getByRole('region', { name: t('title'), exact: true })
      await expect.element(workspace).toBeVisible()
      const projection = workspace.getByRole('region', { name: t('projection.label'), exact: true })
      const text = (value: string) => projection.getByText(value, { exact: true })
      const button = (key: NativePdfMessage) => workspace.getByRole('button', { name: t(key), exact: true })
      const navigate = (key: 'navigation.next' | 'navigation.previous') =>
        activate(button(key).element() as HTMLButtonElement)
      const input = required<HTMLInputElement>(workspace.element(), 'input[type=file]')
      dispatchFile(input, source, 'read-only-text.pdf')
      await expect.poll(() => staleReply).toBeTypeOf('function')
      await expect.element(projection).toHaveAttribute('aria-busy', 'true')
      expect(text(firstText).query()).toBeNull()
      const firstRead = post.mock.calls.map(([message]) => message as PdfReaderWorkerInboundMessage)
        .find((message) => message.type === 'read')
      expect(firstRead?.request).toMatchObject({ bytesHex: nativeBytesHex(source), firstPage: 0, pageCount: 1 })

      await navigate('navigation.next')
      await expect.element(text(secondReadback)).toBeVisible()
      const limitedText = locale === 'uk' ? 'Текст вилучено не повністю. Показано лише розпізнаний текст.'
        : 'Text extraction was incomplete. Only recognized text is shown.'
      await expect.element(text(limitedText)).toBeVisible()
      expect(post.mock.calls.some(([message]) => {
        const request = message as PdfReaderWorkerInboundMessage
        return request.type === 'cancel' && request.requestId === firstRead?.request.requestId
      })).toBe(true)
      staleReply?.()
      expect(text(firstText).query()).toBeNull()

      await navigate('navigation.previous')
      await expect.element(text(firstText)).toBeVisible()
      expect(text(secondReadback).query()).toBeNull()
      expect(workspace.getByRole('region', { name: t('text.title'), exact: true }).element()
        .querySelector('[id$="-text-capability"]')).not.toBeNull()
      await expect.element(text(limitedText)).toBeVisible()

      const addNote = button('note.add')
      const edited = button('download.edited')
      const previousHash = edited.element().getAttribute('data-output-hash')
      await activate(addNote.element() as HTMLButtonElement)
      await workspace.getByRole('textbox', { name: t('note.contents'), exact: true }).fill('Accepted note')
      await activate(addNote.element() as HTMLButtonElement)
      await expect.element(edited).toBeEnabled()
      await expect.element(text(firstText)).toBeVisible()
      const updatedRead = post.mock.calls.map(([message]) => message as PdfReaderWorkerInboundMessage)
        .findLast((message) => message.type === 'read')
      if (!updatedRead) throw new Error('Missing accepted PDF text request')
      expect(updatedRead.request).toMatchObject({ firstPage: 0, pageCount: 1 })
      const accepted = await readPdf(nativeHexBytes(updatedRead.request.bytesHex))
      const acceptedHash = nativeBytesHex(Uint8Array.from(accepted.outputHash))
      expect(acceptedHash).not.toBe(previousHash)
      await expect.element(edited).toHaveAttribute('data-output-hash', acceptedHash)

      substituteBytes = true
      await navigate('navigation.next')
      await expect.element(text(locale === 'uk' ? 'Вилучення тексту для цієї сторінки недоступне.'
        : 'Text extraction is unavailable for this page.')).toBeVisible()
      expect(substituteBytes).toBe(false)
      expect(text(wrongText).query()).toBeNull()
      expect(text(firstText).query()).toBeNull()
      expect(text(secondReadback).query()).toBeNull()
    } finally {
      lifecycle.abort()
      nativePdf.dispose()
      post.mockRestore()
      root.remove()
    }
  }, 30_000)

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
    let refuseNextCommand = false
    let refuseReadbackFor: number | undefined
    const post = vi.spyOn(Worker.prototype, 'postMessage').mockImplementation(
      function (this: Worker, ...args: Parameters<Worker['postMessage']>) {
        const message = args[0] as NativePdfWorkerRequest | undefined
        const request = message?.request
        const refuseCommand = refuseNextCommand && request?.operation === 'command'
        if (message && request && (refuseCommand || (request.operation === 'snapshot' && request.requestId === refuseReadbackFor))) {
          refuseNextCommand = false
          refuseReadbackFor = refuseCommand ? request.requestId : undefined
          const data: NativePdfWorkerResponse = {
            dispatchId: message.dispatchId, type: 'failed', code: 'FLOW_PDF_NATIVE_RESPONSE_INVALID',
          }
          queueMicrotask(() => this.dispatchEvent(new MessageEvent('message', { data })))
          return
        }
        if (pauseNextCommand && request?.operation === 'command') {
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
      const y = panelLocator.getByRole('spinbutton', { name: t('geometry.y') })
      const noteText = panelLocator.getByRole('textbox', { name: t('note.contents') })
      const draftText = `${firstText} — draft / чернетка`
      await noteText.fill(draftText)
      await x.fill('48')
      await y.fill('64')
      await activate(firstButton)
      await expect.element(noteText).toHaveFocus()
      await expect.element(noteText).toHaveValue(draftText)
      await activate(notes().find((entry) => entry !== firstButton)!)
      await expect.element(noteText).toHaveValue('Confirm delivery date')
      await activate(firstButton)
      await expect.element(noteText).toHaveFocus()
      await expect.element(noteText).toHaveValue(draftText)
      await expect.element(x).toHaveValue(48)
      await expect.element(y).toHaveValue(64)
      await noteText.fill(firstText)
      await expect.poll(() => panel.querySelector<HTMLElement>('[data-native-preview-revision="2"]')?.dataset.nativePreviewHash)
        .toBe(edited.dataset.outputHash)
      await x.fill('999')
      const beforeInvalid = post.mock.calls.length
      const beforeInvalidHash = edited.dataset.outputHash
      await activate(button('note.move'))
      expect(x.element()).toBe(document.activeElement)
      expect(edited.dataset.outputHash).toBe(beforeInvalidHash)
      expect(post.mock.calls).toHaveLength(beforeInvalid)
      await x.fill('48')
      await y.fill('64')
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

      const acceptedBytes = await downloads.at(-1)!.bytes
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
      await noteText.fill(draft)
      refuseNextCommand = true
      await activate(button('note.save'))
      await expect.element(control('session.reconcile')).not.toHaveAttribute('aria-disabled', 'true')
      await activate(button('session.reconcile'))
      await expect.element(control('session.reopenAccepted')).toBeEnabled()
      expect(required(panel, '[role=alert]').textContent).toContain(t('session.commandRecovery'))
      expect(edited.dataset.outputHash).toBe(savedHash)

      const reopenedSourceHash = (await hashPdfSourceBytes(acceptedBytes)).slice('sha256:v1:'.length)
      expect(reopenedSourceHash).not.toBe(sourceHash)
      const beforeReopen = post.mock.calls.length
      await activate(button('session.reopenAccepted'))
      await expect.element(control('note.add')).toBeEnabled()
      await expect.poll(() => status.textContent).toContain(t('status.reopenedAccepted'))
      expect(edited.disabled).toBe(false)
      expect(edited.dataset.outputHash).toBe(savedHash)
      expect(notes()).toHaveLength(1)
      expect(notes()[0]!.textContent).toContain('Confirm delivery date')
      expect(panel.querySelector('textarea')).toBeNull()
      const reopenRequests = post.mock.calls.slice(beforeReopen).map(([message]) => (message as NativePdfWorkerRequest).request)
      expect(reopenRequests.find(({ operation }) => operation === 'open')).toMatchObject({
        originStoreHash: reopenedSourceHash, bytesHex: nativeBytesHex(acceptedBytes),
      })
      expect(reopenRequests.some(({ operation }) => operation === 'command')).toBe(false)
      expect(await store.get(reopenedSourceHash)).toEqual(acceptedBytes)

      const identity = required<HTMLDetailsElement>(panel, 'details')
      await activate(required(identity, 'summary'))
      expect(identity.open).toBe(true)
      const beforeOriginal = downloads.length
      await activate(original)
      await expect.poll(() => downloads.length).toBe(beforeOriginal + 1)
      const originalBytes = await downloads[beforeOriginal]!.bytes
      expect(originalBytes).toEqual(initial)
      const originalDigest = (await hashPdfSourceBytes(originalBytes)).slice('sha256:v1:'.length)
      await expect.element(panelLocator.getByText(t('identity.origin', { hash: originalDigest }), { exact: true })).toBeVisible()
      expect(original.dataset.originHash).toBe(originalDigest)

      await activate(button('note.add'))
      const resumedText = 'Editing resumed / Редагування продовжено'
      await noteText.fill(resumedText)
      await activate(button('note.add'))
      const resumedNotes = await readEdited(1)
      expect(resumedNotes).toHaveLength(2)
      expect(resumedNotes).toEqual(expect.arrayContaining([
        expect.objectContaining({ contents: 'Confirm delivery date' }),
        expect.objectContaining({ contents: resumedText }),
      ]))
      const resumedHash = edited.dataset.outputHash
      const lastAcceptedDownload = downloads.at(-1)!

      await activate(notes()[0]!)
      await noteText.fill(draft)
      const workerIndex = post.mock.calls.findLastIndex(([message]) =>
        (message as Partial<NativePdfWorkerRequest> | undefined)?.request?.operation === 'command')
      expect(workerIndex).toBeGreaterThanOrEqual(0)
      const resumedRequest = (post.mock.calls[workerIndex]![0] as NativePdfWorkerRequest).request
      expect(resumedRequest).toMatchObject({
        operation: 'command', originStoreHash: reopenedSourceHash,
        binding: { revision: 0 },
      })
      if (resumedRequest.operation !== 'command') throw new Error('Expected a resumed edit command')
      expect(nativeBytesHex(Uint8Array.from(resumedRequest.binding.originHash))).toBe(savedHash)
      const worker = post.mock.contexts[workerIndex] as Worker
      const onMessageError = worker.onmessageerror
      expect(onMessageError).toBeTypeOf('function')
      onMessageError?.call(worker, new MessageEvent('messageerror'))
      await expect.element(control('session.reopen'), { timeout: 5_000 }).toBeEnabled()
      expect(edited.disabled).toBe(false)
      expect(edited.dataset.outputHash).toBe(resumedHash)
      expect(panel.textContent).toContain(t('output.last', { revision: 1 }))
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
        (message as Partial<NativePdfWorkerRequest> | undefined)?.request?.operation === 'command')).toHaveLength(7)
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
