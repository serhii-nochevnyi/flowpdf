import type {
  DirectionalSelectionDto,
  EditorAppSnapshot,
  EditorBlockViewDto,
  EditorViewDto,
} from './editor-store.js'
import type {
  SourceModality,
  StructuralCommandDto,
} from './editor-controller.js'
import { restoreDomSelection, selectionFromDom } from './selection-bridge.js'

export const MAX_COMPOSITION_UTF8_BYTES = 64 * 1024
export const MAX_COMPOSITION_UTF16_UNITS = 32_768
export const MAX_PASTE_UTF8_BYTES = 256 * 1024
export const MAX_COMMAND_ENVELOPE_BYTES = 1024 * 1024

export type InputPayloadKind = 'composition' | 'paste'

export type InputPhase = 'idle' | 'composing' | 'committing' | 'invalidated'

export interface InputAdapterSnapshot {
  readonly phase: InputPhase
  readonly candidate: string
  readonly baseRevision: number | null
  readonly selection: DirectionalSelectionDto | null
  readonly commandAttempts: number
}

export interface InputCommandTarget {
  snapshot(): EditorAppSnapshot
  replaceText(
    text: string,
    selection?: DirectionalSelectionDto,
    modality?: SourceModality,
  ): Promise<void>
  structuralCommand(command: StructuralCommandDto, modality?: SourceModality): Promise<void>
}

export interface InputAdapterOptions {
  readonly root: HTMLElement
  readonly host: HTMLTextAreaElement
  readonly getView: () => EditorViewDto | null
  readonly commandTarget: InputCommandTarget
  readonly onCandidateChange: (candidate: string) => void
  readonly onError?: ((code: string | null) => void) | undefined
}

export function validateInputPayload(
  value: string,
  kind: InputPayloadKind,
): string | null {
  if (kind === 'composition' && value.length > MAX_COMPOSITION_UTF16_UNITS) {
    return 'FLOW_COMPOSITION_LIMIT'
  }
  return validateText(
    value,
    kind === 'composition' ? MAX_COMPOSITION_UTF8_BYTES : MAX_PASTE_UTF8_BYTES,
  )
}

export class InputAdapter {
  private readonly root: HTMLElement
  private readonly host: HTMLTextAreaElement
  private readonly getView: () => EditorViewDto | null
  private readonly commandTarget: InputCommandTarget
  private readonly onCandidateChange: (candidate: string) => void
  private readonly onError: (code: string | null) => void
  private phaseValue: InputPhase = 'idle'
  private candidateValue = ''
  private baseRevisionValue: number | null = null
  private selectionValue: DirectionalSelectionDto | null = null
  private cancelRequested = false
  private browserCancelObserved = false
  private commandAttemptsValue = 0
  private submitting = false
  private lastView: EditorViewDto | null
  private pending: Promise<void> = Promise.resolve()

  constructor(options: InputAdapterOptions) {
    this.root = options.root
    this.host = options.host
    this.getView = options.getView
    this.commandTarget = options.commandTarget
    this.onCandidateChange = options.onCandidateChange
    this.onError = options.onError ?? (() => undefined)
    this.lastView = this.getView()
    this.host.value = ''

    this.host.addEventListener('beforeinput', this.onBeforeInput)
    this.host.addEventListener('input', this.onInput)
    this.host.addEventListener('paste', this.onPaste)
    this.host.addEventListener('compositionstart', this.onCompositionStart)
    this.host.addEventListener('compositionupdate', this.onCompositionUpdate)
    this.host.addEventListener('compositionend', this.onCompositionEnd)
    this.host.addEventListener('compositioncancel', this.onCompositionCancel)
    this.host.addEventListener('keydown', this.onKeyDown)
    this.root.ownerDocument.addEventListener('selectionchange', this.onSelectionChange)
  }

  snapshot(): InputAdapterSnapshot {
    return Object.freeze({
      phase: this.phaseValue,
      candidate: this.candidateValue,
      baseRevision: this.baseRevisionValue,
      selection: this.selectionValue,
      commandAttempts: this.commandAttemptsValue,
    })
  }

  whenIdle(): Promise<void> {
    return this.pending
  }

  focus(): void {
    this.host.focus()
  }

