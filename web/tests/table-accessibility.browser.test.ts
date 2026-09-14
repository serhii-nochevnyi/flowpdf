import { page } from 'vitest/browser'
import { expect, test } from 'vitest'

import '../src/styles.css'
import { editorMessages } from '../src/editor/editor-messages.js'
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
  const blocks = controller.snapshot().accepted?.editor.view.document.blocks ?? []
  const find = (
    candidates: readonly EditorBlockViewDto[],
  ): (EditorBlockViewDto & { readonly text: string }) | undefined => {
    for (const block of candidates) {
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
  const block = find(blocks)
  if (block === undefined) throw new Error('missing text block')
  return block
}

function firstTextPosition(block: EditorBlockViewDto): DirectionalSelectionDto | null {
  if (
    (block.kind === 'paragraph' || block.kind === 'heading') &&
    typeof block.text === 'string'
  ) {
    const position = { nodeId: block.nodeId, utf16Offset: 0, affinity: 'forward' as const }
    return { anchor: position, focus: position }
  }
  for (const child of block.children ?? []) {
    const found = firstTextPosition(child)
    if (found !== null) return found
  }
  return null
}

async function insertTable(
  root: HTMLElement,
  controller: EditorController,
  options: { readonly rows?: string; readonly columns?: string; readonly header?: boolean } = {},
): Promise<void> {
  root.querySelector<HTMLButtonElement>('[data-action="editor-insert-table"]')?.click()
  await settle(controller)
  const dialog = root.querySelector<HTMLDialogElement>('[data-table-insert-dialog]')
  if (dialog === null) throw new Error('table insertion dialog is required')
  const rows = dialog.querySelector<HTMLInputElement>('[data-control="table-rows"]')
  const columns = dialog.querySelector<HTMLInputElement>('[data-control="table-columns"]')
  const header = dialog.querySelector<HTMLInputElement>('[data-control="table-header-row"]')
  if (rows === null || columns === null || header === null) {
    throw new Error('table insertion controls are required')
  }
  if (options.rows !== undefined) {
    setInputValue(rows, options.rows)
  }
  if (options.columns !== undefined) {
    setInputValue(columns, options.columns)
  }
  if (options.header === true) header.click()
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

test('native tables expose accepted headers, row-major cell navigation, and page-break separators', async () => {
  await page.viewport(1280, 900)
  for (const locale of ['uk', 'en'] as const) {
    const root = document.createElement('div')
    document.body.replaceChildren(root)
    const controller = await mountEditorApp(root, {
      locale,
      databaseName: `flowpdf-table-a11y-${locale}-${crypto.randomUUID()}`,
      clock: () => new Date('2026-09-14T20:00:00Z'),
    })
    await settle(controller)
    await openOlder(root, controller)

    const initial = firstTextBlock(controller)
    await controller.setEditorSelection(firstTextPosition(initial)!)
    await settle(controller)
    await insertTable(root, controller)

    const table = root.querySelector<HTMLTableElement>('[data-block-kind="table"] table')
    if (table === null) throw new Error('native table is required')
    expect(table.querySelectorAll('thead')).toHaveLength(0)
    expect(table.querySelectorAll('tbody > tr')).toHaveLength(2)
    expect(table.querySelectorAll('tbody td')).toHaveLength(4)
    expect(root.querySelector('[data-table-action-bar]')).not.toBeNull()

    root.querySelector<HTMLButtonElement>('[data-action="editor-table-toggle-header"]')?.click()
    await settle(controller)
    expect(table.querySelectorAll('thead > tr > th[scope="col"]')).toHaveLength(2)
    expect(table.querySelectorAll('tbody > tr')).toHaveLength(1)
    expect(table.querySelectorAll('tbody td')).toHaveLength(2)

    const cells = Array.from(root.querySelectorAll<HTMLElement>('[data-cell-id]'))
    expect(cells).toHaveLength(4)
    const firstCellText = cells[0]?.querySelector<HTMLElement>('[data-text-block]')
    const secondCellText = cells[1]?.querySelector<HTMLElement>('[data-text-block]')
    if (firstCellText === null || secondCellText === null || firstCellText === undefined || secondCellText === undefined) {
      throw new Error('table cell text projections are required')
    }
    cells[0]?.focus()
    cells[0]?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }))
    await settle(controller)
    expect(controller.snapshot().accepted?.editor.view.selection.anchor.nodeId).toBe(
      secondCellText.dataset.nodeId,
    )

    const selectedText = firstTextBlock(controller)
    await controller.setEditorSelection(firstTextPosition(selectedText)!)
    await settle(controller)
    root.querySelector<HTMLButtonElement>('[data-action="editor-insert-page-break"]')?.click()
    await settle(controller)
    const separator = root.querySelector<HTMLElement>('[data-block-kind="pageBreak"]')
    if (separator === null) throw new Error('page-break separator is required')
    expect(separator.getAttribute('role')).toBe('separator')
    expect(separator.tabIndex).toBe(0)
    expect(separator.getAttribute('aria-label')).toBe(editorMessages[locale].pageBreak)
    expect(separator.querySelector('hr')).not.toBeNull()

    separator.focus()
    await settle(controller)
    expect(root.querySelector<HTMLButtonElement>('[data-action="editor-page-break-remove"]')?.disabled).toBe(false)
    root.querySelector<HTMLButtonElement>('[data-action="editor-page-break-remove"]')?.click()
    await settle(controller)
    expect(root.querySelector('[data-block-kind="pageBreak"]')).toBeNull()
  }
})

test('table insertion validates Rust-advertised bounds without publishing a mutation', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(root, {
    databaseName: `flowpdf-table-validation-${crypto.randomUUID()}`,
    clock: () => new Date('2026-09-14T20:10:00Z'),
  })
  await settle(controller)
  await openOlder(root, controller)
  const before = controller.snapshot().accepted?.session.revision
  const initial = firstTextBlock(controller)
  await controller.setEditorSelection(firstTextPosition(initial)!)
  await settle(controller)
  await insertTable(root, controller, { rows: '51', columns: '1' })
  expect(root.querySelector('[data-table-insert-error]')?.textContent).toContain('50')
  expect(controller.snapshot().accepted?.session.revision).toBe(before)
  root.querySelector<HTMLButtonElement>('[data-action="editor-table-insert-cancel"]')?.click()
  await settle(controller)
  expect(root.querySelector('[data-block-kind="table"]')).toBeNull()
})
