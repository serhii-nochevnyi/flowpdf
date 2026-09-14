import { useLayoutEffect, useRef, useState } from 'react'

import { InputAdapter } from './input-adapter.js'
import type {
  EditorBlockViewDto,
  EditorViewDto,
} from './editor-store.js'
import type { InputCommandTarget } from './input-adapter.js'

export interface SemanticDocumentProps {
  readonly view: EditorViewDto
  readonly controller: InputCommandTarget
  readonly onInputError?: (code: string | null) => void
}

export function SemanticDocument({
  view,
  controller,
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
      return (
        <figure key={block.nodeId} data-block-kind={block.nodeKind}>
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