  syncAccepted(view: EditorViewDto | null): void {
    if (view === null) return
    this.lastView = view
    if (
      this.phaseValue === 'composing' &&
      this.baseRevisionValue !== null &&
      view.revision !== this.baseRevisionValue
    ) {
      this.invalidate('FLOW_STALE_COMPOSITION')
      return
    }
    if (this.phaseValue === 'invalidated') {
      this.finishIdle(view)
      return
    }
    if (this.phaseValue === 'idle') {
      queueMicrotask(() => {
        restoreDomSelection(this.root, view, view.selection)
      })
    }
  }

  dispose(): void {
    this.host.removeEventListener('beforeinput', this.onBeforeInput)
    this.host.removeEventListener('input', this.onInput)
    this.host.removeEventListener('paste', this.onPaste)
    this.host.removeEventListener('compositionstart', this.onCompositionStart)
    this.host.removeEventListener('compositionupdate', this.onCompositionUpdate)
    this.host.removeEventListener('compositionend', this.onCompositionEnd)
    this.host.removeEventListener('compositioncancel', this.onCompositionCancel)
    this.host.removeEventListener('keydown', this.onKeyDown)
    this.root.ownerDocument.removeEventListener('selectionchange', this.onSelectionChange)
  }

  private readonly onBeforeInput = (event: Event): void => {
    const input = event as InputEvent
    if (this.phaseValue === 'composing') {
      if (input.inputType === 'insertCompositionText' || input.isComposing) return
      if (input.inputType === 'insertText') {
        if (!event.cancelable) {
          this.invalidate('FLOW_UNEXPECTED_INPUT')
          return
        }
        event.preventDefault()
        const view = this.currentView()
        if (view === null) {
          this.invalidate('FLOW_NO_ACTIVE_DOCUMENT')
          return
        }
        const candidate = input.data ?? this.candidateValue
        if (!this.acceptCandidate(candidate)) {
          this.invalidate('FLOW_COMPOSITION_LIMIT')
          return
        }
        this.submit(
          candidate,
          this.selectionValue ?? view.selection,
          'keyboard',
          MAX_COMPOSITION_UTF8_BYTES,
        )
        return
      }
      this.preventIfPossible(event)
      this.invalidate('FLOW_UNEXPECTED_INPUT')
      return
    }
    if (this.phaseValue !== 'idle' || this.commandTarget.snapshot().phase !== 'ready') {
      this.preventIfPossible(event)
      return
    }

    if (input.inputType === 'insertCompositionText' || input.isComposing) return
    if (input.inputType === 'insertFromPaste') return

    const view = this.currentView()
    if (view === null) {
      this.preventIfPossible(event)
      return
    }

    if (
      input.inputType === 'insertText' ||
      input.inputType === 'insertReplacementText'
    ) {
      if (!event.cancelable) {
        this.invalidate('FLOW_UNEXPECTED_INPUT')
        return
      }
      event.preventDefault()
      const text = input.data ?? ''
      const selection = this.currentSelection(view)
      this.submit(text, selection, 'keyboard', MAX_PASTE_UTF8_BYTES)
      return
    }

    if (input.inputType === 'insertLineBreak') {
      if (!event.cancelable) {
        this.invalidate('FLOW_UNEXPECTED_INPUT')
        return
      }
      event.preventDefault()
      const command = this.structuralCommandFor('Enter', view)
      if (command === null) {
        this.onError('FLOW_STRUCTURAL_SELECTION_REQUIRED')
        return
      }
      this.submitStructural(command, 'keyboard', view)
      return
    }

    if (
      input.inputType === 'deleteContentBackward' ||
      input.inputType === 'deleteContentForward'
    ) {
      if (!event.cancelable) {
        this.invalidate('FLOW_UNEXPECTED_INPUT')
        return
      }
      event.preventDefault()
      const selection = this.deletionSelection(
        view,
        input.inputType === 'deleteContentBackward',
      )
      this.submit('', selection, 'keyboard', MAX_PASTE_UTF8_BYTES)
    }
  }

