import { page } from 'vitest/browser'
import { expect, test } from 'vitest'

import '../src/styles.css'
import '../src/editor/editor.css'
import {
  mountEditorApp,
  type EditorBlockViewDto,
  type EditorController,
} from '../src/editor/editor-app.js'
import type { DirectionalSelectionDto } from '../src/editor/editor-store.js'
import type {
  SpeechRecognitionLike,
  SpeechRecognitionResultEventLike,
} from '../src/voice/voice-recognition.js'

interface FakeResult {
  readonly isFinal: boolean
  readonly length: number
  readonly 0: { readonly transcript: string }
}

class BrowserFakeRecognition implements SpeechRecognitionLike {
  static instances: BrowserFakeRecognition[] = []

  lang = ''
  continuous = false
  interimResults = false
  maxAlternatives = 0
  onstart: (() => void) | null = null
  onresult: ((event: SpeechRecognitionResultEventLike) => void) | null = null
  onerror: ((event: { readonly error?: string }) => void) | null = null
  onend: (() => void) | null = null
  stopCalls = 0

  constructor() {
    BrowserFakeRecognition.instances.push(this)
  }

  start(): void {
    this.onstart?.()
  }

  stop(): void {
    this.stopCalls += 1
  }

  abort(): void {}

  emitResult(resultIndex: number, isFinal: boolean, transcript: string): void {
    const results = [
      ...Array.from({ length: Math.max(0, resultIndex) }, () => fakeResult(false, '')),
      fakeResult(isFinal, transcript),
    ]
    this.onresult?.({
      resultIndex,
      results: results as unknown as SpeechRecognitionResultEventLike['results'],
    })
  }

  emitEnd(): void {
    this.onend?.()
  }
}

function fakeResult(isFinal: boolean, transcript: string): FakeResult {
  return { isFinal, length: 1, 0: { transcript } }
}

function lastRecognition(): BrowserFakeRecognition {
  const recognition = BrowserFakeRecognition.instances.at(-1)
  if (recognition === undefined) throw new Error('a fake recognition session is required')
  return recognition
}

async function settle(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await new Promise<void>((resolve) => setTimeout(resolve, 50))
}

async function openOlder(root: HTMLElement, controller: EditorController): Promise<void> {
  root.querySelector<HTMLButtonElement>('[data-action="editor-open-older"]')?.click()
  await settle(controller)
}

function firstTextBlock(controller: EditorController): EditorBlockViewDto & { readonly text: string } {
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
  if (block === undefined) throw new Error('a text block is required')
  return block
}

function selectedRange(block: EditorBlockViewDto & { readonly text: string }): DirectionalSelectionDto {
  const start = Math.min(1, block.text.length)
  const end = Math.max(start, Math.min(block.text.length, 6))
  return {
    anchor: { nodeId: block.nodeId, utf16Offset: start, affinity: 'forward' },
    focus: { nodeId: block.nodeId, utf16Offset: end, affinity: 'forward' },
  }
}

function fingerprint(controller: EditorController): {
  readonly revision: number
  readonly hash: string
  readonly canonicalJson: string
} {
  const accepted = controller.snapshot().accepted
  if (accepted === null) throw new Error('accepted editor state is required')
  return {
    revision: accepted.session.revision,
    hash: accepted.session.canonicalHash,
    canonicalJson: accepted.session.canonicalJson,
  }
}

for (const locale of ['uk', 'en'] as const) {
  test(`voice controls are localized and keep interim speech outside the document in ${locale}`, async () => {
    await page.viewport(1280, 1000)
    BrowserFakeRecognition.instances = []
    const root = document.createElement('div')
    document.body.replaceChildren(root)
    const controller = await mountEditorApp(
      root,
      {
        locale,
        databaseName: `flowpdf-voice-controls-${locale}-${crypto.randomUUID()}`,
        clock: () => new Date('2026-09-22T10:00:00Z'),
      },
      {},
      BrowserFakeRecognition,
    )
    await settle(controller)
    await openOlder(root, controller)

    const controls = root.querySelector<HTMLElement>('[data-voice-controls]')
    if (controls === null) throw new Error('voice controls are required')
    expect(controls.querySelector('h2')?.textContent).toBe(
      locale === 'uk' ? 'Голосове введення' : 'Voice input',
    )
    expect(controls.querySelector('[data-action="voice-start"]')?.textContent).toContain(
      locale === 'uk' ? 'Почати слухання' : 'Start listening',
    )
    expect(controls.querySelector('input[value="dictation"]')).not.toBeNull()
    expect(controls.querySelector('input[value="command"]')).not.toBeNull()

    const documentRegion = root.querySelector<HTMLElement>('[data-editor-document]')
    if (documentRegion === null) throw new Error('semantic document is required')
    const documentBefore = documentRegion.textContent
    const before = fingerprint(controller)
    controls.querySelector<HTMLButtonElement>('[data-action="voice-start"]')?.click()
    const recognition = lastRecognition()
    await settle(controller)
    expect(controls.dataset.voicePhase).toBe('listening')
    expect(controls.querySelector<HTMLInputElement>('input[value="command"]')?.disabled).toBe(true)

    recognition.emitResult(0, false, 'interim phrase')
    await settle(controller)
    expect(controls.querySelector('[data-voice-interim]')?.textContent).toContain('interim phrase')
    expect(documentRegion.textContent).toBe(documentBefore)
    expect(documentRegion.querySelector('[data-voice-interim]')).toBeNull()

    controls.querySelector<HTMLButtonElement>('[data-action="voice-abort"]')?.click()
    await settle(controller)
    expect(controls.dataset.voicePhase).toBe('idle')
    expect(controls.querySelector('[data-voice-interim]')).toBeNull()
    expect(fingerprint(controller)).toEqual(before)
  })
}

