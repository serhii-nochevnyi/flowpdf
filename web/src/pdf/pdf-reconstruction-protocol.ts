import type {
  PdfReadReportDto,
  PdfReaderRectDto,
  PdfReaderSceneDto,
  PdfReaderSceneElementDto,
} from './pdf-protocol.js'

export const PDF_RECONSTRUCTION_PROTOCOL_VERSION = 1 as const

export type PdfReconstructionLocale = 'uk-UA' | 'en-US'

export interface PdfOcrCandidateDto {
  readonly pageIndex: number
  readonly rect: PdfReaderRectDto
  readonly text: string
  readonly confidenceBasisPoints: number
  readonly provider: string
}

export interface PdfReconstructionRequestDto {
  readonly protocolVersion: typeof PDF_RECONSTRUCTION_PROTOCOL_VERSION
  readonly requestId: string
  readonly sourceHash: string
  readonly scene: PdfReaderSceneDto
  readonly locale: PdfReconstructionLocale
  readonly ocrCandidates: readonly PdfOcrCandidateDto[]
  /** Pages explicitly selected for caller-owned OCR; never sent to Rust. */
  readonly ocrPageIndexes?: readonly number[]
}

export type PdfMappingOriginDto =
  | { readonly kind: 'pdfText'; readonly mapping: 'toUnicode' | 'simpleEncoding' | 'missing' }
  | { readonly kind: 'ocr'; readonly provider: string }

export interface PdfSourceMappingDto {
  readonly pageIndex: number
  readonly rect: PdfReaderRectDto
  readonly objectRef: { readonly objectNumber: number; readonly generation: number } | null
  readonly operator: string | null
  readonly origin: PdfMappingOriginDto
}

export interface PdfReconstructedBlockDto {
  readonly nodeId: string
  readonly text: string
  readonly rect: PdfReaderRectDto
  readonly confidenceBasisPoints: number
  readonly reviewRequired: boolean
  readonly mappings: readonly PdfSourceMappingDto[]
}

export type PdfOpaqueIslandKindDto =
  | 'path'
  | 'clip'
  | 'image'
  | 'form'
  | 'link'
  | 'annotation'

export interface PdfOpaqueIslandDto {
  readonly pageIndex: number
  readonly elementIndex: number
  readonly rect: PdfReaderRectDto
  readonly kind: PdfOpaqueIslandKindDto
  readonly objectRef: { readonly objectNumber: number; readonly generation: number } | null
  readonly operator: string | null
}

export interface PdfReconstructionDiagnosticDto {
  readonly code: string
  readonly pageIndex: number | null
}

export interface PdfReconstructionReportDto {
  readonly readerReport: PdfReadReportDto
  readonly diagnostics: readonly PdfReconstructionDiagnosticDto[]
  readonly reviewRequiredCount: number
  readonly partial: boolean
}

export interface PdfDocumentCandidateDto {
  /** The closed canonical FlowDocument payload; Rust remains authoritative. */
  readonly document: unknown
  readonly canonicalJson: string
  readonly canonicalHash: string
}

export interface PdfReconstructionResultDto {
  readonly schemaVersion: number
  readonly sourceHash: string
  readonly sceneResultHash: string
  readonly candidate: PdfDocumentCandidateDto
  readonly blocks: readonly PdfReconstructedBlockDto[]
  readonly opaqueIslands: readonly PdfOpaqueIslandDto[]
  readonly report: PdfReconstructionReportDto
  readonly resultHash: string
}

export interface PdfReconstructionWorkerResultDto extends PdfReconstructionResultDto {
  readonly protocolVersion: typeof PDF_RECONSTRUCTION_PROTOCOL_VERSION
  readonly requestId: string
}

export interface PdfReviewDecisionDto {
  readonly nodeId: string
  readonly action:
    | { readonly kind: 'keep' }
    | { readonly kind: 'replace'; readonly text: string }
}

export interface PdfReconstructionAcceptRequestDto {
  readonly protocolVersion: typeof PDF_RECONSTRUCTION_PROTOCOL_VERSION
  readonly requestId: string
  readonly sourceRequestId: string
  readonly result: PdfReconstructionResultDto
  readonly decisions: readonly PdfReviewDecisionDto[]
}

