import { useLayoutEffect, useRef, useState } from 'react'

import { InputAdapter } from './input-adapter.js'
import type {
  EditorBlockViewDto,
  EditorLocale,
  EditorViewDto,
} from './editor-store.js'
import type { InputCommandTarget } from './input-adapter.js'

export interface SemanticDocumentProps {
  readonly view: EditorViewDto
  readonly controller: InputCommandTarget
  readonly locale?: EditorLocale
  readonly onInputError?: (code: string | null) => void
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

  return (
    <article
      ref={rootRef}
      className="semantic-document"
      data-editor-document=""
      aria-label="FlowPDF semantic document"
      onClick={(event) => {
        const target = event.target
        if (!(target instanceof HTMLElement) || !target.matches('[data-editor-input-host]')) {
          adapterRef.current?.focus()
        }
      }}
    >
      <div className="editor-structure-controls" data-structure-controls="">
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
      {view.document.blocks.map((block) => renderBlock(block))}
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
    </article>
  )
}

function renderBlock(block: EditorBlockViewDto) {
  switch (block.kind) {
    case 'paragraph':
      return (
        <p key={block.nodeId} data-node-id={block.nodeId}>
          {block.text}
        </p>
      )
    case 'heading':
      return renderHeading(block.nodeId, block.level ?? 1, block.text ?? '')
    case 'atomic':
      return renderAtomic(block)
  }
}

function renderAtomic(block: EditorBlockViewDto) {
  const children = block.children ?? []
  switch (block.nodeKind) {
    case 'orderedList':
      return (
        <ol key={block.nodeId} data-node-id={block.nodeId}>
          {children.map((child) => renderBlock(child))}
        </ol>
      )
    case 'unorderedList':
      return (
        <ul key={block.nodeId} data-node-id={block.nodeId}>
          {children.map((child) => renderBlock(child))}
        </ul>
      )
    case 'listItem':
      return (
        <li key={block.nodeId} data-node-id={block.nodeId}>
          {children.map((child) => renderBlock(child))}
        </li>
      )
    case 'table':
      return (
        <div key={block.nodeId} className="editor-table-wrapper" data-node-id={block.nodeId}>
          <table>
            <tbody>{children.map((child) => renderBlock(child))}</tbody>
          </table>
        </div>
      )
    case 'tableRow':
      return (
        <tr key={block.nodeId} data-node-id={block.nodeId}>
          {children.map((child) => renderBlock(child))}
        </tr>
      )
    case 'tableCell':
      return (
        <td key={block.nodeId} data-node-id={block.nodeId}>
          {children.map((child) => renderBlock(child))}
        </td>
      )
    case 'pageBreak':
      return (
        <div key={block.nodeId} data-node-id={block.nodeId} data-block-kind="pageBreak">
          <hr />
          <span>Розрив сторінки</span>
        </div>
      )
    default:
      return (
        <figure key={block.nodeId} data-node-id={block.nodeId} data-block-kind={block.nodeKind}>
          <figcaption>{block.nodeKind ?? 'unsupported'}</figcaption>
        </figure>
      )
  }
}

function renderHeading(nodeId: string, level: number, text: string) {
  const content = { children: text, key: nodeId, 'data-node-id': nodeId }
  switch (Math.min(6, Math.max(1, level))) {
    case 1:
      return <h1 {...content} />
    case 2:
      return <h2 {...content} />
    case 3:
      return <h3 {...content} />
    case 4:
      return <h4 {...content} />
    case 5:
      return <h5 {...content} />
    default:
      return <h6 {...content} />
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
