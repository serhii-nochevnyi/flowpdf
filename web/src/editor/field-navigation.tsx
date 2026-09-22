import { useEffect, useRef, useState, type FormEvent } from 'react'

import type {
  EditorFieldReviewDto,
  EditorFieldReviewStatusDto,
  EditorFieldValueSummaryDto,
  EditorFieldViewDto,
  EditorLocale,
  FieldDescriptorDto,
  FieldOptionDto,
} from './editor-store.js'
import { editorMessages } from './editor-messages.js'
import type { InputCommandTarget } from './input-adapter.js'
import type { FormSessionValueDto } from '../../persistence/indexeddb-store.js'
import type {
  FormSessionCoordinator,
  FormSessionCoordinatorSnapshot,
} from '../forms/form-session.js'

export interface FieldNavigationProps {
  readonly fields: readonly EditorFieldViewDto[]
  readonly fieldReview: readonly EditorFieldReviewDto[]
  readonly locale: EditorLocale
  readonly controller?: InputCommandTarget | undefined
  readonly canonicalJson?: string | undefined
  readonly formSession?: FormSessionCoordinator | undefined
  readonly formSessionSnapshot?: FormSessionCoordinatorSnapshot | undefined
}

export function FieldNavigation({
  fields,
  fieldReview,
  locale,
  controller,
  canonicalJson,
  formSession,
  formSessionSnapshot,
}: FieldNavigationProps) {
  const labels = editorMessages[locale]
  return (
    <>
      <section
        className="editor-fields-region"
        aria-labelledby="flowpdf-fields-heading"
        data-fields-region=""
      >
        <h2 id="flowpdf-fields-heading">{labels.fields}</h2>
        {formSessionSnapshot === undefined ? null : (
          <FormSessionStatus snapshot={formSessionSnapshot} locale={locale} />
        )}
        {fields.length === 0 ? (
          <p className="editor-field-empty" data-fields-empty="">
            {labels.fieldEmpty}
          </p>
        ) : (
          <div className="editor-field-list" role="list">
            {fields.map((field) => (
              <FieldCard
                key={field.descriptor.id}
                field={field}
                locale={locale}
                controller={controller}
                canonicalJson={canonicalJson}
                formSession={formSession}
                formSessionSnapshot={formSessionSnapshot}
              />
            ))}
          </div>
        )}
      </section>
      {fieldReview.length === 0 ? null : (
        <section
          className="editor-field-review-region"
          aria-labelledby="flowpdf-field-review-heading"
          data-field-review=""
        >
          <h2 id="flowpdf-field-review-heading">{labels.fieldReview}</h2>
          <p className="editor-field-review-intro">
            {locale === 'uk'
              ? 'Поля не змінюються цим редактором; перегляд зберігає вихідний стан і розміщення.'
              : 'This editor does not change fields; review keeps their original state and placement.'}
          </p>
          <div className="editor-field-list" role="list">
            {fieldReview.map((field) => (
              <ReviewCard key={field.descriptor.id} field={field} locale={locale} />
            ))}
          </div>
        </section>
      )}
    </>
  )
}

interface FieldCardProps {
  readonly field: EditorFieldViewDto
  readonly locale: EditorLocale
  readonly controller?: InputCommandTarget | undefined
  readonly canonicalJson?: string | undefined
  readonly formSession?: FormSessionCoordinator | undefined
  readonly formSessionSnapshot?: FormSessionCoordinatorSnapshot | undefined
}

