/** String-only export protocol shared by the PDF worker and Rust/WASM. */
export const PDF_PROTOCOL_VERSION = 1 as const
export const PDF_READER_PROTOCOL_VERSION = 1 as const

export interface PdfExportRequestDto {
  readonly protocolVersion: typeof PDF_PROTOCOL_VERSION
  readonly requestId: string
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly layoutSettingsFingerprint: string
  readonly layoutResultHash: string
  /** Closed Rust JSON request; the worker never rewrites its semantic fields. */
  readonly serializedRequest: string
}

export interface PdfExportManifestDto {
  readonly manifestVersion: number
  readonly exportSchemaVersion: number
  readonly sourceDocumentId: string | null
  readonly sourceRevision: number
  readonly sourceSchemaVersion: number | null
  readonly sourceHash: string
  readonly sourcePayloadHash: string | null
  readonly layoutSettingsFingerprint: string
  readonly layoutResultHash: string | null
  readonly engine: string
  readonly engineVersion: string
  readonly fontCatalogIdentity: string | null
  readonly fontFaces: readonly PdfFontManifestIdentityDto[]
  readonly hyphenationIdentity: string | null
  readonly options: unknown
  readonly exportFingerprint: string
}

export interface PdfFontManifestIdentityDto {
  readonly faceId: string
  readonly contentHash: string
}

export interface PdfSupportReportDto {
  readonly supported: readonly string[]
  readonly unsupported: readonly string[]
}

export interface PdfExportResultDto {
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly layoutSettingsFingerprint: string
  readonly exportFingerprint: string
  readonly byteHash: string
  readonly bytesHex: string
  readonly privateSourceStreamHex: string
  readonly manifest: PdfExportManifestDto
  readonly supportReport: PdfSupportReportDto
}

export interface PdfExportWorkerResultDto extends PdfExportResultDto {
  readonly protocolVersion: typeof PDF_PROTOCOL_VERSION
  readonly requestId: string
}

export interface AcceptedPdfExportDto {
  readonly request: PdfExportRequestDto
  readonly result: PdfExportWorkerResultDto
}

export type PdfExportSchedulerPhase = 'idle' | 'pending' | 'ready' | 'error'

export interface PdfExportSchedulerSnapshotDto {
  readonly phase: PdfExportSchedulerPhase
  readonly accepted: AcceptedPdfExportDto | null
  readonly requestId: string | null
  readonly errorCode: string | null
}

export type PdfWorkerInboundMessage =
  | { readonly type: 'export'; readonly request: PdfExportRequestDto }
  | { readonly type: 'cancel'; readonly requestId: string }

export type PdfWorkerOutboundMessage =
  | {
      readonly type: 'accepted'
      readonly requestId: string
      readonly result: PdfExportWorkerResultDto
    }
  | { readonly type: 'discarded'; readonly requestId: string; readonly code: string }
  | { readonly type: 'failed'; readonly requestId: string; readonly code: string }
  | { readonly type: 'cancelled'; readonly requestId: string }

export interface PdfRecoveryRequestDto {
  readonly protocolVersion: typeof PDF_PROTOCOL_VERSION
  readonly requestId: string
  readonly privateSourceStreamHex: string | null
  readonly expected: {
    readonly documentId: string | null
    readonly revision: number | null
    readonly canonicalHash: string | null
    readonly exportFingerprint: string | null
  }
}

export interface PdfRecoveryResultDto {
  readonly exact: boolean
  readonly canonicalJson: string
  readonly canonicalHash: string
  readonly documentId: string
  readonly revision: number
  readonly manifest: PdfExportManifestDto
}

export interface PdfWasmBoundary {
  readonly export_pdf: (requestJson: string) => string
  readonly verify_pdf_export_response: (responseJson: string) => boolean
  readonly recover_owned_source: (requestJson: string) => string
}

export interface PdfReaderLimitsDto {
  readonly maxInputBytes: number
  readonly maxObjects: number
  readonly maxPages: number
  readonly maxNesting: number
  readonly maxStringBytes: number
  readonly maxArrayItems: number
  readonly maxDictionaryEntries: number
  readonly maxStreamBytes: number
  readonly maxDecodedStreamBytes: number
  readonly maxContentTokens: number
  readonly maxSceneElements: number
  readonly maxImagePixels: number
  readonly maxFormDepth: number
}