  private readonly onInput = (event: Event): void => {
    const input = event as InputEvent
    if (
      this.phaseValue === 'composing' &&
      (input.inputType === 'insertCompositionText' || input.isComposing)
    ) {
      const candidate = input.data ?? this.host.value
      if (this.acceptCandidate(candidate)) this.setCandidate(candidate)
      return
    }
    if (this.phaseValue === 'committing') {
      this.clearHost()
      return
    }
    this.invalidate('FLOW_UNEXPECTED_INPUT')
  }

  private readonly onPaste = (event: ClipboardEvent): void => {
    if (this.phaseValue !== 'idle' || this.commandTarget.snapshot().phase !== 'ready') {
      this.preventIfPossible(event)
      return
    }
    const view = this.currentView()
    if (view === null) {
      this.preventIfPossible(event)
      return
    }
    if (event.cancelable) event.preventDefault()
    const text = event.clipboardData?.getData('text/plain') ?? ''
    const selection = this.currentSelection(view)
    this.submit(text, selection, 'keyboard', MAX_PASTE_UTF8_BYTES)
  }

  private readonly onCompositionStart = (): void => {
    if (this.phaseValue !== 'idle' || this.commandTarget.snapshot().phase !== 'ready') {
      this.invalidate('FLOW_INPUT_NOT_READY')
      return
    }
    const view = this.currentView()
    if (view === null) {
      this.invalidate('FLOW_NO_ACTIVE_DOCUMENT')
      return
    }
    this.lastView = view
    this.baseRevisionValue = view.revision
    this.selectionValue = this.currentSelection(view)
    this.cancelRequested = false
    this.browserCancelObserved = false
    this.phaseValue = 'composing'
    this.setCandidate('')
    this.onError(null)
  }

  private readonly onCompositionUpdate = (event: CompositionEvent): void => {
    if (this.phaseValue !== 'composing') return
    if (!this.acceptCandidate(event.data)) {
      this.invalidate('FLOW_COMPOSITION_LIMIT')
      return
    }
    this.setCandidate(event.data)
  }

  private readonly onCompositionEnd = (event: CompositionEvent): void => {
    if (this.phaseValue === 'invalidated') {
      this.finishIdle(this.currentView())
      return
    }
    if (this.phaseValue !== 'composing') return
    const candidate = event.data
    if (!this.acceptCandidate(candidate)) {
      this.invalidate('FLOW_COMPOSITION_LIMIT')
      this.finishIdle(this.currentView())
      return
    }
    if (this.cancelRequested || this.browserCancelObserved) {
      this.finishIdle(this.currentView())
      return
    }
    const view = this.currentView()
    if (
      view === null ||
      this.baseRevisionValue === null ||
      view.revision !== this.baseRevisionValue
    ) {
      this.invalidate('FLOW_STALE_COMPOSITION')
      this.finishIdle(view)
      return
    }
    const selection = this.selectionValue ?? view.selection
    this.submit(candidate, selection, 'keyboard', MAX_COMPOSITION_UTF8_BYTES)
  }