function FieldCard({
  field,
  locale,
  controller,
  canonicalJson,
  formSession,
  formSessionSnapshot,
}: FieldCardProps) {
  const labels = editorMessages[locale]
  const session = formSessionSnapshot?.session ?? null
  const identity = formSessionSnapshot?.identity ?? null
  const effectiveValue = session?.overrides[field.descriptor.id] ?? field.descriptor.defaultValue
  const valueSummary =
    session === null
      ? field.valueSummary
      : valueSummaryFor(field.descriptor, effectiveValue)
  const canEdit =
    formSession !== undefined &&
    canonicalJson !== undefined &&
    identity !== null &&
    session !== null
  const clearEnabled =
    canEdit && Object.prototype.hasOwnProperty.call(session.overrides, field.descriptor.id)
  const submitValue = (value: FormSessionValueDto): void => {
    if (!canEdit || formSession === undefined || canonicalJson === undefined || identity === null) {
      return
    }
    void formSession
      .setValue(canonicalJson, identity, field.descriptor.id, value)
      .catch(() => undefined)
  }
  const clear = (): void => {
    if (!canEdit || formSession === undefined || canonicalJson === undefined || identity === null) {
      return
    }
    void formSession
      .clearValue(canonicalJson, identity, field.descriptor.id)
      .catch(() => undefined)
  }
  return (
    <article
      className="editor-field-card"
      role="listitem"
      tabIndex={0}
      data-field-id={field.descriptor.id}
      data-field-node-id={field.descriptor.anchor.original.nodeId}
      aria-labelledby={`flowpdf-field-${field.descriptor.id}`}
    >
      <h3 id={`flowpdf-field-${field.descriptor.id}`}>
        {field.descriptor.label ?? field.descriptor.name}
      </h3>
      <FieldDetails field={field.descriptor} value={valueSummary} locale={locale} />
      <p className="editor-field-state" data-field-state="valid">
        {labels.fieldRequired}: {booleanLabel(field.descriptor.required, locale)} ·{' '}
        {labels.fieldReadOnly}: {booleanLabel(field.descriptor.readOnly, locale)}
      </p>
      {controller === undefined ? null : (
        <>
          <FieldRemovalAction
            fieldId={field.descriptor.id}
            locale={locale}
            controller={controller}
          />
          <FieldDescriptorEditor
            field={field.descriptor}
            locale={locale}
            controller={controller}
          />
        </>
      )}
      {formSession === undefined ? null : (
        <FieldControl
          field={field.descriptor}
          value={effectiveValue}
          disabled={!canEdit || field.descriptor.readOnly}
          clearEnabled={clearEnabled}
          errorCode={formSessionSnapshot?.errorCode ?? null}
          locale={locale}
          onSetValue={submitValue}
          onClear={clear}
        />
      )}
    </article>
  )
}

function FieldRemovalAction({
  fieldId,
  locale,
  controller,
}: {
  readonly fieldId: string
  readonly locale: EditorLocale
  readonly controller: InputCommandTarget
}) {
  const labels = editorMessages[locale]
  const [open, setOpen] = useState(false)
  const cancelRef = useRef<HTMLButtonElement>(null)
  const busy = controller.snapshot().phase === 'pending'

  useEffect(() => {
    if (open) cancelRef.current?.focus()
  }, [open])

  const focusEditor = (): void => {
    document.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')?.focus()
  }
  const close = (): void => {
    setOpen(false)
    focusEditor()
  }
  const confirm = (): void => {
    setOpen(false)
    void controller
      .structuralCommand({ type: 'removeField', fieldId, confirmed: true }, 'ui')
      .finally(focusEditor)
  }

  return (
    <div className="editor-field-actions" data-field-actions="">
      <button
        type="button"
        className="editor-destructive-action"
        data-action="editor-field-remove"
        disabled={busy}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => setOpen(true)}
      >
        {labels.fieldRemove}
      </button>
      {open ? (
        <dialog
          open
          role="alertdialog"
          aria-modal="true"
          aria-labelledby={`flowpdf-field-remove-heading-${fieldId}`}
          aria-describedby={`flowpdf-field-remove-description-${fieldId}`}
          className="editor-dialog editor-confirm-dialog"
          data-field-remove-dialog=""
          onKeyDown={(event) => {
            if (event.key !== 'Escape') return
            event.preventDefault()
            close()
          }}
        >
          <h2 id={`flowpdf-field-remove-heading-${fieldId}`}>{labels.fieldRemoveHeading}</h2>
          <p id={`flowpdf-field-remove-description-${fieldId}`}>{labels.fieldRemoveBody}</p>
          <div className="editor-dialog-actions">
            <button
              ref={cancelRef}
              type="button"
              data-action="editor-field-remove-cancel"
              onClick={close}
            >
              {labels.fieldRemoveCancel}
            </button>
            <button
              type="button"
              className="editor-destructive-action"
              data-action="editor-field-remove-confirm"
              onClick={confirm}
            >
              {labels.fieldRemoveConfirm}
            </button>
          </div>
        </dialog>
      ) : null}
    </div>
  )
}

interface FieldDraft {
  readonly name: string
  readonly label: string
  readonly kind: FieldDescriptorDto['kind']
  readonly required: boolean
  readonly readOnly: boolean
  readonly defaultValue: FieldDescriptorDto['defaultValue']
  readonly options: readonly FieldOptionDto[]
}

