import type {
  VoiceCommandCaptureDto,
  VoiceDictationCaptureDto,
  VoiceDispatchOutcome,
  VoiceLocaleDto,
  VoiceSourceCaptureDto,
} from './voice-protocol.js'

export type VoiceMode = 'dictation' | 'command'

export type VoiceRecognitionPhase =
  | 'idle'
  | 'listening'
  | 'stopping'
  | 'preview'
  | 'error'
  | 'unavailable'

export interface VoiceRecognitionSnapshot {
  readonly phase: VoiceRecognitionPhase
  readonly mode: VoiceMode | null
  readonly locale: VoiceLocaleDto
  readonly sourceRevision: number | null
  readonly interimText: string
  readonly finalText: string
  readonly errorCode: string | null
}

export interface SpeechRecognitionAlternativeLike {
  readonly transcript: string
}

export interface SpeechRecognitionResultLike {
  readonly isFinal: boolean
  readonly length: number
  readonly [index: number]: SpeechRecognitionAlternativeLike
}

export interface SpeechRecognitionResultListLike {
  readonly length: number
  readonly [index: number]: SpeechRecognitionResultLike
}

export interface SpeechRecognitionResultEventLike {
  readonly resultIndex: number
  readonly results: SpeechRecognitionResultListLike
}

export interface SpeechRecognitionErrorEventLike {
  readonly error?: string
}

export interface SpeechRecognitionLike {
  lang: string
  continuous: boolean
  interimResults: boolean
  maxAlternatives: number
  onstart: (() => void) | null
  onresult: ((event: SpeechRecognitionResultEventLike) => void) | null
  onerror: ((event: SpeechRecognitionErrorEventLike) => void) | null
  onend: (() => void) | null
  start(): void
  stop(): void
  abort(): void
}

export interface SpeechRecognitionConstructorLike {
  new (): SpeechRecognitionLike
}

export interface VoiceRecognitionOptions {
  readonly locale: VoiceLocaleDto
  readonly getSource: () => VoiceSourceCaptureDto | null
  readonly recognitionConstructor?: SpeechRecognitionConstructorLike | null
  readonly onSnapshot?: (snapshot: VoiceRecognitionSnapshot) => void
  readonly onDictationFinal?: (
    capture: VoiceDictationCaptureDto,
  ) => void | Promise<VoiceDispatchOutcome | void>
  readonly onCommandFinal?: (
    capture: VoiceCommandCaptureDto,
  ) => void | Promise<VoiceDispatchOutcome | void>
}

export const MAX_VOICE_RECOGNITION_UTF16_UNITS = 32_768

const EMPTY_SNAPSHOT = (locale: VoiceLocaleDto): VoiceRecognitionSnapshot => ({
  phase: 'idle',
  mode: null,
  locale,
  sourceRevision: null,
  interimText: '',
  finalText: '',
  errorCode: null,
})

/**
 * Feature-detects the unprefixed and Chromium-prefixed Web Speech
 * constructors without requesting microphone permission.
 */
export function detectSpeechRecognitionConstructor(
  scope: unknown = globalThis,
): SpeechRecognitionConstructorLike | null {
  if (scope === null || typeof scope !== 'object') return null
  const candidateScope = scope as Record<string, unknown>
  const candidate = candidateScope.SpeechRecognition ?? candidateScope.webkitSpeechRecognition
  return typeof candidate === 'function'
    ? (candidate as SpeechRecognitionConstructorLike)
    : null
}

/** Memory-only push-to-talk recognition lifecycle. */
export class VoiceRecognition {
  private readonly localeValue: VoiceLocaleDto
  private readonly getSource: () => VoiceSourceCaptureDto | null
  private readonly recognitionConstructor: SpeechRecognitionConstructorLike | null
  private readonly onSnapshot: ((snapshot: VoiceRecognitionSnapshot) => void) | undefined
  private readonly onDictationFinal: VoiceRecognitionOptions['onDictationFinal'] | undefined
  private readonly onCommandFinal: VoiceRecognitionOptions['onCommandFinal'] | undefined
  private snapshotValue: VoiceRecognitionSnapshot
  private recognition: SpeechRecognitionLike | null = null
  private source: VoiceSourceCaptureDto | null = null
  private modeValue: VoiceMode | null = null
  private finalSegments: string[] = []
  private interimSegments = new Map<number, string>()
  private finalResultIndices = new Set<number>()
  private abortRequested = false
  private finalized = false

