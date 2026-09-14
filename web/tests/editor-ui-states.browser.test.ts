import { page } from 'vitest/browser'
import { expect, test } from 'vitest'
import { createElement, createRef } from 'react'
import { createRoot } from 'react-dom/client'

import '../src/styles.css'
import '../src/editor/editor.css'
import {
  editorCopy,
  mountEditorApp,
  type EditorBlockViewDto,
  type EditorController,
} from '../src/editor/editor-app.js'
import { EditorShell } from '../src/editor/editor-shell.js'
import type { DirectionalSelectionDto, EditorLocale } from '../src/editor/editor-store.js'

const LONG_TOKEN = `ДужеДовгийНерозривнийТокен-${'x'.repeat(180)}`

async function settle(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await new Promise<void>((resolve) => setTimeout(resolve, 50))
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
  if (block === undefined) throw new Error('a text block is required for the state fixture')
  return block
}

function selectionAtStart(block: EditorBlockViewDto): DirectionalSelectionDto {
  const position = { nodeId: block.nodeId, utf16Offset: 0, affinity: 'forward' as const }
  return { anchor: position, focus: position }
}

function setInputValue(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
  setter?.call(input, value)
  input.dispatchEvent(new Event('input', { bubbles: true }))
  input.dispatchEvent(new Event('change', { bubbles: true }))
}

export function assertEmptyState(root: HTMLElement, locale: EditorLocale): void {
  const labels = editorCopy(locale)
  expect(root.querySelector('[data-empty-document]')).not.toBeNull()
  expect(root.querySelector('[data-action="editor-create"]')?.textContent).toContain(labels.create)
  expect(root.querySelector('[data-action="editor-open-older"]')?.textContent).toContain(
    labels.openOlder,
  )
  expect(root.querySelector('[data-editor-status]')).not.toBeNull()
}

export async function assertLoadingState(locale: EditorLocale): Promise<void> {
  const labels = editorCopy(locale)
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const reactRoot = createRoot(root)
  reactRoot.render(
    createElement(EditorShell, {
      labels,
      busy: true,
      phase: 'loading',
      status: labels.loading,
      visibleError: null,
      diagnosticsRoot: createRef<HTMLDivElement>(),
      diagnosticsError: false,
      onOpenDiagnostics: () => undefined,
      children: createElement('p', null, labels.loading),
    }),
  )
  await new Promise<void>((resolve) => setTimeout(resolve, 50))
  const shell = root.querySelector<HTMLElement>('.editor-shell')
  expect(shell?.dataset.editorPhase).toBe('loading')
  expect(shell?.getAttribute('aria-busy')).toBe('true')
  expect(root.querySelector('[data-editor-status]')?.textContent).toContain(
    labels.loading,
  )
  reactRoot.unmount()
}

export async function assertErrorState(locale: EditorLocale): Promise<void> {
  const labels = editorCopy(locale)
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const reactRoot = createRoot(root)
  reactRoot.render(
    createElement(EditorShell, {
      labels,
      busy: false,
      phase: 'error',
      status: labels.pending,
      visibleError: 'FLOW_STATE_FIXTURE',
      diagnosticsRoot: createRef<HTMLDivElement>(),
      diagnosticsError: false,
      onOpenDiagnostics: () => undefined,
      children: createElement('p', null, labels.error),
    }),
  )
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
  const alert = root.querySelector<HTMLElement>('[data-editor-error]')
  expect(alert?.getAttribute('role')).toBe('alert')
  expect(alert?.getAttribute('aria-atomic')).toBe('true')
  expect(alert?.textContent).toContain('FLOW_STATE_FIXTURE')
  reactRoot.unmount()
}

export function assertPopulatedState(root: HTMLElement, locale: EditorLocale): void {
  const labels = editorCopy(locale)
  const documentRegion = root.querySelector<HTMLElement>('[data-editor-document]')
  expect(documentRegion?.getAttribute('aria-label')).toBe(labels.region)
  expect(documentRegion?.querySelectorAll('[data-text-block]').length).toBeGreaterThan(0)
  expect(documentRegion?.querySelector('[data-block-kind="image"]')).not.toBeNull()
  expect(documentRegion?.querySelector('img')?.getAttribute('alt')).not.toBeNull()
  expect(root.querySelector('[data-fields-region]')).not.toBeNull()
}

