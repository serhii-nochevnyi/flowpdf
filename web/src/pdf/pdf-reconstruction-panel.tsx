import { useEffect, useMemo, useRef, useState } from 'react'

import type {
  EditorLocale,
  EditorPdfReaderScheduler,
  EditorPdfReconstructionScheduler,
} from '../editor/editor-controller.js'
import {
  PDF_READER_PROTOCOL_VERSION,
  type PdfReaderSceneElementDto,
  type PdfReaderScenePageDto,
} from './pdf-protocol.js'
import {
  PDF_RECONSTRUCTION_PROTOCOL_VERSION,
  type PdfReconstructionRequestDto,
  type PdfReviewDecisionDto,
} from './pdf-reconstruction-protocol.js'
import {
  MAX_PDF_SOURCE_BYTES,
  PdfSourceStore,
  hashPdfSourceBytes,
  type PdfSourceMetadataDto,
} from '../../persistence/pdf-source-store.js'
import './pdf-reconstruction-panel.css'

const UNITS_PER_POINT = 64

export interface PdfReconstructionPanelProps {
  readonly readerScheduler: EditorPdfReaderScheduler
  readonly reconstructionScheduler: EditorPdfReconstructionScheduler
  readonly sourceStore?: PdfSourceStore
  readonly locale?: EditorLocale
}

type ReviewDraft =
  | { readonly kind: 'keep' }
  | { readonly kind: 'replace'; readonly text: string }

/**
 * Report-first external reconstruction surface. The original scene stays
 * visual-only and the candidate remains a review projection until the caller
 * explicitly accepts every required decision.
 */
