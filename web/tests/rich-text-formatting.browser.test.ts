import { page } from 'vitest/browser'
import { expect, test } from 'vitest'

import '../src/styles.css'
import {
  mountEditorApp,
  type EditorBlockViewDto,
  type EditorController,
} from '../src/editor/editor-app.js'

async function settle(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
}

async function openOlder(root: HTMLElement, controller: EditorController): Promise<void> {
  root.querySelector<HTMLButtonElement>('[data-action="editor-open-older"]')?.click()
  await settle(controller)
}

function firstTextBlock(
  controller: EditorController,
): EditorBlockViewDto & { readonly text: string } {
  const blocks = controller.snapshot().accepted?.editor.view.document.blocks ?? []
  const block = findTextBlock(blocks)
  if (block === undefined || typeof block.text !== 'string') throw new Error('missing text block')
  return block as EditorBlockViewDto & { readonly text: string }
}

function findTextBlock(
  blocks: readonly EditorBlockViewDto[],
): EditorBlockViewDto | undefined {
  for (const block of blocks) {
    if (
      (block.kind === 'paragraph' || block.kind === 'heading') &&
      typeof block.text === 'string'
    ) {
      return block
    }
    const nested = findTextBlock(block.children ?? [])
    if (nested !== undefined) return nested
  }
  return undefined
}

function blockElement(root: HTMLElement, nodeId: string): HTMLElement {
  const element = root.querySelector<HTMLElement>(`[data-node-id="${nodeId}"]`)
  if (element === null) throw new Error(`missing block ${nodeId}`)
  return element
}

function selectRange(
  root: HTMLElement,
  block: EditorBlockViewDto & { readonly text: string },
  reverse = false,
): void {
  const element = blockElement(root, block.nodeId)
  const text = element.firstChild
  if (text === null || text.nodeType !== Node.TEXT_NODE) throw new Error('missing block text')
  const selection = document.getSelection()
  if (selection === null) throw new Error('selection API is unavailable')
  selection.removeAllRanges()
  if (reverse) {
    selection.setBaseAndExtent(text, block.text.length, text, 0)
  } else {
    const range = document.createRange()
    range.setStart(text, 0)
    range.setEnd(text, block.text.length)
    selection.addRange(range)
  }
  document.dispatchEvent(new Event('selectionchange'))
}

function caretAtStart(
  root: HTMLElement,
  block: EditorBlockViewDto & { readonly text: string },
): void {
  const element = blockElement(root, block.nodeId)
  const text = element.firstChild
  if (text === null || text.nodeType !== Node.TEXT_NODE) throw new Error('missing block text')
  const range = document.createRange()
  range.setStart(text, 0)
  range.setEnd(text, 0)
  const selection = document.getSelection()
  if (selection === null) throw new Error('selection API is unavailable')
  selection.removeAllRanges()
  selection.addRange(range)
  document.dispatchEvent(new Event('selectionchange'))
}

function trace(controller: EditorController) {
  const accepted = controller.snapshot().accepted
  if (accepted === null) throw new Error('accepted state is required')
  return {
    revision: accepted.session.revision,
    hash: accepted.session.canonicalHash,
    canonicalJson: accepted.session.canonicalJson,
    cursor: accepted.session.history.cursor,
    length: accepted.session.history.entries.length,
  }
}

test('visible and keyboard inline-formatting routes have equal durable traces', async () => {
  await page.viewport(1280, 900)
  const visibleRoot = document.createElement('div')
  document.body.replaceChildren(visibleRoot)
  const visible = await mountEditorApp(visibleRoot, {
    databaseName: `flowpdf-format-visible-${crypto.randomUUID()}`,
    clock: () => new Date('2026-09-14T19:20:00Z'),
  })
  await settle(visible)
  await openOlder(visibleRoot, visible)
  selectRange(visibleRoot, firstTextBlock(visible))
  await settle(visible)
  visibleRoot.querySelector<HTMLButtonElement>('[data-control="inline-bold"]')?.click()
  await settle(visible)
  const visibleTrace = trace(visible)

  const keyboardRoot = document.createElement('div')
  document.body.replaceChildren(keyboardRoot)
  const keyboard = await mountEditorApp(keyboardRoot, {
    databaseName: `flowpdf-format-keyboard-${crypto.randomUUID()}`,
    clock: () => new Date('2026-09-14T19:20:00Z'),
  })
  await settle(keyboard)
  await openOlder(keyboardRoot, keyboard)
  selectRange(keyboardRoot, firstTextBlock(keyboard))
  await settle(keyboard)
  const host = keyboardRoot.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')
  if (host === null) throw new Error('missing input host')
  host.focus()
  const shortcut = new KeyboardEvent('keydown', {
    key: 'b',
    ctrlKey: true,
    bubbles: true,
    cancelable: true,
  })
  host.dispatchEvent(shortcut)
  await settle(keyboard)

  expect(shortcut.defaultPrevented).toBe(true)
  expect(trace(keyboard)).toEqual(visibleTrace)
  expect(keyboard.snapshot().accepted?.editor.view.formatting.bold).toBe('on')
  expect(document.activeElement).toBe(host)
})

