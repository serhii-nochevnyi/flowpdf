import type { EditorLocale } from '../editor/editor-store.js'
import type { FormSessionValueDto } from '../../persistence/indexeddb-store.js'
import type {
  FormProjectionSchedulerSnapshotDto,
} from './form-projection.js'
import './form-projection.css'

export interface FormProjectionViewportProps {
  readonly projection: FormProjectionSchedulerSnapshotDto
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly locale?: EditorLocale
  readonly selectedFieldIds: readonly string[]
  readonly onSelectedFieldIdsChange: (fieldIds: readonly string[]) => void
}

/**
 * Accessible, read-only reporting for the verified derived form projection.
 * Editable values remain in the semantic editor controls; this surface only
 * reports page/widget identity and explicit review state.
 */
export function FormProjectionViewport({
  projection,
  sourceRevision,
  sourceHash,
  locale = 'uk',
  selectedFieldIds,
  onSelectedFieldIdsChange,
}: FormProjectionViewportProps) {
  const accepted = projection.accepted
  const result = accepted?.result ?? null
  const synchronized =
    projection.phase === 'ready' &&
    result !== null &&
    result.sourceRevision === sourceRevision &&
    result.sourceHash === sourceHash &&
    accepted?.request.sourceRevision === sourceRevision &&
    accepted.request.sourceHash === sourceHash &&
    accepted.request.layoutResultHash === result.layoutResultHash
  const derived = synchronized && result !== null ? result.projection : null
  const plan =
    synchronized &&
    result !== null &&
    result.formPlan !== null &&
    result.projection.review.length === 0
    ? result.formPlan
    : null
  const selected = new Set(selectedFieldIds)
  const selectedCount =
    plan === null ? 0 : plan.fields.filter((field) => selected.has(field.fieldId)).length
  const labels = locale === 'uk' ? ukLabels : enLabels
  const status = projection.errorCode !== null
    ? `${labels.error} ${projection.errorCode}`
    : synchronized && derived !== null
      ? `${labels.ready} ${sourceRevision}.`
      : projection.phase === 'pending'
        ? labels.pending
        : labels.waiting

  return (
    <section
      className="form-projection-viewport"
      data-form-projection-viewport=""
      data-form-projection-phase={projection.phase}
      data-form-projection-source-revision={derived?.sourceRevision ?? ''}
      data-form-projection-display-list-hash={synchronized ? derived?.displayListHash ?? '' : ''}
      aria-label={labels.region}
    >
      <h2 className="form-projection-title">{labels.region}</h2>
      <p
        className={`form-projection-status${
          projection.errorCode === null ? '' : ' form-projection-status-error'
        }`}
        role={projection.errorCode === null ? 'status' : 'alert'}
        aria-live="polite"
        aria-atomic="true"
        data-form-projection-status=""
      >
        {status}
      </p>
      {derived === null ? (
        <div className="form-projection-empty" data-form-projection-empty="">
          {labels.waiting}
        </div>
      ) : (
        <>
          <p data-form-projection-count="">
            {labels.widgets}: {derived.widgets.length}
          </p>
          {derived.widgets.length === 0 ? null : (
            <ol className="form-projection-widget-list" data-form-projection-widgets="">
              {derived.widgets.map((widget) => (
                <li
                  key={widget.widgetId}
                  className="form-projection-widget"
                  data-form-projection-widget=""
                  data-field-id={widget.fieldId}
                  data-widget-id={widget.widgetId}
                  data-widget-page={widget.pageIndex}
                >
                  <span className="form-projection-widget-name">
                    {widget.label ?? widget.name}
                  </span>
                  <span className="form-projection-widget-meta">
                    {labels.page} {widget.pageIndex + 1} · {valueSummary(widget.value)}
                  </span>
                </li>
              ))}
            </ol>
          )}
          {derived.review.length === 0 ? null : (
            <div className="form-projection-review" data-form-projection-review="">
              <h3>{labels.review}</h3>
              <ul>
                {derived.review.map((entry) => (
                  <li
                    key={`${entry.fieldId}-${entry.reason.kind}`}
                    data-form-projection-review-entry=""
                  >
                    <span data-form-review-field-id="">{entry.fieldId}</span>{' '}
                    <span data-form-review-reason="">{entry.reason.kind}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {plan === null ? (
            <p
              className="form-projection-selection-unavailable"
              data-form-projection-selection-unavailable=""
            >
              {derived.review.length > 0 ? labels.selectionReview : labels.selectionUnavailable}
            </p>
          ) : (
            <fieldset className="form-projection-selection" data-form-projection-selection="">
              <legend>{labels.selection}</legend>
              <p
                className="form-projection-selection-status"
                data-form-projection-selection-count=""
              >
                {labels.selected}: {selectedCount} / {plan.fields.length}
              </p>
              <div className="form-projection-selection-actions">
                <button
                  type="button"
                  data-form-projection-select-all=""
                  disabled={plan.fields.length === 0 || selectedCount === plan.fields.length}
                  onClick={() =>
                    onSelectedFieldIdsChange(plan.fields.map((field) => field.fieldId))
                  }
                >
                  {labels.selectAll}
                </button>
                <button
                  type="button"
                  data-form-projection-clear-selection=""
                  disabled={selectedCount === 0}
                  onClick={() => onSelectedFieldIdsChange([])}
                >
                  {labels.clear}
                </button>
              </div>
              {plan.fields.length === 0 ? (
                <p
                  className="form-projection-selection-empty"
                  data-form-projection-selection-empty=""
                >
                  {labels.noSelectableFields}
                </p>
              ) : (
                <ol className="form-projection-selection-list">
                  {plan.fields.map((field) => (
                    <li key={field.fieldId} className="form-projection-selection-item">
                      <label>
                        <input
                          type="checkbox"
                          checked={selected.has(field.fieldId)}
                          data-form-projection-field-selection=""
                          data-field-id={field.fieldId}
                          onChange={(event) => {
                            const next = new Set(selected)
                            if (event.target.checked) next.add(field.fieldId)
                            else next.delete(field.fieldId)
                            onSelectedFieldIdsChange(
                              plan.fields
                                .filter((candidate) => next.has(candidate.fieldId))
                                .map((candidate) => candidate.fieldId),
                            )
                          }}
                        />
                        <span>
                          {field.label ?? field.name} · {labels.page} {field.pageIndex + 1}
                        </span>
                      </label>
                    </li>
                  ))}
                </ol>
              )}
            </fieldset>
          )}
          <p className="form-projection-derived-note" data-form-projection-derived-note="">
            {labels.derived}
          </p>
        </>
      )}
    </section>
  )
}

function valueSummary(value: FormSessionValueDto): string {
  if (value.type === 'empty') return 'empty'
  if (value.type === 'text') return value.value
  if (value.type === 'checked') return value.value ? 'checked' : 'unchecked'
  return value.optionIds.length === 0 ? 'no selection' : `${value.optionIds.length} selected`
}

const ukLabels = {
  region: 'Проєкція полів документа',
  pending: 'Проєкцію полів обчислює Rust у фоні…',
  waiting: 'Очікується прийнята проєкція для поточної ревізії…',
  ready: 'Проєкцію полів синхронізовано з ревізією',
  error: 'Проєкцію полів не прийнято. Код',
  widgets: 'Віджети',
  page: 'сторінка',
  review: 'Потребують перевірки',
  selection: 'Явне сплощення під час PDF-експорту',
  selectionReview: 'Вибір сплощення недоступний: спочатку потрібно перевірити проєкцію полів.',
  selectionUnavailable: 'Вибір сплощення недоступний для цієї проєкції.',
  selected: 'Обрано',
  selectAll: 'Обрати всі',
  clear: 'Очистити вибір',
  noSelectableFields: 'Немає полів, доступних для сплощення.',
  derived: 'Це похідні дані; семантичні поля та редагування залишаються в редакторі документа.',
} as const

const enLabels = {
  region: 'Document form projection',
  pending: 'Rust is computing the form projection in the background…',
  waiting: 'Waiting for a projection matching the current revision…',
  ready: 'Form projection synchronized with revision',
  error: 'Form projection was not accepted. Code',
  widgets: 'Widgets',
  page: 'page',
  review: 'Needs review',
  selection: 'Explicit flattening for PDF export',
  selectionReview: 'Flatten selection is unavailable: review the field projection first.',
  selectionUnavailable: 'Flatten selection is unavailable for this projection.',
  selected: 'Selected',
  selectAll: 'Select all',
  clear: 'Clear selection',
  noSelectableFields: 'There are no fields available for flattening.',
  derived: 'This is derived data; semantic fields and editing remain in the document editor.',
} as const
