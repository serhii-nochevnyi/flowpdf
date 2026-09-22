import { useEffect, useRef, useState } from 'react'
import type { CSSProperties } from 'react'

import type {
  LayoutFragmentDto,
  LayoutPageDto,
  LayoutSchedulerSnapshotDto,
} from './layout-protocol.js'
import type {
  FormWidgetProjectionDto,
  FormProjectionSchedulerSnapshotDto,
} from '../forms/form-projection.js'
import './page-viewport.css'
import '../forms/form-projection.css'

export interface PageViewportProps {
  readonly layout: LayoutSchedulerSnapshotDto
  readonly projection?: FormProjectionSchedulerSnapshotDto
  readonly sourceRevision: number
  readonly locale?: 'uk' | 'en'
}

/**
 * A visual adapter over Rust-owned fixed-point page geometry. It deliberately
 * paints fragment boxes and provenance, while the semantic document remains
 * the only authored text/input/accessibility surface.
 */
export function PageViewport({
  layout,
  projection,
  sourceRevision,
  locale = 'uk',
}: PageViewportProps) {
  const accepted = layout.accepted
  const result = accepted?.result ?? null
  const synchronized =
    result !== null &&
    result.sourceRevision === sourceRevision &&
    accepted?.request.sourceRevision === sourceRevision &&
    accepted.request.sourceHash === result.sourceHash
  const pages = synchronized && result !== null ? result.pages : []
  const projected = projection?.accepted?.result ?? null
  const projectionSynchronized =
    projection?.phase === 'ready' &&
    projected !== null &&
    projected.sourceRevision === sourceRevision &&
    projected.sourceHash === result?.sourceHash &&
    accepted?.result.resultHash === projected.layoutResultHash
  const widgetProjection = projectionSynchronized ? projected?.projection ?? null : null
  const [activePage, setActivePage] = useState(0)
  const pageRefs = useRef(new Map<number, HTMLElement>())

  useEffect(() => {
    setActivePage((current) => Math.min(current, Math.max(0, pages.length - 1)))
  }, [result?.resultHash, pages.length])

  useEffect(() => {
    pageRefs.current.get(activePage)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  }, [activePage])

  const labels = locale === 'uk'
    ? {
        viewport: 'Візуальний перегляд сторінок',
        previous: 'Попередня сторінка',
        next: 'Наступна сторінка',
        page: 'Сторінка',
        pending: 'Пагінація виконується у фоні…',
        ready: 'Візуальна проєкція синхронізована з ревізією',
        stale: 'Очікується результат для поточної ревізії…',
        error: 'Пагінацію не прийнято. Код',
      }
    : {
        viewport: 'Visual page viewport',
        previous: 'Previous page',
        next: 'Next page',
        page: 'Page',
        pending: 'Pagination is running in the background…',
        ready: 'Visual projection is synchronized with the revision',
        stale: 'Waiting for the current revision result…',
        error: 'Pagination was not accepted. Code',
      }

  const status = layout.errorCode !== null
    ? `${labels.error} ${layout.errorCode}`
    : synchronized && layout.phase === 'ready'
      ? `${labels.ready} ${sourceRevision}.`
      : layout.phase === 'pending'
        ? labels.pending
        : labels.stale

  return (
    <section
      className="layout-viewport"
      data-layout-viewport=""
      data-layout-phase={layout.phase}
      data-layout-source-revision={result?.sourceRevision ?? ''}
      data-layout-result-hash={synchronized ? result?.resultHash ?? '' : ''}
      aria-label={labels.viewport}
    >
      <div className="layout-viewport-toolbar">
        <p className="layout-viewport-title">{labels.viewport}</p>
        <nav className="layout-page-navigation" aria-label={labels.viewport}>
          <button
            type="button"
            data-layout-page-previous=""
            disabled={!synchronized || activePage === 0}
            onClick={() => setActivePage((current) => Math.max(0, current - 1))}
          >
            {labels.previous}
          </button>
          <span data-layout-page-position="">
            {pages.length === 0 ? '—' : `${labels.page} ${activePage + 1} / ${pages.length}`}
          </span>
          <button
            type="button"
            data-layout-page-next=""
            disabled={!synchronized || activePage >= pages.length - 1}
            onClick={() => setActivePage((current) => Math.min(pages.length - 1, current + 1))}
          >
            {labels.next}
          </button>
        </nav>
      </div>
      <p
        className="layout-viewport-status"
        role="status"
        aria-atomic="true"
        data-layout-status=""
      >
        {status}
      </p>
      {pages.length === 0 ? (
        <div className="layout-viewport-empty" data-layout-empty="">
          {labels.stale}
        </div>
      ) : (
        <div className="layout-pages" data-layout-pages="">
          {pages.map((page) => (
              <LayoutPage
                key={`${result?.resultHash ?? 'layout'}-${page.pageIndex}`}
                page={page}
                projection={widgetProjection}
                register={(element) => {
                if (element === null) pageRefs.current.delete(page.pageIndex)
                else pageRefs.current.set(page.pageIndex, element)
              }}
            />
          ))}
        </div>
      )}
    </section>
  )
}

