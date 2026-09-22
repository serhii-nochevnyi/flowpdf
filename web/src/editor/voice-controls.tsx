import { useEffect, useRef, useState, useSyncExternalStore } from 'react'

import type { EditorController } from './editor-controller.js'
import { editorMessages } from './editor-messages.js'
import type { EditorLocale } from './editor-store.js'
import {
  voiceLocaleForEditorLocale,
} from '../voice/voice-command.js'
import {
  VoiceRecognition,
  type SpeechRecognitionConstructorLike,
  type VoiceMode,
  type VoiceRecognitionSnapshot,
} from '../voice/voice-recognition.js'
import type {
  AcceptedVoiceIntentDto,
  VoiceCommandCaptureDto,
  VoiceDictationCaptureDto,
} from '../voice/voice-protocol.js'

export interface VoiceControlsProps {
  readonly controller: EditorController
  readonly locale: EditorLocale
  /** Injectable only for deterministic browser tests; undefined feature-detects the browser. */
  readonly recognitionConstructor?: SpeechRecognitionConstructorLike | null
}

interface PendingVoiceIntent {
  readonly intent: AcceptedVoiceIntentDto
  readonly transcript: string
}

interface VoiceCallbacks {
  readonly onDictationFinal: (capture: VoiceDictationCaptureDto) => void | Promise<void>
  readonly onCommandFinal: (capture: VoiceCommandCaptureDto) => void | Promise<void>
}

