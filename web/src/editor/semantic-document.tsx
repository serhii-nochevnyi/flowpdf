import { useLayoutEffect, useRef, useState } from 'react'

import { InputAdapter } from './input-adapter.js'
import type {
  ConfirmationMetadataDto,
  DirectionalSelectionDto,
  EditorBlockViewDto,
  EditorLocale,
  EditorViewDto,
  TableCellFocusDto,
} from './editor-store.js'
import type { InputCommandTarget } from './input-adapter.js'
import {
  DestructiveConfirmDialog,
  StructuralControls,
  TableActionBar,
  capabilityFor,
  focusEditorInput,
} from './structural-controls.js'

export interface SemanticDocumentProps {
  readonly view: EditorViewDto
  readonly controller: InputCommandTarget
  readonly locale?: EditorLocale
  readonly onInputError?: (code: string | null) => void
}

interface RenderContext {
  readonly view: EditorViewDto
  readonly controller: InputCommandTarget
  readonly locale: EditorLocale
  readonly rootRef: React.RefObject<HTMLElement | null>
  readonly onRequestRemoveTable: (
    tableId: string,
    confirmation: ConfirmationMetadataDto,
  ) => void
}

interface PendingTableRemoval {
  readonly tableId: string
  readonly confirmation: ConfirmationMetadataDto
}

export function SemanticDocument({
  view,
  controller,
  locale = 'uk',
  onInputError,
}: SemanticDocumentProps) {
  const rootRef = useRef<HTMLElement>(null)
  const hostRef = useRef<HTMLTextAreaElement>(null)
  const viewRef = useRef(view)
  const adapterRef = useRef<InputAdapter | null>(null)
  const [candidate, setCandidate] = useState('')
  const [pendingTableRemoval, setPendingTableRemoval] =
    useState<PendingTableRemoval | null>(null)
  viewRef.current = view

  useLayoutEffect(() => {
    const root = rootRef.current
    const host = hostRef.current
    if (root === null || host === null) return
    const adapter = new InputAdapter({
      root,
      host,
      getView: () => viewRef.current,
      commandTarget: controller,
      onCandidateChange: setCandidate,
      onError: onInputError,
    })
    adapterRef.current = adapter
    adapter.syncAccepted(viewRef.current)
    return () => {
      adapter.dispose()
      if (adapterRef.current === adapter) adapterRef.current = null
    }
  }, [controller, onInputError])

  useLayoutEffect(() => {
    adapterRef.current?.syncAccepted(view)
  }, [view])

  const context: RenderContext = {
    view,
    controller,
    locale,
    rootRef,
    onRequestRemoveTable: (tableId, confirmation) =>
      setPendingTableRemoval({ tableId, confirmation }),
  }
  const closeRemoval = (): void => {
    setPendingTableRemoval(null)
    focusEditorInput()
  }
  const confirmRemoval = (): void => {
    const removal = pendingTableRemoval
    setPendingTableRemoval(null)
    if (removal === null) return
    void controller
      .structuralCommand(
        {
          type: 'removeTable',
          tableId: removal.tableId,
          confirmed: true,
        },
        'ui',
      )
      .finally(focusEditorInput)
  }

  return (
    <article
      ref={rootRef}
      className="semantic-document"
      data-editor-document=""
      lang={locale === 'uk' ? 'uk' : 'en'}
      aria-label={locale === 'uk' ? 'Редактор документа' : 'Document editor'}
      onClick={(event) => {
        const target = event.target
        if (
          !(target instanceof HTMLElement) ||
          target.closest(
            '[data-editor-input-host], button, input, select, summary, dialog',
          ) === null
        ) {
          adapterRef.current?.focus()
        }
      }}
    >
      <StructuralControls view={view} controller={controller} locale={locale} />
      <div className="editor-structure-controls" data-structure-edit-controls="">
        <button
          type="button"
          data-action="editor-split"
          disabled={!structuralCapability(view, 'splitTextBlock')}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => adapterRef.current?.dispatchSplit('ui')}
        >
          {locale === 'uk' ? 'Розділити блок' : 'Split block'}
        </button>
        <button
          type="button"
          data-action="editor-merge-previous"
          disabled={!mergeActionEnabled(view, 'previous')}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => adapterRef.current?.dispatchMerge('previous', 'ui')}
        >
          {locale === 'uk' ? 'Об’єднати з попереднім' : 'Merge with previous'}
        </button>
        <button
          type="button"
          data-action="editor-merge-next"
          disabled={!mergeActionEnabled(view, 'next')}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => adapterRef.current?.dispatchMerge('next', 'ui')}
        >
          {locale === 'uk' ? 'Об’єднати з наступним' : 'Merge with next'}
        </button>
      </div>
      {view.document.blocks.map((block) => renderBlock(block, context))}
      <textarea
        ref={hostRef}
        className="editor-input-host"
        data-editor-input-host=""
        aria-label="FlowPDF text input"
        value={candidate}
        onChange={() => undefined}
        rows={1}
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
      />
      {pendingTableRemoval === null ? null : (
        <DestructiveConfirmDialog
          locale={locale}
          confirmation={pendingTableRemoval.confirmation}
          onCancel={closeRemoval}
          onConfirm={confirmRemoval}
        />
      )}
    </article>
  )
}