  private readonly onCompositionCancel = (): void => {
    if (this.phaseValue !== 'composing') return
    this.browserCancelObserved = true
    this.finishIdle(this.currentView())
  }

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    if (event.key === 'Escape' && this.phaseValue === 'composing') {
      event.preventDefault()
      this.cancelRequested = true
      this.finishIdle(this.currentView())
      return
    }
    if (
      this.phaseValue !== 'idle' ||
      this.commandTarget.snapshot().phase !== 'ready' ||
      event.isComposing ||
      !['Enter', 'Backspace', 'Delete'].includes(event.key)
    ) {
      return
    }
    const view = this.currentView()
    if (view === null) return
    const command = this.structuralCommandFor(event.key, view)
    if (command === null) {
      if (event.key === 'Enter') {
        this.preventIfPossible(event)
        this.onError('FLOW_STRUCTURAL_SELECTION_REQUIRED')
      }
      return
    }
    if (!event.cancelable) {
      this.invalidate('FLOW_UNEXPECTED_INPUT')
      return
    }
    event.preventDefault()
    this.submitStructural(command, 'keyboard', view)
  }

  private readonly onSelectionChange = (): void => {
    const view = this.currentView()
    if (view === null) return
    const selection = selectionFromDom(this.root, view)
    if (selection !== null) this.selectionValue = selection
  }

  private currentView(): EditorViewDto | null {
    const view = this.getView()
    if (view !== null) this.lastView = view
    return view ?? this.lastView
  }

  private currentSelection(view: EditorViewDto): DirectionalSelectionDto {
    const selection = selectionFromDom(this.root, view)
    if (selection !== null) {
      this.selectionValue = selection
      return selection
    }
    return this.selectionValue ?? view.selection
  }

  private deletionSelection(
    view: EditorViewDto,
    backward: boolean,
  ): DirectionalSelectionDto {
    const selection = this.currentSelection(view)
    if (
      selection.anchor.nodeId !== selection.focus.nodeId ||
      selection.anchor.utf16Offset !== selection.focus.utf16Offset
    ) {
      return selection
    }
    const block = findBlock(view.document.blocks, selection.focus.nodeId)
    const spans = block?.spans
    if (spans === undefined) return selection
    const boundaries = uniqueBoundaries(spans)
    const index = boundaries.indexOf(selection.focus.utf16Offset)
    if (index < 0) return selection
    const neighbor = boundaries[backward ? index - 1 : index + 1]
    if (neighbor === undefined) return selection
    const edge = {
      ...selection.focus,
      utf16Offset: neighbor,
      affinity: backward ? 'forward' : 'backward',
    } as const
    return backward
      ? { anchor: edge, focus: selection.focus }
      : { anchor: selection.focus, focus: edge }
  }

  dispatchSplit(modality: SourceModality = 'ui'): void {
    const view = this.currentView()
    const command = view === null ? null : this.structuralCommandFor('Enter', view)
    if (view === null || command === null) {
      this.onError('FLOW_STRUCTURAL_SELECTION_REQUIRED')
      return
    }
    this.submitStructural(command, modality, view)
  }

  dispatchMerge(
    direction: 'previous' | 'next',
    modality: SourceModality = 'ui',
  ): void {
    const view = this.currentView()
    const command =
      view === null
        ? null
        : this.structuralCommandFor(direction === 'previous' ? 'Backspace' : 'Delete', view)
    if (view === null || command === null) {
      this.onError('FLOW_INCOMPATIBLE_STRUCTURE')
      return
    }
    this.submitStructural(command, modality, view)
  }

  private structuralCommandFor(
    key: string,
    view: EditorViewDto,
  ): StructuralCommandDto | null {
    const selection = this.currentSelection(view)
    if (
      selection.anchor.nodeId !== selection.focus.nodeId ||
      selection.anchor.utf16Offset !== selection.focus.utf16Offset
    ) {
      return null
    }
    const block = findBlock(view.document.blocks, selection.focus.nodeId)
    if (block === undefined || !isTextBlock(block)) return null
    if (key === 'Enter') {
      return {
        type: 'splitTextBlock',
        nodeId: block.nodeId,
        utf16Offset: selection.focus.utf16Offset,
        newNodeId: deterministicSplitNodeId(view, block.nodeId, selection.focus.utf16Offset),
      }
    }
    const backward = key === 'Backspace'
    const atBoundary = backward
      ? selection.focus.utf16Offset === 0
      : selection.focus.utf16Offset === block.text.length
    if (!atBoundary) return null
    const neighbor = adjacentTextBlock(view.document.blocks, block.nodeId, backward ? -1 : 1)
    if (neighbor === undefined) return null
    return backward
      ? {
          type: 'mergeTextBlocks',
          firstNodeId: neighbor.nodeId,
          secondNodeId: block.nodeId,
        }
      : {
          type: 'mergeTextBlocks',
          firstNodeId: block.nodeId,
          secondNodeId: neighbor.nodeId,
        }
  }

  private submit(
    text: string,
    selection: DirectionalSelectionDto,
    modality: SourceModality,
    maxUtf8Bytes: number,
  ): void {
    if (this.submitting || this.phaseValue === 'committing') {
      this.invalidate('FLOW_INPUT_BUSY')
      return
    }
    const limitError = validateText(text, maxUtf8Bytes)
    if (limitError !== null) {
      this.clearHost()
      this.onError(limitError)
      return
    }
    const envelopeBytes = new TextEncoder().encode(
      JSON.stringify({ type: 'replaceSelection', selection, text }),
    ).byteLength
    if (envelopeBytes > MAX_COMMAND_ENVELOPE_BYTES) {
      this.clearHost()
      this.onError('FLOW_COMMAND_ENVELOPE_LIMIT')
      return
    }

    this.submitting = true
    this.phaseValue = 'committing'
    this.commandAttemptsValue += 1
    this.onError(null)
    const task = this.commit(text, selection, modality)
    this.pending = task.then(
      () => undefined,
      () => undefined,
    )
  }

  private submitStructural(
    command: StructuralCommandDto,
    modality: SourceModality,
    view: EditorViewDto,
  ): void {
    if (this.submitting || this.phaseValue === 'committing') {
      this.invalidate('FLOW_INPUT_BUSY')
      return
    }
    const capabilityName =
      command.type === 'splitTextBlock'
        ? 'splitTextBlock'
        : command.type === 'mergeTextBlocks'
          ? 'mergeTextBlocks'
          : 'deleteSubtree'
    if (!capabilityEnabled(view, capabilityName)) {
      this.onError(
        command.type === 'mergeTextBlocks'
          ? 'FLOW_INCOMPATIBLE_STRUCTURE'
          : 'FLOW_STRUCTURAL_SELECTION_REQUIRED',
      )
      return
    }
    const envelopeBytes = new TextEncoder().encode(JSON.stringify(command)).byteLength
    if (envelopeBytes > MAX_COMMAND_ENVELOPE_BYTES) {
      this.onError('FLOW_COMMAND_ENVELOPE_LIMIT')
      return
    }
    this.submitting = true
    this.phaseValue = 'committing'
    this.commandAttemptsValue += 1
    this.onError(null)
    const task = this.commitStructural(command, modality)
    this.pending = task.then(
      () => undefined,
      () => undefined,
    )
  }

  private async commit(
    text: string,
    selection: DirectionalSelectionDto,
    modality: SourceModality,
  ): Promise<void> {
    try {
      await this.commandTarget.replaceText(text, selection, modality)
      const snapshot = this.commandTarget.snapshot()
      if (snapshot.phase === 'error' && snapshot.errorCode !== null) {
        this.onError(snapshot.errorCode)
      }
      const acceptedView = snapshot.accepted?.editor.view ?? this.currentView()
      if (acceptedView !== null) {
        this.lastView = acceptedView
        queueMicrotask(() => {
          restoreDomSelection(this.root, acceptedView, acceptedView.selection)
        })
      }
    } catch (error: unknown) {
      this.onError(inputErrorCode(error))
    } finally {
      this.submitting = false
      this.finishIdle(this.currentView())
    }
  }

  private async commitStructural(
    command: StructuralCommandDto,
    modality: SourceModality,
  ): Promise<void> {
    try {
      await this.commandTarget.structuralCommand(command, modality)
      const snapshot = this.commandTarget.snapshot()
      if (snapshot.phase === 'error' && snapshot.errorCode !== null) {
        this.onError(snapshot.errorCode)
      }
      const acceptedView = snapshot.accepted?.editor.view ?? this.currentView()
      if (acceptedView !== null) {
        this.lastView = acceptedView
        queueMicrotask(() => {
          restoreDomSelection(this.root, acceptedView, acceptedView.selection)
        })
      }
    } catch (error: unknown) {
      this.onError(inputErrorCode(error))
    } finally {
      this.submitting = false
      this.finishIdle(this.currentView())
    }
  }

  private invalidate(code: string): void {
    this.phaseValue = 'invalidated'
    this.clearHost()
    this.onError(code)
  }

  private finishIdle(view: EditorViewDto | null): void {
    this.phaseValue = 'idle'
    this.baseRevisionValue = null
    this.cancelRequested = false
    this.browserCancelObserved = false
    this.clearHost()
    if (view !== null) {
      this.lastView = view
      queueMicrotask(() => {
        restoreDomSelection(this.root, view, view.selection)
      })
    }
  }

  private setCandidate(candidate: string): void {
    this.candidateValue = candidate
    this.host.value = candidate
    this.onCandidateChange(candidate)
  }

  private clearHost(): void {
    this.setCandidate('')
  }

  private acceptCandidate(value: string): boolean {
    return validateInputPayload(value, 'composition') === null
  }

  private preventIfPossible(event: Event): void {
    if (event.cancelable) event.preventDefault()
  }
}