function FieldDescriptorEditor({
  field,
  locale,
  controller,
}: {
  readonly field: FieldDescriptorDto
  readonly locale: EditorLocale
  readonly controller: InputCommandTarget
}) {
  const labels = editorMessages[locale]
  const [draft, setDraft] = useState<FieldDraft>(() => createFieldDraft(field))

  useEffect(() => {
    setDraft(createFieldDraft(field))
  }, [field])

  const updateOption = (index: number, patch: Partial<FieldOptionDto>): void => {
    setDraft((current) => ({
      ...current,
      options: current.options.map((option, optionIndex) =>
        optionIndex === index ? { ...option, ...patch } : option,
      ),
    }))
  }
  const removeOption = (index: number): void => {
    setDraft((current) => {
      const options = current.options.filter((_, optionIndex) => optionIndex !== index)
      return {
        ...current,
        options,
        defaultValue: normalizeDefaultValue(current.kind, current.defaultValue, options),
      }
    })
  }
  const addOption = (): void => {
    setDraft((current) => {
      const number = current.options.length + 1
      const option: FieldOptionDto = {
        id: globalThis.crypto.randomUUID(),
        label: `${labels.fieldOption} ${number}`,
        exportValue: `option-${number}`,
      }
      return { ...current, options: [...current.options, option] }
    })
  }
  const changeKind = (type: string): void => {
    setDraft((current) => {
      const kind = fieldKindFor(type, current.kind)
      const options = kind.type === 'radioGroup' || kind.type === 'select' ? current.options : []
      return {
        ...current,
        kind,
        options,
        defaultValue: normalizeDefaultValue(kind, current.defaultValue, options),
      }
    })
  }
  const submit = (event: FormEvent<HTMLFormElement>): void => {
    event.preventDefault()
    const next: FieldDescriptorDto = {
      ...field,
      name: draft.name,
      label: draft.label.trim() === '' ? null : draft.label,
      kind: draft.kind,
      required: draft.required,
      readOnly: draft.readOnly,
      defaultValue: normalizeDefaultValue(draft.kind, draft.defaultValue, draft.options),
      options: draft.options,
    }
    void controller.structuralCommand(
      { type: 'setField', fieldId: field.id, field: next },
      'ui',
    )
  }
  const acceptedSelection = controller.snapshot().accepted?.editor.view.selection ?? null
  const selectionIsCollapsed =
    acceptedSelection !== null &&
    acceptedSelection.anchor.nodeId === acceptedSelection.focus.nodeId &&
    acceptedSelection.anchor.utf16Offset === acceptedSelection.focus.utf16Offset
  const canPlaceAtCaret =
    selectionIsCollapsed &&
    field.anchor.status === 'graphemeSafe' &&
    !sameLogicalPosition(field.anchor.original, acceptedSelection?.focus ?? null)
  const placeAtCaret = (): void => {
    const selection = controller.snapshot().accepted?.editor.view.selection
    if (
      selection === undefined ||
      selection === null ||
      selection.anchor.nodeId !== selection.focus.nodeId ||
      selection.anchor.utf16Offset !== selection.focus.utf16Offset ||
      field.anchor.status !== 'graphemeSafe'
    ) {
      return
    }
    void controller.structuralCommand(
      {
        type: 'setField',
        fieldId: field.id,
        field: {
          ...field,
          anchor: { status: 'graphemeSafe', original: selection.focus },
        },
      },
      'ui',
    )
  }

  return (
    <details className="editor-field-editor" data-field-editor="">
      <summary data-action="editor-field-configure">{labels.fieldConfigure}</summary>
      <form aria-label={labels.fieldConfigure} data-field-editor-form="" onSubmit={submit}>
        <p className="editor-field-editor-description">{labels.fieldConfigureDescription}</p>
        <div className="editor-field-editor-placement">
          <p>{labels.fieldPlaceAtCaretDescription}</p>
          <button
            type="button"
            data-action="editor-field-place"
            disabled={!canPlaceAtCaret}
            aria-describedby={`flowpdf-field-place-${field.id}`}
            onClick={placeAtCaret}
          >
            {labels.fieldPlaceAtCaret}
          </button>
          <small id={`flowpdf-field-place-${field.id}`}>
            {selectionIsCollapsed
              ? labels.fieldPlaceAtCaretDescription
              : labels.fieldPlaceAtCaretRequiresCaret}
          </small>
        </div>
        <label htmlFor={`flowpdf-field-name-${field.id}`}>
          {labels.fieldName}
          <input
            id={`flowpdf-field-name-${field.id}`}
            data-field-editor-name=""
            value={draft.name}
            onChange={(event) => setDraft((current) => ({ ...current, name: event.currentTarget.value }))}
          />
        </label>
        <label htmlFor={`flowpdf-field-label-${field.id}`}>
          {labels.fieldLabel}
          <input
            id={`flowpdf-field-label-${field.id}`}
            data-field-editor-label=""
            value={draft.label}
            onChange={(event) => setDraft((current) => ({ ...current, label: event.currentTarget.value }))}
          />
        </label>
        <label htmlFor={`flowpdf-field-kind-${field.id}`}>
          {labels.fieldKind}
          <select
            id={`flowpdf-field-kind-${field.id}`}
            data-field-editor-kind=""
            value={draft.kind.type}
            onChange={(event) => changeKind(event.currentTarget.value)}
          >
            <option value="text">{labels.fieldText}</option>
            <option value="checkbox">{labels.fieldCheckbox}</option>
            <option value="radioGroup">{labels.fieldRadioGroup}</option>
            <option value="select">{labels.fieldSelect}</option>
            <option value="signature">{labels.fieldSignature}</option>
            <option value="button">{labels.fieldButton}</option>
          </select>
        </label>
        <div className="editor-field-editor-checks">
          <label>
            <input
              type="checkbox"
              data-field-editor-required=""
              checked={draft.required}
              onChange={(event) => setDraft((current) => ({ ...current, required: event.currentTarget.checked }))}
            />{' '}
            {labels.fieldRequired}
          </label>
          <label>
            <input
              type="checkbox"
              data-field-editor-readonly=""
              checked={draft.readOnly}
              onChange={(event) => setDraft((current) => ({ ...current, readOnly: event.currentTarget.checked }))}
            />{' '}
            {labels.fieldReadOnly}
          </label>
        </div>
        {draft.kind.type === 'text' ? (
          <fieldset>
            <legend>{labels.fieldText}</legend>
            <label htmlFor={`flowpdf-field-hint-${field.id}`}>
              {labels.fieldInputHint}
              <select
                id={`flowpdf-field-hint-${field.id}`}
                data-field-editor-hint=""
                value={draft.kind.inputHint}
                onChange={(event) =>
                  setDraft((current) => ({
                    ...current,
                    kind:
                      current.kind.type === 'text'
                        ? {
                            ...current.kind,
                            inputHint: event.currentTarget.value as
                              | 'plain'
                              | 'date'
                              | 'number'
                              | 'email',
                          }
                        : current.kind,
                  }))
                }
              >
                <option value="plain">{labels.fieldHintPlain}</option>
                <option value="date">{labels.fieldHintDate}</option>
                <option value="number">{labels.fieldHintNumber}</option>
                <option value="email">{labels.fieldHintEmail}</option>
              </select>
            </label>
            <label>
              <input
                type="checkbox"
                data-field-editor-multiline=""
                checked={draft.kind.multiline}
                onChange={(event) =>
                  setDraft((current) => ({
                    ...current,
                    kind:
                      current.kind.type === 'text'
                        ? { ...current.kind, multiline: event.currentTarget.checked }
                        : current.kind,
                  }))
                }
              />{' '}
              {labels.fieldMultiline}
            </label>
          </fieldset>
        ) : null}
        {draft.kind.type === 'select' ? (
          <label>
            <input
              type="checkbox"
              data-field-editor-multiple=""
              checked={draft.kind.multiple}
              onChange={(event) =>
                setDraft((current) => ({
                  ...current,
                  kind:
                    current.kind.type === 'select'
                      ? { ...current.kind, multiple: event.currentTarget.checked }
                      : current.kind,
                  defaultValue: normalizeDefaultValue(
                    current.kind.type === 'select'
                      ? { ...current.kind, multiple: event.currentTarget.checked }
                      : current.kind,
                    current.defaultValue,
                    current.options,
                  ),
                }))
              }
            />{' '}
            {labels.fieldMultiple}
          </label>
        ) : null}
        <FieldDefaultEditor
          fieldId={field.id}
          kind={draft.kind}
          options={draft.options}
          value={draft.defaultValue}
          locale={locale}
          onChange={(defaultValue) => setDraft((current) => ({ ...current, defaultValue }))}
        />
        {draft.kind.type === 'radioGroup' || draft.kind.type === 'select' ? (
          <fieldset className="editor-field-editor-options" data-field-editor-options="">
            <legend>{labels.fieldOptions}</legend>
            {draft.options.map((option, index) => (
              <div className="editor-field-editor-option" data-field-editor-option="" key={option.id}>
                <label>
                  {labels.fieldOptionLabel}
                  <input
                    data-field-editor-option-label=""
                    value={option.label}
                    onChange={(event) => updateOption(index, { label: event.currentTarget.value })}
                  />
                </label>
                <label>
                  {labels.fieldExportValue}
                  <input
                    data-field-editor-option-export=""
                    value={option.exportValue}
                    onChange={(event) => updateOption(index, { exportValue: event.currentTarget.value })}
                  />
                </label>
                <button
                  type="button"
                  data-action="editor-field-option-remove"
                  onClick={() => removeOption(index)}
                >
                  {labels.fieldRemoveOption}
                </button>
              </div>
            ))}
            <button type="button" data-action="editor-field-option-add" onClick={addOption}>
              {labels.fieldAddOption}
            </button>
          </fieldset>
        ) : null}
        <button type="submit" data-action="editor-field-save">
          {labels.fieldSave}
        </button>
      </form>
    </details>
  )
}

