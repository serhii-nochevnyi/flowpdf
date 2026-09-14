import { expect, test } from 'vitest'

import {
  mountEditorApp,
  type EditorController,
  type EditorBlockViewDto,
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

function acceptedBlocks(controller: EditorController): readonly EditorBlockViewDto[] {
  const accepted = controller.snapshot().accepted
  if (accepted === null) throw new Error('accepted editor state is required')
  return accepted.editor.view.document.blocks
}

function textBlock(
  blocks: readonly EditorBlockViewDto[],
  nodeId: string,
): (EditorBlockViewDto & { readonly text: string }) | undefined {
  for (const block of blocks) {
    if (
      block.nodeId === nodeId &&
      (block.kind === 'paragraph' || block.kind === 'heading') &&
      typeof block.text === 'string'
    ) {
      return block as EditorBlockViewDto & { readonly text: string }
    }
    const nested = textBlock(block.children ?? [], nodeId)
    if (nested !== undefined) return nested
  }
  return undefined
}

function firstTextBlock(
  blocks: readonly EditorBlockViewDto[],
): EditorBlockViewDto & { readonly text: string } {
  for (const block of blocks) {
    if (
      (block.kind === 'paragraph' || block.kind === 'heading') &&
      typeof block.text === 'string'
    ) {
      return block as EditorBlockViewDto & { readonly text: string }
    }
    const nested = firstTextBlockOrNull(block.children ?? [])
    if (nested !== undefined) return nested
  }
  throw new Error('document has no text block')
}

function firstTextBlockOrNull(
  blocks: readonly EditorBlockViewDto[],
): (EditorBlockViewDto & { readonly text: string }) | undefined {
  try {
    return firstTextBlock(blocks)
  } catch {
    return undefined
  }
}

function preorderIds(blocks: readonly EditorBlockViewDto[]): string[] {
  return blocks.flatMap((block) => [block.nodeId, ...preorderIds(block.children ?? [])])
}

function setDomSelection(root: HTMLElement, nodeId: string, offset: number): void {
  const blockElement = Array.from(
    root.querySelectorAll<HTMLElement>('[data-node-id]'),
  ).find((candidate) => candidate.dataset.nodeId === nodeId)
  if (blockElement === undefined) throw new Error(`missing DOM block ${nodeId}`)
  const textNode = blockElement.ownerDocument.createTreeWalker(
    blockElement,
    NodeFilter.SHOW_TEXT,
  ).nextNode()
  const range = document.createRange()
  if (textNode === null) {
    range.setStart(blockElement, 0)
    range.setEnd(blockElement, 0)
  } else {
    range.setStart(textNode, offset)
    range.setEnd(textNode, offset)
  }
  const selection = document.getSelection()
  if (selection === null) throw new Error('selection API is unavailable')
  selection.removeAllRanges()
  selection.addRange(range)
  document.dispatchEvent(new Event('selectionchange'))
  root.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')?.focus()
}

function action(root: HTMLElement, name: string): HTMLButtonElement {
  const button = root.querySelector<HTMLButtonElement>(`[data-action="${name}"]`)
  if (button === null) throw new Error(`missing ${name}`)
  return button
}

function stateFingerprint(controller: EditorController) {
  const accepted = controller.snapshot().accepted
  if (accepted === null) throw new Error('accepted editor state is required')
  return {
    revision: accepted.session.revision,
    hash: accepted.session.canonicalHash,
    canonicalJson: accepted.session.canonicalJson,
    selection: accepted.editor.view.selection,
    historyCursor: accepted.session.history.cursor,
    historyLength: accepted.session.history.entries.length,
    ids: preorderIds(accepted.editor.view.document.blocks),
  }
}

test('structural keys and controls use the accepted Rust projection and stable rejection path', async () => {
  const databaseName = `flowpdf-structure-controls-${crypto.randomUUID()}`
  const keyboardRoot = document.createElement('div')
  document.body.replaceChildren(keyboardRoot)
  const keyboard = await mountEditorApp(keyboardRoot, {
    databaseName,
    clock: () => new Date('2026-08-14T23:30:00Z'),
  })
  await settle(keyboard)
  await openOlder(keyboardRoot, keyboard)

  const initial = stateFingerprint(keyboard)
  const initialBlocks = acceptedBlocks(keyboard)
  const first = firstTextBlock(initialBlocks)
  const originalSecond = initialBlocks.find(
    (block) => block.nodeId !== first.nodeId && block.kind === 'paragraph',
  )
  if (originalSecond === undefined) throw new Error('second paragraph projection is required')
  const splitOffset = first.spans?.[1]?.endUtf16 ?? 1
  setDomSelection(keyboardRoot, first.nodeId, splitOffset)
  const host = keyboardRoot.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')
  if (host === null) throw new Error('missing input host')
  const key = new KeyboardEvent('keydown', {
    key: 'Enter',
    bubbles: true,
    cancelable: true,
  })
  host.dispatchEvent(key)
  expect(key.defaultPrevented).toBe(true)
  await settle(keyboard)

  const keyboardAfter = stateFingerprint(keyboard)
  expect(keyboardAfter.revision).toBe(initial.revision + 1)
  expect(keyboardAfter.historyCursor).toBe(initial.historyCursor + 1)
  const keyboardShape = acceptedBlocks(keyboard).map((block) => [
    block.kind,
    block.text ?? block.nodeKind,
  ])
  expect(keyboardAfter.ids).toEqual(preorderIds(acceptedBlocks(keyboard)))
  const keyboardSelection = keyboard.snapshot().accepted?.editor.view.selection
  expect(keyboardSelection?.anchor).toEqual(keyboardSelection?.focus)
  expect(keyboardSelection?.anchor.utf16Offset).toBe(0)
  expect(keyboardRoot.querySelector('[data-editor-error]')).toBeNull()

  const splitBlocks = acceptedBlocks(keyboard)
  setDomSelection(keyboardRoot, originalSecond.nodeId, 0)
  await settle(keyboard)
  const beforeRejectedMerge = stateFingerprint(keyboard)
  const rejectedKey = new KeyboardEvent('keydown', {
    key: 'Backspace',
    bubbles: true,
    cancelable: true,
  })
  host.dispatchEvent(rejectedKey)
  expect(rejectedKey.defaultPrevented).toBe(true)
  await settle(keyboard)
  const afterRejectedMerge = stateFingerprint(keyboard)
  expect(afterRejectedMerge).toEqual(beforeRejectedMerge)
  expect(keyboardRoot.querySelector('[data-editor-error]')?.textContent).toContain(
    'FLOW_INCOMPATIBLE_STRUCTURE',
  )

  const undo = action(keyboardRoot, 'editor-undo')
  undo.click()
  await settle(keyboard)
  const visibleSplit = action(keyboardRoot, 'editor-split')
  expect(visibleSplit.disabled).toBe(false)
  setDomSelection(keyboardRoot, first.nodeId, splitOffset)
  visibleSplit.click()
  await settle(keyboard)
  const visibleAfter = stateFingerprint(keyboard)
  expect(visibleAfter.revision).toBe(initial.revision + 3)
  expect(acceptedBlocks(keyboard).map((block) => [block.kind, block.text ?? block.nodeKind])).toEqual(
    keyboardShape,
  )

  const mergePrevious = action(keyboardRoot, 'editor-merge-previous')
  expect(mergePrevious.disabled).toBe(false)
  mergePrevious.click()
  await settle(keyboard)
  const merged = stateFingerprint(keyboard)
  expect(merged.revision).toBe(initial.revision + 4)
  expect(merged.ids).toHaveLength(initial.ids.length)
})

test('durable structural parity survives undo, redo, reload, recovery, and cold remount', async () => {
  const keyboardDatabaseName = `flowpdf-structure-durable-keyboard-${crypto.randomUUID()}`
  const visibleDatabaseName = `flowpdf-structure-durable-visible-${crypto.randomUUID()}`
  const keyboardRoot = document.createElement('div')
  document.body.replaceChildren(keyboardRoot)
  const keyboard = await mountEditorApp(keyboardRoot, {
    databaseName: keyboardDatabaseName,
    clock: () => new Date('2026-08-14T23:40:00Z'),
  })
  await settle(keyboard)
  await openOlder(keyboardRoot, keyboard)
  const initial = stateFingerprint(keyboard)

  const visibleRoot = document.createElement('div')
  document.body.replaceChildren(visibleRoot)
  const visible = await mountEditorApp(visibleRoot, {
    databaseName: visibleDatabaseName,
    clock: () => new Date('2026-08-14T23:40:00Z'),
  })
  await settle(visible)
  await openOlder(visibleRoot, visible)
  expect(stateFingerprint(visible).hash).toBe(initial.hash)

  const visibleFirst = firstTextBlock(acceptedBlocks(visible))
  const splitOffset = visibleFirst.spans?.[1]?.endUtf16 ?? 1
  setDomSelection(visibleRoot, visibleFirst.nodeId, splitOffset)
  const splitButton = action(visibleRoot, 'editor-split')
  splitButton.click()
  await settle(visible)
  const visibleTrace = stateFingerprint(visible)

  document.body.replaceChildren(keyboardRoot)
  const first = firstTextBlock(acceptedBlocks(keyboard))
  const keyboardSplitOffset = first.spans?.[1]?.endUtf16 ?? 1
  setDomSelection(keyboardRoot, first.nodeId, keyboardSplitOffset)
  const splitKey = new KeyboardEvent('keydown', {
    key: 'Enter',
    bubbles: true,
    cancelable: true,
  })
  keyboardRoot.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')?.dispatchEvent(
    splitKey,
  )
  await settle(keyboard)
  const keyboardTrace = stateFingerprint(keyboard)
  expect(visibleTrace.revision).toBe(keyboardTrace.revision)
  expect(visibleTrace.hash).toBe(keyboardTrace.hash)
  expect(visibleTrace.canonicalJson).toBe(keyboardTrace.canonicalJson)
  expect(visibleTrace.ids).toEqual(keyboardTrace.ids)

  document.body.replaceChildren(visibleRoot)
  const merge = action(visibleRoot, 'editor-merge-previous')
  expect(merge.disabled).toBe(false)
  merge.click()
  await settle(visible)
  const merged = stateFingerprint(visible)
  expect(merged.revision).toBe(initial.revision + 2)
  expect(merged.historyCursor).toBe(initial.historyCursor + 2)

  action(visibleRoot, 'editor-undo').click()
  await settle(visible)
  const undone = stateFingerprint(visible)
  action(visibleRoot, 'editor-redo').click()
  await settle(visible)
  const redone = stateFingerprint(visible)
  action(visibleRoot, 'editor-undo').click()
  await settle(visible)
  const undoRedoUndo = stateFingerprint(visible)
  expect(undone.ids).toEqual(undoRedoUndo.ids)
  expect(redone.ids).toEqual(merged.ids)

  action(visibleRoot, 'editor-reload').click()
  await settle(visible)
  expect(stateFingerprint(visible)).toEqual(undoRedoUndo)
  action(visibleRoot, 'editor-recover').click()
  await settle(visible)
  expect(stateFingerprint(visible)).toEqual(undoRedoUndo)

  const coldRoot = document.createElement('div')
  document.body.replaceChildren(coldRoot)
  const cold = await mountEditorApp(coldRoot, { databaseName: visibleDatabaseName })
  await settle(cold)
  expect(stateFingerprint(cold)).toEqual(undoRedoUndo)
  expect(coldRoot.querySelector('details summary')).not.toBeNull()
})