function renderBlock(block: EditorBlockViewDto, context: RenderContext) {
  switch (block.kind) {
    case 'paragraph':
      return (
        <p key={block.nodeId} data-node-id={block.nodeId} data-text-block="">
          {block.text}
        </p>
      )
    case 'heading':
      return renderHeading(block.nodeId, block.level ?? 1, block.text ?? '')
    case 'atomic':
      return renderAtomic(block, context)
  }
}

function renderAtomic(block: EditorBlockViewDto, context: RenderContext) {
  const children = block.children ?? []
  switch (block.nodeKind) {
    case 'orderedList':
      return (
        <ol key={block.nodeId} data-node-id={block.nodeId}>
          {children.map((child) => renderBlock(child, context))}
        </ol>
      )
    case 'unorderedList':
      return (
        <ul key={block.nodeId} data-node-id={block.nodeId}>
          {children.map((child) => renderBlock(child, context))}
        </ul>
      )
    case 'listItem':
      return (
        <li key={block.nodeId} data-node-id={block.nodeId}>
          {children.map((child) => renderBlock(child, context))}
        </li>
      )
    case 'table':
      return renderTable(block, context)
    case 'tableRow':
      return (
        <tr key={block.nodeId} data-node-id={block.nodeId}>
          {children.map((child) => renderBlock(child, context))}
        </tr>
      )
    case 'tableCell':
      return (
        <td key={block.nodeId} data-node-id={block.nodeId}>
          {children.map((child) => renderBlock(child, context))}
        </td>
      )
    case 'pageBreak':
      return renderPageBreak(block, context)
    default:
      return (
        <figure key={block.nodeId} data-node-id={block.nodeId} data-block-kind={block.nodeKind}>
          <figcaption>{block.nodeKind ?? 'unsupported'}</figcaption>
        </figure>
      )
  }
}

function renderTable(block: EditorBlockViewDto, context: RenderContext) {
  const rows = (block.children ?? []).filter((child) => child.nodeKind === 'tableRow')
  const hasHeader = block.tableHeaderRows === 1 && rows.length > 0
  const header = hasHeader ? rows[0] : undefined
  const bodyRows = hasHeader ? rows.slice(1) : rows
  const cellFocusOrder = new Map(
    (block.tableCellFocusOrder ?? []).map((focus) => [focus.cellId, focus]),
  )
  const selected = capabilityFor(context.view, 'removeTable').targetNodeId === block.nodeId
  return (
    <div
      key={block.nodeId}
      className="editor-table-wrapper"
      data-node-id={block.nodeId}
      data-block-kind="table"
      data-selected={selected ? 'true' : 'false'}
    >
      <table aria-label={context.locale === 'uk' ? 'Таблиця документа' : 'Document table'}>
        <caption className="visually-hidden">
          {context.locale === 'uk' ? 'Таблиця документа' : 'Document table'}
        </caption>
        {header === undefined ? null : (
          <thead>{renderTableRow(header, true, cellFocusOrder, context)}</thead>
        )}
        <tbody>
          {bodyRows.map((row) => renderTableRow(row, false, cellFocusOrder, context))}
        </tbody>
      </table>
      <TableActionBar
        block={block}
        view={context.view}
        controller={context.controller}
        locale={context.locale}
        onRequestRemoveTable={context.onRequestRemoveTable}
      />
    </div>
  )
}