function sameLogicalPosition(
  left: { readonly nodeId: string; readonly utf16Offset: number; readonly affinity: string },
  right: { readonly nodeId: string; readonly utf16Offset: number; readonly affinity: string } | null,
): boolean {
  return (
    right !== null &&
    left.nodeId === right.nodeId &&
    left.utf16Offset === right.utf16Offset &&
    left.affinity === right.affinity
  )
}

function FieldDefaultEditor({
  fieldId,
  kind,
  options,
  value,
  locale,
  onChange,
}: {
  readonly fieldId: string
  readonly kind: FieldDescriptorDto['kind']
  readonly options: readonly FieldOptionDto[]
  readonly value: FieldDescriptorDto['defaultValue']
  readonly locale: EditorLocale
  readonly onChange: (value: FieldDescriptorDto['defaultValue']) => void
}) {
  const labels = editorMessages[locale]
  const id = `flowpdf-field-default-${fieldId}`
  switch (kind.type) {
    case 'text': {
      const inputType =
        kind.inputHint === 'date'
          ? 'date'
          : kind.inputHint === 'number'
            ? 'number'
            : kind.inputHint === 'email'
              ? 'email'
              : 'text'
      return (
        <label htmlFor={id}>
          {labels.fieldDefault}
          {kind.multiline ? (
            <textarea
              id={id}
              data-field-editor-default=""
              value={value.type === 'text' ? value.value : ''}
              onChange={(event) => onChange({ type: 'text', value: event.currentTarget.value })}
              rows={3}
            />
          ) : (
            <input
              id={id}
              type={inputType}
              data-field-editor-default=""
              value={value.type === 'text' ? value.value : ''}
              onChange={(event) => onChange({ type: 'text', value: event.currentTarget.value })}
            />
          )}
        </label>
      )
    }
    case 'checkbox':
      return (
        <label>
          {labels.fieldDefault}
          <input
            type="checkbox"
            data-field-editor-default=""
            checked={value.type === 'checked' && value.value}
            onChange={(event) => onChange({ type: 'checked', value: event.currentTarget.checked })}
          />
        </label>
      )
    case 'radioGroup':
    case 'select': {
      const selected = value.type === 'selected' ? value.optionIds : []
      const multiple = kind.type === 'select' && kind.multiple
      return (
        <label htmlFor={id}>
          {labels.fieldDefault}
          <select
            id={id}
            data-field-editor-default=""
            multiple={multiple}
            value={multiple ? selected : selected[0] ?? ''}
            onChange={(event) =>
              onChange({
                type: 'selected',
                optionIds: multiple
                  ? Array.from(event.currentTarget.selectedOptions, (option) => option.value)
                  : event.currentTarget.value === ''
                    ? []
                    : [event.currentTarget.value],
              })
            }
          >
            <option value="">{labels.clearValue}</option>
            {options.map((option) => (
              <option value={option.id} key={option.id}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
      )
    }
    case 'signature':
    case 'button':
      return <p className="editor-field-editor-default-status" role="status">{labels.fieldEmpty}</p>
  }
}

function createFieldDraft(field: FieldDescriptorDto): FieldDraft {
  return {
    name: field.name,
    label: field.label ?? '',
    kind: field.kind,
    required: field.required,
    readOnly: field.readOnly,
    defaultValue: field.defaultValue,
    options: field.options,
  }
}

function fieldKindFor(
  type: string,
  previous: FieldDescriptorDto['kind'],
): FieldDescriptorDto['kind'] {
  switch (type) {
    case 'text':
      return previous.type === 'text' ? previous : { type: 'text', multiline: false, inputHint: 'plain' }
    case 'checkbox':
      return { type: 'checkbox' }
    case 'radioGroup':
      return { type: 'radioGroup' }
    case 'select':
      return previous.type === 'select' ? previous : { type: 'select', multiple: false }
    case 'signature':
      return { type: 'signature' }
    case 'button':
      return { type: 'button' }
    default:
      return previous
  }
}

function normalizeDefaultValue(
  kind: FieldDescriptorDto['kind'],
  value: FieldDescriptorDto['defaultValue'],
  options: readonly FieldOptionDto[],
): FieldDescriptorDto['defaultValue'] {
  switch (kind.type) {
    case 'text':
      return value.type === 'text' || value.type === 'empty' ? value : { type: 'empty' }
    case 'checkbox':
      return value.type === 'checked' || value.type === 'empty' ? value : { type: 'empty' }
    case 'radioGroup':
      return selectedDefault(value, options, 1)
    case 'select':
      return selectedDefault(value, options, kind.multiple ? Number.POSITIVE_INFINITY : 1)
    case 'signature':
    case 'button':
      return { type: 'empty' }
  }
}

function selectedDefault(
  value: FieldDescriptorDto['defaultValue'],
  options: readonly FieldOptionDto[],
  maximum: number,
): FieldDescriptorDto['defaultValue'] {
  if (value.type === 'empty') return value
  if (value.type !== 'selected') return { type: 'empty' }
  const available = new Set(options.map((option) => option.id))
  const optionIds = value.optionIds.filter(
    (optionId, index, all) => available.has(optionId) && all.indexOf(optionId) === index,
  )
  return optionIds.length > maximum
    ? { type: 'selected', optionIds: optionIds.slice(0, maximum) }
    : { type: 'selected', optionIds }
}

function FormSessionStatus({
  snapshot,
  locale,
}: {
  readonly snapshot: FormSessionCoordinatorSnapshot
  readonly locale: EditorLocale
}) {
  const labels = editorMessages[locale]
  if (snapshot.phase === 'loading') {
    return (
      <p className="editor-field-session-status" data-form-session-status="loading" role="status">
        {labels.formSessionLoading}
      </p>
    )
  }
  if (snapshot.phase === 'error') {
    return (
      <p
        id="flowpdf-form-session-error"
        className="editor-field-session-status editor-field-session-error"
        data-form-session-status="error"
        role="alert"
      >
        {labels.formSessionError}: {snapshot.errorCode ?? 'FLOW_FORM_SESSION_UNEXPECTED'}
      </p>
    )
  }
  return null
}

function FieldControl({
  field,
  value,
  disabled,
  clearEnabled,
  errorCode,
  locale,
  onSetValue,
  onClear,
}: {
  readonly field: FieldDescriptorDto
  readonly value: FormSessionValueDto
  readonly disabled: boolean
  readonly clearEnabled: boolean
  readonly errorCode: string | null
  readonly locale: EditorLocale
  readonly onSetValue: (value: FormSessionValueDto) => void
  readonly onClear: () => void
}) {
  const labels = editorMessages[locale]
  const controlId = `flowpdf-form-control-${field.id}`
  const describedBy = errorCode === null ? undefined : 'flowpdf-form-session-error'
  const label = field.label ?? field.name
  const clear = (
    <button
      type="button"
      className="editor-field-clear"
      data-form-action="clear"
      data-form-field-id={field.id}
      disabled={disabled || !clearEnabled}
      onClick={onClear}
    >
      {labels.clearValue}
    </button>
  )

  switch (field.kind.type) {
    case 'text': {
      const text = value.type === 'text' ? value.value : ''
      const inputType =
        field.kind.inputHint === 'date'
          ? 'date'
          : field.kind.inputHint === 'number'
            ? 'number'
            : field.kind.inputHint === 'email'
              ? 'email'
              : 'text'
      return (
        <div className="editor-field-control" data-form-control-kind="text">
          <label htmlFor={controlId}>{label}</label>
          {field.kind.multiline ? (
            <textarea
              id={controlId}
              value={text}
              disabled={disabled}
              required={field.required}
              aria-describedby={describedBy}
              onChange={(event) => onSetValue({ type: 'text', value: event.currentTarget.value })}
              rows={4}
            />
          ) : (
            <input
              id={controlId}
              type={inputType}
              value={text}
              disabled={disabled}
              required={field.required}
              aria-describedby={describedBy}
              onChange={(event) => onSetValue({ type: 'text', value: event.currentTarget.value })}
            />
          )}
          {clear}
        </div>
      )
    }
    case 'checkbox':
      return (
        <div className="editor-field-control" data-form-control-kind="checkbox">
          <label htmlFor={controlId}>
            <input
              id={controlId}
              type="checkbox"
              checked={value.type === 'checked' && value.value}
              disabled={disabled}
              required={field.required}
              aria-describedby={describedBy}
              onChange={(event) =>
                onSetValue({ type: 'checked', value: event.currentTarget.checked })
              }
            />{' '}
            {label}
          </label>
          {clear}
        </div>
      )
    case 'radioGroup': {
      const selected = value.type === 'selected' ? value.optionIds : []
      return (
        <fieldset
          className="editor-field-control"
          data-form-control-kind="radioGroup"
          disabled={disabled}
          aria-describedby={describedBy}
        >
          <legend>{label}</legend>
          {field.options.map((option) => (
            <label key={option.id}>
              <input
                type="radio"
                name={field.id}
                value={option.id}
                checked={selected.includes(option.id)}
                required={field.required}
                onChange={() => onSetValue({ type: 'selected', optionIds: [option.id] })}
              />{' '}
              {option.label}
            </label>
          ))}
          {clear}
        </fieldset>
      )
    }
    case 'select': {
      const multiple = field.kind.multiple
      const selected = value.type === 'selected' ? value.optionIds : []
      return (
        <div className="editor-field-control" data-form-control-kind="select">
          <label htmlFor={controlId}>{label}</label>
          <select
            id={controlId}
            multiple={multiple}
            value={multiple ? selected : selected[0] ?? ''}
            disabled={disabled}
            required={field.required}
            aria-describedby={describedBy}
            onChange={(event) =>
              onSetValue({
                type: 'selected',
                optionIds: multiple
                  ? Array.from(event.currentTarget.selectedOptions, (option) => option.value)
                  : event.currentTarget.value === ''
                    ? []
                    : [event.currentTarget.value],
              })
            }
          >
            {!multiple && <option value="">{labels.clearValue}</option>}
            {field.options.map((option) => (
              <option key={option.id} value={option.id}>
                {option.label}
              </option>
            ))}
          </select>
          {clear}
        </div>
      )
    }
    case 'signature':
    case 'button':
      return (
        <p className="editor-field-noneditable" data-field-noneditable="" role="status">
          {labels.fieldNotEditable}
        </p>
      )
  }
}

interface ReviewCardProps {
  readonly field: EditorFieldReviewDto
  readonly locale: EditorLocale
}

function ReviewCard({ field, locale }: ReviewCardProps) {
  const labels = editorMessages[locale]
  return (
    <article
      className="editor-field-card editor-field-card-review"
      role="listitem"
      tabIndex={0}
      data-field-id={field.descriptor.id}
      data-field-node-id={field.descriptor.anchor.original.nodeId}
      data-field-status={field.status.kind}
      aria-labelledby={`flowpdf-field-review-${field.descriptor.id}`}
    >
      <h3 id={`flowpdf-field-review-${field.descriptor.id}`}>
        {field.descriptor.label ?? field.descriptor.name}
      </h3>
      <p className="editor-field-review-status" role="status">
        {reviewStatusLabel(field.status, locale)}
      </p>
      <FieldDetails field={field.descriptor} value={field.valueSummary} locale={locale} />
      <dl className="editor-field-metadata">
        <div>
          <dt>{labels.fieldPlacement}</dt>
          <dd>
            {field.descriptor.anchor.original.nodeId} ·{' '}
            {field.descriptor.anchor.original.utf16Offset}
          </dd>
        </div>
      </dl>
    </article>
  )
}

function FieldDetails({
  field,
  value,
  locale,
}: {
  readonly field: FieldDescriptorDto
  readonly value: EditorFieldValueSummaryDto
  readonly locale: EditorLocale
}) {
  const labels = editorMessages[locale]
  return (
    <dl className="editor-field-metadata">
      <div>
        <dt>{labels.fieldName}</dt>
        <dd>{field.name}</dd>
      </div>
      <div>
        <dt>{labels.fieldLabel}</dt>
        <dd>{field.label ?? labels.fieldEmpty}</dd>
      </div>
      <div>
        <dt>{labels.fieldKind}</dt>
        <dd>{fieldKindLabel(field, locale)}</dd>
      </div>
      <div>
        <dt>{labels.fieldValue}</dt>
        <dd data-field-value="">{valueLabel(value, locale)}</dd>
      </div>
      {field.options.length === 0 ? null : (
        <div>
          <dt>{labels.fieldOptions}</dt>
          <dd>
            <ul className="editor-field-options">
              {field.options.map((option) => (
                <li key={option.id} data-field-option-id={option.id}>
                  {option.label}
                </li>
              ))}
            </ul>
          </dd>
        </div>
      )}
    </dl>
  )
}

function valueSummaryFor(
  field: FieldDescriptorDto,
  value: FormSessionValueDto,
): EditorFieldValueSummaryDto {
  switch (value.type) {
    case 'empty':
      return { kind: 'empty' }
    case 'text':
      return { kind: 'text', value: value.value }
    case 'checked':
      return { kind: 'checked', value: value.value }
    case 'selected':
      return {
        kind: 'selected',
        optionIds: value.optionIds,
        labels: value.optionIds.map(
          (optionId) => field.options.find((option) => option.id === optionId)?.label ?? optionId,
        ),
      }
  }
}

function fieldKindLabel(field: FieldDescriptorDto, locale: EditorLocale): string {
  const labels = editorMessages[locale]
  switch (field.kind.type) {
    case 'text':
      return `${labels.fieldText} (${textInputHintLabel(field.kind.inputHint, locale)})`
    case 'checkbox':
      return labels.fieldCheckbox
    case 'radioGroup':
      return labels.fieldRadioGroup
    case 'select':
      return field.kind.multiple ? `${labels.fieldSelect} (${labels.fieldSelected})` : labels.fieldSelect
    case 'signature':
      return labels.fieldSignature
    case 'button':
      return labels.fieldButton
  }
}

function textInputHintLabel(
  hint: 'plain' | 'date' | 'number' | 'email',
  locale: EditorLocale,
): string {
  const labels = editorMessages[locale]
  switch (hint) {
    case 'plain':
      return labels.fieldHintPlain
    case 'date':
      return labels.fieldHintDate
    case 'number':
      return labels.fieldHintNumber
    case 'email':
      return labels.fieldHintEmail
  }
}

function valueLabel(value: EditorFieldValueSummaryDto, locale: EditorLocale): string {
  const labels = editorMessages[locale]
  switch (value.kind) {
    case 'empty':
      return labels.fieldEmpty
    case 'text':
      return value.value
    case 'checked':
      return value.value ? labels.fieldChecked : labels.fieldUnchecked
    case 'selected':
      return value.labels.length > 0 ? value.labels.join(', ') : value.optionIds.join(', ')
  }
}

function booleanLabel(value: boolean, locale: EditorLocale): string {
  const labels = editorMessages[locale]
  return value ? labels.fieldYes : labels.fieldNo
}

function reviewStatusLabel(status: EditorFieldReviewStatusDto, locale: EditorLocale): string {
  const labels = editorMessages[locale]
  switch (status.kind) {
    case 'legacyInvalid':
      return labels.fieldReviewLegacyInvalid.replace(
        '{reason}',
        status.reason === 'missingNode'
          ? labels.fieldReasonMissingNode
          : labels.fieldReasonNonGraphemeBoundary,
      )
    case 'targetDeleted':
      return labels.fieldReviewTargetDeleted
    case 'graphemeSafeTargetMissing':
      return labels.fieldReviewTargetMissing
    case 'graphemeSafePositionInvalid':
      return labels.fieldReviewPositionInvalid
  }
}