  constructor(options: VoiceRecognitionOptions) {
    this.localeValue = options.locale
    this.getSource = options.getSource
    this.recognitionConstructor =
      options.recognitionConstructor === undefined
        ? detectSpeechRecognitionConstructor()
        : options.recognitionConstructor
    this.onSnapshot = options.onSnapshot
    this.onDictationFinal = options.onDictationFinal
    this.onCommandFinal = options.onCommandFinal
    this.snapshotValue = EMPTY_SNAPSHOT(options.locale)
  }

  snapshot(): VoiceRecognitionSnapshot {
    return this.snapshotValue
  }

  start(mode: VoiceMode): boolean {
    if (this.snapshotValue.phase === 'listening' || this.snapshotValue.phase === 'stopping') {
      this.publish({ errorCode: 'FLOW_VOICE_MODE_CHANGE_REQUIRES_STOP' })
      return false
    }
    if (this.recognitionConstructor === null) {
      this.publish({ phase: 'unavailable', errorCode: 'FLOW_VOICE_UNAVAILABLE' })
      return false
    }
    const source = this.getSource()
    if (source === null || !isSource(source)) {
      this.publish({ phase: 'error', errorCode: 'FLOW_VOICE_SOURCE_UNAVAILABLE' })
      return false
    }

    this.resetBuffers()
    this.source = { ...source }
    this.modeValue = mode
    this.abortRequested = false
    this.finalized = false
    try {
      const recognition = new this.recognitionConstructor()
      recognition.lang = this.localeValue
      recognition.continuous = true
      recognition.interimResults = true
      recognition.maxAlternatives = 1
      this.recognition = recognition
      this.attachHandlers(recognition)
      this.publish({
        phase: 'listening',
        mode,
        sourceRevision: source.sourceRevision,
        interimText: '',
        finalText: '',
        errorCode: null,
      })
      recognition.start()
      return true
    } catch {
      this.finishError('FLOW_VOICE_UNAVAILABLE')
      return false
    }
  }

  stop(): void {
    if (this.recognition === null || this.finalized) return
    if (this.snapshotValue.phase === 'stopping') return
    if (this.snapshotValue.phase !== 'listening') return
    this.publish({ phase: 'stopping', errorCode: null })
    try {
      this.recognition.stop()
    } catch {
      this.finishError('FLOW_VOICE_STOP_FAILED')
    }
  }

  abort(): void {
    if (this.recognition === null || this.finalized) return
    this.abortRequested = true
    try {
      this.recognition.abort()
    } catch {
      this.finishError('FLOW_VOICE_ABORT_FAILED')
      return
    }
    this.finishAbort()
  }

  reset(): void {
    if (this.recognition !== null && !this.finalized) {
      this.abortRequested = true
      try {
        this.recognition.abort()
      } catch {
        // Reset still clears all in-memory speech state.
      }
    }
    this.cleanupRecognition()
    this.resetBuffers()
    this.source = null
    this.modeValue = null
    this.abortRequested = false
    this.finalized = false
    this.publish(EMPTY_SNAPSHOT(this.localeValue))
  }

  private attachHandlers(recognition: SpeechRecognitionLike): void {
    recognition.onstart = () => {
      if (this.recognition !== recognition || this.finalized) return
      this.publish({ phase: 'listening', errorCode: null })
    }
    recognition.onresult = (event) => {
      if (
        this.recognition !== recognition ||
        this.finalized ||
        (this.snapshotValue.phase !== 'listening' && this.snapshotValue.phase !== 'stopping')
      ) {
        return
      }
      this.consumeResults(event)
    }
    recognition.onerror = (event) => {
      if (this.recognition !== recognition || this.finalized) return
      if (event.error === 'aborted' || this.abortRequested) {
        this.finishAbort()
        return
      }
      this.finishError(mapRecognitionError(event.error))
    }
    recognition.onend = () => {
      if (this.recognition !== recognition || this.finalized) return
      if (this.abortRequested) {
        this.finishAbort()
        return
      }
      this.finishFinal()
    }
  }

  private consumeResults(event: SpeechRecognitionResultEventLike): void {
    if (!Number.isSafeInteger(event.resultIndex) || event.resultIndex < 0) {
      this.finishError('FLOW_VOICE_RECOGNITION_ERROR')
      return
    }
    const start = Math.max(0, Math.trunc(event.resultIndex))
    for (let index = start; index < event.results.length; index += 1) {
      const result = event.results[index]
      if (result === undefined || result.length === 0) continue
      const transcript = result[0]?.transcript?.trim() ?? ''
      if (transcript.length === 0) continue
      if (result.isFinal) {
        this.interimSegments.delete(index)
        if (this.finalResultIndices.has(index)) continue
        this.finalResultIndices.add(index)
        this.finalSegments.push(transcript)
      } else if (!this.finalResultIndices.has(index)) {
        this.interimSegments.set(index, transcript)
      }
    }
    const interimText = [...this.interimSegments.entries()]
      .sort(([left], [right]) => left - right)
      .map(([, text]) => text)
      .join(' ')
    const finalText = this.finalSegments.join(' ')
    if (
      interimText.length > MAX_VOICE_RECOGNITION_UTF16_UNITS ||
      finalText.length > MAX_VOICE_RECOGNITION_UTF16_UNITS
    ) {
      this.finishError('FLOW_VOICE_RECOGNITION_LIMIT')
      return
    }
    this.publish({
      interimText,
      finalText,
      errorCode: null,
    })
  }