function uniqueBoundaries(
  spans: readonly { readonly startUtf16: number; readonly endUtf16: number }[],
): number[] {
  return Array.from(
    new Set(spans.flatMap((span) => [span.startUtf16, span.endUtf16])),
  ).sort((left, right) => left - right)
}

function isTextBlock(
  block: EditorBlockViewDto,
): block is EditorBlockViewDto & {
  readonly text: string
  readonly spans: readonly { readonly startUtf16: number; readonly endUtf16: number }[]
} {
  return (
    (block.kind === 'paragraph' || block.kind === 'heading') &&
    typeof block.text === 'string' &&
    Array.isArray(block.spans)
  )
}

function findBlock(
  blocks: readonly EditorBlockViewDto[],
  nodeId: string,
): EditorBlockViewDto | undefined {
  for (const block of blocks) {
    if (block.nodeId === nodeId) return block
    const nested = findBlock(block.children ?? [], nodeId)
    if (nested !== undefined) return nested
  }
  return undefined
}

function textBlocksInPreorder(
  blocks: readonly EditorBlockViewDto[],
): Array<EditorBlockViewDto & { readonly text: string }> {
  const result: Array<EditorBlockViewDto & { readonly text: string }> = []
  for (const block of blocks) {
    if (isTextBlock(block)) result.push(block)
    result.push(...textBlocksInPreorder(block.children ?? []))
  }
  return result
}

