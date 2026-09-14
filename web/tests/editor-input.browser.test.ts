import { expect, test } from 'vitest'

import {
  MAX_PASTE_UTF8_BYTES,
} from '../src/editor/input-adapter.js'
import {
  mountEditorApp,
  type EditorController,
} from '../src/editor/editor-app.js'

async function settle(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
}

async function openOlder(root: HTMLElement, controller: EditorController): Promise<void> {
  const button = root.querySelector<HTMLButtonElement>('[data-action="editor-open-older"]')
  if (button === null) throw new Error('missing editor-open-older')
  button.click()
  await settle(controller)
}

function firstParagraph(root: HTMLElement): HTMLParagraphElement {
  const paragraph = root.querySelector<HTMLParagraphElement>('[data-editor-document] p')
  if (paragraph === null || paragraph.firstChild === null) {
    throw new Error('missing paragraph text')
  }
  return paragraph
}

function setDomSelection(
  root: HTMLElement,
  start: number,
  end = start,
): void {
  const paragraph = firstParagraph(root)
  const text = paragraph.firstChild
  if (text === null || text.nodeType !== Node.TEXT_NODE) throw new Error('paragraph is not text')
  const range = document.createRange()
  range.setStart(text, start)
  range.setEnd(text, end)
  const selection = document.getSelection()
  if (selection === null) throw new Error('selection API is unavailable')
  selection.removeAllRanges()
  selection.addRange(range)
  document.dispatchEvent(new Event('selectionchange'))
}

function inputHost(root: HTMLElement): HTMLTextAreaElement {
  const host = root.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')
  if (host === null) throw new Error('missing editor input host')
  return host
}

test('Phase 2 controlled input commits typing, plain paste, and IME atomically', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(root, {
    databaseName: 'flowpdf-phase2-input-browser-test',
    clock: () => new Date('2026-08-14T23:00:00Z'),
  })
  await settle(controller)
  await openOlder(root, controller)

  const initial = controller.snapshot()
  if (initial.accepted === null) throw new Error('accepted document is required')
  const initialRevision = initial.accepted.session.revision
  const initialText = initial.accepted.editor.view.document.blocks
    .map((block) => block.text ?? '')
    .join('\n')
  const host = inputHost(root)

  setDomSelection(root, 0)
  host.focus()
  host.dispatchEvent(
    new InputEvent('beforeinput', {
      bubbles: true,
      cancelable: true,
      inputType: 'insertText',
      data: 'ї',
    }),
  )
  await settle(controller)
  const typed = controller.snapshot()
  expect(typed.accepted?.session.revision).toBe(initialRevision + 1)
  expect(typed.accepted?.editor.view.document.blocks[0]?.text).toMatch(/^ї/)

  const dataTransfer = new DataTransfer()
  dataTransfer.setData('text/plain', ' вставка')
  setDomSelection(root, 1)
  host.focus()
  const paste = new ClipboardEvent('paste', {
    bubbles: true,
    cancelable: true,
    clipboardData: dataTransfer,
  })
  host.dispatchEvent(paste)
  await settle(controller)
  const pasted = controller.snapshot()
  expect(pasted.accepted?.session.revision).toBe(initialRevision + 2)
  expect(pasted.accepted?.editor.view.document.blocks[0]?.text).toMatch(/^ї вставка/)

  const beforeComposition = pasted.accepted
  host.focus()
  host.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: '' }))
  host.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: 'й' }))
  host.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: 'й' }))
  await settle(controller)
  const composed = controller.snapshot()
  expect(composed.accepted?.session.revision).toBe(initialRevision + 3)
  expect(composed.accepted?.editor.view.document.blocks[0]?.text).toContain('й')
  expect(composed.accepted).not.toBe(beforeComposition)
  expect(composed.accepted?.editor.view.selection.anchor).toEqual(
    composed.accepted?.editor.view.selection.focus,
  )
  expect(initialText).not.toBe(composed.accepted?.editor.view.document.blocks[0]?.text)
})

test('Phase 2 composition cancellation, empty commit, invalidation, and paste limits fail closed', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(root, {
    databaseName: 'flowpdf-phase2-input-boundaries-browser-test',
    clock: () => new Date('2026-08-14T23:10:00Z'),
  })
  await settle(controller)
  await openOlder(root, controller)
  const host = inputHost(root)

  const beforeCancel = controller.snapshot()
  setDomSelection(root, 0)
  host.focus()
  host.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: '' }))
  host.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: 'скасовано' }))
  host.dispatchEvent(new CompositionEvent('compositioncancel', { bubbles: true }))
  host.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: 'скасовано' }))
  await settle(controller)
  expect(controller.snapshot().accepted?.session.revision).toBe(
    beforeCancel.accepted?.session.revision,
  )

  const accepted = controller.snapshot().accepted
  if (accepted === null) throw new Error('accepted document is required')
  const removableSpan = accepted.editor.view.document.blocks[0]?.spans?.[1]
  if (removableSpan === undefined) throw new Error('Rust grapheme span is required')
  setDomSelection(root, removableSpan.startUtf16, removableSpan.endUtf16)
  host.focus()
  const beforeEmptyCommit = controller.snapshot().accepted?.session.revision ?? 0
  host.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: '' }))
  host.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '' }))
  await settle(controller)
  const afterEmptyCommit = controller.snapshot()
  expect(afterEmptyCommit.accepted?.session.revision).toBe(beforeEmptyCommit + 1)

  const beforeInvalidation = afterEmptyCommit.accepted?.session.revision ?? 0
  setDomSelection(root, 0)
  host.focus()
  host.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: '' }))
  host.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: 'stale' }))
  const externalCommand = controller.replaceSelection()
  await externalCommand
  await settle(controller)
  host.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: 'stale' }))
  await settle(controller)
  expect(controller.snapshot().accepted?.session.revision).toBe(beforeInvalidation + 1)

  const beforeLimit = controller.snapshot().accepted?.session.revision ?? 0
  setDomSelection(root, 0)
  host.focus()
  const tooLarge = 'a'.repeat(MAX_PASTE_UTF8_BYTES + 1)
  const dataTransfer = new DataTransfer()
  dataTransfer.setData('text/plain', tooLarge)
  host.dispatchEvent(
    new ClipboardEvent('paste', {
      bubbles: true,
      cancelable: true,
      clipboardData: dataTransfer,
    }),
  )
  await settle(controller)
  expect(controller.snapshot().accepted?.session.revision).toBe(beforeLimit)
  expect(root.querySelector('[data-editor-error]')?.textContent).toContain('FLOW_PASTE_LIMIT')
  expect(host.value).toBe('')
})
