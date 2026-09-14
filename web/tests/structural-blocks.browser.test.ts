import { expect, test } from 'vitest'

import '../src/styles.css'
import {
  mountEditorApp,
  type EditorBlockViewDto,
  type EditorController,
} from '../src/editor/editor-app.js'
import type { DirectionalSelectionDto } from '../src/editor/editor-store.js'

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
  const find = (
    blocks: readonly EditorBlockViewDto[],
  ): (EditorBlockViewDto & { readonly text: string }) | undefined => {
    for (const block of blocks) {
      if (
        (block.kind === 'paragraph' || block.kind === 'heading') &&
        typeof block.text === 'string'
      ) {
        return block as EditorBlockViewDto & { readonly text: string }
      }
      const nested = find(block.children ?? [])
      if (nested !== undefined) return nested
    }
    return undefined
  }
  const block = find(controller.snapshot().accepted?.editor.view.document.blocks ?? [])
  if (block === undefined) throw new Error('missing text block')
  return block
}

function selectionAtStart(block: EditorBlockViewDto): DirectionalSelectionDto | null {
  if (
    (block.kind === 'paragraph' || block.kind === 'heading') &&
    typeof block.text === 'string'
  ) {
    const position = { nodeId: block.nodeId, utf16Offset: 0, affinity: 'forward' as const }
    return { anchor: position, focus: position }
  }
  for (const child of block.children ?? []) {
    const found = selectionAtStart(child)
    if (found !== null) return found
  }
  return null
}

async function insertTable(
  root: HTMLElement,
  controller: EditorController,
  rows = '2',
  columns = '2',
): Promise<void> {
  root.querySelector<HTMLButtonElement>('[data-action="editor-insert-table"]')?.click()
  await settle(controller)
  const dialog = root.querySelector<HTMLDialogElement>('[data-table-insert-dialog]')
  if (dialog === null) throw new Error('missing table dialog')
  const rowInput = dialog.querySelector<HTMLInputElement>('[data-control="table-rows"]')
  const columnInput = dialog.querySelector<HTMLInputElement>('[data-control="table-columns"]')
  if (rowInput === null || columnInput === null) throw new Error('missing table inputs')
  setInputValue(rowInput, rows)
  setInputValue(columnInput, columns)
  await settle(controller)
  dialog.querySelector<HTMLButtonElement>('[data-action="editor-table-insert-confirm"]')?.click()
  await settle(controller)
}

function setInputValue(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
  setter?.call(input, value)
  input.dispatchEvent(new Event('input', { bubbles: true }))
  input.dispatchEvent(new Event('change', { bubbles: true }))
}

function stateFingerprint(controller: EditorController) {
  const accepted = controller.snapshot().accepted
  if (accepted === null) throw new Error('accepted editor state is required')
  return {
    revision: accepted.session.revision,
    hash: accepted.session.canonicalHash,
    canonicalJson: accepted.session.canonicalJson,
    historyCursor: accepted.session.history.cursor,
    tableCount: accepted.editor.view.document.blocks.filter(
      (block) => block.kind === 'atomic' && block.nodeKind === 'table',
    ).length,
  }
}

test('visible table actions share Rust state with confirmation, cancel, undo, and redo', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(root, {
    databaseName: `flowpdf-structural-controls-${crypto.randomUUID()}`,
    clock: () => new Date('2026-09-14T20:20:00Z'),
  })
  await settle(controller)
  await openOlder(root, controller)
  const initial = firstTextBlock(controller)
  await controller.setEditorSelection(selectionAtStart(initial)!)
  await settle(controller)
  await insertTable(root, controller)
  const inserted = stateFingerprint(controller)
  expect(inserted.tableCount).toBe(1)

  root.querySelector<HTMLButtonElement>('[data-action="editor-table-remove"]')?.click()
  await settle(controller)
  const dialog = root.querySelector<HTMLDialogElement>('[data-destructive-confirm]')
  if (dialog === null) throw new Error('destructive confirmation is required')
  expect(document.activeElement).toBe(
    dialog.querySelector('[data-action="editor-confirm-cancel"]'),
  )
  expect(dialog.querySelector('[data-action="editor-confirm-accept"]')?.className).toContain(
    'editor-destructive-action',
  )
  dialog.querySelector<HTMLButtonElement>('[data-action="editor-confirm-cancel"]')?.click()
  await settle(controller)
  expect(stateFingerprint(controller)).toEqual(inserted)

  root.querySelector<HTMLButtonElement>('[data-action="editor-table-remove"]')?.click()
  await settle(controller)
  root.querySelector<HTMLButtonElement>('[data-action="editor-confirm-accept"]')?.click()
  await settle(controller)
  const removed = stateFingerprint(controller)
  expect(removed.tableCount).toBe(0)
  expect(root.querySelector('[data-block-kind="table"]')).toBeNull()
  expect(root.querySelector('[data-text-block]')).not.toBeNull()

  root.querySelector<HTMLButtonElement>('[data-action="editor-undo"]')?.click()
  await settle(controller)
  expect(stateFingerprint(controller).tableCount).toBe(1)
  root.querySelector<HTMLButtonElement>('[data-action="editor-redo"]')?.click()
  await settle(controller)
  expect(stateFingerprint(controller).tableCount).toBe(0)
})

test('last-row removal escalates through the same confirmation policy and Escape cancels safely', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(root, {
    databaseName: `flowpdf-structural-last-dimension-${crypto.randomUUID()}`,
    clock: () => new Date('2026-09-14T20:30:00Z'),
  })
  await settle(controller)
  await openOlder(root, controller)
  const initial = firstTextBlock(controller)
  await controller.setEditorSelection(selectionAtStart(initial)!)
  await settle(controller)
  await insertTable(root, controller, '1', '1')
  expect(
    root.querySelector<HTMLButtonElement>('[data-action="editor-table-remove-row"]')?.disabled,
  ).toBe(false)

  root.querySelector<HTMLButtonElement>('[data-action="editor-table-remove-row"]')?.click()
  await settle(controller)
  expect(root.querySelector('[data-destructive-confirm]')).not.toBeNull()
  root
    .querySelector<HTMLDialogElement>('[data-destructive-confirm]')
    ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
  await settle(controller)
  expect(root.querySelector('[data-destructive-confirm]')).toBeNull()
  expect(root.querySelector('[data-block-kind="table"]')).not.toBeNull()

  root.querySelector<HTMLButtonElement>('[data-action="editor-table-remove-row"]')?.click()
  await settle(controller)
  root.querySelector<HTMLButtonElement>('[data-action="editor-confirm-accept"]')?.click()
  await settle(controller)
  expect(root.querySelector('[data-block-kind="table"]')).toBeNull()
})
