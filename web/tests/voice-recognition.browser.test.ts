import { page } from 'vitest/browser'
import { expect, test } from 'vitest'

import {
  type SpeechRecognitionLike,
  type SpeechRecognitionResultEventLike,
  type VoiceRecognitionSnapshot,
  VoiceRecognition,
} from '../src/voice/voice-recognition.js'
import type { DirectionalSelectionDto } from '../src/editor/editor-store.js'
import type { VoiceCommandCaptureDto, VoiceDictationCaptureDto } from '../src/voice/voice-protocol.js'

const selection: DirectionalSelectionDto = {
  anchor: {
    nodeId: '00000000-0000-4000-8000-000000000001',
    utf16Offset: 0,
    affinity: 'forward',
  },
  focus: {
    nodeId: '00000000-0000-4000-8000-000000000001',
    utf16Offset: 5,
    affinity: 'forward',
  },
}

const source = {
  sourceRevision: 4,
  sourceHash: 'hash-4',
  selection,
} as const

interface TestResult {
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

  emitResult(resultIndex: number, results: readonly TestResult[]): void {
    const resultList = [
      ...Array.from({ length: Math.max(0, resultIndex) }, () => result(false, '')),
      ...results,
    ]
    this.onresult?.({
      resultIndex,
      results: resultList as unknown as SpeechRecognitionResultEventLike['results'],
    })
  }

  emitEnd(): void {
    this.onend?.()
  }
}

function result(isFinal: boolean, transcript: string): TestResult {
  return { isFinal, length: 1, 0: { transcript } }
}

function lastRecognition(): BrowserFakeRecognition {
  const recognition = BrowserFakeRecognition.instances.at(-1)
  if (recognition === undefined) throw new Error('browser fake recognition was not created')
  return recognition
}

function render(root: HTMLElement, snapshot: VoiceRecognitionSnapshot): void {
  root.dataset.phase = snapshot.phase
  root.dataset.mode = snapshot.mode ?? ''
  root.dataset.errorCode = snapshot.errorCode ?? ''
  const interim = root.querySelector<HTMLElement>('[data-interim]')
  const final = root.querySelector<HTMLElement>('[data-final]')
  if (interim !== null) interim.textContent = snapshot.interimText
  if (final !== null) final.textContent = snapshot.finalText
}

function makeVoice(
  root: HTMLElement,
  callbacks: {
    readonly dictation: VoiceDictationCaptureDto[]
    readonly commands: VoiceCommandCaptureDto[]
  },
): VoiceRecognition {
  return new VoiceRecognition({
    locale: 'en-US',
    recognitionConstructor: BrowserFakeRecognition,
    getSource: () => source,
    onSnapshot: (snapshot) => render(root, snapshot),
    onDictationFinal: (capture) => {
      callbacks.dictation.push(capture)
    },
    onCommandFinal: (capture) => {
      callbacks.commands.push(capture)
    },
  })
}

test('Chromium proves push-to-talk mode separation and one final dispatch', async () => {
  await page.viewport(1024, 768)
  const root = document.createElement('section')
  root.innerHTML = '<output data-interim></output><output data-final></output>'
  document.body.replaceChildren(root)
  const callbacks = { dictation: [], commands: [] } as {
    dictation: VoiceDictationCaptureDto[]
    commands: VoiceCommandCaptureDto[]
  }
  const voice = makeVoice(root, callbacks)

  expect(voice.start('dictation')).toBe(true)
  const recognition = lastRecognition()
  expect(root.dataset.phase).toBe('listening')
  recognition.emitResult(0, [result(false, 'draft')])
  expect(root.querySelector('[data-interim]')?.textContent).toBe('draft')
  expect(root.querySelector('[data-final]')?.textContent).toBe('')
  expect(voice.start('command')).toBe(false)
  expect(root.dataset.errorCode).toBe('FLOW_VOICE_MODE_CHANGE_REQUIRES_STOP')

  recognition.emitResult(0, [result(true, 'first')])
  recognition.emitResult(1, [result(true, 'second')])
  voice.stop()
  expect(root.dataset.phase).toBe('stopping')
  recognition.emitEnd()

  expect(recognition.stopCalls).toBe(1)
  expect(callbacks.dictation).toHaveLength(1)
  expect(callbacks.commands).toHaveLength(0)
  expect(callbacks.dictation[0]?.text).toBe('first second')
  expect(root.dataset.phase).toBe('preview')
  expect(root.querySelector('[data-final]')?.textContent).toBe('first second')
})

test('Chromium proves interim-only and aborted sessions never dispatch', async () => {
  await page.viewport(1024, 768)
  const root = document.createElement('section')
  root.innerHTML = '<output data-interim></output><output data-final></output>'
  document.body.replaceChildren(root)
  const callbacks = { dictation: [], commands: [] } as {
    dictation: VoiceDictationCaptureDto[]
    commands: VoiceCommandCaptureDto[]
  }
  const voice = makeVoice(root, callbacks)

  expect(voice.start('command')).toBe(true)
  const recognition = lastRecognition()
  recognition.emitResult(0, [result(false, 'never final')])
  voice.abort()

  expect(callbacks.dictation).toHaveLength(0)
  expect(callbacks.commands).toHaveLength(0)
  expect(root.dataset.phase).toBe('idle')
  expect(root.querySelector('[data-interim]')?.textContent).toBe('')
  expect(root.querySelector('[data-final]')?.textContent).toBe('')

  const unavailable = new VoiceRecognition({
    locale: 'en-US',
    recognitionConstructor: null,
    getSource: () => source,
    onSnapshot: (snapshot) => render(root, snapshot),
  })
  expect(unavailable.start('dictation')).toBe(false)
  expect(root.dataset.phase).toBe('unavailable')
  expect(root.dataset.errorCode).toBe('FLOW_VOICE_UNAVAILABLE')
})