export interface PdfReaderRequestDto {
  readonly protocolVersion: typeof PDF_READER_PROTOCOL_VERSION
  readonly requestId: string
  readonly bytesHex: string
  readonly firstPage: number
  readonly pageCount: number
  readonly limits?: PdfReaderLimitsDto
}

export type PdfReadSeverityDto = 'warning' | 'blocked'

export interface PdfReadDiagnosticDto {
  readonly code: string
  readonly severity: PdfReadSeverityDto
  readonly pageIndex: number | null
  readonly objectNumber: number | null
  readonly operator: string | null
}

export interface PdfReadReportDto {
  readonly diagnostics: readonly PdfReadDiagnosticDto[]
  readonly partial: boolean
}

export interface PdfReaderSummaryDto {
  readonly schemaVersion: number
  readonly pdfVersion: string
  readonly sourceHash: string
  readonly pageCount: number
}

export interface PdfReaderObjectRefDto {
  readonly objectNumber: number
  readonly generation: number
}

export interface PdfReaderProvenanceDto {
  readonly objectRef: PdfReaderObjectRefDto
  readonly streamOffset: number
  readonly operator: string | null
}

export interface PdfReaderRectDto {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

export interface PdfReaderGlyphDto {
  readonly code: number
  readonly unicode: string | null
  readonly rect: PdfReaderRectDto
  readonly advance: number
  readonly provenance: PdfReaderProvenanceDto
}

export interface PdfReaderTextDto {
  readonly text: string
  readonly rect: PdfReaderRectDto
  readonly fontName: string | null
  readonly mapping: 'toUnicode' | 'simpleEncoding' | 'missing'
  readonly glyphs: readonly PdfReaderGlyphDto[]
  readonly provenance: PdfReaderProvenanceDto
}

export type PdfReaderPathCommandDto =
  | { readonly kind: 'moveTo'; readonly x: number; readonly y: number }
  | { readonly kind: 'lineTo'; readonly x: number; readonly y: number }
  | {
      readonly kind: 'curveTo'
      readonly x1: number
      readonly y1: number
      readonly x2: number
      readonly y2: number
      readonly x3: number
      readonly y3: number
    }
  | { readonly kind: 'closePath' }

export interface PdfReaderPathDto {
  readonly commands: readonly PdfReaderPathCommandDto[]
  readonly rect: PdfReaderRectDto
  readonly stroke: boolean
  readonly fill: boolean
  readonly provenance: PdfReaderProvenanceDto
}

export interface PdfReaderClipDto {
  readonly commands: readonly PdfReaderPathCommandDto[]
  readonly evenOdd: boolean
  readonly rect: PdfReaderRectDto
  readonly provenance: PdfReaderProvenanceDto
}

export interface PdfReaderImageDto {
  readonly width: number
  readonly height: number
  readonly mediaType: string
  readonly filter: string
  readonly dataHex: string
  readonly rect: PdfReaderRectDto
  readonly provenance: PdfReaderProvenanceDto
}

export interface PdfReaderFormDto {
  readonly fieldType: string | null
  readonly name: string | null
  readonly value: string | null
  readonly rect: PdfReaderRectDto
  readonly provenance: PdfReaderProvenanceDto
}

export interface PdfReaderLinkDto {
  readonly destinationPage: number | null
  readonly rect: PdfReaderRectDto
  readonly internal: boolean
  readonly provenance: PdfReaderProvenanceDto
}

export interface PdfReaderAnnotationDto {
  readonly subtype: string | null
  readonly kind: 'link' | 'widget' | 'supported' | 'unsupported'
  readonly rect: PdfReaderRectDto
  readonly provenance: PdfReaderProvenanceDto
}

export type PdfReaderSceneElementDto =
  | { readonly kind: 'text'; readonly value: PdfReaderTextDto }
  | { readonly kind: 'path'; readonly value: PdfReaderPathDto }
  | { readonly kind: 'clip'; readonly value: PdfReaderClipDto }
  | { readonly kind: 'image'; readonly value: PdfReaderImageDto }
  | { readonly kind: 'form'; readonly value: PdfReaderFormDto }
  | { readonly kind: 'link'; readonly value: PdfReaderLinkDto }
  | { readonly kind: 'annotation'; readonly value: PdfReaderAnnotationDto }

export interface PdfReaderScenePageDto {
  readonly pageIndex: number
  readonly sourceObject: PdfReaderObjectRefDto
  readonly bounds: PdfReaderRectDto
  readonly elements: readonly PdfReaderSceneElementDto[]
  readonly report: PdfReadReportDto
  readonly partial: boolean
}

export interface PdfReaderSceneDto {
  readonly schemaVersion: number
  readonly sourceHash: string
  readonly pageCount: number
  readonly firstPage: number
  readonly pages: readonly PdfReaderScenePageDto[]
  readonly report: PdfReadReportDto
  readonly resultHash: string
}

export interface PdfReaderResultDto {
  readonly summary: PdfReaderSummaryDto
  readonly scene: PdfReaderSceneDto
  readonly resultHash: string
}

export interface PdfReaderWorkerResultDto extends PdfReaderResultDto {
  readonly protocolVersion: typeof PDF_READER_PROTOCOL_VERSION
  readonly requestId: string
}

export interface PdfReaderWasmBoundary {
  readonly read_pdf: (requestJson: string) => string
  readonly verify_pdf_reader_response: (responseJson: string) => boolean
}

export interface AcceptedPdfReaderDto {
  readonly request: PdfReaderRequestDto
  readonly result: PdfReaderWorkerResultDto
}

export type PdfReaderSchedulerPhase = 'idle' | 'pending' | 'ready' | 'error'

export interface PdfReaderSchedulerSnapshotDto {
  readonly phase: PdfReaderSchedulerPhase
  readonly accepted: AcceptedPdfReaderDto | null
  readonly requestId: string | null
  readonly errorCode: string | null
}

export type PdfReaderWorkerInboundMessage =
  | { readonly type: 'read'; readonly request: PdfReaderRequestDto }
  | { readonly type: 'cancel'; readonly requestId: string }

export type PdfReaderWorkerOutboundMessage =
  | {
      readonly type: 'accepted'
      readonly requestId: string
      readonly result: PdfReaderWorkerResultDto
    }
  | { readonly type: 'discarded'; readonly requestId: string; readonly code: string }
  | { readonly type: 'failed'; readonly requestId: string; readonly code: string }
  | { readonly type: 'cancelled'; readonly requestId: string }

export interface PdfWorkerDiagnosticDto {
  readonly requestId: string
  readonly sourceRevision: number
  readonly code: string
}

const MAX_REQUEST_ID_BYTES = 128
const MAX_SERIALIZED_REQUEST_CHARS = 96 * 1024 * 1024
const MAX_HEX_CHARS = 128 * 1024 * 1024
const MAX_READER_INPUT_BYTES = 64 * 1024 * 1024
const MAX_READER_HEX_CHARS = MAX_READER_INPUT_BYTES * 2

export function validatePdfReaderRequest(request: PdfReaderRequestDto): string | null {
  if (request.protocolVersion !== PDF_READER_PROTOCOL_VERSION) {
    return 'FLOW_PDF_READER_PROTOCOL_VERSION'
  }
  if (
    request.requestId.trim().length === 0 ||
    request.requestId.length > MAX_REQUEST_ID_BYTES
  ) {
    return 'FLOW_PDF_REQUEST_ID_INVALID'
  }
  if (!isHex(request.bytesHex) || request.bytesHex.length > MAX_READER_HEX_CHARS) {
    return 'FLOW_PDF_READER_BYTES_INVALID'
  }
  if (!Number.isSafeInteger(request.firstPage) || request.firstPage < 0) {
    return 'FLOW_PDF_READER_PAGE_WINDOW_INVALID'
  }
  if (!Number.isSafeInteger(request.pageCount) || request.pageCount <= 0) {
    return 'FLOW_PDF_READER_PAGE_WINDOW_INVALID'
  }
  return null
}

export function validatePdfReaderResult(
  request: PdfReaderRequestDto,
  result: PdfReaderWorkerResultDto,
): string | null {
  if (result.protocolVersion !== PDF_READER_PROTOCOL_VERSION) {
    return 'FLOW_PDF_READER_PROTOCOL_VERSION'
  }
  if (result.requestId !== request.requestId) return 'FLOW_PDF_STALE_REQUEST_ID'
  if (result.summary.sourceHash !== result.scene.sourceHash) {
    return 'FLOW_PDF_READER_SOURCE_HASH_MISMATCH'
  }
  if (result.summary.pageCount !== result.scene.pageCount) {
    return 'FLOW_PDF_READER_PAGE_COUNT_MISMATCH'
  }
  if (result.scene.firstPage !== request.firstPage) {
    return 'FLOW_PDF_READER_FIRST_PAGE_MISMATCH'
  }
  if (result.scene.pages.length > request.pageCount) {
    return 'FLOW_PDF_READER_PAGE_WINDOW_MISMATCH'
  }
  if (result.resultHash.trim().length === 0 || result.resultHash !== result.scene.resultHash) {
    return 'FLOW_PDF_READER_RESULT_HASH_INVALID'
  }
  if (result.scene.report === undefined) return 'FLOW_PDF_READER_REPORT_MISSING'
  return null
}

/** Validates only request metadata; Rust remains authoritative for the JSON. */
export function validatePdfExportRequest(request: PdfExportRequestDto): string | null {
  if (request.protocolVersion !== PDF_PROTOCOL_VERSION) return 'FLOW_PDF_PROTOCOL_VERSION'
  if (
    request.requestId.trim().length === 0 ||
    request.requestId.length > MAX_REQUEST_ID_BYTES
  ) {
    return 'FLOW_PDF_REQUEST_ID_INVALID'
  }
  if (!Number.isSafeInteger(request.sourceRevision) || request.sourceRevision < 0) {
    return 'FLOW_PDF_SOURCE_REVISION_INVALID'
  }
  if (request.sourceHash.trim().length === 0) return 'FLOW_PDF_SOURCE_HASH_INVALID'
  if (request.layoutSettingsFingerprint.trim().length === 0) {
    return 'FLOW_PDF_LAYOUT_SETTINGS_INVALID'
  }
  if (request.layoutResultHash.trim().length === 0) return 'FLOW_PDF_LAYOUT_RESULT_INVALID'
  if (request.serializedRequest.length === 0) return 'FLOW_PDF_REQUEST_EMPTY'
  if (request.serializedRequest.length > MAX_SERIALIZED_REQUEST_CHARS) {
    return 'FLOW_PDF_REQUEST_LIMIT'
  }
  return null
}

/** Compares every source/layout identity before export bytes can publish. */
export function validatePdfExportResult(
  request: PdfExportRequestDto,
  result: PdfExportWorkerResultDto,
): string | null {
  if (result.protocolVersion !== PDF_PROTOCOL_VERSION) return 'FLOW_PDF_PROTOCOL_VERSION'
  if (result.requestId !== request.requestId) return 'FLOW_PDF_STALE_REQUEST_ID'
  if (result.sourceRevision !== request.sourceRevision) return 'FLOW_PDF_STALE_SOURCE_REVISION'
  if (result.sourceHash !== request.sourceHash) return 'FLOW_PDF_STALE_SOURCE_HASH'
  if (result.layoutSettingsFingerprint !== request.layoutSettingsFingerprint) {
    return 'FLOW_PDF_STALE_LAYOUT_SETTINGS'
  }
  if (result.manifest.sourceRevision !== result.sourceRevision) {
    return 'FLOW_PDF_MANIFEST_SOURCE_REVISION_MISMATCH'
  }
  if (result.manifest.sourceHash !== result.sourceHash) {
    return 'FLOW_PDF_MANIFEST_SOURCE_HASH_MISMATCH'
  }
  if (result.manifest.layoutSettingsFingerprint !== result.layoutSettingsFingerprint) {
    return 'FLOW_PDF_MANIFEST_LAYOUT_SETTINGS_MISMATCH'
  }
  if (result.manifest.exportFingerprint !== result.exportFingerprint) {
    return 'FLOW_PDF_MANIFEST_EXPORT_FINGERPRINT_MISMATCH'
  }
  if (result.manifest.layoutResultHash !== request.layoutResultHash) {
    return 'FLOW_PDF_STALE_LAYOUT_RESULT'
  }
  if (result.exportFingerprint.trim().length === 0) return 'FLOW_PDF_EXPORT_HASH_INVALID'
  if (result.byteHash.trim().length === 0) return 'FLOW_PDF_BYTE_HASH_INVALID'
  if (!isHex(result.bytesHex) || result.bytesHex.length > MAX_HEX_CHARS) {
    return 'FLOW_PDF_BYTES_INVALID'
  }
  if (!isHex(result.privateSourceStreamHex) || result.privateSourceStreamHex.length === 0) {
    return 'FLOW_PDF_SOURCE_STREAM_INVALID'
  }
  return null
}

export function isHex(value: string): boolean {
  return value.length % 2 === 0 && /^[0-9a-fA-F]*$/.test(value)
}