test('destructive command preview keeps the document unchanged until confirmation and cancel is safe', async () => {
  await page.viewport(1280, 1000)
  BrowserFakeRecognition.instances = []
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(
    root,
    {
      locale: 'en',
      databaseName: `flowpdf-voice-preview-${crypto.randomUUID()}`,
      clock: () => new Date('2026-09-22T10:10:00Z'),
    },
    {},
    BrowserFakeRecognition,
  )
  await settle(controller)
  await openOlder(root, controller)
  const block = firstTextBlock(controller)
  await controller.setEditorSelection(selectedRange(block))
  await settle(controller)

  const controls = root.querySelector<HTMLElement>('[data-voice-controls]')
  if (controls === null) throw new Error('voice controls are required')
  controls.querySelector<HTMLInputElement>('input[value="command"]')?.click()
  const before = fingerprint(controller)
  controls.querySelector<HTMLButtonElement>('[data-action="voice-start"]')?.click()
  const recognition = lastRecognition()
  recognition.emitResult(0, true, 'delete selection')
  controls.querySelector<HTMLButtonElement>('[data-action="voice-stop"]')?.click()
  recognition.emitEnd()
  await settle(controller)

  const dialog = root.querySelector<HTMLDialogElement>('[data-voice-preview]')
  if (dialog === null) throw new Error('voice confirmation dialog is required')
  expect(dialog.getAttribute('role')).toBe('alertdialog')
  expect(dialog.querySelector('[data-voice-preview-action]')?.textContent).toContain(
    'Delete the current selection',
  )
  expect(dialog.querySelector('[data-voice-preview-transcript]')?.textContent).toContain(
    'delete selection',
  )
  expect(fingerprint(controller)).toEqual(before)
  expect(document.activeElement).toBe(
    dialog.querySelector('[data-action="voice-preview-cancel"]'),
  )

  dialog.querySelector<HTMLButtonElement>('[data-action="voice-preview-cancel"]')?.click()
  await settle(controller)
  expect(root.querySelector('[data-voice-preview]')).toBeNull()
  expect(root.querySelector('[data-voice-preview-transcript]')).toBeNull()
  expect(fingerprint(controller)).toEqual(before)

  controls.querySelector<HTMLButtonElement>('[data-action="voice-start"]')?.click()
  const secondRecognition = lastRecognition()
  secondRecognition.emitResult(0, true, 'delete selection')
  controls.querySelector<HTMLButtonElement>('[data-action="voice-stop"]')?.click()
  secondRecognition.emitEnd()
  await settle(controller)
  root.querySelector<HTMLButtonElement>('[data-action="voice-preview-confirm"]')?.click()
  await settle(controller)

  expect(root.querySelector('[data-voice-preview]')).toBeNull()
  expect(fingerprint(controller).revision).toBeGreaterThan(before.revision)
  expect(root.querySelector('[data-editor-status]')?.textContent).toContain(
    'voice command',
  )
})

test('unavailable recognition is visible and remains non-mutating', async () => {
  await page.viewport(1280, 1000)
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(
    root,
    {
      locale: 'uk',
      databaseName: `flowpdf-voice-unavailable-${crypto.randomUUID()}`,
      clock: () => new Date('2026-09-22T10:20:00Z'),
    },
    {},
    null,
  )
  await settle(controller)
  await openOlder(root, controller)
  const controls = root.querySelector<HTMLElement>('[data-voice-controls]')
  if (controls === null) throw new Error('voice controls are required')
  const before = fingerprint(controller)
  controls.querySelector<HTMLButtonElement>('[data-action="voice-start"]')?.click()
  await settle(controller)
  expect(controls.dataset.voicePhase).toBe('unavailable')
  expect(controls.querySelector('[data-voice-error]')?.textContent).toContain('недоступне')
  expect(fingerprint(controller)).toEqual(before)
})