export function VoiceControls({
  controller,
  locale,
  recognitionConstructor,
}: VoiceControlsProps) {
  const labels = editorMessages[locale]
  const voiceLocale = voiceLocaleForEditorLocale(locale)
  const controllerSnapshot = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
    controller.getSnapshot,
  )
  const [mode, setMode] = useState<VoiceMode>('dictation')
  const [recognitionSnapshot, setRecognitionSnapshot] = useState<VoiceRecognitionSnapshot>(() =>
    idleSnapshot(voiceLocale),
  )
  const [pendingIntent, setPendingIntent] = useState<PendingVoiceIntent | null>(null)
  const [localErrorCode, setLocalErrorCode] = useState<string | null>(null)
  const [dispatching, setDispatching] = useState(false)
  const recognitionRef = useRef<VoiceRecognition | null>(null)
  const activeFieldIdRef = useRef<string | undefined>(undefined)
  const startButtonRef = useRef<HTMLButtonElement>(null)
  const cancelButtonRef = useRef<HTMLButtonElement>(null)
  const callbackRef = useRef<VoiceCallbacks>({
    onDictationFinal: () => undefined,
    onCommandFinal: () => undefined,
  })

  const resetRecognition = (): void => {
    recognitionRef.current?.reset()
  }

  const focusStart = (): void => {
    startButtonRef.current?.focus()
  }

  const finishDispatch = (code: string | null): void => {
    setLocalErrorCode(code)
    setDispatching(false)
    resetRecognition()
    setTimeout(focusStart, 0)
  }

  const dispatchIntent = async (intent: AcceptedVoiceIntentDto): Promise<void> => {
    setDispatching(true)
    try {
      const outcome = await controller.dispatchVoiceCommand(intent)
      finishDispatch(outcome.kind === 'rejected' ? outcome.code : null)
    } catch (error: unknown) {
      finishDispatch(errorCode(error))
    }
  }

  const dispatchDictation = async (capture: VoiceDictationCaptureDto): Promise<void> => {
    setDispatching(true)
    try {
      const outcome = await controller.dispatchVoiceDictation(capture)
      finishDispatch(outcome.kind === 'rejected' ? outcome.code : null)
    } catch (error: unknown) {
      finishDispatch(errorCode(error))
    }
  }

  const resolveCommand = async (capture: VoiceCommandCaptureDto): Promise<void> => {
    const staleCode = sourceDriftCode(controller, capture)
    if (staleCode !== null) {
      finishDispatch(staleCode)
      return
    }
    try {
      const intent = await controller.resolveVoiceCommand(
        capture.transcript,
        capture.selection,
        activeFieldIdRef.current,
      )
      if (intent.capability.confirmation !== 'none') {
        setPendingIntent({ intent, transcript: capture.transcript })
        return
      }
      await dispatchIntent(intent)
    } catch (error: unknown) {
      finishDispatch(errorCode(error))
    }
  }

  callbackRef.current = {
    onDictationFinal: dispatchDictation,
    onCommandFinal: resolveCommand,
  }

  useEffect(() => {
    const recognition = new VoiceRecognition({
      locale: voiceLocale,
      ...(recognitionConstructor === undefined ? {} : { recognitionConstructor }),
      getSource: () => {
        const accepted = controller.snapshot().accepted
        if (accepted === null) return null
        return {
          sourceRevision: accepted.session.revision,
          sourceHash: accepted.session.canonicalHash,
          selection: accepted.editor.session.selection,
        }
      },
      onSnapshot: setRecognitionSnapshot,
      onDictationFinal: (capture) => callbackRef.current.onDictationFinal(capture),
      onCommandFinal: (capture) => callbackRef.current.onCommandFinal(capture),
    })
    recognitionRef.current = recognition
    setRecognitionSnapshot(recognition.snapshot())
    return () => {
      recognition.reset()
      if (recognitionRef.current === recognition) recognitionRef.current = null
    }
  }, [controller, recognitionConstructor, voiceLocale])

  useEffect(() => {
    const updateActiveField = (event: FocusEvent): void => {
      const target = event.target
      if (!(target instanceof HTMLElement)) return
      if (target.closest('[data-voice-controls]') !== null) return
      const field = target.closest<HTMLElement>('[data-field-id]')
      activeFieldIdRef.current = field?.dataset.fieldId
    }
    document.addEventListener('focusin', updateActiveField)
    return () => document.removeEventListener('focusin', updateActiveField)
  }, [])

  useEffect(() => {
    if (pendingIntent !== null && !dispatching) cancelButtonRef.current?.focus()
  }, [dispatching, pendingIntent])

  const isListening =
    recognitionSnapshot.phase === 'listening' || recognitionSnapshot.phase === 'stopping'
  const busy =
    controllerSnapshot.phase === 'pending' ||
    dispatching ||
    pendingIntent !== null ||
    isListening
  const hasDocument = controllerSnapshot.accepted !== null
  const errorCodeValue = localErrorCode ?? recognitionSnapshot.errorCode
  const status = recognitionStatus(
    recognitionSnapshot,
    hasDocument,
    labels,
  )

  const start = (event?: React.MouseEvent<HTMLButtonElement>): void => {
    if (event !== undefined) event.preventDefault()
    setLocalErrorCode(null)
    const recognition = recognitionRef.current
    if (recognition === null) return
    recognition.start(mode)
  }

  const cancelPreview = (): void => {
    if (dispatching) return
    setPendingIntent(null)
    setLocalErrorCode(null)
    resetRecognition()
    focusStart()
  }

  const confirmPreview = (): void => {
    if (dispatching || pendingIntent === null) return
    const intent = pendingIntent.intent
    setPendingIntent(null)
    void dispatchIntent(intent)
  }

  return (
    <section
      className="voice-controls"
      aria-labelledby="flowpdf-voice-heading"
      data-voice-controls=""
      data-voice-phase={recognitionSnapshot.phase}
      data-voice-mode={mode}
      data-voice-error-code={errorCodeValue ?? ''}
    >
      <h2 id="flowpdf-voice-heading" className="section-heading">
        {labels.voiceTitle}
      </h2>
      <p className="secondary-text">{labels.voiceDescription}</p>
      <fieldset className="voice-mode-group">
        <legend>{labels.voiceMode}</legend>
        <label>
          <input
            type="radio"
            name="flowpdf-voice-mode"
            value="dictation"
            checked={mode === 'dictation'}
            disabled={busy}
            onChange={() => setMode('dictation')}
          />
          {labels.voiceModeDictation}
        </label>
        <label>
          <input
            type="radio"
            name="flowpdf-voice-mode"
            value="command"
            checked={mode === 'command'}
            disabled={busy}
            onChange={() => setMode('command')}
          />
          {labels.voiceModeCommand}
        </label>
      </fieldset>
      <div className="voice-action-group">
        <button
          ref={startButtonRef}
          type="button"
          className="primary-action"
          data-action="voice-start"
          disabled={busy || !hasDocument || recognitionRef.current === null}
          onClick={start}
        >
          {labels.voiceStart}
        </button>
        <button
          type="button"
          data-action="voice-stop"
          disabled={!isListening || dispatching}
          onClick={() => recognitionRef.current?.stop()}
        >
          {labels.voiceStop}
        </button>
        <button
          type="button"
          data-action="voice-abort"
          disabled={!isListening || dispatching}
          onClick={() => recognitionRef.current?.abort()}
        >
          {labels.voiceAbort}
        </button>
      </div>
      <p
        className="voice-status"
        role="status"
        aria-live="polite"
        aria-atomic="true"
        data-voice-status=""
      >
        {status}
      </p>
      {recognitionSnapshot.interimText.length === 0 ? null : (
        <p className="voice-ghost-text" data-voice-interim="" aria-live="off">
          <span className="voice-ghost-label">{labels.voiceInterim}:</span>{' '}
          <span>{recognitionSnapshot.interimText}</span>
        </p>
      )}
      {recognitionSnapshot.finalText.length === 0 ? null : (
        <p className="voice-final-text" data-voice-final="" aria-live="polite">
          <span className="voice-ghost-label">{labels.voiceFinal}:</span>{' '}
          <span>{recognitionSnapshot.finalText}</span>
        </p>
      )}
      {errorCodeValue === null ? null : (
        <p className="alert-region" role="alert" aria-atomic="true" data-voice-error="">
          {voiceErrorText(locale, errorCodeValue)} ({errorCodeValue})
        </p>
      )}
      {controllerSnapshot.voice?.phase === 'committed' ? (
        <p className="voice-feedback" data-voice-feedback="">
          {labels.voiceCommitted}
        </p>
      ) : null}
      {pendingIntent === null ? null : (
        <dialog
          open
          role="alertdialog"
          aria-modal="true"
          aria-labelledby="flowpdf-voice-preview-heading"
          aria-describedby="flowpdf-voice-preview-description"
          className="editor-dialog editor-confirm-dialog voice-preview-dialog"
          data-voice-preview=""
          onKeyDown={(event) => {
            if (event.key !== 'Escape') return
            event.preventDefault()
            cancelPreview()
          }}
        >
          <h3 id="flowpdf-voice-preview-heading">{labels.voicePreviewHeading}</h3>
          <p id="flowpdf-voice-preview-description">{labels.voicePreviewBody}</p>
          <dl className="voice-preview-details">
            <div>
              <dt>{labels.voicePreviewAction}</dt>
              <dd data-voice-preview-action="">
                {describeIntent(pendingIntent.intent, labels)}
              </dd>
            </div>
            <div>
              <dt>{labels.voicePreviewTranscript}</dt>
              <dd data-voice-preview-transcript="">{pendingIntent.transcript}</dd>
            </div>
          </dl>
          <div className="editor-dialog-actions">
            <button
              ref={cancelButtonRef}
              type="button"
              data-action="voice-preview-cancel"
              disabled={dispatching}
              onClick={cancelPreview}
            >
              {labels.voiceCancel}
            </button>
            <button
              type="button"
              className="editor-destructive-action"
              data-action="voice-preview-confirm"
              disabled={dispatching}
              onClick={confirmPreview}
            >
              {labels.voiceConfirm}
            </button>
          </div>
        </dialog>
      )}
    </section>
  )
}

