import { describe, expect, it } from 'vitest'

import {
  detectSpeechRecognitionConstructor,
  type SpeechRecognitionLike,
  type SpeechRecognitionResultEventLike,
  type VoiceRecognitionSnapshot,
  VoiceRecognition,
} from '../src/voice/voice-recognition.js'
import type {
  DirectionalSelectionDto,
} from '../src/editor/editor-store.js'
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

class FakeRecognition implements SpeechRecognitionLike {
  static instances: FakeRecognition[] = []

  lang = ''
  continuous = false
  interimResults = false
  maxAlternatives = 0
  onstart: (() => void) | null = null
  onresult: ((event: SpeechRecognitionResultEventLike) => void) | null = null
  onerror: ((event: { readonly error?: string }) => void) | null = null
  onend: (() => void) | null = null
  startCalls = 0
  stopCalls = 0
  abortCalls = 0

  constructor() {
    FakeRecognition.instances.push(this)
  }

  start(): void {
    this.startCalls += 1
    this.onstart?.()
  }

  stop(): void {
    this.stopCalls += 1
  }

  abort(): void {
    this.abortCalls += 1
  }

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

  emitError(error?: string): void {
    this.onerror?.(error === undefined ? {} : { error })
  }

  emitEnd(): void {
    this.onend?.()
  }
}

interface TestResult {
  readonly isFinal: boolean
  readonly length: number
  readonly 0: { readonly transcript: string }
}

function result(isFinal: boolean, transcript: string): TestResult {
  return { isFinal, length: 1, 0: { transcript } }
}

function lastRecognition(): FakeRecognition {
  const recognition = FakeRecognition.instances.at(-1)
  if (recognition === undefined) throw new Error('fake recognition was not created')
  return recognition
}

function session(
  overrides: Partial<ConstructorParameters<typeof VoiceRecognition>[0]> = {},
): VoiceRecognition {
  return new VoiceRecognition({
    locale: 'en-US',
    recognitionConstructor: FakeRecognition,
    getSource: () => source,
    ...overrides,
  })
}

describe('feature-detected voice recognition', () => {
  it('prefers the standard constructor and falls back to the Chromium prefix', () => {
    expect(detectSpeechRecognitionConstructor({ SpeechRecognition: FakeRecognition })).toBe(
      FakeRecognition,
    )
    expect(
      detectSpeechRecognitionConstructor({ webkitSpeechRecognition: FakeRecognition }),
    ).toBe(FakeRecognition)
    expect(detectSpeechRecognitionConstructor({ SpeechRecognition: 3 })).toBeNull()
    expect(detectSpeechRecognitionConstructor(null)).toBeNull()
  })

  it('reports explicit unavailability without trying to open a microphone', () => {
    const snapshots: VoiceRecognitionSnapshot[] = []
    const voice = session({ recognitionConstructor: null, onSnapshot: (value) => snapshots.push(value) })

    expect(voice.start('dictation')).toBe(false)
    expect(voice.snapshot()).toMatchObject({
      phase: 'unavailable',
      mode: null,
      interimText: '',
      finalText: '',
      errorCode: 'FLOW_VOICE_UNAVAILABLE',
    })
    expect(snapshots.at(-1)?.errorCode).toBe('FLOW_VOICE_UNAVAILABLE')
    expect(FakeRecognition.instances).toHaveLength(0)
  })

  it('keeps mode fixed, exposes interim ghost text, and dispatches one coalesced final', () => {
    FakeRecognition.instances = []
    const captures: VoiceDictationCaptureDto[] = []
    const voice = session({
      onDictationFinal: (capture) => {
        captures.push(capture)
      },
    })

    expect(voice.start('dictation')).toBe(true)
    const recognition = lastRecognition()
    expect(recognition).toMatchObject({
      lang: 'en-US',
      continuous: true,
      interimResults: true,
      maxAlternatives: 1,
      startCalls: 1,
    })

    recognition.emitResult(0, [result(false, 'hello')])
    expect(voice.snapshot()).toMatchObject({
      phase: 'listening',
      mode: 'dictation',
      interimText: 'hello',
      finalText: '',
    })
    expect(voice.start('command')).toBe(false)
    expect(voice.snapshot().errorCode).toBe('FLOW_VOICE_MODE_CHANGE_REQUIRES_STOP')

    recognition.emitResult(0, [result(true, 'hello')])
    recognition.emitResult(0, [result(true, 'hello')])
    recognition.emitResult(1, [result(true, 'world')])
    expect(voice.snapshot().finalText).toBe('hello world')
    expect(voice.snapshot().interimText).toBe('')

    voice.stop()
    expect(recognition.stopCalls).toBe(1)
    expect(voice.snapshot().phase).toBe('stopping')
    recognition.emitEnd()

    expect(captures).toHaveLength(1)
    expect(captures[0]).toMatchObject({
      sourceRevision: 4,
      sourceHash: 'hash-4',
      selection,
      text: 'hello world',
    })
    expect(voice.snapshot()).toMatchObject({ phase: 'preview', finalText: 'hello world' })
  })

  it('routes command speech only to the command callback and abort discards all buffers', () => {
    FakeRecognition.instances = []
    const dictation: VoiceDictationCaptureDto[] = []
    const commands: VoiceCommandCaptureDto[] = []
    const voice = session({
      onDictationFinal: (capture) => {
        dictation.push(capture)
      },
      onCommandFinal: (capture) => {
        commands.push(capture)
      },
    })

    expect(voice.start('command')).toBe(true)
    const recognition = lastRecognition()
    recognition.emitResult(0, [result(false, 'make bold')])
    voice.abort()

    expect(recognition.abortCalls).toBe(1)
    expect(dictation).toHaveLength(0)
    expect(commands).toHaveLength(0)
    expect(voice.snapshot()).toMatchObject({
      phase: 'idle',
      mode: null,
      interimText: '',
      finalText: '',
      errorCode: null,
    })

    expect(voice.start('command')).toBe(true)
    const nextRecognition = lastRecognition()
    nextRecognition.emitResult(0, [result(true, 'make bold')])
    nextRecognition.emitEnd()
    expect(dictation).toHaveLength(0)
    expect(commands).toHaveLength(1)
    expect(commands[0]).toMatchObject({
      locale: 'en-US',
      transcript: 'make bold',
      sourceRevision: 4,
    })
  })

  it('maps service errors to stable privacy-safe states and releases event handlers', () => {
    FakeRecognition.instances = []
    const voice = session()
    expect(voice.start('dictation')).toBe(true)
    const recognition = lastRecognition()
    recognition.emitError('not-allowed')

    expect(voice.snapshot()).toMatchObject({
      phase: 'error',
      mode: null,
      interimText: '',
      finalText: '',
      errorCode: 'FLOW_VOICE_PERMISSION_DENIED',
    })
    expect(recognition.onresult).toBeNull()
    expect(recognition.onerror).toBeNull()
    expect(recognition.onend).toBeNull()
    recognition.emitEnd()
    expect(voice.snapshot().errorCode).toBe('FLOW_VOICE_PERMISSION_DENIED')
  })
})
