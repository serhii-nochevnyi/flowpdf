import { useState } from 'react'

import type { FormattingCommandTarget } from './editor-controller.js'
import { editorMessages, capabilityReason } from './editor-messages.js'
import {
  BlockStyleControl,
  MoreBlockControls,
} from './block-controls.js'
import type {
  EditorLocale,
  EditorViewDto,
  FormattingStateDto,
} from './editor-store.js'

import './editor.css'

export interface EditorToolbarProps {
  readonly view: EditorViewDto
  readonly controller: FormattingCommandTarget & { snapshot(): { phase: string } }
  readonly locale: EditorLocale
}

export function EditorToolbar({ view, controller, locale }: EditorToolbarProps) {
  const labels = editorMessages[locale]
  const [moreOpen, setMoreOpen] = useState(() =>
    typeof window === 'undefined' ? true : window.innerWidth >= 768,
  )
  const busy = controller.snapshot().phase === 'pending'
  const selection = view.selection
  const inline = inlineCapability(view)
  const execute = (action: () => Promise<void>): void => {
    try {
      void action().finally(() => focusEditorInput())
    } catch {
      focusEditorInput()
    }
  }

  return (
    <div
      className="editor-formatting-toolbar"
      role="toolbar"
      aria-label={labels.toolbar}
      data-formatting-toolbar=""
      onKeyDown={(event) => {
        if (event.key !== 'Escape' || !moreOpen) return
        event.preventDefault()
        setMoreOpen(false)
        focusEditorInput()
      }}
    >
      <BlockStyleControl
        view={view}
        controller={controller}
        locale={locale}
        disabled={busy}
        execute={execute}
      />

      <fieldset className="editor-control-group editor-inline-mark-group">
        <legend>{labels.toolbar}</legend>
        <MarkButton
          action="bold"
          label={labels.bold}
          state={markState(view, 'bold')}
          disabled={busy || !inline.enabled}
          reason={inline.reasonKey}
          locale={locale}
          onClick={(value) =>
            execute(() =>
              controller.setInlineMark({ kind: 'bold', value }, selection, 'ui'),
            )
          }
        />
        <MarkButton
          action="italic"
          label={labels.italic}
          state={markState(view, 'italic')}
          disabled={busy || !inline.enabled}
          reason={inline.reasonKey}
          locale={locale}
          onClick={(value) =>
            execute(() =>
              controller.setInlineMark({ kind: 'italic', value }, selection, 'ui'),
            )
          }
        />
        <MarkButton
          action="underline"
          label={labels.underline}
          state={markState(view, 'underline')}
          disabled={busy || !inline.enabled}
          reason={inline.reasonKey}
          locale={locale}
          onClick={(value) =>
            execute(() =>
              controller.setInlineMark({ kind: 'underline', value }, selection, 'ui'),
            )
          }
        />
        {reasonText(locale, 'inline', inline.reasonKey)}
      </fieldset>

      <details
        className="editor-more-disclosure"
        open={moreOpen}
        data-formatting-more=""
        onToggle={(event) => setMoreOpen(event.currentTarget.open)}
      >
        <summary>{labels.moreFormatting}</summary>
        <MoreBlockControls
          view={view}
          controller={controller}
          locale={locale}
          disabled={busy}
          execute={execute}
        />
      </details>
    </div>
  )
}

interface MarkButtonProps {
  readonly action: 'bold' | 'italic' | 'underline'
  readonly label: string
  readonly state: FormattingStateDto
  readonly disabled: boolean
  readonly reason: string | null | undefined
  readonly locale: EditorLocale
  readonly onClick: (value: boolean) => void
}

function MarkButton({
  action,
  label,
  state,
  disabled,
  reason,
  locale,
  onClick,
}: MarkButtonProps) {
  return (
    <button
      type="button"
      data-control={`inline-${action}`}
      aria-pressed={pressedState(state)}
      disabled={disabled}
      aria-describedby={reasonId(`inline-${action}`, reason)}
      onMouseDown={(event) => event.preventDefault()}
      onClick={() => onClick(state !== 'on')}
    >
      {label}
    </button>
  )
}

function inlineCapability(view: EditorViewDto) {
  const range = capability(view, 'setInlineMarks')
  if (range.enabled) return range
  const pending = capability(view, 'setPendingMarks')
  if (pending.enabled && view.formatting.blockStyle !== null) return pending
  return range
}

function markState(
  view: EditorViewDto,
  mark: 'bold' | 'italic' | 'underline',
): FormattingStateDto {
  if (
    view.selection.anchor.nodeId === view.selection.focus.nodeId &&
    view.selection.anchor.utf16Offset === view.selection.focus.utf16Offset
  ) {
    return view.pendingMarks[mark] ? 'on' : 'off'
  }
  return view.formatting[mark]
}

function capability(view: EditorViewDto, name: string) {
  return (
    view.capabilities.find((candidate) => candidate.name === name) ?? {
      enabled: true,
      reasonKey: null,
    }
  )
}

function pressedState(state: FormattingStateDto): boolean | 'mixed' {
  if (state === 'mixed') return 'mixed'
  return state === 'on'
}

function reasonId(control: string, reasonKey: string | null | undefined): string | undefined {
  return reasonKey === null || reasonKey === undefined
    ? undefined
    : `flowpdf-${control}-reason`
}

function reasonText(
  locale: EditorLocale,
  control: string,
  reasonKey: string | null | undefined,
) {
  const id = reasonId(control, reasonKey)
  return id === undefined ? null : (
    <small id={id} className="editor-control-reason">
      {capabilityReason(locale, reasonKey)}
    </small>
  )
}

function focusEditorInput(): void {
  document.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')?.focus()
}