function idleSnapshot(locale: VoiceRecognitionSnapshot['locale']): VoiceRecognitionSnapshot {
  return {
    phase: 'idle',
    mode: null,
    locale,
    sourceRevision: null,
    interimText: '',
    finalText: '',
    errorCode: null,
  }
}

function recognitionStatus(
  snapshot: VoiceRecognitionSnapshot,
  hasDocument: boolean,
  labels: (typeof editorMessages)[EditorLocale],
): string {
  if (!hasDocument) return labels.voiceNoDocument
  switch (snapshot.phase) {
    case 'listening':
      return `${labels.voiceStatusListening} ${modeLabel(snapshot.mode, labels)}`
    case 'stopping':
      return labels.voiceStatusStopping
    case 'preview':
      return labels.voiceStatusPreview
    case 'error':
      return labels.voiceStatusError
    case 'unavailable':
      return labels.voiceStatusUnavailable
    case 'idle':
      return labels.voiceStatusIdle
  }
}

function modeLabel(
  mode: VoiceMode | null,
  labels: (typeof editorMessages)[EditorLocale],
): string {
  return mode === 'command' ? labels.voiceModeCommand : labels.voiceModeDictation
}

function voiceErrorText(locale: EditorLocale, code: string): string {
  const labels = editorMessages[locale]
  switch (code) {
    case 'FLOW_VOICE_PERMISSION_DENIED':
      return labels.voiceErrorPermissionDenied
    case 'FLOW_VOICE_AUDIO_UNAVAILABLE':
      return labels.voiceErrorAudioUnavailable
    case 'FLOW_VOICE_NETWORK_ERROR':
      return labels.voiceErrorNetwork
    case 'FLOW_VOICE_NO_MATCH':
    case 'FLOW_VOICE_NO_FINAL_TEXT':
      return labels.voiceErrorNoMatch
    case 'FLOW_VOICE_LOCALE_UNSUPPORTED':
      return labels.voiceErrorLocale
    case 'FLOW_VOICE_SOURCE_STALE':
    case 'FLOW_VOICE_SOURCE_MISMATCH':
    case 'FLOW_VOICE_SOURCE_UNAVAILABLE':
      return labels.voiceErrorSource
    case 'FLOW_VOICE_SELECTION_STALE':
    case 'FLOW_VOICE_SELECTION_MISMATCH':
      return labels.voiceErrorSelection
    case 'FLOW_VOICE_DISPATCH_FAILED':
      return labels.voiceErrorDispatch
    case 'FLOW_VOICE_UNAVAILABLE':
      return labels.voiceStatusUnavailable
    default:
      return labels.voiceErrorFallback
  }
}

