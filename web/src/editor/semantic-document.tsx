import type { EditorBlockViewDto, EditorViewDto } from './editor-app.js'

export interface SemanticDocumentProps {
  readonly view: EditorViewDto
}

export function SemanticDocument({ view }: SemanticDocumentProps) {
  return (
    <article
      className="semantic-document"
      data-editor-document=""
      aria-label="FlowPDF semantic document"
    >
      {view.document.blocks.map((block) => renderBlock(block))}
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
