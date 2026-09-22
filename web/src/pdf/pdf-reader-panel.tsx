import { useEffect, useState } from 'react'

import type { EditorLocale } from '../editor/editor-store.js'
import type { EditorPdfReaderScheduler } from '../editor/editor-controller.js'
import {
  PDF_READER_PROTOCOL_VERSION,
  type PdfReaderSceneElementDto,
  type PdfReaderScenePageDto,
} from './pdf-protocol.js'
import './pdf-reader-panel.css'

const MAX_INPUT_BYTES = 64 * 1024 * 1024
const UNITS_PER_POINT = 64

export interface PdfReaderPanelProps {
  readonly scheduler: EditorPdfReaderScheduler
  readonly locale?: EditorLocale
}

/**
 * Read-only projection for imported PDF scenes. It deliberately has no editor
 * mutation callback and exposes diagnostics before the visual layer.
 */
export function PdfReaderPanel({ scheduler, locale = 'uk' }: PdfReaderPanelProps) {
  const [snapshot, setSnapshot] = useState(() => scheduler.snapshot())
  useEffect(() => {
    setSnapshot(scheduler.snapshot())
    return scheduler.subscribe?.(() => setSnapshot(scheduler.snapshot()))
  }, [scheduler])
  const [activePage, setActivePage] = useState(0)
  const [zoom, setZoom] = useState(1)
  const [inputError, setInputError] = useState<string | null>(null)
  const [fileName, setFileName] = useState<string | null>(null)
  const labels = locale === 'uk' ? ukLabels : enLabels
  const accepted = snapshot.accepted?.result ?? null
  const pages = accepted?.scene.pages ?? []
  const page = pages[Math.min(activePage, Math.max(0, pages.length - 1))] ?? null
  const report = accepted?.scene.report ?? null
  const status = inputError ?? readerStatus(snapshot.phase, snapshot.errorCode, labels)

  const onFileChange = (file: File | undefined): void => {
    if (file === undefined) return
    setFileName(file.name)
    setInputError(null)
    if (file.size > MAX_INPUT_BYTES) {
      setInputError(labels.fileTooLarge)
      return
    }
    void file.arrayBuffer().then((buffer) => {
      const bytes = new Uint8Array(buffer)
      if (bytes.length > MAX_INPUT_BYTES) {
        setInputError(labels.fileTooLarge)
        return
      }
      const requestId = `pdf-reader-${Date.now()}-${Math.random().toString(16).slice(2)}`
      void scheduler.request({
        protocolVersion: PDF_READER_PROTOCOL_VERSION,
        requestId,
        bytesHex: bytesToHex(bytes),
        firstPage: 0,
        pageCount: 2_048,
      })
    }).catch(() => setInputError(labels.fileReadError))
  }

  return (
    <section className="pdf-reader-panel" data-pdf-reader="" aria-label={labels.title}>
      <div className="pdf-reader-toolbar">
        <div>
          <h2 className="pdf-reader-title">{labels.title}</h2>
          <p className="pdf-reader-description">{labels.description}</p>
        </div>
        <label className="pdf-reader-file-label">
          <span>{labels.open}</span>
          <input
            type="file"
            accept="application/pdf,.pdf"
            data-pdf-reader-input=""
            onChange={(event) => onFileChange(event.currentTarget.files?.[0])}
          />
        </label>
      </div>
      {fileName === null ? null : (
        <p className="pdf-reader-file-name" data-pdf-reader-file="">
          {fileName}
        </p>
      )}
      <p
        className={`pdf-reader-status${inputError !== null || snapshot.errorCode !== null ? ' pdf-reader-status-error' : ''}`}
        role={inputError !== null || snapshot.errorCode !== null ? 'alert' : 'status'}
        aria-live="polite"
        data-pdf-reader-status=""
      >
        {status}
      </p>
      {report === null ? null : <PdfReaderReport report={report} labels={labels} />}
      <div className="pdf-reader-controls">
        <button
          type="button"
          data-pdf-reader-previous=""
          disabled={pages.length === 0 || activePage === 0}
          onClick={() => setActivePage((value) => Math.max(0, value - 1))}
        >
          {labels.previous}
        </button>
        <span data-pdf-reader-position="">
          {page === null ? '—' : `${labels.page} ${activePage + 1} / ${pages.length}`}
        </span>
        <button
          type="button"
          data-pdf-reader-next=""
          disabled={pages.length === 0 || activePage >= pages.length - 1}
          onClick={() => setActivePage((value) => Math.min(pages.length - 1, value + 1))}
        >
          {labels.next}
        </button>
        <button
          type="button"
          data-pdf-reader-zoom-out=""
          disabled={zoom <= 0.75}
          onClick={() => setZoom((value) => Math.max(0.75, value - 0.25))}
        >
          −
        </button>
        <span data-pdf-reader-zoom="">{Math.round(zoom * 100)}%</span>
        <button
          type="button"
          data-pdf-reader-zoom-in=""
          disabled={zoom >= 1.5}
          onClick={() => setZoom((value) => Math.min(1.5, value + 0.25))}
        >
          +
        </button>
      </div>
      {page === null ? (
        <div className="pdf-reader-empty" data-pdf-reader-empty="">
          {labels.empty}
        </div>
      ) : (
        <div className="pdf-reader-viewport" data-pdf-reader-viewport="">
          <PdfReaderPage page={page} zoom={zoom} />
        </div>
      )}
    </section>
  )
}

