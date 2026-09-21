import type {
  EditorFieldReviewDto,
  EditorFieldReviewStatusDto,
  EditorFieldValueSummaryDto,
  EditorFieldViewDto,
  EditorLocale,
  FieldDescriptorDto,
} from './editor-store.js'
import { editorMessages } from './editor-messages.js'
import type { FormSessionValueDto } from '../../persistence/indexeddb-store.js'
import type {
  FormSessionCoordinator,
  FormSessionCoordinatorSnapshot,
} from '../forms/form-session.js'

export interface FieldNavigationProps {
  readonly fields: readonly EditorFieldViewDto[]
  readonly fieldReview: readonly EditorFieldReviewDto[]
  readonly locale: EditorLocale
  readonly canonicalJson?: string | undefined
  readonly formSession?: FormSessionCoordinator | undefined
  readonly formSessionSnapshot?: FormSessionCoordinatorSnapshot | undefined
}

export function FieldNavigation({
  fields,
  fieldReview,
  locale,
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
  readonly canonicalJson?: string | undefined
  readonly formSession?: FormSessionCoordinator | undefined
  readonly formSessionSnapshot?: FormSessionCoordinatorSnapshot | undefined
}

function FieldCard({
  field,
  locale,
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
