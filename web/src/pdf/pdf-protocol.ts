/** String-only export protocol shared by the PDF worker and Rust/WASM. */
export const PDF_PROTOCOL_VERSION = 1 as const

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

export interface PdfWorkerDiagnosticDto {
  readonly requestId: string
  readonly sourceRevision: number
  readonly code: string
}

const MAX_REQUEST_ID_BYTES = 128
const MAX_SERIALIZED_REQUEST_CHARS = 96 * 1024 * 1024
const MAX_HEX_CHARS = 128 * 1024 * 1024

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
