import type { MouseEvent } from 'react'

import type { FormattingCommandTarget } from './editor-controller.js'
import { capabilityReason, editorMessages } from './editor-messages.js'
import type {
  AlignmentDto,
  BlockStyleDto,
  EditorLocale,
  EditorViewDto,
  FontFamilyDto,
  ListKindDto,
} from './editor-store.js'

export interface BlockControlsProps {
  readonly view: EditorViewDto
  readonly controller: FormattingCommandTarget
  readonly locale: EditorLocale
  readonly disabled: boolean
  readonly execute: (action: () => Promise<void>) => void
}

export function BlockStyleControl({
  view,
  controller,
  locale,
  disabled,
  execute,
}: BlockControlsProps) {
  const labels = editorMessages[locale]
  const formatting = view.formatting
  const styleCapability = capability(view, 'setBlockAttributes')
  return (
    <label className="editor-control editor-block-style-control">
      <span>{labels.blockStyle}</span>
      <select
        data-control="block-style"
        value={blockStyleValue(formatting.blockStyle)}
        disabled={disabled || !styleCapability.enabled}
        aria-describedby={reasonId('block-style', styleCapability.reasonKey)}
        onChange={(event) => {
          const style = parseBlockStyle(event.currentTarget.value)
          if (style === null) return
          execute(() => controller.setBlockStyle(style, view.selection, 'ui'))
        }}
      >
        {formatting.blockStyle === null ? (
          <option value="mixed" disabled>
            {labels.mixedBlockStyle}
          </option>
        ) : null}
        <option value="paragraph">{labels.paragraph}</option>
        {[1, 2, 3, 4, 5, 6].map((level) => (
          <option key={level} value={`heading:${level}`}>
            {labels.heading} {level}
          </option>
        ))}
      </select>
      {reasonText(locale, 'block-style', styleCapability.reasonKey)}
    </label>
  )
}

