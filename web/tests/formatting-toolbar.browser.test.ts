import { page } from 'vitest/browser'
import { expect, test } from 'vitest'

import '../src/styles.css'
import { editorMessages } from '../src/editor/editor-messages.js'
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
  const block = blocks.find(
    (candidate) =>
      (candidate.kind === 'paragraph' || candidate.kind === 'heading') &&
      typeof candidate.text === 'string',
  )
  if (block === undefined || typeof block.text !== 'string') {
    throw new Error('missing text block')
  }
  return block as EditorBlockViewDto & { readonly text: string }
}

function selectFirstBlock(root: HTMLElement, block: EditorBlockViewDto & { readonly text: string }): void {
  const element = root.querySelector<HTMLElement>(`[data-node-id="${block.nodeId}"]`)
  const text = element?.firstChild
  if (element === null || element === undefined || text == null || text.nodeType !== Node.TEXT_NODE) {
    throw new Error('missing block text')
  }
  const range = document.createRange()
  range.setStart(text, 0)
  range.setEnd(text, block.text.length)
  const selection = document.getSelection()
  if (selection === null) throw new Error('selection API is unavailable')
  selection.removeAllRanges()
  selection.addRange(range)
  document.dispatchEvent(new Event('selectionchange'))
  root.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')?.focus()
}

test('formatting toolbar exposes Rust state and complete locale-key parity', async () => {
  expect(Object.keys(editorMessages.uk).sort()).toEqual(Object.keys(editorMessages.en).sort())

  for (const locale of ['uk', 'en'] as const) {
    await page.viewport(320, 900)
    const root = document.createElement('div')
    document.body.replaceChildren(root)
    const controller = await mountEditorApp(root, {
      locale,
      databaseName: `flowpdf-formatting-toolbar-${locale}-${crypto.randomUUID()}`,
      clock: () => new Date('2026-09-14T19:00:00Z'),
    })
    await settle(controller)
    await openOlder(root, controller)

    const toolbar = root.querySelector<HTMLElement>('[data-formatting-toolbar]')
    expect(toolbar?.getAttribute('aria-label')).toBe(editorMessages[locale].toolbar)
    expect(root.querySelector('[data-control="inline-bold"]')).not.toBeNull()
    expect(root.querySelector('[data-control="inline-italic"]')).not.toBeNull()
    expect(root.querySelector('[data-control="inline-underline"]')).not.toBeNull()
    expect(root.querySelector('[data-formatting-more] summary')?.textContent).toContain(
      editorMessages[locale].moreFormatting,
    )
    expect(root.querySelector('[data-formatting-more]')?.hasAttribute('open')).toBe(false)

    const block = firstTextBlock(controller)
    selectFirstBlock(root, block)
    await settle(controller)
    const bold = root.querySelector<HTMLButtonElement>('[data-control="inline-bold"]')
    if (bold === null) throw new Error('missing bold control')
    expect(bold.disabled).toBe(false)
    expect(bold.getAttribute('aria-pressed')).toBe('false')
    bold.click()
    await settle(controller)
    expect(controller.snapshot().accepted?.editor.view.formatting.bold).toBe('on')
    expect(document.activeElement).toBe(
      root.querySelector('[data-editor-input-host]'),
    )
    expect(root.querySelector('[data-editor-error]')).toBeNull()
    expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(window.innerWidth + 1)
  }
})

test('desktop formatting controls expose full vocabulary and mixed state', async () => {
  await page.viewport(1280, 900)
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(root, {
    databaseName: `flowpdf-formatting-toolbar-desktop-${crypto.randomUUID()}`,
    clock: () => new Date('2026-09-14T19:10:00Z'),
  })
  await settle(controller)
  await openOlder(root, controller)

  const details = root.querySelector<HTMLDetailsElement>('[data-formatting-more]')
  expect(details?.open).toBe(true)
  for (const control of [
    'block-style',
    'font-family',
    'font-size',
    'text-color',
    'language',
    'alignment-start',
    'alignment-center',
    'alignment-end',
    'alignment-justify',
    'spacing-before',
    'spacing-after',
    'list-unordered',
    'list-ordered',
    'list-indent',
    'list-outdent',
  ]) {
    expect(root.querySelector(`[data-control="${control}"]`)).not.toBeNull()
  }

  const first = firstTextBlock(controller)
  selectFirstBlock(root, first)
  await settle(controller)
  const bold = root.querySelector<HTMLButtonElement>('[data-control="inline-bold"]')
  bold?.click()
  await settle(controller)
  expect(bold?.getAttribute('aria-pressed')).toBe('true')
  expect(root.querySelector('[data-editor-error]')).toBeNull()
})