function PdfReaderReport({
  report,
  labels,
}: {
  readonly report: { readonly diagnostics: readonly { readonly code: string; readonly severity: string; readonly pageIndex: number | null; readonly objectNumber: number | null }[]; readonly partial: boolean }
  readonly labels: ReaderLabels
}) {
  if (report.diagnostics.length === 0) {
    return <p className="pdf-reader-report" data-pdf-reader-report="">{labels.noDiagnostics}</p>
  }
  return (
    <div className="pdf-reader-report" data-pdf-reader-report="">
      <p>{report.partial ? labels.partial : labels.diagnostics}</p>
      <ul>
        {report.diagnostics.map((diagnostic, index) => (
          <li key={`${diagnostic.code}-${index}`} data-pdf-reader-diagnostic={diagnostic.code}>
            <code>{diagnostic.code}</code>
            <span>
              {diagnostic.severity}
              {diagnostic.pageIndex === null ? '' : ` · ${labels.page} ${diagnostic.pageIndex + 1}`}
              {diagnostic.objectNumber === null ? '' : ` · ${diagnostic.objectNumber}`}
            </span>
          </li>
        ))}
      </ul>
    </div>
  )
}

function PdfReaderPage({ page, zoom }: { readonly page: PdfReaderScenePageDto; readonly zoom: number }) {
  const width = Math.max(1, page.bounds.width / UNITS_PER_POINT)
  const height = Math.max(1, page.bounds.height / UNITS_PER_POINT)
  return (
    <div
      className="pdf-reader-page-shell"
      style={{ width: width * zoom, height: height * zoom }}
      data-pdf-reader-page={page.pageIndex}
    >
      <div
        className="pdf-reader-page"
        style={{ width, height, transform: `scale(${zoom})` }}
        aria-hidden="true"
      >
        {page.elements.map((element, index) => (
          <PdfReaderElement
            key={`${element.kind}-${index}`}
            element={element}
            pageHeight={height}
          />
        ))}
      </div>
    </div>
  )
}

function PdfReaderElement({
  element,
  pageHeight,
}: {
  readonly element: PdfReaderSceneElementDto
  readonly pageHeight: number
}) {
  if (element.kind === 'text') {
    const value = element.value
    const rect = value.rect
    return (
      <span
        className="pdf-reader-text"
        style={position(rect, pageHeight)}
        data-pdf-reader-provenance={`${value.provenance.objectRef.objectNumber}:${value.provenance.operator ?? ''}`}
      >
        {value.text}
      </span>
    )
  }
  if (element.kind === 'image') {
    const value = element.value
    const rect = value.rect
    return (
      <img
        className="pdf-reader-image"
        src={`data:${value.mediaType};base64,${hexToBase64(value.dataHex)}`}
        alt=""
        style={position(rect, pageHeight)}
        data-pdf-reader-provenance={`${value.provenance.objectRef.objectNumber}:${value.provenance.operator ?? ''}`}
      />
    )
  }
  if (element.kind === 'form' || element.kind === 'link' || element.kind === 'annotation') {
    const value = element.value
    return (
      <div
        className={`pdf-reader-overlay pdf-reader-overlay-${element.kind}`}
        style={position(value.rect, pageHeight)}
        data-pdf-reader-provenance={`${value.provenance.objectRef.objectNumber}:${value.provenance.operator ?? ''}`}
      />
    )
  }
  const value = element.value
  return (
    <div
      className={`pdf-reader-geometry pdf-reader-geometry-${element.kind}`}
      style={position(value.rect, pageHeight)}
      data-pdf-reader-provenance={`${value.provenance.objectRef.objectNumber}:${value.provenance.operator ?? ''}`}
    />
  )
}