export interface PdfReconstructionAcceptResultDto {
  readonly candidate: PdfDocumentCandidateDto
}

export interface PdfReconstructionAcceptedCandidateDto {
  readonly request: PdfReconstructionAcceptRequestDto
  readonly result: PdfReconstructionAcceptResultDto
}

export interface PdfReconstructionWasmBoundary {
  readonly reconstruct_pdf: (requestJson: string) => string
  readonly verify_pdf_reconstruction_response: (responseJson: string) => boolean
  readonly accept_pdf_reconstruction: (requestJson: string) => string
  readonly verify_pdf_reconstruction_accept_response: (responseJson: string) => boolean
}

export type PdfReconstructionSchedulerPhase = 'idle' | 'pending' | 'ready' | 'error'

export interface AcceptedPdfReconstructionDto {
  readonly request: PdfReconstructionRequestDto
  readonly result: PdfReconstructionWorkerResultDto
}

export interface PdfReconstructionSchedulerSnapshotDto {
  readonly phase: PdfReconstructionSchedulerPhase
  readonly accepted: AcceptedPdfReconstructionDto | null
  readonly acceptedCandidate: PdfReconstructionAcceptedCandidateDto | null
  readonly requestId: string | null
  readonly errorCode: string | null
}

export type PdfReconstructionWorkerInboundMessage =
  | { readonly type: 'reconstruct'; readonly request: PdfReconstructionRequestDto }
  | {
      readonly type: 'accept'
      readonly requestId: string
      readonly sourceRequestId: string
      readonly decisions: readonly PdfReviewDecisionDto[]
    }
  | { readonly type: 'cancel'; readonly requestId: string }

export type PdfReconstructionWorkerOutboundMessage =
  | {
      readonly type: 'accepted'
      readonly requestId: string
      readonly result: PdfReconstructionWorkerResultDto
    }
  | {
      readonly type: 'acceptedCandidate'
      readonly requestId: string
      readonly candidate: PdfReconstructionAcceptedCandidateDto
    }
  | { readonly type: 'discarded'; readonly requestId: string; readonly code: string }
  | { readonly type: 'failed'; readonly requestId: string; readonly code: string }
  | { readonly type: 'cancelled'; readonly requestId: string }

export interface PdfOcrPageDto {
  readonly pageIndex: number
  readonly bounds: PdfReaderRectDto
  readonly imageDataHex: readonly string[]
}

export interface PdfOcrRequestDto {
  readonly sourceHash: string
  readonly pages: readonly PdfOcrPageDto[]
}

export interface PdfOcrResponseDto {
  readonly sourceHash: string
  readonly candidates: readonly PdfOcrCandidateDto[]
}

export interface PdfOcrAdapter {
  recognize(request: PdfOcrRequestDto, signal: AbortSignal): Promise<PdfOcrResponseDto>
}

export class PdfOcrAdapterError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'PdfOcrAdapterError'
  }
}

export const MAX_PDF_RECONSTRUCTION_PAGES = 2_048
export const MAX_PDF_RECONSTRUCTION_ELEMENTS = 100_000
export const MAX_PDF_RECONSTRUCTION_BLOCKS = 16_384
export const MAX_PDF_RECONSTRUCTION_TEXT_BYTES = 512 * 1024
export const MAX_PDF_RECONSTRUCTION_OCR_CANDIDATES = 16_384
export const MAX_PDF_RECONSTRUCTION_OCR_TEXT_BYTES = 512 * 1024
export const MAX_PDF_RECONSTRUCTION_REQUEST_CHARS = 144 * 1024 * 1024
export const MAX_PDF_RECONSTRUCTION_OCR_IMAGE_HEX_CHARS = 16 * 1024 * 1024
export const MAX_PDF_RECONSTRUCTION_OCR_TOTAL_IMAGE_HEX_CHARS = 32 * 1024 * 1024
const MAX_REQUEST_ID_BYTES = 128