test('lists, block style, direction, and closed color errors stay Rust-owned', async () => {
  await page.viewport(1280, 900)
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(root, {
    databaseName: `flowpdf-format-structure-${crypto.randomUUID()}`,
    clock: () => new Date('2026-09-14T19:30:00Z'),
  })
  await settle(controller)
  await openOlder(root, controller)

  const initial = trace(controller)
  const first = firstTextBlock(controller)
  selectRange(root, first, true)
  await settle(controller)
  const underline = root.querySelector<HTMLButtonElement>('[data-control="inline-underline"]')
  underline?.click()
  await settle(controller)
  const reverse = controller.snapshot().accepted?.editor.view.selection
  expect(reverse?.anchor.utf16Offset).toBe(first.text.length)
  expect(reverse?.focus.utf16Offset).toBe(0)
  expect(trace(controller).revision).toBe(initial.revision + 1)

  const blockStyle = root.querySelector<HTMLSelectElement>('[data-control="block-style"]')
  if (blockStyle === null) throw new Error('missing block style control')
  blockStyle.value = 'heading:2'
  blockStyle.dispatchEvent(new Event('change', { bubbles: true }))
  await settle(controller)
  expect(controller.snapshot().accepted?.editor.view.document.blocks[0]).toMatchObject({
    kind: 'heading',
    level: 2,
  })

  selectRange(root, firstTextBlock(controller))
  await settle(controller)
  root.querySelector<HTMLButtonElement>('[data-control="list-unordered"]')?.click()
  await settle(controller)
  expect(controller.snapshot().accepted?.editor.view.formatting.listKind).toBe('unordered')
  expect(root.querySelector('ul')).not.toBeNull()

  const beforeInvalidColor = trace(controller)
  const color = root.querySelector<HTMLInputElement>('[data-control="text-color"]')
  if (color === null) throw new Error('missing color control')
  selectRange(root, firstTextBlock(controller))
  await settle(controller)
  color.value = '#ff0000'
  color.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
  await settle(controller)
  expect(trace(controller)).toEqual(beforeInvalidColor)
  expect(root.querySelector('[data-editor-error]')?.textContent).toContain(
    'FLOW_INVALID_FORMATTING',
  )
  expect(document.activeElement).toBe(
    root.querySelector('[data-editor-input-host]'),
  )
})

test('collapsed keyboard formatting changes pending Rust marks without a revision', async () => {
  await page.viewport(320, 900)
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(root, {
    databaseName: `flowpdf-format-pending-${crypto.randomUUID()}`,
    clock: () => new Date('2026-09-14T19:40:00Z'),
  })
  await settle(controller)
  await openOlder(root, controller)
  caretAtStart(root, firstTextBlock(controller))
  await settle(controller)
  const before = trace(controller)
  const host = root.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')
  if (host === null) throw new Error('missing input host')
  host.dispatchEvent(
    new KeyboardEvent('keydown', { key: 'i', ctrlKey: true, bubbles: true, cancelable: true }),
  )
  await settle(controller)
  expect(trace(controller)).toEqual(before)
  expect(controller.snapshot().accepted?.editor.view.pendingMarks.italic).toBe(true)
  expect(root.querySelector<HTMLButtonElement>('[data-control="inline-italic"]')?.getAttribute('aria-pressed')).toBe(
    'true',
  )
})
