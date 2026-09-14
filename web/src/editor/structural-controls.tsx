import { useEffect, useRef, useState } from 'react'

import type {
  ConfirmationMetadataDto,
  EditorBlockViewDto,
  EditorCapabilityDto,
  EditorLocale,
  EditorViewDto,
  StructuralPlacementDto,
  TableLimitsDto,
} from './editor-store.js'
import type {
  StructuralCommandDto,
  StructuralCommandTarget,
} from './editor-controller.js'
import { capabilityReason, editorMessages } from './editor-messages.js'
import { ImageInsertDialog } from './embedded-blocks.js'

export interface StructuralControlsProps {
  readonly view: EditorViewDto
  readonly controller: StructuralCommandTarget
  readonly locale: EditorLocale
}

export interface TableActionBarProps {
  readonly block: EditorBlockViewDto
  readonly view: EditorViewDto
  readonly controller: StructuralCommandTarget
  readonly locale: EditorLocale
  readonly onRequestRemoveTable: (
    tableId: string,
    confirmation: ConfirmationMetadataDto,
  ) => void
}

export interface DestructiveConfirmDialogProps {
  readonly locale: EditorLocale
  readonly confirmation: ConfirmationMetadataDto
  readonly onCancel: () => void
  readonly onConfirm: () => void
}

export function StructuralControls({
  view,
  controller,
  locale,
}: StructuralControlsProps) {
  const labels = editorMessages[locale]
  const tableCapability = capabilityFor(view, 'insertTable')
  const pageBreakCapability = capabilityFor(view, 'insertPageBreak')
  const imageCapability = capabilityFor(view, 'insertImage')
  const busy = controller.snapshot().phase === 'pending'
  const execute = (command: StructuralCommandDto): void => {
    void controller.structuralCommand(command, 'ui').finally(focusEditorInput)
  }

  return (
    <>
      <div
        className="editor-structure-controls"
        role="toolbar"
        aria-label={labels.insert}
        data-structure-controls=""
      >
        <button
          type="button"
          data-action="editor-insert-page-break"
          disabled={
            busy || !pageBreakCapability.enabled || pageBreakCapability.placement == null
          }
          aria-describedby={reasonId('insert-page-break', pageBreakCapability.reasonKey)}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => {
            if (pageBreakCapability.placement == null) return
            execute({
              type: 'insertPageBreak',
              placement: pageBreakCapability.placement,
            })
          }}
        >
          {labels.insertPageBreak}
        </button>
        {imageCapability.placement == null ? null : (
          <ImageInsertDialog
            controller={controller}
            locale={locale}
            placement={imageCapability.placement}
            disabled={busy || !imageCapability.enabled}
          />
        )}
        {reasonText(locale, 'insert-table', tableCapability.reasonKey)}
        {reasonText(locale, 'insert-page-break', pageBreakCapability.reasonKey)}
        {reasonText(locale, 'insert-image', imageCapability.reasonKey)}
      </div>
      {tableCapability.placement == null ? null : (
        <TableInsertDialog
          controller={controller}
          locale={locale}
          limits={tableCapability.tableLimits}
          placement={tableCapability.placement}
          disabled={busy || !tableCapability.enabled}
        />
      )}
    </>
  )
}

interface TableInsertDialogProps {
  readonly controller: StructuralCommandTarget
  readonly locale: EditorLocale
  readonly limits: TableLimitsDto | null | undefined
  readonly placement: StructuralPlacementDto
  readonly disabled: boolean
}