export async function assertPartialState(
  root: HTMLElement,
  controller: EditorController,
  locale: EditorLocale,
): Promise<void> {
  const accepted = controller.snapshot().accepted
  if (accepted === null) throw new Error('accepted state is required')
  const textBlocks = accepted.editor.view.document.blocks.filter(
    (block) =>
      (block.kind === 'paragraph' || block.kind === 'heading') &&
      typeof block.text === 'string',
  )
  const first = textBlocks[0]
  const second = textBlocks[1]
  if (first === undefined || second === undefined) throw new Error('mixed-range fixture is required')
  await controller.setEditorSelection({
    anchor: selectionAtStart(first).anchor,
    focus: selectionAtStart(second).focus,
  })
  await settle(controller)
  const mixedStyle = root.querySelector<HTMLOptionElement>(
    '[data-control="block-style"] option[value="mixed"]',
  )
  const mixedToggle = root.querySelector('[aria-pressed="mixed"]')
  expect(mixedStyle?.selected || mixedToggle !== null).toBe(true)

  // Structural insertion requires a collapsed logical placement; return there
  // after asserting the range-level partial/mixed state.
  await controller.setEditorSelection(selectionAtStart(first))
  await settle(controller)

  const tableButton = root.querySelector<HTMLButtonElement>('[data-action="editor-insert-table"]')
  if (tableButton === null) throw new Error('table insert control is required')
  tableButton.click()
  await settle(controller)
  const tableDialog = root.querySelector<HTMLDialogElement>('[data-table-insert-dialog]')
  if (tableDialog === null) throw new Error('table dialog is required')
  const rows = tableDialog.querySelector<HTMLInputElement>('[data-control="table-rows"]')
  if (rows === null) throw new Error('table row control is required')
  setInputValue(rows, '9999')
  tableDialog.querySelector<HTMLButtonElement>('[data-action="editor-table-insert-confirm"]')?.click()
  await settle(controller)
  expect(tableDialog.querySelector('[data-table-insert-error]')).not.toBeNull()
  expect(rows.value).toBe('9999')
  tableDialog.querySelector<HTMLButtonElement>('[data-action="editor-table-insert-cancel"]')?.click()
  await settle(controller)
  expect(root.querySelector('[data-table-insert-dialog]')).toBeNull()

  const imageButton = root.querySelector<HTMLButtonElement>('[data-action="editor-insert-image"]')
  if (imageButton === null) throw new Error(`image insert control is required in ${locale}`)
  imageButton.click()
  await settle(controller)
  const imageDialog = root.querySelector<HTMLDialogElement>('[data-image-dialog]')
  if (imageDialog === null) throw new Error('image dialog is required')
  imageDialog.querySelector<HTMLButtonElement>('[data-action="editor-image-confirm"]')?.click()
  await settle(controller)
  expect(imageDialog.querySelector('[data-image-dialog-error]')).not.toBeNull()
  imageDialog.querySelector<HTMLButtonElement>('[data-action="editor-image-cancel"]')?.click()
  await settle(controller)
}

export function assertZeroOneManyState(root: HTMLElement): void {
  const blocks = root.querySelectorAll('[data-editor-document] [data-node-id]')
  const textBlocks = root.querySelectorAll('[data-editor-document] [data-text-block]')
  const fields = root.querySelectorAll('[data-fields-region] [data-field-id]')
  expect(blocks.length).toBeGreaterThan(1)
  expect(textBlocks.length).toBeGreaterThan(0)
  expect(fields.length).toBeGreaterThan(1)
}

export async function assertLongTextState(
  root: HTMLElement,
  controller: EditorController,
): Promise<void> {
  const block = firstTextBlock(controller)
  await controller.replaceText(LONG_TOKEN, selectionAtStart(block), 'ui')
  await settle(controller)
  const documentRegion = root.querySelector<HTMLElement>('[data-editor-document]')
  expect(documentRegion?.textContent).toContain(LONG_TOKEN)
  const textBlock = documentRegion?.querySelector<HTMLElement>('[data-text-block]')
  expect(textBlock === null || textBlock === undefined ? '' : getComputedStyle(textBlock).overflowWrap).toBe(
    'anywhere',
  )
  expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(window.innerWidth + 1)
}

for (const locale of ['uk', 'en'] as const) {
  test(`editor state taxonomy is explicit and localized in ${locale}`, async () => {
    await page.viewport(1280, 1000)
    await assertLoadingState(locale)
    await assertErrorState(locale)

    const root = document.createElement('div')
    document.body.replaceChildren(root)
    const controller = await mountEditorApp(root, {
      databaseName: `flowpdf-editor-ui-states-${locale}-${crypto.randomUUID()}`,
      locale,
      clock: () => new Date('2026-09-15T00:00:00Z'),
    })
    await settle(controller)
    assertEmptyState(root, locale)
    await openOlder(root, controller)
    assertPopulatedState(root, locale)
    assertZeroOneManyState(root)
    await assertPartialState(root, controller, locale)
    await assertLongTextState(root, controller)
    expect(root.querySelector('[data-editor-error]')).toBeNull()
    controller.dispose()
  })
}