function errorCode(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'code' in error) {
    const code = (error as { readonly code?: unknown }).code
    if (typeof code === 'string' && code.trim().length > 0) return code
  }
  return 'FLOW_VOICE_DISPATCH_FAILED'
}

function sourceDriftCode(
  controller: EditorController,
  capture: Pick<VoiceCommandCaptureDto, 'sourceRevision' | 'sourceHash' | 'selection'>,
): string | null {
  const accepted = controller.snapshot().accepted
  if (accepted === null) return 'FLOW_VOICE_SOURCE_UNAVAILABLE'
  if (
    accepted.session.revision !== capture.sourceRevision ||
    accepted.session.canonicalHash !== capture.sourceHash
  ) {
    return 'FLOW_VOICE_SOURCE_STALE'
  }
  if (!sameSelection(accepted.editor.session.selection, capture.selection)) {
    return 'FLOW_VOICE_SELECTION_STALE'
  }
  return null
}

function sameSelection(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right)
}

function describeIntent(
  intent: AcceptedVoiceIntentDto,
  labels: (typeof editorMessages)[EditorLocale],
): string {
  switch (intent.action.type) {
    case 'undo':
      return labels.voiceActionUndo
    case 'redo':
      return labels.voiceActionRedo
    case 'deleteSelection':
      return labels.voiceActionDeleteSelection
    case 'setInlineMark':
      switch (intent.action.mark.kind) {
        case 'bold':
          return labels.voiceActionBold
        case 'italic':
          return labels.voiceActionItalic
        case 'underline':
          return labels.voiceActionUnderline
        default:
          return labels.voiceActionUnknown
      }
    case 'insertPageBreak':
      return labels.voiceActionInsertPageBreak
    case 'removePageBreak':
      return labels.voiceActionRemovePageBreak
    case 'navigateField':
      return intent.action.direction === 'next'
        ? labels.voiceActionNextField
        : labels.voiceActionPreviousField
    case 'clearField':
      return labels.voiceActionClearField
  }
}