function TableInsertDialog({
  controller,
  locale,
  limits,
  placement,
  disabled,
}: TableInsertDialogProps) {
  const labels = editorMessages[locale]
  const [open, setOpen] = useState(false)
  const [rows, setRows] = useState('2')
  const [columns, setColumns] = useState('2')
  const [headerRow, setHeaderRow] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const cancelRef = useRef<HTMLButtonElement>(null)
  const rowsRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (open) cancelRef.current?.focus()
  }, [open])

  const close = (): void => {
    setOpen(false)
    setError(null)
    focusEditorInput()
  }

  const submit = (): void => {
    const maxRows = limits?.maxRows ?? 0
    const maxColumns = limits?.maxColumns ?? 0
    const maxCells = limits?.maxCells ?? 0
    const rowCount = Number.parseInt(rows, 10)
    const columnCount = Number.parseInt(columns, 10)
    if (
      !Number.isInteger(rowCount) ||
      !Number.isInteger(columnCount) ||
      rowCount < 1 ||
      columnCount < 1 ||
      rowCount > maxRows ||
      columnCount > maxColumns ||
      rowCount * columnCount > maxCells
    ) {
      setError(
        labels.tableInvalid
          .replace('{maxRows}', String(maxRows))
          .replace('{maxColumns}', String(maxColumns)),
      )
      rowsRef.current?.focus()
      return
    }
    setOpen(false)
    setError(null)
    void controller
      .structuralCommand(
        {
          type: 'insertTable',
          placement,
          rows: rowCount,
          columns: columnCount,
          headerRow,
        },
        'ui',
      )
      .finally(focusEditorInput)
  }

  return (
    <>
      <button
        type="button"
        data-action="editor-insert-table"
        disabled={disabled}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => setOpen(true)}
      >
        {labels.insertTable}
      </button>
      {open ? (
        <dialog
          open
          role="dialog"
          aria-modal="true"
          aria-labelledby="flowpdf-table-insert-heading"
          aria-describedby="flowpdf-table-insert-description"
          className="editor-dialog"
          data-table-insert-dialog=""
          onKeyDown={(event) => {
            if (event.key !== 'Escape') return
            event.preventDefault()
            close()
          }}
        >
          <div role="group" aria-label={labels.insertTable}>
            <h2 id="flowpdf-table-insert-heading">{labels.insertTable}</h2>
            <p id="flowpdf-table-insert-description">{labels.tableInsertDescription}</p>
            <label className="editor-dialog-field">
              <span>{labels.tableRows}</span>
              <input
                ref={rowsRef}
                data-control="table-rows"
                type="number"
                min={1}
                max={limits?.maxRows}
                value={rows}
                onChange={(event) => setRows(event.currentTarget.value)}
              />
            </label>
            <label className="editor-dialog-field">
              <span>{labels.tableColumns}</span>
              <input
                data-control="table-columns"
                type="number"
                min={1}
                max={limits?.maxColumns}
                value={columns}
                onChange={(event) => setColumns(event.currentTarget.value)}
              />
            </label>
            <label className="editor-dialog-checkbox">
              <input
                data-control="table-header-row"
                type="checkbox"
                checked={headerRow}
                onChange={(event) => setHeaderRow(event.currentTarget.checked)}
              />
              <span>{labels.tableHeaderRow}</span>
            </label>
            {error === null ? null : (
              <p role="alert" className="editor-dialog-error" data-table-insert-error="">
                {error}
              </p>
            )}
            <div className="editor-dialog-actions">
              <button ref={cancelRef} type="button" data-action="editor-table-insert-cancel" onClick={close}>
                {labels.tableInsertCancel}
              </button>
              <button type="button" data-action="editor-table-insert-confirm" onClick={submit}>
                {labels.insertTable}
              </button>
            </div>
          </div>
        </dialog>
      ) : null}
    </>
  )
}

export function TableActionBar({
  block,
  view,
  controller,
  locale,
  onRequestRemoveTable,
}: TableActionBarProps) {
  const labels = editorMessages[locale]
  const tableId = block.nodeId
  const removeTable = capabilityFor(view, 'removeTable')
  if (removeTable.targetNodeId !== tableId || removeTable.confirmation == null) return null
  const busy = controller.snapshot().phase === 'pending'
  const header = capabilityFor(view, 'setTableHeaderRow')
  const addRow = capabilityFor(view, 'addTableRow')
  const removeRow = capabilityFor(view, 'removeTableRow')
  const addColumn = capabilityFor(view, 'addTableColumn')
  const removeColumn = capabilityFor(view, 'removeTableColumn')
  const execute = (command: StructuralCommandDto): void => {
    void controller.structuralCommand(command, 'ui').finally(focusEditorInput)
  }
  const selection = view.selection
  const requestRemoval = (capability: EditorCapabilityDto): void => {
    if (capability.confirmation == null) return
    onRequestRemoveTable(tableId, capability.confirmation)
  }

  return (
    <div className="editor-table-actions" data-table-action-bar="" aria-label={labels.tableActions}>
      <button
        type="button"
        data-action="editor-table-add-row"
        disabled={busy || !addRow.enabled}
        aria-describedby={reasonId('table-add-row', addRow.reasonKey)}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => execute({ type: 'addTableRow', selection })}
      >
        {labels.tableAddRow}
      </button>
      <button
        type="button"
        data-action="editor-table-remove-row"
        disabled={busy || (!removeRow.enabled && removeRow.confirmation == null)}
        aria-describedby={reasonId('table-remove-row', removeRow.reasonKey)}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() =>
          removeRow.enabled
            ? execute({ type: 'removeTableRow', selection })
            : requestRemoval(removeRow)
        }
      >
        {labels.tableRemoveRow}
      </button>
      <button
        type="button"
        data-action="editor-table-add-column"
        disabled={busy || !addColumn.enabled}
        aria-describedby={reasonId('table-add-column', addColumn.reasonKey)}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => execute({ type: 'addTableColumn', selection })}
      >
        {labels.tableAddColumn}
      </button>
      <button
        type="button"
        data-action="editor-table-remove-column"
        disabled={busy || (!removeColumn.enabled && removeColumn.confirmation == null)}
        aria-describedby={reasonId('table-remove-column', removeColumn.reasonKey)}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() =>
          removeColumn.enabled
            ? execute({ type: 'removeTableColumn', selection })
            : requestRemoval(removeColumn)
        }
      >
        {labels.tableRemoveColumn}
      </button>
      <button
        type="button"
        data-action="editor-table-toggle-header"
        disabled={busy || !header.enabled}
        aria-pressed={block.tableHeaderRows === 1}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() =>
          execute({
            type: 'setTableHeaderRow',
            tableId,
            enabled: block.tableHeaderRows !== 1,
          })
        }
      >
        {labels.tableHeaderRow}
      </button>
      <button
        type="button"
        className="editor-destructive-action"
        data-action="editor-table-remove"
        disabled={busy || !removeTable.enabled}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => requestRemoval(removeTable)}
      >
        {labels.tableRemove}
      </button>
      {reasonText(locale, 'table-add-row', addRow.reasonKey)}
      {reasonText(locale, 'table-remove-row', removeRow.reasonKey)}
      {reasonText(locale, 'table-add-column', addColumn.reasonKey)}
      {reasonText(locale, 'table-remove-column', removeColumn.reasonKey)}
    </div>
  )
}