export function BlockControls({
  view,
  controller,
  locale,
  disabled,
  execute,
}: BlockControlsProps) {
  const labels = editorMessages[locale]
  const formatting = view.formatting
  const styleCapability = capability(view, 'setBlockAttributes')
  const listCapability = capability(view, 'setListKind')
  const itemCapability = capability(view, 'indentListItem')
  const outdentCapability = capability(view, 'outdentListItem')
  const selection = view.selection
  const listItemId = formatting.listItemId

  return (
    <div className="editor-block-controls" data-block-controls="">
      <fieldset className="editor-control-group editor-alignment-group">
        <legend>{labels.alignment}</legend>
        {(
          [
            ['start', labels.alignStart],
            ['center', labels.alignCenter],
            ['end', labels.alignEnd],
            ['justify', labels.alignJustify],
          ] as const
        ).map(([alignment, label]) => (
          <button
            key={alignment}
            type="button"
            data-control={`alignment-${alignment}`}
            aria-pressed={pressedState(formatting.alignment, alignment)}
            disabled={disabled || !styleCapability.enabled}
            aria-describedby={reasonId(`alignment-${alignment}`, styleCapability.reasonKey)}
            onMouseDown={preserveEditorSelection}
            onClick={() =>
              execute(() =>
                controller.setBlockAttributes({ alignment }, selection, 'ui'),
              )
            }
          >
            {label}
          </button>
        ))}
        {reasonText(locale, 'alignment', styleCapability.reasonKey)}
      </fieldset>

      <fieldset className="editor-control-group editor-spacing-group">
        <legend>{labels.paragraphSpacing}</legend>
        <label className="editor-control editor-number-control">
          <span>{labels.spacingBefore}</span>
          <input
            key={`before-${view.revision}-${formatting.spacingBeforeMillipoints ?? 'mixed'}`}
            type="number"
            min="0"
            max="144000"
            step="1"
            defaultValue={formatting.spacingBeforeMillipoints ?? ''}
            data-control="spacing-before"
            aria-describedby={reasonId('spacing-before', styleCapability.reasonKey)}
            disabled={disabled || !styleCapability.enabled}
            onKeyDown={(event) => {
              if (event.key !== 'Enter') return
              event.preventDefault()
              dispatchSpacing(
                event.currentTarget.value,
                'spacingBeforeMillipoints',
                controller,
                selection,
                execute,
              )
            }}
            onBlur={(event) =>
              dispatchSpacing(
                event.currentTarget.value,
                'spacingBeforeMillipoints',
                controller,
                selection,
                execute,
              )
            }
          />
        </label>
        <label className="editor-control editor-number-control">
          <span>{labels.spacingAfter}</span>
          <input
            key={`after-${view.revision}-${formatting.spacingAfterMillipoints ?? 'mixed'}`}
            type="number"
            min="0"
            max="144000"
            step="1"
            defaultValue={formatting.spacingAfterMillipoints ?? ''}
            data-control="spacing-after"
            aria-describedby={reasonId('spacing-after', styleCapability.reasonKey)}
            disabled={disabled || !styleCapability.enabled}
            onKeyDown={(event) => {
              if (event.key !== 'Enter') return
              event.preventDefault()
              dispatchSpacing(
                event.currentTarget.value,
                'spacingAfterMillipoints',
                controller,
                selection,
                execute,
              )
            }}
            onBlur={(event) =>
              dispatchSpacing(
                event.currentTarget.value,
                'spacingAfterMillipoints',
                controller,
                selection,
                execute,
              )
            }
          />
        </label>
        {reasonText(locale, 'spacing', styleCapability.reasonKey)}
      </fieldset>

      <fieldset className="editor-control-group editor-list-group">
        <legend>{labels.list}</legend>
        <button
          type="button"
          data-control="list-unordered"
          aria-pressed={listPressed(formatting.listKind, 'unordered')}
          disabled={disabled || !listCapability.enabled}
          aria-describedby={reasonId('list', listCapability.reasonKey)}
          onMouseDown={preserveEditorSelection}
          onClick={() =>
            execute(() =>
              controller.setListKind(listTarget(formatting.listKind, 'unordered'), selection, 'ui'),
            )
          }
        >
          {labels.unorderedList}
        </button>
        <button
          type="button"
          data-control="list-ordered"
          aria-pressed={listPressed(formatting.listKind, 'ordered')}
          disabled={disabled || !listCapability.enabled}
          aria-describedby={reasonId('list', listCapability.reasonKey)}
          onMouseDown={preserveEditorSelection}
          onClick={() =>
            execute(() =>
              controller.setListKind(listTarget(formatting.listKind, 'ordered'), selection, 'ui'),
            )
          }
        >
          {labels.orderedList}
        </button>
        <button
          type="button"
          data-control="list-indent"
          disabled={disabled || !itemCapability.enabled || listItemId === null}
          aria-describedby={reasonId('list-indent', itemCapability.reasonKey)}
          onMouseDown={preserveEditorSelection}
          onClick={() => {
            if (listItemId !== null) execute(() => controller.indentListItem(listItemId, 'ui'))
          }}
        >
          {labels.indentListItem}
        </button>
        <button
          type="button"
          data-control="list-outdent"
          disabled={disabled || !outdentCapability.enabled || listItemId === null}
          aria-describedby={reasonId('list-outdent', outdentCapability.reasonKey)}
          onMouseDown={preserveEditorSelection}
          onClick={() => {
            if (listItemId !== null) execute(() => controller.outdentListItem(listItemId, 'ui'))
          }}
        >
          {labels.outdentListItem}
        </button>
        {reasonText(locale, 'list', listCapability.reasonKey)}
      </fieldset>
    </div>
  )
}