function adjacentTextBlock(
  blocks: readonly EditorBlockViewDto[],
  nodeId: string,
  direction: -1 | 1,
): (EditorBlockViewDto & { readonly text: string }) | undefined {
  const textBlocks = textBlocksInPreorder(blocks)
  const index = textBlocks.findIndex((block) => block.nodeId === nodeId)
  if (index < 0) return undefined
  return textBlocks[index + direction]
}

function capabilityEnabled(view: EditorViewDto, name: string): boolean {
  const capability = view.capabilities.find((candidate) => candidate.name === name)
  return capability?.enabled ?? true
}

function deterministicSplitNodeId(
  view: EditorViewDto,
  nodeId: string,
  utf16Offset: number,
): string {
  const source = `${view.documentId}:${view.revision}:${nodeId}:${utf16Offset}`
  let first = 0x811c9dc5
  let second = 0x9e3779b9
  let third = 0x85ebca6b
  let fourth = 0xc2b2ae35
  for (let index = 0; index < source.length; index += 1) {
    const code = source.charCodeAt(index)
    first = Math.imul(first ^ code, 0x01000193)
    second = Math.imul(second ^ (code + index), 0x01000193)
    third = Math.imul(third ^ (code * 31 + index), 0x01000193)
    fourth = Math.imul(fourth ^ (code * 131 + index), 0x01000193)
  }
  const hex = [first, second, third, fourth]
    .map((value) => (value >>> 0).toString(16).padStart(8, '0'))
    .join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-4${hex.slice(13, 16)}-8${hex.slice(17, 20)}-${hex.slice(20, 32)}`
}

function validateText(value: string, maxUtf8Bytes: number): string | null {
  if (hasUnpairedSurrogate(value)) return 'FLOW_INVALID_UTF16_BOUNDARY'
  if (new TextEncoder().encode(value).byteLength > maxUtf8Bytes) {
    return maxUtf8Bytes === MAX_PASTE_UTF8_BYTES
      ? 'FLOW_PASTE_LIMIT'
      : 'FLOW_COMPOSITION_LIMIT'
  }
  return null
}

function hasUnpairedSurrogate(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const codePoint = value.codePointAt(index)
    if (codePoint === undefined) return true
    if (codePoint >= 0xd800 && codePoint <= 0xdfff) return true
    if (codePoint > 0xffff) {
      index += 1
    }
  }
  return false
}

function inputErrorCode(error: unknown): string {
  if (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof error.code === 'string'
  ) {
    return error.code
  }
  return 'FLOW_INPUT_COMMAND_FAILED'
}
