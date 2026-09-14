import type {
  DirectionalSelectionDto,
  EditorBlockViewDto,
  EditorViewDto,
} from './editor-store.js'

interface DomEndpoint {
  readonly node: Node
  readonly offset: number
}

export function selectionFromDom(
  root: HTMLElement,
  view: EditorViewDto,
): DirectionalSelectionDto | null {
  const selection = root.ownerDocument.getSelection()
  if (
    selection === null ||
    selection.anchorNode === null ||
    selection.focusNode === null ||
    !root.contains(selection.anchorNode) ||
    !root.contains(selection.focusNode)
  ) {
    return null
  }

  const anchor = endpointFromDom(root, view, {
    node: selection.anchorNode,
    offset: selection.anchorOffset,
  })
  const focus = endpointFromDom(root, view, {
    node: selection.focusNode,
    offset: selection.focusOffset,
  })
  if (anchor === null || focus === null) return null
  return { anchor, focus }
}

export function restoreDomSelection(
  root: HTMLElement,
  view: EditorViewDto,
  selection: DirectionalSelectionDto = view.selection,
): boolean {
  const anchor = endpointToDom(root, view, selection.anchor)
  const focus = endpointToDom(root, view, selection.focus)
  if (anchor === null || focus === null) return false

  const range = root.ownerDocument.createRange()
  range.setStart(anchor.node, anchor.offset)
  range.setEnd(focus.node, focus.offset)
  const browserSelection = root.ownerDocument.getSelection()
  if (browserSelection === null) return false
  browserSelection.removeAllRanges()
  browserSelection.addRange(range)
  return true
}

function endpointFromDom(
  root: HTMLElement,
  view: EditorViewDto,
  endpoint: DomEndpoint,
) {
  const blockElement = blockElementForNode(root, endpoint.node)
  if (blockElement === null) return null
  const block = findBlock(view.document.blocks, blockElement.dataset.nodeId)
  if (block === undefined || !isTextBlock(block)) return null

  if (endpoint.node.nodeType === Node.TEXT_NODE) {
    if (!isRustBoundary(block, endpoint.offset)) return null
    return {
      nodeId: block.nodeId,
      utf16Offset: endpoint.offset,
      affinity: affinityFor(block, endpoint.offset),
    } as const
  }

  if (endpoint.node === blockElement && block.text === '' && endpoint.offset === 0) {
    return {
      nodeId: block.nodeId,
      utf16Offset: 0,
      affinity: 'forward',
    } as const
  }
  return null
}

function endpointToDom(
  root: HTMLElement,
  view: EditorViewDto,
  position: DirectionalSelectionDto['anchor'],
): DomEndpoint | null {
  const block = findBlock(view.document.blocks, position.nodeId)
  if (block === undefined || !isTextBlock(block) || !isRustBoundary(block, position.utf16Offset)) {
    return null
  }
  const blockElement = Array.from(root.querySelectorAll<HTMLElement>('[data-node-id]')).find(
    (candidate) => candidate.dataset.nodeId === position.nodeId,
  )
  if (blockElement === undefined) return null

  const textNode = firstTextNode(blockElement)
  if (textNode !== null) {
    return { node: textNode, offset: position.utf16Offset }
  }
  if (block.text === '' && position.utf16Offset === 0) {
    return { node: blockElement, offset: 0 }
  }
  return null
}

function blockElementForNode(root: HTMLElement, node: Node): HTMLElement | null {
  const candidate =
    node.nodeType === Node.ELEMENT_NODE
      ? (node as HTMLElement)
      : node.parentElement
  const blockElement = candidate?.closest<HTMLElement>('[data-node-id]') ?? null
  if (blockElement === null || !root.contains(blockElement)) return null
  return blockElement
}

function findBlock(
  blocks: readonly EditorBlockViewDto[],
  nodeId: string | undefined,
): EditorBlockViewDto | undefined {
  if (nodeId === undefined) return undefined
  for (const block of blocks) {
    if (block.nodeId === nodeId) return block
    const nested = findBlock(block.children ?? [], nodeId)
    if (nested !== undefined) return nested
  }
  return undefined
}

function firstTextNode(element: HTMLElement): Text | null {
  const walker = element.ownerDocument.createTreeWalker(element, NodeFilter.SHOW_TEXT)
  return walker.nextNode() as Text | null
}

function isTextBlock(block: EditorBlockViewDto): block is EditorBlockViewDto & {
  readonly text: string
  readonly spans: readonly { readonly startUtf16: number; readonly endUtf16: number }[]
} {
  return (
    (block.kind === 'paragraph' || block.kind === 'heading') &&
    typeof block.text === 'string' &&
    Array.isArray(block.spans)
  )
}

function isRustBoundary(
  block: EditorBlockViewDto & {
    readonly text: string
    readonly spans: readonly { readonly startUtf16: number; readonly endUtf16: number }[]
  },
  offset: number,
): boolean {
  return block.spans.some(
    (span) => span.startUtf16 === offset || span.endUtf16 === offset,
  )
}

function affinityFor(
  block: EditorBlockViewDto & {
    readonly text: string
    readonly spans: readonly { readonly startUtf16: number; readonly endUtf16: number }[]
  },
  offset: number,
): 'forward' | 'backward' {
  return offset === 0 || offset < utf16Length(block.text) ? 'forward' : 'backward'
}

function utf16Length(value: string): number {
  return value.length
}