function renderTableRow(
  row: EditorBlockViewDto,
  header: boolean,
  cellFocusOrder: ReadonlyMap<string, TableCellFocusDto>,
  context: RenderContext,
) {
  return (
    <tr key={row.nodeId} data-node-id={row.nodeId}>
      {(row.children ?? []).map((cell) =>
        renderTableCell(cell, header, cellFocusOrder, context),
      )}
    </tr>
  )
}

function renderTableCell(
  cell: EditorBlockViewDto,
  header: boolean,
  cellFocusOrder: ReadonlyMap<string, TableCellFocusDto>,
  context: RenderContext,
) {
  const focus = cellFocusOrder.get(cell.nodeId)
  const selected = containsNodeId(cell, context.view.selection.anchor.nodeId)
  const focusCell = (): void => {
    if (focus === undefined) return
    void context.controller
      .setEditorSelection(focus.selection)
      .then(() => focusCellElement(context.rootRef, cell.nodeId))
  }
  const moveCell = (previous: boolean, event: React.KeyboardEvent<HTMLElement>): void => {
    const nextCellId = previous ? focus?.previousCellId : focus?.nextCellId
    const nextFocus = nextCellId === null || nextCellId === undefined
      ? undefined
      : cellFocusOrder.get(nextCellId)
    if (nextFocus === undefined) return
    event.preventDefault()
    void context.controller
      .setEditorSelection(nextFocus.selection)
      .then(() => focusCellElement(context.rootRef, nextFocus.cellId))
  }
  const content = (cell.children ?? []).map((child) => renderBlock(child, context))
  const props = {
    'data-node-id': cell.nodeId,
    'data-cell-id': cell.nodeId,
    'data-selected': selected ? 'true' : 'false',
    tabIndex: 0,
    onFocus: focusCell,
    onClick: (event: React.MouseEvent<HTMLElement>) => {
      if (event.target === event.currentTarget) focusCell()
    },
    onKeyDown: (event: React.KeyboardEvent<HTMLElement>) => {
      if (event.key !== 'Tab') return
      moveCell(event.shiftKey, event)
    },
  }
  return header ? (
    <th key={cell.nodeId} {...props} scope="col">
      {content}
    </th>
  ) : (
    <td key={cell.nodeId} {...props}>
      {content}
    </td>
  )
}

function renderPageBreak(block: EditorBlockViewDto, context: RenderContext) {
  const labels = context.locale === 'uk'
    ? { pageBreak: 'Розрив сторінки', remove: 'Видалити розрив сторінки' }
    : { pageBreak: 'Page break', remove: 'Remove page break' }
  const remove = capabilityFor(context.view, 'removePageBreak')
  const selected = context.view.selection.anchor.nodeId === block.nodeId
  const selection = atomicSelection(block.nodeId)
  const focusSeparator = (): void => {
    void context.controller
      .setEditorSelection(selection)
      .then(() => focusAtomicElement(context.rootRef, block.nodeId))
  }
  const removeBreak = (): void => {
    void context.controller
      .structuralCommand({ type: 'removePageBreak', pageBreakId: block.nodeId }, 'ui')
      .finally(focusEditorInput)
  }
  return (
    <div
      key={block.nodeId}
      data-node-id={block.nodeId}
      data-block-kind="pageBreak"
      data-selected={selected ? 'true' : 'false'}
      className="editor-page-break"
      role="separator"
      aria-label={labels.pageBreak}
      tabIndex={0}
      onFocus={focusSeparator}
      onClick={(event) => {
        if ((event.target as HTMLElement).closest('button') === null) focusSeparator()
      }}
      onKeyDown={(event) => {
        if (!['Backspace', 'Delete'].includes(event.key) || !remove.enabled) return
        event.preventDefault()
        removeBreak()
      }}
    >
      <hr aria-hidden="true" />
      <span>{labels.pageBreak}</span>
      <button
        type="button"
        data-action="editor-page-break-remove"
        disabled={!remove.enabled}
        onMouseDown={(event) => event.preventDefault()}
        onClick={removeBreak}
      >
        {labels.remove}
      </button>
    </div>
  )
}

