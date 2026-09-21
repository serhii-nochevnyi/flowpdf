import { useEffect, useMemo, useRef, useState } from 'react'
import type { CSSProperties } from 'react'

import type {
  DirectionalSelectionDto,
  EditorBlockViewDto,
  EditorLocale,
} from '../editor/editor-store.js'
import type {
  LayoutFragmentDto,
  LayoutPageDto,
  LayoutSchedulerSnapshotDto,
} from '../layout/layout-protocol.js'
import type { PdfExportSchedulerSnapshotDto } from './pdf-protocol.js'
import './pdf-preview.css'

const MIN_ZOOM = 0.75
const MAX_ZOOM = 1.5
const ZOOM_STEP = 0.25
const PAGE_RADIUS = 1
const MAX_SEARCH_HITS = 64

export interface PdfPreviewProps {
  readonly layout: LayoutSchedulerSnapshotDto
  readonly pdf: PdfExportSchedulerSnapshotDto
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly sourceBlocks: readonly EditorBlockViewDto[]
  readonly selection: DirectionalSelectionDto
  readonly locale?: EditorLocale
  readonly onExport: () => void
  readonly onSelectSource: (selection: DirectionalSelectionDto) => void
}

interface SearchHit {
  readonly nodeId: string
  readonly startUtf16: number
  readonly endUtf16: number
}

/**
 * Projects Rust-owned PDF/page geometry into a visual-only surface. The
 * semantic editor remains the sole authored text and accessibility tree.
 */