export function validatePdfReconstructionRequest(
  request: PdfReconstructionRequestDto,
): string | null {
  if (request.protocolVersion !== PDF_RECONSTRUCTION_PROTOCOL_VERSION) {
    return 'FLOW_PDF_RECONSTRUCTION_PROTOCOL_VERSION'
  }
  if (request.requestId.trim().length === 0 || request.requestId.length > MAX_REQUEST_ID_BYTES) {
    return 'FLOW_PDF_REQUEST_ID_INVALID'
  }
  if (!isRawPdfSourceHash(request.sourceHash)) {
    return 'FLOW_PDF_RECONSTRUCTION_SOURCE_INVALID'
  }
  if (!isPdfReaderScene(request.scene)) return 'FLOW_PDF_RECONSTRUCTION_SCENE_INVALID'
  if (request.scene.sourceHash !== request.sourceHash) {
    return 'FLOW_PDF_RECONSTRUCTION_SOURCE_INVALID'
  }
  if (request.scene.resultHash.trim().length === 0) {
    return 'FLOW_PDF_RECONSTRUCTION_SCENE_INVALID'
  }
  if (request.locale !== 'uk-UA' && request.locale !== 'en-US') {
    return 'FLOW_PDF_RECONSTRUCTION_LOCALE_UNSUPPORTED'
  }
  if (request.ocrCandidates.length > MAX_PDF_RECONSTRUCTION_OCR_CANDIDATES) {
    return 'FLOW_PDF_RECONSTRUCTION_LIMIT'
  }
  let ocrBytes = 0
  for (const candidate of request.ocrCandidates) {
    const error = validatePdfOcrCandidate(candidate, request.scene.pageCount)
    if (error !== null) return error
    ocrBytes += candidate.text.length
    if (ocrBytes > MAX_PDF_RECONSTRUCTION_OCR_TEXT_BYTES) {
      return 'FLOW_PDF_RECONSTRUCTION_LIMIT'
    }
  }
  if (request.ocrPageIndexes !== undefined) {
    const seen = new Set<number>()
    for (const pageIndex of request.ocrPageIndexes) {
      if (
        !Number.isSafeInteger(pageIndex) ||
        pageIndex < 0 ||
        pageIndex >= request.scene.pageCount ||
        seen.has(pageIndex)
      ) {
        return 'FLOW_PDF_RECONSTRUCTION_OCR_PAGE_INVALID'
      }
      seen.add(pageIndex)
    }
  }
  return null
}

export function validatePdfReconstructionResult(
  request: PdfReconstructionRequestDto,
  result: PdfReconstructionWorkerResultDto,
): string | null {
  if (result.protocolVersion !== PDF_RECONSTRUCTION_PROTOCOL_VERSION) {
    return 'FLOW_PDF_RECONSTRUCTION_PROTOCOL_VERSION'
  }
  if (result.requestId !== request.requestId) return 'FLOW_PDF_STALE_REQUEST_ID'
  if (result.sourceHash !== request.sourceHash) return 'FLOW_PDF_RECONSTRUCTION_SOURCE_INVALID'
  if (result.sceneResultHash !== request.scene.resultHash) {
    return 'FLOW_PDF_RECONSTRUCTION_SCENE_INVALID'
  }
  if (result.schemaVersion !== 1 || result.resultHash.trim().length === 0) {
    return 'FLOW_PDF_RECONSTRUCTION_RESULT_INVALID'
  }
  if (
    result.blocks.length > MAX_PDF_RECONSTRUCTION_BLOCKS ||
    result.opaqueIslands.length > MAX_PDF_RECONSTRUCTION_ELEMENTS
  ) {
    return 'FLOW_PDF_RECONSTRUCTION_LIMIT'
  }
  const candidateError = validatePdfDocumentCandidate(result.candidate, result.sourceHash)
  if (candidateError !== null) return candidateError
  const nodeIds = new Set<string>()
  for (const block of result.blocks) {
    if (
      block.nodeId.trim().length === 0 ||
      nodeIds.has(block.nodeId) ||
      block.text.length > MAX_PDF_RECONSTRUCTION_TEXT_BYTES ||
      !Number.isSafeInteger(block.confidenceBasisPoints) ||
      block.confidenceBasisPoints < 0 ||
      block.confidenceBasisPoints > 10_000 ||
      !isPdfReaderRect(block.rect) ||
      !Array.isArray(block.mappings) ||
      !block.mappings.every(isPdfSourceMapping)
    ) {
      return 'FLOW_PDF_RECONSTRUCTION_RESULT_INVALID'
    }
    nodeIds.add(block.nodeId)
  }
  if (
    !isRecord(result.report) ||
    !isReaderReport(result.report.readerReport) ||
    !Array.isArray(result.report.diagnostics) ||
    !result.report.diagnostics.every(isPdfReconstructionDiagnostic) ||
    !Number.isSafeInteger(result.report.reviewRequiredCount) ||
    result.report.reviewRequiredCount !== result.blocks.filter((block) => block.reviewRequired).length
  ) {
    return 'FLOW_PDF_RECONSTRUCTION_RESULT_INVALID'
  }
  if (!result.opaqueIslands.every(isPdfOpaqueIsland)) {
    return 'FLOW_PDF_RECONSTRUCTION_RESULT_INVALID'
  }
  return null
}