  private finishFinal(): void {
    if (this.finalized) return
    this.finalized = true
    const finalText = this.finalSegments.join(' ').trim()
    const source = this.source
    const mode = this.modeValue
    this.cleanupRecognition()
    if (finalText.length === 0 || source === null || mode === null) {
      this.resetBuffers()
      this.source = null
      this.modeValue = null
      this.publish({
        phase: 'error',
        mode: null,
        sourceRevision: null,
        interimText: '',
        finalText: '',
        errorCode: 'FLOW_VOICE_NO_FINAL_TEXT',
      })
      return
    }
    this.publish({
      phase: 'preview',
      mode,
      sourceRevision: source.sourceRevision,
      interimText: '',
      finalText,
      errorCode: null,
    })
    this.resetBuffers()
    this.source = null
    this.modeValue = null
    if (mode === 'dictation') {
      if (this.onDictationFinal === undefined) return
      void Promise.resolve(this.onDictationFinal({ ...source, text: finalText })).catch(() => {
        this.publish({ phase: 'error', errorCode: 'FLOW_VOICE_DISPATCH_FAILED' })
      })
      return
    }
    if (this.onCommandFinal === undefined) return
    void Promise.resolve(
      this.onCommandFinal({ ...source, locale: this.localeValue, transcript: finalText }),
    ).catch(() => {
      this.publish({ phase: 'error', errorCode: 'FLOW_VOICE_DISPATCH_FAILED' })
    })
  }

  private finishAbort(): void {
    if (this.finalized) return
    this.finalized = true
    this.cleanupRecognition()
    this.resetBuffers()
    this.source = null
    this.modeValue = null
    this.abortRequested = false
    this.publish(EMPTY_SNAPSHOT(this.localeValue))
  }

  private finishError(code: string): void {
    if (this.finalized) return
    this.finalized = true
    this.cleanupRecognition()
    this.resetBuffers()
    this.source = null
    this.modeValue = null
    this.abortRequested = false
    this.publish({
      phase: code === 'FLOW_VOICE_UNAVAILABLE' ? 'unavailable' : 'error',
      mode: null,
      sourceRevision: null,
      interimText: '',
      finalText: '',
      errorCode: code,
    })
  }

  private cleanupRecognition(): void {
    if (this.recognition === null) return
    this.recognition.onstart = null
    this.recognition.onresult = null
    this.recognition.onerror = null
    this.recognition.onend = null
    this.recognition = null
  }

  private resetBuffers(): void {
    this.finalSegments = []
    this.interimSegments.clear()
    this.finalResultIndices.clear()
  }

  private publish(patch: Partial<VoiceRecognitionSnapshot>): void {
    this.snapshotValue = Object.freeze({
      ...this.snapshotValue,
      ...patch,
      locale: this.localeValue,
    })
    this.onSnapshot?.(this.snapshotValue)
  }
}

function mapRecognitionError(error: string | undefined): string {
  switch (error) {
    case 'not-allowed':
    case 'service-not-allowed':
      return 'FLOW_VOICE_PERMISSION_DENIED'
    case 'audio-capture':
      return 'FLOW_VOICE_AUDIO_UNAVAILABLE'
    case 'network':
      return 'FLOW_VOICE_NETWORK_ERROR'
    case 'no-speech':
      return 'FLOW_VOICE_NO_MATCH'
    case 'language-not-supported':
    case 'phrases-not-supported':
      return 'FLOW_VOICE_LOCALE_UNSUPPORTED'
    default:
      return 'FLOW_VOICE_RECOGNITION_ERROR'
  }
}

function isSource(value: VoiceSourceCaptureDto): boolean {
  return (
    Number.isSafeInteger(value.sourceRevision) &&
    value.sourceRevision >= 0 &&
    typeof value.sourceHash === 'string' &&
    value.sourceHash.trim().length > 0 &&
    value.selection !== null &&
    typeof value.selection === 'object'
  )
}