export function MoreBlockControls({
  view,
  controller,
  locale,
  disabled,
  execute,
}: BlockControlsProps) {
  const labels = editorMessages[locale]
  const formatting = view.formatting
  const selection = view.selection
  const collapsed = selection.anchor.nodeId === selection.focus.nodeId &&
    selection.anchor.utf16Offset === selection.focus.utf16Offset
  const inlineFontFamily = collapsed ? view.pendingMarks.fontFamily : formatting.fontFamily
  const inlineFontSize = collapsed
    ? view.pendingMarks.fontSizeMillipoints
    : formatting.fontSizeMillipoints
  const inlineColor = collapsed ? view.pendingMarks.color : formatting.color
  const inlineLanguage = collapsed ? view.pendingMarks.language : formatting.language
  const capabilityValue = inlineCapability(view)
  const blockCapability = capability(view, 'setBlockAttributes')
  return (
    <div className="editor-more-controls" data-more-controls="">
      <label className="editor-control">
        <span>{labels.fontFamily}</span>
        <select
          data-control="font-family"
          value={fontFamilyValue(inlineFontFamily)}
          disabled={disabled || !capabilityValue.enabled}
          aria-describedby={reasonId('font-family', capabilityValue.reasonKey)}
          onChange={(event) => {
            const family = parseFontFamily(event.currentTarget.value)
            if (family === undefined) return
            execute(() =>
              controller.setInlineMark(
                { kind: 'fontFamily', value: family },
                selection,
                'ui',
              ),
            )
          }}
        >
          {inlineFontFamily === null ? (
            <option value="mixed" disabled>
              {labels.mixedValues}
            </option>
          ) : null}
          <option value="notoSans">{labels.plainFont}</option>
          <option value="notoSerif">{labels.serifFont}</option>
          <option value="notoSansMono">{labels.monoFont}</option>
          {inlineFontFamily?.kind === 'legacyUnknown' ? (
            <option value={fontFamilyValue(inlineFontFamily)}>
              {inlineFontFamily.original}
            </option>
          ) : null}
        </select>
        {reasonText(locale, 'font-family', capabilityValue.reasonKey)}
      </label>

      <label className="editor-control">
        <span>{labels.fontSize}</span>
        <input
          key={`font-size-${view.revision}-${inlineFontSize ?? 'mixed'}`}
          data-control="font-size"
          list="flowpdf-font-size-options"
          type="number"
          min="6000"
          max="288000"
          step="1000"
          defaultValue={inlineFontSize ?? ''}
          disabled={disabled || !capabilityValue.enabled}
          aria-describedby={reasonId('font-size', capabilityValue.reasonKey)}
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return
            event.preventDefault()
            dispatchInlineNumber(event.currentTarget.value, controller, selection, execute)
          }}
          onBlur={(event) =>
            dispatchInlineNumber(event.currentTarget.value, controller, selection, execute)
          }
        />
        <datalist id="flowpdf-font-size-options">
          {[10000, 12000, 14000, 16000, 18000, 24000, 32000, 48000].map((size) => (
            <option key={size} value={size} />
          ))}
        </datalist>
        <small>{labels.fontSizeUnit}</small>
        {reasonText(locale, 'font-size', capabilityValue.reasonKey)}
      </label>

      <label className="editor-control">
        <span>{labels.textColor}</span>
        <input
          key={`color-${view.revision}-${inlineColor?.join('-') ?? 'mixed'}`}
          type="text"
          inputMode="text"
          data-control="text-color"
          defaultValue={inlineColor === null ? '' : colorHex(inlineColor)}
          aria-describedby={reasonId('text-color', capabilityValue.reasonKey)}
          disabled={disabled || !capabilityValue.enabled}
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return
            event.preventDefault()
            dispatchColor(event.currentTarget.value, controller, selection, execute)
          }}
          onBlur={(event) => dispatchColor(event.currentTarget.value, controller, selection, execute)}
        />
        <small>{labels.colorHint}</small>
        {reasonText(locale, 'text-color', capabilityValue.reasonKey)}
      </label>

      <label className="editor-control">
        <span>{labels.language}</span>
        <select
          data-control="language"
          value={inlineLanguage ?? 'mixed'}
          disabled={disabled || !capabilityValue.enabled}
          aria-describedby={reasonId('language', capabilityValue.reasonKey)}
          onChange={(event) => {
            if (event.currentTarget.value !== 'uk-UA' && event.currentTarget.value !== 'en-US') return
            const language = event.currentTarget.value as 'uk-UA' | 'en-US'
            execute(() =>
              controller.setInlineMark(
                { kind: 'language', value: language },
                selection,
                'ui',
              ),
            )
          }}
        >
          {inlineLanguage === null ? (
            <option value="mixed" disabled>
              {labels.mixedValues}
            </option>
          ) : null}
          <option value="uk-UA">{labels.ukrainian}</option>
          <option value="en-US">{labels.english}</option>
        </select>
        {reasonText(locale, 'language', capabilityValue.reasonKey)}
      </label>

      <BlockControls
        view={view}
        controller={controller}
        locale={locale}
        disabled={disabled || !blockCapability.enabled}
        execute={execute}
      />
    </div>
  )
}