function LayoutPage({
  page,
  projection,
  register,
}: {
  readonly page: LayoutPageDto
  readonly projection: FormWidgetProjectionDto | null
  readonly register: (element: HTMLElement | null) => void
}) {
  const pageStyle: CSSProperties = {
    width: cssLength(page.bounds.width),
    height: cssLength(page.bounds.height),
  }
  const contentStyle: CSSProperties = {
    left: cssLength(page.contentRect.x),
    top: cssLength(page.contentRect.y),
    width: cssLength(page.contentRect.width),
    height: cssLength(page.contentRect.height),
  }
  return (
    <article
      ref={register}
      className="layout-page"
      style={pageStyle}
      data-layout-page=""
      data-page-index={page.pageIndex}
      data-section-id={page.sectionId}
      aria-hidden="true"
    >
      <div className="layout-page-content" style={contentStyle}>
        {page.header === null ? null : <LayoutFragment fragment={page.header} />}
        {page.fragments.map((fragment) => (
          <LayoutFragment key={fragment.id} fragment={fragment} />
        ))}
        {page.footer === null ? null : <LayoutFragment fragment={page.footer} />}
      </div>
      {projection === null ? null : (
        <div className="form-widget-overlays" aria-hidden="true" data-form-widget-overlays="">
          {projection.widgets
            .filter((widget) => widget.pageIndex === page.pageIndex)
            .map((widget) => (
              <div
                key={widget.widgetId}
                className="form-widget-overlay"
                style={{
                  left: cssLength(widget.rect.x),
                  top: cssLength(widget.rect.y),
                  width: cssLength(widget.rect.width),
                  height: cssLength(widget.rect.height),
                }}
                data-form-widget-overlay=""
                data-field-id={widget.fieldId}
                data-widget-id={widget.widgetId}
              />
            ))}
        </div>
      )}
    </article>
  )
}

function LayoutFragment({ fragment }: { readonly fragment: LayoutFragmentDto }) {
  const style: CSSProperties = {
    left: cssLength(fragment.rect.x),
    top: cssLength(fragment.rect.y),
    width: cssLength(fragment.rect.width),
    height: cssLength(fragment.rect.height),
  }
  return (
    <div
      className="layout-fragment"
      style={style}
      data-layout-fragment=""
      data-fragment-id={fragment.id}
      data-fragment-kind={fragment.kind}
      data-source-node-id={fragment.sourceNodeId ?? ''}
      data-derived={fragment.derived ? 'true' : 'false'}
    >
      {fragment.children.map((child) => (
        <LayoutFragment key={child.id} fragment={child} />
      ))}
    </div>
  )
}

/** Rust LayoutUnit is 1/64 point; CSS pixels are 96/72 points. */
function cssLength(raw: number): string {
  return `${raw / 48}px`
}