function renderHeading(nodeId: string, level: number, text: string) {
  const content = { children: text, 'data-node-id': nodeId, 'data-text-block': '' }
  switch (Math.min(6, Math.max(1, level))) {
    case 1:
      return <h1 key={nodeId} {...content} />
    case 2:
      return <h2 key={nodeId} {...content} />
    case 3:
      return <h3 key={nodeId} {...content} />
    case 4:
      return <h4 key={nodeId} {...content} />
    case 5:
      return <h5 key={nodeId} {...content} />
    default:
      return <h6 key={nodeId} {...content} />
  }
}

function structuralCapability(view: EditorViewDto, name: string): boolean {
  const capability = view.capabilities.find((candidate) => candidate.name === name)
  return capability?.enabled ?? true
}

function mergeActionEnabled(
  view: EditorViewDto,
  direction: 'previous' | 'next',
): boolean {
  const selection = view.selection
  if (
    selection.anchor.nodeId !== selection.focus.nodeId ||
    selection.anchor.utf16Offset !== selection.focus.utf16Offset
  ) {
    return false
  }
  const block = findTextBlock(view.document.blocks, selection.focus.nodeId)
  if (block === undefined) return false
  const atBoundary =
    direction === 'previous'
      ? selection.focus.utf16Offset === 0
      : selection.focus.utf16Offset === block.text.length
  if (!atBoundary || !structuralCapability(view, 'mergeTextBlocks')) return false
  const textBlocks = flattenTextBlocks(view.document.blocks)
  const index = textBlocks.findIndex((candidate) => candidate.nodeId === block.nodeId)
  return index >= 0 && textBlocks[index + (direction === 'previous' ? -1 : 1)] !== undefined
}

function findTextBlock(
  blocks: readonly EditorBlockViewDto[],
  nodeId: string,
): (EditorBlockViewDto & { readonly text: string }) | undefined {
  for (const block of blocks) {
    if (
      block.nodeId === nodeId &&
      (block.kind === 'paragraph' || block.kind === 'heading') &&
      typeof block.text === 'string'
    ) {
      return block as EditorBlockViewDto & { readonly text: string }
    }
    const nested = findTextBlock(block.children ?? [], nodeId)
    if (nested !== undefined) return nested
  }
  return undefined
}

function flattenTextBlocks(
  blocks: readonly EditorBlockViewDto[],
): Array<EditorBlockViewDto & { readonly text: string }> {
  return blocks.flatMap((block) => {
    const current =
      (block.kind === 'paragraph' || block.kind === 'heading') &&
      typeof block.text === 'string'
        ? [block as EditorBlockViewDto & { readonly text: string }]
        : []
    return [...current, ...flattenTextBlocks(block.children ?? [])]
  })
}

function atomicSelection(nodeId: string): DirectionalSelectionDto {
  return {
    anchor: { nodeId, utf16Offset: 0, affinity: 'forward' },
    focus: { nodeId, utf16Offset: 1, affinity: 'backward' },
  }
}

function containsNodeId(block: EditorBlockViewDto, nodeId: string): boolean {
  return (
    block.nodeId === nodeId ||
    (block.children ?? []).some((child) => containsNodeId(child, nodeId))
  )
}

function focusCellElement(
  rootRef: React.RefObject<HTMLElement | null>,
  nodeId: string,
): void {
  const cell = Array.from(
    rootRef.current?.querySelectorAll<HTMLElement>('[data-cell-id]') ?? [],
  ).find((candidate) => candidate.dataset.cellId === nodeId)
  cell?.focus()
}

function focusAtomicElement(
  rootRef: React.RefObject<HTMLElement | null>,
  nodeId: string,
): void {
  const block = Array.from(
    rootRef.current?.querySelectorAll<HTMLElement>('[data-node-id]') ?? [],
  ).find((candidate) => candidate.dataset.nodeId === nodeId)
  block?.focus()
}