export function validatePdfReconstructionAcceptResult(
  request: PdfReconstructionAcceptRequestDto,
  candidate: PdfDocumentCandidateDto,
): string | null {
  if (request.protocolVersion !== PDF_RECONSTRUCTION_PROTOCOL_VERSION) {
    return 'FLOW_PDF_RECONSTRUCTION_PROTOCOL_VERSION'
  }
  if (request.requestId.trim().length === 0 || request.requestId.length > MAX_REQUEST_ID_BYTES) {
    return 'FLOW_PDF_REQUEST_ID_INVALID'
  }
  return validatePdfDocumentCandidate(candidate, request.result.sourceHash)
}

export function validatePdfOcrResponse(
  request: PdfOcrRequestDto,
  response: PdfOcrResponseDto,
): string | null {
  if (response.sourceHash !== request.sourceHash) {
    return 'FLOW_PDF_RECONSTRUCTION_OCR_SOURCE_MISMATCH'
  }
  if (!Array.isArray(response.candidates) || response.candidates.length > MAX_PDF_RECONSTRUCTION_OCR_CANDIDATES) {
    return 'FLOW_PDF_RECONSTRUCTION_OCR_LIMIT'
  }
  const requestedPages = new Set(request.pages.map((page) => page.pageIndex))
  let textBytes = 0
  for (const candidate of response.candidates) {
    if (!requestedPages.has(candidate.pageIndex)) {
      return 'FLOW_PDF_RECONSTRUCTION_OCR_PAGE_INVALID'
    }
    const error = validatePdfOcrCandidate(candidate, Math.max(...requestedPages, 0) + 1)
    if (error !== null) return `FLOW_PDF_RECONSTRUCTION_OCR_${error.replace('FLOW_PDF_RECONSTRUCTION_', '')}`
    textBytes += candidate.text.length
    if (textBytes > MAX_PDF_RECONSTRUCTION_OCR_TEXT_BYTES) {
      return 'FLOW_PDF_RECONSTRUCTION_OCR_LIMIT'
    }
  }
  return null
}

/** Builds a bounded OCR input from image scene elements only. */
export function buildPdfOcrRequest(
  scene: PdfReaderSceneDto,
  pageIndexes: readonly number[],
): PdfOcrRequestDto | null {
  const selected = new Set(pageIndexes)
  const pages: PdfOcrPageDto[] = []
  let totalImageChars = 0
  for (const page of scene.pages) {
    if (!selected.has(page.pageIndex) || page.elements.some((element) => element.kind === 'text')) {
      continue
    }
    const imageDataHex: string[] = []
    for (const element of page.elements) {
      if (element.kind !== 'image') continue
      const dataHex = element.value.dataHex
      if (
        !isHex(dataHex) ||
        dataHex.length > MAX_PDF_RECONSTRUCTION_OCR_IMAGE_HEX_CHARS ||
        totalImageChars + dataHex.length > MAX_PDF_RECONSTRUCTION_OCR_TOTAL_IMAGE_HEX_CHARS
      ) {
        return null
      }
      imageDataHex.push(dataHex)
      totalImageChars += dataHex.length
    }
    pages.push({ pageIndex: page.pageIndex, bounds: page.bounds, imageDataHex })
  }
  return { sourceHash: scene.sourceHash, pages }
}