function position(
  rect: { readonly x: number; readonly y: number; readonly width: number; readonly height: number },
  pageHeight: number,
): { readonly left: number; readonly top: number; readonly width: number; readonly height: number } {
  const width = rect.width / UNITS_PER_POINT
  const height = rect.height / UNITS_PER_POINT
  return {
    left: rect.x / UNITS_PER_POINT,
    top: pageHeight - (rect.y + rect.height) / UNITS_PER_POINT,
    width: Math.max(1, width),
    height: Math.max(1, height),
  }
}

function bytesToHex(bytes: Uint8Array): string {
  let result = ''
  for (const byte of bytes) result += byte.toString(16).padStart(2, '0')
  return result
}

function hexToBase64(value: string): string {
  const bytes = value.match(/[0-9a-f]{2}/gi) ?? []
  let binary = ''
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.slice(index, index + 0x8000).map((item) => Number.parseInt(item, 16)))
  }
  return globalThis.btoa(binary)
}

function readerStatus(
  phase: 'idle' | 'pending' | 'ready' | 'error',
  errorCode: string | null,
  labels: ReaderLabels,
): string {
  if (errorCode !== null) return `${labels.error} ${errorCode}`
  if (phase === 'pending') return labels.pending
  if (phase === 'ready') return labels.ready
  return labels.idle
}

interface ReaderLabels {
  readonly title: string
  readonly description: string
  readonly open: string
  readonly fileTooLarge: string
  readonly fileReadError: string
  readonly idle: string
  readonly pending: string
  readonly ready: string
  readonly error: string
  readonly partial: string
  readonly diagnostics: string
  readonly noDiagnostics: string
  readonly previous: string
  readonly next: string
  readonly page: string
  readonly empty: string
}

const ukLabels: ReaderLabels = {
  title: 'Безпечний перегляд PDF',
  description: 'Імпортований PDF показується лише для читання; непідтримані частини позначаються.',
  open: 'Відкрити PDF',
  fileTooLarge: 'Файл перевищує обмеження reader: 64 МБ.',
  fileReadError: 'Не вдалося прочитати файл.',
  idle: 'Оберіть PDF для bounded-читання.',
  pending: 'Читаємо PDF у worker…',
  ready: 'Сцену PDF прочитано; редактор не змінено.',
  error: 'PDF reader помилка:',
  partial: 'Частковий результат і діагностика reader:',
  diagnostics: 'Діагностика reader:',
  noDiagnostics: 'Діагностик reader немає.',
  previous: 'Попередня',
  next: 'Наступна',
  page: 'Сторінка',
  empty: 'Попередній перегляд з’явиться після імпорту PDF.',
}

const enLabels: ReaderLabels = {
  title: 'Safe PDF reader',
  description: 'Imported PDFs are read-only; unsupported content is reported explicitly.',
  open: 'Open PDF',
  fileTooLarge: 'The file exceeds the reader limit of 64 MB.',
  fileReadError: 'The file could not be read.',
  idle: 'Choose a PDF for bounded reading.',
  pending: 'Reading the PDF in a worker…',
  ready: 'PDF scene ready; the editor was not changed.',
  error: 'PDF reader error:',
  partial: 'Partial result and reader diagnostics:',
  diagnostics: 'Reader diagnostics:',
  noDiagnostics: 'No reader diagnostics.',
  previous: 'Previous',
  next: 'Next',
  page: 'Page',
  empty: 'The preview appears after importing a PDF.',
}
