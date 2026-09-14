import type {
  EditorFieldReviewDto,
  EditorFieldReviewStatusDto,
  EditorFieldValueSummaryDto,
  EditorFieldViewDto,
  EditorLocale,
  FieldDescriptorDto,
} from './editor-store.js'
import { editorMessages } from './editor-messages.js'

export interface FieldNavigationProps {
  readonly fields: readonly EditorFieldViewDto[]
  readonly fieldReview: readonly EditorFieldReviewDto[]
  readonly locale: EditorLocale
}

export function FieldNavigation({
  fields,
  fieldReview,
  locale,
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
        {fields.length === 0 ? (
          <p className="editor-field-empty" data-fields-empty="">
            {labels.fieldEmpty}
          </p>
        ) : (
          <div className="editor-field-list" role="list">
            {fields.map((field) => (
              <FieldCard key={field.descriptor.id} field={field} locale={locale} />
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
}

function FieldCard({ field, locale }: FieldCardProps) {
  const labels = editorMessages[locale]
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
      <FieldDetails field={field.descriptor} value={field.valueSummary} locale={locale} />
      <p className="editor-field-state" data-field-state="valid">
        {labels.fieldRequired}: {booleanLabel(field.descriptor.required, locale)} ·{' '}
        {labels.fieldReadOnly}: {booleanLabel(field.descriptor.readOnly, locale)}
      </p>
    </article>
  )
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