function validatePdfOcrCandidate(candidate: PdfOcrCandidateDto, pageCount: number): string | null {
  if (
    !Number.isSafeInteger(candidate.pageIndex) ||
    candidate.pageIndex < 0 ||
    candidate.pageIndex >= pageCount ||
    !isPdfReaderRect(candidate.rect) ||
    candidate.text.length === 0 ||
    candidate.text.length > MAX_PDF_RECONSTRUCTION_TEXT_BYTES ||
    candidate.text.includes('\0') ||
    candidate.provider.trim().length === 0 ||
    candidate.provider.length > 128 ||
    !Number.isSafeInteger(candidate.confidenceBasisPoints) ||
    candidate.confidenceBasisPoints < 0 ||
    candidate.confidenceBasisPoints > 10_000
  ) {
    return 'FLOW_PDF_RECONSTRUCTION_LIMIT'
  }
  return null
}

function validatePdfDocumentCandidate(
  candidate: PdfDocumentCandidateDto,
  sourceHash: string,
): string | null {
  if (!isRecord(candidate) || typeof candidate.canonicalJson !== 'string' || candidate.canonicalJson.length === 0) {
    return 'FLOW_PDF_RECONSTRUCTION_CANDIDATE_INVALID'
  }
  if (candidate.canonicalJson.length > MAX_PDF_RECONSTRUCTION_REQUEST_CHARS) {
    return 'FLOW_PDF_RECONSTRUCTION_LIMIT'
  }
  if (typeof candidate.canonicalHash !== 'string' || candidate.canonicalHash.trim().length === 0) {
    return 'FLOW_PDF_RECONSTRUCTION_CANDIDATE_INVALID'
  }
  let parsed: unknown
  try {
    parsed = JSON.parse(candidate.canonicalJson) as unknown
  } catch {
    return 'FLOW_PDF_RECONSTRUCTION_CANDIDATE_INVALID'
  }
  if (!isRecord(parsed) || !isRecord(parsed.provenance) || !isRecord(candidate.document)) {
    return 'FLOW_PDF_RECONSTRUCTION_CANDIDATE_INVALID'
  }
  if (JSON.stringify(candidate.document) !== candidate.canonicalJson) {
    return 'FLOW_PDF_RECONSTRUCTION_CANDIDATE_INVALID'
  }
  if (
    parsed.provenance.kind !== 'externalReconstruction' ||
    parsed.provenance.sourceHash !== `pdf:blake3:v1:${sourceHash}` ||
    parsed.provenance.reconstructionSchemaVersion !== 1
  ) {
    return 'FLOW_PDF_RECONSTRUCTION_CANDIDATE_INVALID'
  }
  return null
}

function isPdfReaderScene(value: unknown): value is PdfReaderSceneDto {
  if (!isRecord(value)) return false
  if (
    value.schemaVersion !== 1 ||
    typeof value.sourceHash !== 'string' ||
    typeof value.pageCount !== 'number' ||
    !Number.isSafeInteger(value.pageCount) ||
    value.pageCount < 0 ||
    typeof value.firstPage !== 'number' ||
    !Array.isArray(value.pages) ||
    !isReaderReport(value.report) ||
    typeof value.resultHash !== 'string'
  ) {
    return false
  }
  if (value.pages.length > MAX_PDF_RECONSTRUCTION_PAGES) return false
  let elementCount = 0
  return value.pages.every((page) => {
    if (!isRecord(page) || !isPdfReaderRect(page.bounds) || !Array.isArray(page.elements)) {
      return false
    }
    elementCount += page.elements.length
    if (elementCount > MAX_PDF_RECONSTRUCTION_ELEMENTS) return false
    return page.elements.every(isPdfReaderSceneElement)
  })
}