export function PdfReconstructionPanel({
  readerScheduler,
  reconstructionScheduler,
  sourceStore: suppliedSourceStore,
  locale = 'uk',
}: PdfReconstructionPanelProps) {
  const [ownedSourceStore] = useState(() => new PdfSourceStore())
  const sourceStore = suppliedSourceStore ?? ownedSourceStore
  const labels = locale === 'uk' ? ukLabels : enLabels
  const [readerSnapshot, setReaderSnapshot] = useState(() => readerScheduler.snapshot())
  const [reconstructionSnapshot, setReconstructionSnapshot] = useState(() =>
    reconstructionScheduler.snapshot(),
  )
  const [activePage, setActivePage] = useState(0)
  const [fileName, setFileName] = useState<string | null>(null)
  const [sourceMetadata, setSourceMetadata] = useState<PdfSourceMetadataDto | null>(null)
  const [readerRequestId, setReaderRequestId] = useState<string | null>(null)
  const [reviewDrafts, setReviewDrafts] = useState<Readonly<Record<string, ReviewDraft>>>({})
  const [inputError, setInputError] = useState<string | null>(null)
  const requestSequence = useRef(0)
  const reconstructedReaderIdentity = useRef<string | null>(null)

  useEffect(() => {
    setReaderSnapshot(readerScheduler.snapshot())
    return readerScheduler.subscribe?.(() => setReaderSnapshot(readerScheduler.snapshot()))
  }, [readerScheduler])

  useEffect(() => {
    setReconstructionSnapshot(reconstructionScheduler.snapshot())
    return reconstructionScheduler.subscribe?.(() =>
      setReconstructionSnapshot(reconstructionScheduler.snapshot()),
    )
  }, [reconstructionScheduler])

  useEffect(() => {
    if (suppliedSourceStore !== undefined) return undefined
    return () => ownedSourceStore.close()
  }, [ownedSourceStore, suppliedSourceStore])

  const acceptedReader =
    readerSnapshot.accepted !== null &&
    readerSnapshot.accepted.request.requestId === readerRequestId
      ? readerSnapshot.accepted
      : null
  const scene = acceptedReader?.result.scene ?? null
  const acceptedReconstruction = reconstructionSnapshot.accepted
  const candidate = acceptedReconstruction?.result ?? null
  const page = scene?.pages[Math.min(activePage, Math.max(0, (scene?.pages.length ?? 1) - 1))] ?? null
  const requiredBlocks = candidate?.blocks.filter((block) => block.reviewRequired) ?? []
  const decisions = useMemo(
    () =>
      requiredBlocks.flatMap((block): PdfReviewDecisionDto[] => {
        const draft = reviewDrafts[block.nodeId]
        if (draft === undefined) return []
        return [{ nodeId: block.nodeId, action: draft }]
      }),
    [requiredBlocks, reviewDrafts],
  )
  const decisionsComplete = requiredBlocks.every((block) => {
    const draft = reviewDrafts[block.nodeId]
    return draft?.kind === 'keep' || (draft?.kind === 'replace' && draft.text.trim().length > 0)
  })
  const canAccept =
    candidate !== null &&
    reconstructionSnapshot.phase === 'ready' &&
    decisionsComplete
  const scannedPageIndexes =
    scene?.pages
      .filter((candidatePage) => !candidatePage.elements.some((element) => element.kind === 'text'))
      .map((candidatePage) => candidatePage.pageIndex) ?? []
  const acceptedCandidate = reconstructionSnapshot.acceptedCandidate
  const status = inputError ?? reconstructionStatus(reconstructionSnapshot, labels)
  const readerStatus = readerStatusText(readerSnapshot.phase, labels)

  useEffect(() => {
    const accepted = acceptedReader
    if (accepted === null) return
    const identity = `${accepted.request.requestId}:${accepted.result.resultHash}`
    if (reconstructedReaderIdentity.current === identity) return
    reconstructedReaderIdentity.current = identity
    setReviewDrafts({})
    setActivePage(0)
    void requestReconstruction(accepted.result.scene, [])
  }, [acceptedReader])

  const onFileChange = (file: File | undefined): void => {
    if (file === undefined) return
    setFileName(file.name)
    setInputError(null)
    setSourceMetadata(null)
    setReaderRequestId(null)
    reconstructedReaderIdentity.current = null
    readerScheduler.cancel()
    reconstructionScheduler.cancel()
    if (file.size > MAX_PDF_SOURCE_BYTES) {
      setInputError(labels.fileTooLarge)
      return
    }
    void file
      .arrayBuffer()
      .then(async (buffer) => {
        const bytes = new Uint8Array(buffer)
        if (bytes.byteLength > MAX_PDF_SOURCE_BYTES) {
          setInputError(labels.fileTooLarge)
          return
        }
        const browserHash = await hashPdfSourceBytes(bytes)
        const metadata = await sourceStore.put(browserHash, bytes)
        const storedBytes = await sourceStore.get(browserHash)
        if (storedBytes === null) throw new Error('FLOW_PDF_SOURCE_STORE_MISSING')
        setSourceMetadata(metadata)
        const requestId = nextRequestId('pdf-reader')
        setReaderRequestId(requestId)
        void readerScheduler.request({
          protocolVersion: PDF_READER_PROTOCOL_VERSION,
          requestId,
          bytesHex: bytesToHex(storedBytes),
          firstPage: 0,
          pageCount: 2_048,
        })
      })
      .catch((error: unknown) => {
        setInputError(error instanceof Error ? error.message : labels.fileReadError)
      })
  }

  const requestReconstruction = async (
    sourceScene: NonNullable<typeof scene>,
    ocrPageIndexes: readonly number[],
  ): Promise<void> => {
    const request: PdfReconstructionRequestDto = {
      protocolVersion: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
      requestId: nextRequestId('pdf-reconstruction'),
      sourceHash: sourceScene.sourceHash,
      scene: sourceScene,
      locale: locale === 'uk' ? 'uk-UA' : 'en-US',
      ocrCandidates: [],
      ...(ocrPageIndexes.length === 0 ? {} : { ocrPageIndexes }),
    }
    setReviewDrafts({})
    await reconstructionScheduler.request(request)
  }

  const onRunOcr = (): void => {
    if (scene === null || scannedPageIndexes.length === 0) return
    void requestReconstruction(scene, scannedPageIndexes)
  }

  const onAccept = (): void => {
    if (!canAccept) return
    void reconstructionScheduler.accept(decisions)
  }

  const updateReview = (nodeId: string, draft: ReviewDraft): void => {
    setReviewDrafts((current) => ({ ...current, [nodeId]: draft }))
  }

  return (
    <section
      className="pdf-reconstruction-panel"
      data-pdf-reconstruction=""
      aria-label={labels.title}
    >
      <div className="pdf-reconstruction-toolbar">
        <div>
          <h2 className="pdf-reconstruction-title">{labels.title}</h2>
          <p className="pdf-reconstruction-description">{labels.description}</p>
        </div>
        <label className="pdf-reconstruction-file-label">
          <span>{labels.open}</span>
          <input
            type="file"
            accept="application/pdf,.pdf"
            data-pdf-reconstruction-input=""
            onChange={(event) => onFileChange(event.currentTarget.files?.[0])}
          />
        </label>
      </div>
      {fileName === null ? null : (
        <p className="pdf-reconstruction-file-name" data-pdf-reconstruction-file="">
          {fileName}
        </p>
      )}
      <div
        className={`pdf-reconstruction-status${inputError !== null || reconstructionSnapshot.errorCode !== null ? ' pdf-reconstruction-status-error' : ''}`}
        role={inputError !== null || reconstructionSnapshot.errorCode !== null ? 'alert' : 'status'}
        aria-live="polite"
        data-pdf-reconstruction-status=""
      >
        <span>{readerStatus}</span>
        <span> · {status}</span>
      </div>
      {sourceMetadata === null ? null : (
        <dl className="pdf-reconstruction-identities" data-pdf-reconstruction-identities="">
          <div>
            <dt>{labels.sourceIdentity}</dt>
            <dd>{sourceMetadata.sourceHash}</dd>
          </div>
          <div>
            <dt>{labels.sourceBytes}</dt>
            <dd>{sourceMetadata.byteLength}</dd>
          </div>
          {scene === null ? null : (
            <div>
              <dt>{labels.readerIdentity}</dt>
              <dd>{scene.sourceHash}</dd>
            </div>
          )}
        </dl>
      )}
      {scene === null ? (
        <div className="pdf-reconstruction-empty" data-pdf-reconstruction-empty="">
          {labels.empty}
        </div>
      ) : (
        <>
          {candidate === null ? null : (
            <ReconstructionReport candidate={candidate} labels={labels} />
          )}
          <div className="pdf-reconstruction-actions">
            <button
              type="button"
              data-pdf-reconstruction-ocr=""
              disabled={scannedPageIndexes.length === 0 || reconstructionSnapshot.phase === 'pending'}
              onClick={onRunOcr}
            >
              {labels.runOcr}
            </button>
            <button
              type="button"
              className="primary-action"
              data-pdf-reconstruction-accept=""
              disabled={!canAccept}
              onClick={onAccept}
            >
              {labels.accept}
            </button>
            {!decisionsComplete && requiredBlocks.length > 0 ? (
              <span data-pdf-reconstruction-accept-blocked="">{labels.acceptBlocked}</span>
            ) : null}
          </div>
          <div className="pdf-reconstruction-page-controls">
            <button
              type="button"
              disabled={activePage === 0}
              onClick={() => setActivePage((value) => Math.max(0, value - 1))}
            >
              {labels.previous}
            </button>
            <span data-pdf-reconstruction-position="">
              {labels.page} {activePage + 1} / {scene.pages.length}
            </span>
            <button
              type="button"
              disabled={activePage >= scene.pages.length - 1}
              onClick={() => setActivePage((value) => Math.min(scene.pages.length - 1, value + 1))}
            >
              {labels.next}
            </button>
          </div>
          <div className="pdf-reconstruction-columns">
            <section className="pdf-reconstruction-source-column" aria-labelledby="pdf-source-title">
              <h3 id="pdf-source-title">{labels.original}</h3>
              {page === null ? null : <SourcePage page={page} />}
              {candidate === null ? null : (
                <OpaqueIslandList candidate={candidate} pageIndex={page?.pageIndex ?? 0} labels={labels} />
              )}
            </section>
            <section className="pdf-reconstruction-candidate-column" aria-labelledby="pdf-candidate-title">
              <h3 id="pdf-candidate-title">{labels.candidate}</h3>
              {candidate === null ? (
                <p data-pdf-reconstruction-candidate-empty="">{labels.candidateEmpty}</p>
              ) : (
                <div data-pdf-reconstruction-candidate="">
                  {candidate.blocks.map((block) => (
                    <article
                      key={block.nodeId}
                      className={block.reviewRequired ? 'pdf-reconstruction-block review-required' : 'pdf-reconstruction-block'}
                      data-pdf-reconstruction-block={block.nodeId}
                    >
                      <p>{block.text}</p>
                      <span className="pdf-reconstruction-confidence">
                        {labels.confidence}: {(block.confidenceBasisPoints / 100).toFixed(0)}%
                      </span>
                      {block.reviewRequired ? (
                        <ReviewControls
                          block={block}
                          draft={reviewDrafts[block.nodeId]}
                          labels={labels}
                          onChange={(draft) => updateReview(block.nodeId, draft)}
                        />
                      ) : null}
                    </article>
                  ))}
                </div>
              )}
              {acceptedCandidate === null ? null : (
                <p className="pdf-reconstruction-accepted" data-pdf-reconstruction-accepted="">
                  {labels.accepted}
                </p>
              )}
            </section>
          </div>
        </>
      )}
    </section>
  )

  function nextRequestId(prefix: string): string {
    requestSequence.current += 1
    return `${prefix}-${requestSequence.current}`
  }
}

