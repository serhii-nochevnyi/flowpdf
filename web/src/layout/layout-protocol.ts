/** Version shared by the browser worker and the Rust/WASM layout envelope. */
export const LAYOUT_PROTOCOL_VERSION = 1 as const

export interface LayoutViewportHintDto {
  readonly firstPage: number
  readonly pageCount: number
}

/**
 * The worker request is immutable and revision-bound. `serializedRequest` is
 * the closed Rust DTO; TypeScript carries it but does not inspect or rewrite
 * canonical document text, line breaks, or page coordinates.
 */
export interface LayoutRequestDto {
  readonly protocolVersion: typeof LAYOUT_PROTOCOL_VERSION
  readonly requestId: string
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly expectedLayoutSettingsFingerprint: string | null
  readonly fontCatalogIdentity: string
  readonly hyphenationDataIdentity: string | null
  readonly viewport: LayoutViewportHintDto
  readonly serializedRequest: string
}

export interface LayoutRectDto {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

export interface LayoutSourceRangeDto {
  readonly utf8Start: number
  readonly utf8End: number
  readonly utf16Start: number
  readonly utf16End: number
}

export interface LayoutFragmentDto {
  readonly id: string
  readonly kind: string
  readonly sourceNodeId: string | null
  readonly source: LayoutSourceRangeDto | null
  readonly rect: LayoutRectDto
  readonly breakReason: string | null
  readonly derived: boolean
  readonly repeatIndex: number
  readonly children: readonly LayoutFragmentDto[]
}

export interface LayoutDiagnosticDto {
  readonly code: string
  readonly pageIndex: number | null
  readonly sourceNodeId: string | null
  readonly value: number | null
}

export interface LayoutPageDto {
  readonly pageIndex: number
  readonly sectionId: string
  readonly pageSettings: unknown
  readonly bounds: LayoutRectDto
  readonly contentRect: LayoutRectDto
  readonly startReason: string
  readonly header: LayoutFragmentDto | null
  readonly footer: LayoutFragmentDto | null
  readonly fragments: readonly LayoutFragmentDto[]
}

/** Rust-derived layout data. It contains no authored text or font bytes. */
export interface LayoutResultDto {
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly layoutSettingsFingerprint: string
  readonly fontCatalogIdentity: string
  readonly hyphenationDataIdentity: string | null
  readonly pages: readonly LayoutPageDto[]
  readonly diagnostics: readonly LayoutDiagnosticDto[]
  readonly resultHash: string
}

/** Result envelope after the worker has associated Rust output with a request. */
export interface LayoutWorkerResultDto extends LayoutResultDto {
  readonly protocolVersion: typeof LAYOUT_PROTOCOL_VERSION
  readonly requestId: string
}

export interface AcceptedLayoutDto {
  readonly request: LayoutRequestDto
  readonly result: LayoutWorkerResultDto
}

export type LayoutSchedulerPhase = 'idle' | 'pending' | 'ready' | 'error'

export interface LayoutSchedulerSnapshotDto {
  readonly phase: LayoutSchedulerPhase
  readonly accepted: AcceptedLayoutDto | null
  readonly requestId: string | null
  readonly errorCode: string | null
}

export type LayoutWorkerInboundMessage =
  | { readonly type: 'paginate'; readonly request: LayoutRequestDto }
  | { readonly type: 'cancel'; readonly requestId: string }

export type LayoutWorkerOutboundMessage =
  | { readonly type: 'accepted'; readonly requestId: string; readonly result: LayoutWorkerResultDto }
  | {
      readonly type: 'discarded'
      readonly requestId: string
      readonly code: string
    }
  | {
      readonly type: 'failed'
      readonly requestId: string
      readonly code: string
    }
  | {
      readonly type: 'cancelled'
      readonly requestId: string
    }

export interface LayoutWorkerDiagnosticDto {
  readonly requestId: string
  readonly sourceRevision: number
  readonly code: string
}

const MAX_REQUEST_ID_BYTES = 128

/** Validates only protocol metadata; Rust remains authoritative for the DTO. */
export function validateLayoutRequest(request: LayoutRequestDto): string | null {
  if (request.protocolVersion !== LAYOUT_PROTOCOL_VERSION) {
    return 'FLOW_LAYOUT_PROTOCOL_VERSION'
  }
  if (
    request.requestId.trim().length === 0 ||
    request.requestId.length > MAX_REQUEST_ID_BYTES
  ) {
    return 'FLOW_LAYOUT_REQUEST_ID_INVALID'
  }
  if (!Number.isSafeInteger(request.sourceRevision) || request.sourceRevision < 0) {
    return 'FLOW_LAYOUT_SOURCE_REVISION_INVALID'
  }
  if (request.sourceHash.trim().length === 0) {
    return 'FLOW_LAYOUT_SOURCE_HASH_INVALID'
  }
  if (request.fontCatalogIdentity.trim().length === 0) {
    return 'FLOW_LAYOUT_FONT_IDENTITY_INVALID'
  }
  if (
    request.expectedLayoutSettingsFingerprint !== null &&
    request.expectedLayoutSettingsFingerprint.trim().length === 0
  ) {
    return 'FLOW_LAYOUT_SETTINGS_FINGERPRINT_INVALID'
  }
  if (
    !Number.isSafeInteger(request.viewport.firstPage) ||
    !Number.isSafeInteger(request.viewport.pageCount) ||
    request.viewport.firstPage < 0 ||
    request.viewport.pageCount < 1
  ) {
    return 'FLOW_LAYOUT_VIEWPORT_INVALID'
  }
  if (request.serializedRequest.length === 0) {
    return 'FLOW_LAYOUT_SERIALIZED_REQUEST_INVALID'
  }
  return null
}

/** Compares all publish guards before a result can reach the accepted store. */
export function validateLayoutResult(
  request: LayoutRequestDto,
  result: LayoutWorkerResultDto,
): string | null {
  if (result.protocolVersion !== LAYOUT_PROTOCOL_VERSION) {
    return 'FLOW_LAYOUT_PROTOCOL_VERSION'
  }
  if (result.requestId !== request.requestId) {
    return 'FLOW_LAYOUT_STALE_REQUEST_ID'
  }
  if (result.sourceRevision !== request.sourceRevision) {
    return 'FLOW_LAYOUT_STALE_SOURCE_REVISION'
  }
  if (result.sourceHash !== request.sourceHash) {
    return 'FLOW_LAYOUT_STALE_SOURCE_HASH'
  }
  if (
    request.expectedLayoutSettingsFingerprint !== null &&
    result.layoutSettingsFingerprint !== request.expectedLayoutSettingsFingerprint
  ) {
    return 'FLOW_LAYOUT_STALE_SETTINGS_FINGERPRINT'
  }
  if (result.fontCatalogIdentity !== request.fontCatalogIdentity) {
    return 'FLOW_LAYOUT_STALE_FONT_IDENTITY'
  }
  if (result.hyphenationDataIdentity !== request.hyphenationDataIdentity) {
    return 'FLOW_LAYOUT_STALE_DATA_IDENTITY'
  }
  if (result.resultHash.trim().length === 0) {
    return 'FLOW_LAYOUT_RESULT_HASH_INVALID'
  }
  return null
}