function isPdfReaderSceneElement(value: unknown): value is PdfReaderSceneElementDto {
  if (!isRecord(value) || typeof value.kind !== 'string' || !isRecord(value.value)) return false
  const rect = value.value.rect
  if (!isPdfReaderRect(rect)) return false
  if (value.kind === 'text') return typeof value.value.text === 'string'
  if (value.kind === 'image') {
    return typeof value.value.dataHex === 'string' && isHex(value.value.dataHex)
  }
  return true
}

function isReaderReport(value: unknown): value is PdfReadReportDto {
  if (!isRecord(value) || typeof value.partial !== 'boolean' || !Array.isArray(value.diagnostics)) {
    return false
  }
  return value.diagnostics.every(
    (diagnostic) =>
      isRecord(diagnostic) &&
      typeof diagnostic.code === 'string' &&
      (diagnostic.severity === 'warning' || diagnostic.severity === 'blocked'),
  )
}

function isPdfSourceMapping(value: unknown): value is PdfSourceMappingDto {
  if (!isRecord(value) || !isPdfReaderRect(value.rect)) return false
  if (
    typeof value.pageIndex !== 'number' ||
    !Number.isSafeInteger(value.pageIndex) ||
    value.pageIndex < 0 ||
    (value.objectRef !== null && !isPdfObjectRef(value.objectRef)) ||
    (value.operator !== null && typeof value.operator !== 'string') ||
    !isRecord(value.origin)
  ) {
    return false
  }
  if (value.origin.kind === 'ocr') return typeof value.origin.provider === 'string'
  return (
    value.origin.kind === 'pdfText' &&
    (value.origin.mapping === 'toUnicode' ||
      value.origin.mapping === 'simpleEncoding' ||
      value.origin.mapping === 'missing')
  )
}

function isPdfOpaqueIsland(value: unknown): value is PdfOpaqueIslandDto {
  return (
    isRecord(value) &&
    typeof value.pageIndex === 'number' &&
    Number.isSafeInteger(value.pageIndex) &&
    value.pageIndex >= 0 &&
    typeof value.elementIndex === 'number' &&
    Number.isSafeInteger(value.elementIndex) &&
    value.elementIndex >= 0 &&
    isPdfReaderRect(value.rect) &&
    (value.kind === 'path' ||
      value.kind === 'clip' ||
      value.kind === 'image' ||
      value.kind === 'form' ||
      value.kind === 'link' ||
      value.kind === 'annotation') &&
    (value.objectRef === null || isPdfObjectRef(value.objectRef)) &&
    (value.operator === null || typeof value.operator === 'string')
  )
}

function isPdfReconstructionDiagnostic(value: unknown): value is PdfReconstructionDiagnosticDto {
  return (
    isRecord(value) &&
    typeof value.code === 'string' &&
    (value.pageIndex === null ||
      (typeof value.pageIndex === 'number' &&
        Number.isSafeInteger(value.pageIndex) &&
        value.pageIndex >= 0))
  )
}

function isPdfObjectRef(value: unknown): value is { readonly objectNumber: number; readonly generation: number } {
  return (
    isRecord(value) &&
    typeof value.objectNumber === 'number' &&
    Number.isSafeInteger(value.objectNumber) &&
    value.objectNumber > 0 &&
    typeof value.generation === 'number' &&
    Number.isSafeInteger(value.generation) &&
    value.generation >= 0
  )
}

function isPdfReaderRect(value: unknown): value is PdfReaderRectDto {
  return (
    isRecord(value) &&
    ['x', 'y', 'width', 'height'].every(
      (key) => typeof value[key] === 'number' && Number.isFinite(value[key]),
    )
  )
}

function isRawPdfSourceHash(value: string): boolean {
  return /^[0-9a-f]{64}$/.test(value)
}

function isHex(value: string): boolean {
  return value.length % 2 === 0 && /^[0-9a-fA-F]*$/.test(value)
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}