export function PdfPreview({
  layout,
  pdf,
  sourceRevision,
  sourceHash,
  sourceBlocks,
  selection,
  locale = 'uk',
  onExport,
  onSelectSource,
}: PdfPreviewProps) {
  const [zoom, setZoom] = useState(1)
  const [activePage, setActivePage] = useState(0)
  const [query, setQuery] = useState('')
  const pageRefs = useRef(new Map<number, HTMLElement>())
  const labels = locale === 'uk' ? ukLabels : enLabels
  const acceptedLayout = layout.accepted
  const layoutResult = acceptedLayout?.result ?? null
  const synchronized =
    layout.phase === 'ready' &&
    layoutResult !== null &&
    layoutResult.sourceRevision === sourceRevision &&
    layoutResult.sourceHash === sourceHash &&
    acceptedLayout?.request.sourceRevision === sourceRevision &&
    acceptedLayout.request.sourceHash === sourceHash
  const pages = synchronized && layoutResult !== null ? layoutResult.pages : []
  const acceptedPdf = pdf.accepted
  const pdfResult = acceptedPdf?.result ?? null
  const exportSynchronized =
    pdf.phase === 'ready' &&
    acceptedPdf !== null &&
    pdfResult !== null &&
    acceptedPdf.request.sourceRevision === sourceRevision &&
    acceptedPdf.request.sourceHash === sourceHash &&
    pdfResult.sourceRevision === sourceRevision &&
    pdfResult.sourceHash === sourceHash &&
    synchronized &&
    pdfResult.manifest.layoutResultHash === layoutResult?.resultHash
  const hits = useMemo(() => findSearchHits(sourceBlocks, query, locale), [
    locale,
    query,
    sourceBlocks,
  ])

  useEffect(() => {
    setActivePage((current) => Math.min(current, Math.max(0, pages.length - 1)))
  }, [layoutResult?.resultHash, pages.length])

  useEffect(() => {
    pageRefs.current.get(activePage)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  }, [activePage])

  const downloadUrl = usePdfDownloadUrl(exportSynchronized ? pdfResult?.bytesHex ?? null : null)
  const visiblePageStart = Math.max(0, activePage - PAGE_RADIUS)
  const visiblePageEnd = Math.min(pages.length - 1, activePage + PAGE_RADIUS)
  const status = pdf.errorCode !== null
    ? `${labels.exportError} ${pdf.errorCode}`
    : pdf.phase === 'pending'
      ? labels.exportPending
      : pdf.phase === 'ready' && exportSynchronized
        ? `${labels.exportReady} ${sourceRevision}.`
        : synchronized
          ? labels.exportIdle
          : labels.waitingForLayout

  const zoomOut = (): void => setZoom((value) => clampZoom(value - ZOOM_STEP))
  const zoomIn = (): void => setZoom((value) => clampZoom(value + ZOOM_STEP))
  const resetZoom = (): void => setZoom(1)

  return (
    <section
      className="pdf-preview"
      data-pdf-preview=""
      data-pdf-phase={pdf.phase}
      data-pdf-source-revision={sourceRevision}
      data-pdf-layout-synchronized={synchronized ? 'true' : 'false'}
      data-pdf-zoom={zoom}
      aria-label={labels.preview}
    >
      <div className="pdf-preview-toolbar">
        <h2 className="pdf-preview-title">{labels.preview}</h2>
        <div className="pdf-preview-actions">
          <button
            type="button"
            data-pdf-export=""
            disabled={!synchronized || pdf.phase === 'pending'}
            onClick={onExport}
          >
            {pdf.phase === 'pending' ? labels.exporting : labels.export}
          </button>
          {downloadUrl === null ? null : (
            <a
              className="pdf-preview-download"
              data-pdf-download=""
              href={downloadUrl}
              download={`flowpdf-revision-${sourceRevision}.pdf`}
            >
              {labels.download}
            </a>
          )}
        </div>
      </div>
      <div className="pdf-preview-controls">
        <div className="pdf-preview-zoom" aria-label={labels.zoomControls}>
          <button type="button" data-pdf-zoom-out="" onClick={zoomOut} disabled={zoom <= MIN_ZOOM}>
            {labels.zoomOut}
          </button>
          <button type="button" data-pdf-zoom-reset="" onClick={resetZoom}>
            {Math.round(zoom * 100)}%
          </button>
          <button type="button" data-pdf-zoom-in="" onClick={zoomIn} disabled={zoom >= MAX_ZOOM}>
            {labels.zoomIn}
          </button>
        </div>
        <nav className="pdf-preview-navigation" aria-label={labels.pageNavigation}>
          <button
            type="button"
            data-pdf-page-previous=""
            disabled={pages.length === 0 || activePage === 0}
            onClick={() => setActivePage((value) => Math.max(0, value - 1))}
          >
            {labels.previous}
          </button>
          <span data-pdf-page-position="">
            {pages.length === 0 ? '—' : `${labels.page} ${activePage + 1} / ${pages.length}`}
          </span>
          <button
            type="button"
            data-pdf-page-next=""
            disabled={pages.length === 0 || activePage >= pages.length - 1}
            onClick={() => setActivePage((value) => Math.min(pages.length - 1, value + 1))}
          >
            {labels.next}
          </button>
        </nav>
      </div>
      <div className="pdf-preview-search">
        <label htmlFor="flowpdf-pdf-search">{labels.search}</label>
        <input
          id="flowpdf-pdf-search"
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={labels.searchPlaceholder}
          data-pdf-search=""
        />
        <span data-pdf-search-count="">
          {query.trim() === '' ? labels.searchHint : `${hits.length} ${labels.matches}`}
        </span>
        {hits.length === 0 || query.trim() === '' ? null : (
          <div className="pdf-preview-search-results" data-pdf-search-results="">
            {hits.map((hit, index) => (
              <button
                key={`${hit.nodeId}-${hit.startUtf16}`}
                type="button"
                data-pdf-search-result=""
                data-pdf-search-node-id={hit.nodeId}
                aria-label={`${labels.result} ${index + 1}, ${hit.nodeId}`}
                onClick={() => onSelectSource(selectionForHit(hit))}
              >
                {labels.result} {index + 1}
              </button>
            ))}
          </div>
        )}
      </div>
      <p
        className={`pdf-preview-status${pdf.errorCode === null ? '' : ' pdf-preview-status-error'}`}
        role={pdf.errorCode === null ? 'status' : 'alert'}
        aria-live="polite"
        aria-atomic="true"
        data-pdf-preview-status=""
      >
        {status}
      </p>
      {pages.length === 0 ? (
        <div className="pdf-preview-empty" data-pdf-preview-empty="">
          {labels.waitingForLayout}
        </div>
      ) : (
        <div className="pdf-preview-pages" data-pdf-preview-pages="">
          {pages.map((page) => {
            const visible = page.pageIndex >= visiblePageStart && page.pageIndex <= visiblePageEnd
            return (
              <PdfPreviewPage
                key={`${layoutResult?.resultHash ?? 'layout'}-${page.pageIndex}`}
                page={page}
                zoom={zoom}
                visible={visible}
                selected={selection}
                register={(element) => {
                  if (element === null) pageRefs.current.delete(page.pageIndex)
                  else pageRefs.current.set(page.pageIndex, element)
                }}
              />
            )
          })}
        </div>
      )}
    </section>
  )
}

