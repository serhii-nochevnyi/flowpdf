import { expect, test } from 'vitest'

import '../src/styles.css'
import {
  mountEditorApp,
  type EditorBlockViewDto,
  type EditorController,
} from '../src/editor/editor-app.js'
import type { DirectionalSelectionDto } from '../src/editor/editor-store.js'

const ONE_PIXEL_PNG = Uint8Array.from(
  atob(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
  ),
  (character) => character.charCodeAt(0),
)

async function settle(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await new Promise<void>((resolve) => setTimeout(resolve, 50))
}

function setInputValue(input: HTMLInputElement | HTMLTextAreaElement, value: string): void {
  const prototype = input instanceof HTMLTextAreaElement
    ? HTMLTextAreaElement.prototype
    : HTMLInputElement.prototype
  const setter = Object.getOwnPropertyDescriptor(prototype, 'value')?.set
  setter?.call(input, value)
  input.dispatchEvent(new Event('input', { bubbles: true }))
  input.dispatchEvent(new Event('change', { bubbles: true }))
}

function setImageFile(input: HTMLInputElement, name: string, bytes = ONE_PIXEL_PNG): void {
  const transfer = new DataTransfer()
  transfer.items.add(new File([bytes], name, { type: 'image/png' }))
  input.files = transfer.files
  input.dispatchEvent(new Event('change', { bubbles: true }))
}

function firstTextBlock(
  blocks: readonly EditorBlockViewDto[],
): (EditorBlockViewDto & { readonly text: string }) | undefined {
  for (const block of blocks) {
    if (
      (block.kind === 'paragraph' || block.kind === 'heading') &&
      typeof block.text === 'string'
    ) {
      return block as EditorBlockViewDto & { readonly text: string }
    }
    const nested = firstTextBlock(block.children ?? [])
    if (nested !== undefined) return nested
  }
  return undefined
}