export function DestructiveConfirmDialog({
  locale,
  confirmation,
  onCancel,
  onConfirm,
}: DestructiveConfirmDialogProps) {
  const labels = editorMessages[locale]
  const cancelRef = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    cancelRef.current?.focus()
  }, [confirmation])
  const text = confirmationText(locale, confirmation)
  return (
    <dialog
      open
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="flowpdf-destructive-heading"
      aria-describedby="flowpdf-destructive-description"
      className="editor-dialog editor-confirm-dialog"
      data-destructive-confirm=""
      onKeyDown={(event) => {
        if (event.key !== 'Escape') return
        event.preventDefault()
        onCancel()
      }}
    >
      <h2 id="flowpdf-destructive-heading">{text.heading}</h2>
      <p id="flowpdf-destructive-description">{text.body}</p>
      <div className="editor-dialog-actions">
        <button ref={cancelRef} type="button" data-action="editor-confirm-cancel" onClick={onCancel}>
          {text.cancel}
        </button>
        <button
          type="button"
          className="editor-destructive-action"
          data-action="editor-confirm-accept"
          onClick={onConfirm}
        >
          {text.confirm}
        </button>
      </div>
      <span className="visually-hidden">{labels.tableRemove}</span>
    </dialog>
  )
}

export function capabilityFor(view: EditorViewDto, name: string): EditorCapabilityDto {
  return (
    view.capabilities.find((candidate) => candidate.name === name) ?? {
      name,
      enabled: false,
      reasonKey: 'unavailable',
    }
  )
}

export function focusEditorInput(): void {
  document.querySelector<HTMLTextAreaElement>('[data-editor-input-host]')?.focus()
}

function reasonId(control: string, reasonKey: string | null | undefined): string | undefined {
  return reasonKey == null ? undefined : `flowpdf-${control}-reason`
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

function confirmationText(
  locale: EditorLocale,
  confirmation: ConfirmationMetadataDto,
): { readonly heading: string; readonly body: string; readonly confirm: string; readonly cancel: string } {
  const labels = editorMessages[locale]
  if (confirmation.headingKey === 'editor.table.remove.heading') {
    return {
      heading: labels.tableRemoveHeading,
      body: labels.tableRemoveBody,
      confirm: labels.tableRemoveConfirm,
      cancel: labels.tableRemoveCancel,
    }
  }
  if (confirmation.headingKey === 'editor.image.remove.heading') {
    return {
      heading: labels.imageRemoveHeading,
      body: labels.imageRemoveBody,
      confirm: labels.imageRemoveConfirm,
      cancel: labels.imageRemoveCancel,
    }
  }
  return {
    heading: labels.tableRemoveHeading,
    body: labels.tableRemoveBody,
    confirm: labels.tableRemoveConfirm,
    cancel: labels.tableRemoveCancel,
  }
}