function ReconstructionReport({
  candidate,
  labels,
}: {
  readonly candidate: NonNullable<ReturnType<EditorPdfReconstructionScheduler['accepted']>>['result']
  readonly labels: ReconstructionLabels
}) {
  return (
    <section className="pdf-reconstruction-report" data-pdf-reconstruction-report="">
      <h3>{labels.report}</h3>
      <p>
        {candidate.report.partial ? labels.partial : labels.reportComplete} · {labels.reviewCount}:{' '}
        {candidate.report.reviewRequiredCount}
      </p>
      {candidate.report.diagnostics.length === 0 ? (
        <p>{labels.noDiagnostics}</p>
      ) : (
        <ul>
          {candidate.report.diagnostics.map((diagnostic, index) => (
            <li key={`${diagnostic.code}-${index}`} data-pdf-reconstruction-diagnostic={diagnostic.code}>
              <code>{diagnostic.code}</code>
              {diagnostic.pageIndex === null ? '' : ` · ${labels.page} ${diagnostic.pageIndex + 1}`}
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

function ReviewControls({
  block,
  draft,
  labels,
  onChange,
}: {
  readonly block: { readonly nodeId: string; readonly text: string }
  readonly draft: ReviewDraft | undefined
  readonly labels: ReconstructionLabels
  readonly onChange: (draft: ReviewDraft) => void
}) {
  const replaceText = draft?.kind === 'replace' ? draft.text : block.text
  return (
    <fieldset className="pdf-reconstruction-review" data-pdf-reconstruction-review={block.nodeId}>
      <legend>{labels.reviewRequired}</legend>
      <label>
        <input
          type="radio"
          name={`review-${block.nodeId}`}
          checked={draft?.kind === 'keep'}
          onChange={() => onChange({ kind: 'keep' })}
        />
        {labels.keep}
      </label>
      <label>
        <input
          type="radio"
          name={`review-${block.nodeId}`}
          checked={draft?.kind === 'replace'}
          onChange={() => onChange({ kind: 'replace', text: replaceText })}
        />
        {labels.replace}
      </label>
      {draft?.kind === 'replace' ? (
        <textarea
          value={draft.text}
          aria-label={labels.replace}
          onChange={(event) => onChange({ kind: 'replace', text: event.currentTarget.value })}
        />
      ) : null}
    </fieldset>
  )
}

function OpaqueIslandList({
  candidate,
  pageIndex,
  labels,
}: {
  readonly candidate: NonNullable<ReturnType<EditorPdfReconstructionScheduler['accepted']>>['result']
  readonly pageIndex: number
  readonly labels: ReconstructionLabels
}) {
  const islands = candidate.opaqueIslands.filter((island) => island.pageIndex === pageIndex)
  return (
    <div className="pdf-reconstruction-opaque" data-pdf-reconstruction-opaque="">
      <h4>{labels.opaque}</h4>
      {islands.length === 0 ? (
        <p>{labels.noOpaque}</p>
      ) : (
        <ul>
          {islands.map((island) => (
            <li key={`${island.pageIndex}-${island.elementIndex}`}>
              {island.kind} · {labels.element} {island.elementIndex + 1}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function SourcePage({ page }: { readonly page: PdfReaderScenePageDto }) {
  const width = Math.max(1, page.bounds.width / UNITS_PER_POINT)
  const height = Math.max(1, page.bounds.height / UNITS_PER_POINT)
  return (
    <div className="pdf-reconstruction-source-viewport">
      <div className="pdf-reconstruction-source-page-shell" style={{ width, height }}>
        <div
          className="pdf-reconstruction-source-page"
          style={{ width, height }}
          aria-hidden="true"
          data-pdf-reconstruction-page={page.pageIndex}
        >
          {page.elements.map((element, index) => (
            <SourceElement key={`${element.kind}-${index}`} element={element} pageHeight={height} />
          ))}
        </div>
      </div>
    </div>
  )
}

function SourceElement({
  element,
  pageHeight,
}: {
  readonly element: PdfReaderSceneElementDto
  readonly pageHeight: number
}) {
  if (element.kind === 'text') {
    return (
      <span
        className="pdf-reconstruction-source-text"
        style={position(element.value.rect, pageHeight)}
      >
        {element.value.text}
      </span>
    )
  }
  if (element.kind === 'image') {
    return (
      <img
        className="pdf-reconstruction-source-image"
        src={`data:${element.value.mediaType};base64,${hexToBase64(element.value.dataHex)}`}
        alt=""
        style={position(element.value.rect, pageHeight)}
      />
    )
  }
  return (
    <div
      className={`pdf-reconstruction-source-geometry geometry-${element.kind}`}
      style={position(element.value.rect, pageHeight)}
    />
  )
}

function position(
  rect: { readonly x: number; readonly y: number; readonly width: number; readonly height: number },
  pageHeight: number,
): { readonly left: number; readonly top: number; readonly width: number; readonly height: number } {
  return {
    left: rect.x / UNITS_PER_POINT,
    top: pageHeight - (rect.y + rect.height) / UNITS_PER_POINT,
    width: Math.max(1, rect.width / UNITS_PER_POINT),
    height: Math.max(1, rect.height / UNITS_PER_POINT),
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
    binary += String.fromCharCode(
      ...bytes.slice(index, index + 0x8000).map((item) => Number.parseInt(item, 16)),
    )
  }
  return globalThis.btoa(binary)
}

function readerStatusText(
  phase: 'idle' | 'pending' | 'ready' | 'error',
  labels: ReconstructionLabels,
): string {
  if (phase === 'pending') return labels.readerPending
  if (phase === 'ready') return labels.readerReady
  if (phase === 'error') return labels.readerError
  return labels.readerIdle
}

function reconstructionStatus(
  snapshot: ReturnType<EditorPdfReconstructionScheduler['snapshot']>,
  labels: ReconstructionLabels,
): string {
  if (snapshot.errorCode !== null) return `${labels.error} ${snapshot.errorCode}`
  if (snapshot.phase === 'pending') return labels.reconstructionPending
  if (snapshot.phase === 'ready') return labels.reconstructionReady
  return labels.reconstructionIdle
}

interface ReconstructionLabels {
  readonly title: string
  readonly description: string
  readonly open: string
  readonly fileTooLarge: string
  readonly fileReadError: string
  readonly readerIdle: string
  readonly readerPending: string
  readonly readerReady: string
  readonly readerError: string
  readonly reconstructionIdle: string
  readonly reconstructionPending: string
  readonly reconstructionReady: string
  readonly error: string
  readonly empty: string
  readonly sourceIdentity: string
  readonly sourceBytes: string
  readonly readerIdentity: string
  readonly original: string
  readonly candidate: string
  readonly candidateEmpty: string
  readonly report: string
  readonly reportComplete: string
  readonly partial: string
  readonly reviewCount: string
  readonly noDiagnostics: string
  readonly runOcr: string
  readonly accept: string
  readonly acceptBlocked: string
  readonly accepted: string
  readonly confidence: string
  readonly reviewRequired: string
  readonly keep: string
  readonly replace: string
  readonly opaque: string
  readonly noOpaque: string
  readonly element: string
  readonly previous: string
  readonly next: string
  readonly page: string
}

const ukLabels: ReconstructionLabels = {
  title: 'Реконструкція PDF',
  description: 'Оригінал зберігається незмінним; кандидат є best-effort і потребує чесного review.',
  open: 'Відкрити PDF',
  fileTooLarge: 'Файл перевищує обмеження source store: 64 МБ.',
  fileReadError: 'Не вдалося зберегти або прочитати PDF.',
  readerIdle: 'Оберіть PDF для reader.',
  readerPending: 'Читаємо оригінал у worker…',
  readerReady: 'Оригінальну сцену перевірено.',
  readerError: 'Reader не прийняв оригінал.',
  reconstructionIdle: 'Кандидат ще не побудований.',
  reconstructionPending: 'Будуємо candidate у reconstruction worker…',
  reconstructionReady: 'Кандидат готовий до review.',
  error: 'Реконструкцію не прийнято. Код:',
  empty: 'Оберіть PDF: bytes будуть збережені до запуску reader.',
  sourceIdentity: 'Immutable source identity',
  sourceBytes: 'Bytes',
  readerIdentity: 'Reader source hash',
  original: 'Оригінальна сцена · read-only',
  candidate: 'Semantic candidate · review',
  candidateEmpty: 'Candidate ще не готовий.',
  report: 'Report перед acceptance',
  reportComplete: 'Повний bounded report',
  partial: 'Частковий report',
  reviewCount: 'Потрібно рішень',
  noDiagnostics: 'Діагностик немає.',
  runOcr: 'Запросити OCR для сканованих сторінок',
  accept: 'Прийняти reviewed candidate',
  acceptBlocked: 'Acceptance заблоковано до всіх явних рішень.',
  accepted: 'Прийнято як reconstructed / best-effort; exact parity не заявляється.',
  confidence: 'Confidence',
  reviewRequired: 'Потрібен review',
  keep: 'Залишити',
  replace: 'Замінити текст',
  opaque: 'Opaque islands',
  noOpaque: 'Opaque islands на цій сторінці немає.',
  element: 'елемент',
  previous: 'Попередня',
  next: 'Наступна',
  page: 'Сторінка',
}

const enLabels: ReconstructionLabels = {
  title: 'PDF reconstruction',
  description: 'The original stays immutable; this best-effort candidate requires explicit review.',
  open: 'Open PDF',
  fileTooLarge: 'The file exceeds the 64 MB source-store limit.',
  fileReadError: 'The PDF could not be stored or read.',
  readerIdle: 'Choose a PDF for the reader.',
  readerPending: 'Reading the original in a worker…',
  readerReady: 'The original scene is verified.',
  readerError: 'The reader rejected the original.',
  reconstructionIdle: 'No candidate has been built yet.',
  reconstructionPending: 'Building the candidate in a reconstruction worker…',
  reconstructionReady: 'The candidate is ready for review.',
  error: 'The reconstruction was not accepted. Code:',
  empty: 'Choose a PDF; its bytes are stored before reader dispatch.',
  sourceIdentity: 'Immutable source identity',
  sourceBytes: 'Bytes',
  readerIdentity: 'Reader source hash',
  original: 'Original scene · read-only',
  candidate: 'Semantic candidate · review',
  candidateEmpty: 'The candidate is not ready yet.',
  report: 'Report before acceptance',
  reportComplete: 'Complete bounded report',
  partial: 'Partial report',
  reviewCount: 'Decisions required',
  noDiagnostics: 'No diagnostics.',
  runOcr: 'Request OCR for scanned pages',
  accept: 'Accept reviewed candidate',
  acceptBlocked: 'Acceptance is blocked until every explicit decision is present.',
  accepted: 'Accepted as reconstructed / best-effort; exact parity is not claimed.',
  confidence: 'Confidence',
  reviewRequired: 'Review required',
  keep: 'Keep',
  replace: 'Replace text',
  opaque: 'Opaque islands',
  noOpaque: 'No opaque islands on this page.',
  element: 'element',
  previous: 'Previous',
  next: 'Next',
  page: 'Page',
}