function imageBlocks(
  blocks: readonly EditorBlockViewDto[],
): EditorBlockViewDto[] {
  const images: EditorBlockViewDto[] = []
  for (const block of blocks) {
    if (block.nodeKind === 'image') images.push(block)
    images.push(...imageBlocks(block.children ?? []))
  }
  return images
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

async function openImageDialog(
  root: HTMLElement,
  controller: EditorController,
  description: string,
  fileName: string,
): Promise<void> {
  const dialog = root.querySelector<HTMLDialogElement>('[data-image-dialog]')
  if (dialog === null) throw new Error('image dialog is required')
  const file = dialog.querySelector<HTMLInputElement>('[data-control="image-file"]')
  const text = dialog.querySelector<HTMLTextAreaElement>('[data-control="image-description"]')
  if (file === null || text === null) throw new Error('image controls are required')
  setImageFile(file, fileName)
  setInputValue(text, description)
  dialog.querySelector<HTMLButtonElement>('[data-action="editor-image-confirm"]')?.click()
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
  await settle(controller)
  await settle(controller)
}

test('image lifecycle uses Rust receipts, accessible controls, persistence, undo, reload, and recovery', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(root, {
    databaseName: `flowpdf-image-lifecycle-${crypto.randomUUID()}`,
    clock: () => new Date('2026-09-15T00:00:00Z'),
  })
  await settle(controller)

  root.querySelector<HTMLButtonElement>('[data-action="editor-open-older"]')?.click()
  await settle(controller)
  const textBlock = firstTextBlock(controller.snapshot().accepted?.editor.view.document.blocks ?? [])
  if (textBlock === undefined) throw new Error('text block is required')
  await controller.setEditorSelection(selectionAtStart(textBlock)!)
  await settle(controller)

  root.querySelector<HTMLButtonElement>('[data-action="editor-insert-image"]')?.click()
  await settle(controller)
  await openImageDialog(root, controller, 'A one-pixel test image.', 'insert.png')

  let image = imageBlocks(controller.snapshot().accepted?.editor.view.document.blocks ?? [])
    .find((candidate) =>
      candidate.image !== undefined && controller.imageSource(candidate.image.contentHash) !== null,
    )
  if (image?.image === undefined) throw new Error('inserted image is required')
  expect(image.image.accessibility).toEqual({ kind: 'described', text: 'A one-pixel test image.' })
  const initialHash = image.image.contentHash
  const initialImage = root.querySelector<HTMLImageElement>('[data-block-kind="image"] img')
  expect(initialImage?.src).toMatch(/^blob:/)

  root.querySelector<HTMLImageElement>('[data-block-kind="image"] img')
    ?.closest<HTMLElement>('[data-block-kind="image"]')
    ?.click()
  await settle(controller)
  expect(root.querySelector('[data-image-action-bar]')).not.toBeNull()
  root.querySelector<HTMLButtonElement>('[data-action="editor-image-replace"]')?.click()
  await settle(controller)
  await openImageDialog(root, controller, 'A replacement image.', 'replace.png')
  image = imageBlocks(controller.snapshot().accepted?.editor.view.document.blocks ?? [])
    .find((candidate) =>
      candidate.image !== undefined &&
      controller.imageSource(candidate.image.contentHash) !== null,
    )
  if (image?.image === undefined) throw new Error('replaced image is required')
  expect(image.image.contentHash).toBe(initialHash)
  expect(image.image.accessibility).toEqual({ kind: 'described', text: 'A replacement image.' })

  root.querySelector<HTMLImageElement>('[data-block-kind="image"] img')
    ?.closest<HTMLElement>('[data-block-kind="image"]')
    ?.click()
  await settle(controller)
  root.querySelector<HTMLButtonElement>('[data-action="editor-image-accessibility"]')?.click()
  await settle(controller)
  const accessibilityDialog = root.querySelector<HTMLDialogElement>(
    '[data-image-accessibility-dialog]',
  )
  if (accessibilityDialog === null) throw new Error('accessibility dialog is required')
  const description = accessibilityDialog.querySelector<HTMLTextAreaElement>(
    '[data-control="image-description"]',
  )
  if (description === null) throw new Error('accessibility description is required')
  setInputValue(description, 'Updated replacement description.')
  accessibilityDialog.querySelector<HTMLButtonElement>('[data-action="editor-image-confirm"]')?.click()
  await settle(controller)
  image = imageBlocks(controller.snapshot().accepted?.editor.view.document.blocks ?? [])
    .find((candidate) =>
      candidate.image !== undefined &&
      controller.imageSource(candidate.image.contentHash) !== null,
    )
  expect(image?.image?.accessibility).toEqual({
    kind: 'described',
    text: 'Updated replacement description.',
  })

  root.querySelector<HTMLImageElement>('[data-block-kind="image"] img')
    ?.closest<HTMLElement>('[data-block-kind="image"]')
    ?.click()
  await settle(controller)
  root.querySelector<HTMLButtonElement>('[data-action="editor-image-remove"]')?.click()
  await settle(controller)
  const confirmation = root.querySelector<HTMLDialogElement>('[data-destructive-confirm]')
  if (confirmation === null) throw new Error('image removal confirmation is required')
  expect(document.activeElement).toBe(
    confirmation.querySelector('[data-action="editor-confirm-cancel"]'),
  )
  confirmation.querySelector<HTMLButtonElement>('[data-action="editor-confirm-cancel"]')?.click()
  await settle(controller)
  expect(imageBlocks(controller.snapshot().accepted?.editor.view.document.blocks ?? [])).toHaveLength(2)

  root.querySelector<HTMLButtonElement>('[data-action="editor-image-remove"]')?.click()
  await settle(controller)
  root.querySelector<HTMLButtonElement>('[data-action="editor-confirm-accept"]')?.click()
  await settle(controller)
  expect(imageBlocks(controller.snapshot().accepted?.editor.view.document.blocks ?? [])).toHaveLength(1)
  expect(initialImage?.src).not.toBe(root.querySelector<HTMLImageElement>('[data-image-preview]')?.src)

  root.querySelector<HTMLButtonElement>('[data-action="editor-undo"]')?.click()
  await settle(controller)
  expect(imageBlocks(controller.snapshot().accepted?.editor.view.document.blocks ?? [])).toHaveLength(2)
  await controller.reloadFromStorage()
  await settle(controller)
  expect(imageBlocks(controller.snapshot().accepted?.editor.view.document.blocks ?? [])).toHaveLength(2)
  await controller.recoverFromStorage()
  await settle(controller)
  expect(imageBlocks(controller.snapshot().accepted?.editor.view.document.blocks ?? [])).toHaveLength(2)
  controller.dispose()
})