function PdfPreviewPage({
  page,
  zoom,
  visible,
  selected,
  register,
}: {
  readonly page: LayoutPageDto
  readonly zoom: number
  readonly visible: boolean
  readonly selected: DirectionalSelectionDto
  readonly register: (element: HTMLElement | null) => void
}) {
  const slotStyle: CSSProperties = {
    width: cssLength(page.bounds.width * zoom),
    height: cssLength(page.bounds.height * zoom),
  }
  const pageStyle: CSSProperties = {
    width: cssLength(page.bounds.width),
    height: cssLength(page.bounds.height),
    transform: `scale(${zoom})`,
  }
  return (
    <div
      ref={register}
      className="pdf-preview-page-slot"
      style={slotStyle}
      data-pdf-preview-page-slot=""
      data-page-index={page.pageIndex}
      aria-hidden="true"
    >
      {!visible ? null : (
        <article
          className="pdf-preview-page"
          style={pageStyle}
          data-pdf-preview-page=""
          data-page-index={page.pageIndex}
          aria-hidden="true"
        >
          <div className="pdf-preview-page-content">
            {page.header === null ? null : (
              <PdfPreviewFragment fragment={page.header} selected={selected} />
            )}
            {page.fragments.map((fragment) => (
              <PdfPreviewFragment
                key={fragment.id}
                fragment={fragment}
                selected={selected}
              />
            ))}
            {page.footer === null ? null : (
              <PdfPreviewFragment fragment={page.footer} selected={selected} />
            )}
          </div>
        </article>
      )}
    </div>
  )
}

function PdfPreviewFragment({
  fragment,
  selected,
}: {
  readonly fragment: LayoutFragmentDto
  readonly selected: DirectionalSelectionDto
}) {
  const style: CSSProperties = {
    left: cssLength(fragment.rect.x),
    top: cssLength(fragment.rect.y),
    width: cssLength(fragment.rect.width),
    height: cssLength(fragment.rect.height),
  }
  return (
    <div
      className="pdf-preview-fragment"
      style={style}
      data-pdf-preview-fragment=""
      data-source-node-id={fragment.sourceNodeId ?? ''}
      data-pdf-selection={fragmentContainsSelection(fragment, selected) ? 'true' : 'false'}
    >
      {fragment.children.map((child) => (
        <PdfPreviewFragment key={child.id} fragment={child} selected={selected} />
      ))}
    </div>
  )
}

function usePdfDownloadUrl(bytesHex: string | null): string | null {
  const [url, setUrl] = useState<string | null>(null)
  useEffect(() => {
    if (bytesHex === null || typeof globalThis.URL?.createObjectURL !== 'function') {
      setUrl(null)
      return
    }
    const bytes = bytesFromHex(bytesHex)
    if (bytes === null || typeof Blob === 'undefined') {
      setUrl(null)
      return
    }
    const objectUrl = globalThis.URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' }))
    setUrl(objectUrl)
    return () => globalThis.URL.revokeObjectURL(objectUrl)
  }, [bytesHex])
  return url
}

function findSearchHits(
  blocks: readonly EditorBlockViewDto[],
  query: string,
  locale: EditorLocale,
): readonly SearchHit[] {
  const normalizedQuery = query.trim().toLocaleLowerCase(locale === 'uk' ? 'uk-UA' : 'en-US')
  if (normalizedQuery === '') return []
  const hits: SearchHit[] = []
  for (const block of flattenBlocks(blocks)) {
    if (typeof block.text !== 'string') continue
    const text = block.text
    const normalizedText = text.toLocaleLowerCase(locale === 'uk' ? 'uk-UA' : 'en-US')
    let offset = 0
    while (hits.length < MAX_SEARCH_HITS) {
      const index = normalizedText.indexOf(normalizedQuery, offset)
      if (index < 0) break
      hits.push({
        nodeId: block.nodeId,
        startUtf16: index,
        endUtf16: index + normalizedQuery.length,
      })
      offset = index + Math.max(1, normalizedQuery.length)
    }
    if (hits.length >= MAX_SEARCH_HITS) break
  }
  return hits
}