function capability(view: EditorViewDto, name: string) {
  return (
    view.capabilities.find((candidate) => candidate.name === name) ?? {
      enabled: true,
      reasonKey: null,
    }
  )
}

function inlineCapability(view: EditorViewDto) {
  const range = capability(view, 'setInlineMarks')
  if (range.enabled) return range
  const pending = capability(view, 'setPendingMarks')
  if (pending.enabled && view.formatting.blockStyle !== null) return pending
  return range
}

function blockStyleValue(style: BlockStyleDto | null): string {
  if (style === null) return 'mixed'
  return style.kind === 'paragraph' ? 'paragraph' : `heading:${style.level}`
}

function parseBlockStyle(value: string): BlockStyleDto | null {
  if (value === 'paragraph') return { kind: 'paragraph' }
  if (!value.startsWith('heading:')) return null
  const level = Number(value.slice('heading:'.length))
  return Number.isInteger(level) && level >= 1 && level <= 6
    ? { kind: 'heading', level }
    : null
}

function pressedState<T extends string>(current: T | null, target: T): boolean | 'mixed' {
  return current === null ? 'mixed' : current === target
}

function listPressed(current: ListKindDto | null, target: Exclude<ListKindDto, 'none'>): boolean | 'mixed' {
  return current === null ? 'mixed' : current === target
}

function listTarget(
  current: ListKindDto | null,
  target: Exclude<ListKindDto, 'none'>,
): ListKindDto {
  return current === target ? 'none' : target
}

function fontFamilyValue(family: FontFamilyDto | null): string {
  if (family === null) return 'mixed'
  return family.kind === 'known' ? family.id : `legacy:${family.original}`
}

function parseFontFamily(value: string): FontFamilyDto | undefined {
  if (value === 'notoSans' || value === 'notoSerif' || value === 'notoSansMono') {
    return { kind: 'known', id: value }
  }
  return undefined
}

function dispatchInlineNumber(
  value: string,
  controller: FormattingCommandTarget,
  selection: EditorViewDto['selection'],
  execute: (action: () => Promise<void>) => void,
): void {
  const number = Number(value)
  if (!Number.isFinite(number)) return
  execute(() =>
    controller.setInlineMark({ kind: 'fontSize', value: Math.trunc(number) }, selection, 'ui'),
  )
}

function dispatchColor(
  value: string,
  controller: FormattingCommandTarget,
  selection: EditorViewDto['selection'],
  execute: (action: () => Promise<void>) => void,
): void {
  if (value.length === 0) {
    execute(() => controller.setInlineMark({ kind: 'color', value: null }, selection, 'ui'))
    return
  }
  execute(() => controller.setInlineMark({ kind: 'color', value }, selection, 'ui'))
}

function dispatchSpacing(
  value: string,
  key: 'spacingBeforeMillipoints' | 'spacingAfterMillipoints',
  controller: FormattingCommandTarget,
  selection: EditorViewDto['selection'],
  execute: (action: () => Promise<void>) => void,
): void {
  const number = Number(value)
  if (!Number.isFinite(number)) return
  execute(() => controller.setBlockAttributes({ [key]: Math.trunc(number) }, selection, 'ui'))
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
  return id === undefined ? null : <small id={id} className="editor-control-reason">{capabilityReason(locale, reasonKey)}</small>
}

function colorHex(color: readonly [number, number, number]): string {
  return `#${color.map((value) => value.toString(16).padStart(2, '0').toUpperCase()).join('')}`
}

function preserveEditorSelection(event: MouseEvent<HTMLButtonElement>): void {
  event.preventDefault()
}