function flattenBlocks(blocks: readonly EditorBlockViewDto[]): readonly EditorBlockViewDto[] {
  const result: EditorBlockViewDto[] = []
  const visit = (block: EditorBlockViewDto): void => {
    if (typeof block.text === 'string') result.push(block)
    for (const child of block.children ?? []) visit(child)
  }
  for (const block of blocks) visit(block)
  return result
}

function selectionForHit(hit: SearchHit): DirectionalSelectionDto {
  return {
    anchor: {
      nodeId: hit.nodeId,
      utf16Offset: hit.startUtf16,
      affinity: 'forward',
    },
    focus: {
      nodeId: hit.nodeId,
      utf16Offset: hit.endUtf16,
      affinity: 'forward',
    },
  }
}

function fragmentContainsSelection(
  fragment: LayoutFragmentDto,
  selection: DirectionalSelectionDto,
): boolean {
  if (fragment.sourceNodeId === null || fragment.source === null) return false
  if (selection.anchor.nodeId !== fragment.sourceNodeId && selection.focus.nodeId !== fragment.sourceNodeId) {
    return false
  }
  const start = Math.min(selection.anchor.utf16Offset, selection.focus.utf16Offset)
  const end = Math.max(selection.anchor.utf16Offset, selection.focus.utf16Offset)
  return fragment.source.utf16Start <= end && fragment.source.utf16End >= start
}

function bytesFromHex(value: string): ArrayBuffer | null {
  if (value.length === 0 || value.length % 2 !== 0 || !/^[0-9a-f]+$/i.test(value)) return null
  const buffer = new ArrayBuffer(value.length / 2)
  const bytes = new Uint8Array(buffer)
  for (let index = 0; index < bytes.length; index += 1) {
    const pair = value.slice(index * 2, index * 2 + 2)
    const parsed = Number.parseInt(pair, 16)
    if (!Number.isFinite(parsed)) return null
    bytes[index] = parsed
  }
  return buffer
}

function clampZoom(value: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, value))
}

/** Rust LayoutUnit is 1/64 point; CSS pixels are 96/72 points. */
function cssLength(raw: number): string {
  return `${raw / 48}px`
}

const ukLabels = {
  preview: 'PDF-перегляд',
  export: 'Експортувати PDF',
  exporting: 'Експорт…',
  download: 'Завантажити PDF',
  zoomControls: 'Масштаб PDF-перегляду',
  zoomOut: 'Зменшити',
  zoomIn: 'Збільшити',
  pageNavigation: 'Навігація PDF-сторінками',
  previous: 'Попередня',
  next: 'Наступна',
  page: 'Сторінка',
  search: 'Пошук у документі',
  searchPlaceholder: 'Знайти текст',
  searchHint: 'Введіть запит',
  matches: 'збігів',
  result: 'Результат',
  exportPending: 'PDF готується у фоні…',
  exportReady: 'PDF готовий для ревізії',
  exportIdle: 'Перегляд синхронізований; PDF ще не експортовано.',
  waitingForLayout: 'Очікується пагінація поточної ревізії…',
  exportError: 'PDF не прийнято. Код',
} as const

const enLabels = {
  preview: 'PDF preview',
  export: 'Export PDF',
  exporting: 'Exporting…',
  download: 'Download PDF',
  zoomControls: 'PDF preview zoom',
  zoomOut: 'Zoom out',
  zoomIn: 'Zoom in',
  pageNavigation: 'PDF page navigation',
  previous: 'Previous',
  next: 'Next',
  page: 'Page',
  search: 'Search document',
  searchPlaceholder: 'Find text',
  searchHint: 'Enter a query',
  matches: 'matches',
  result: 'Result',
  exportPending: 'PDF export is running in the background…',
  exportReady: 'PDF is ready for revision',
  exportIdle: 'Preview is synchronized; PDF has not been exported yet.',
  waitingForLayout: 'Waiting for pagination for the current revision…',
  exportError: 'PDF was not accepted. Code',
} as const
