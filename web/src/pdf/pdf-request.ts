import {
  type FormProjectionSelectionDto,
  validateFormProjectionSelection,
} from '../forms/form-projection.js'
import type { EditorAcceptedSnapshot } from '../editor/editor-store.js'
import type { AcceptedLayoutDto } from '../layout/layout-protocol.js'
import {
  PDF_PROTOCOL_VERSION,
  type PdfFontManifestIdentityDto,
  type PdfExportRequestDto,
  validatePdfExportRequest,
} from './pdf-protocol.js'

export interface PdfExportRequestBuilderOptions {
  readonly requestId: string
  readonly producer?: string
  readonly formSelection?: FormProjectionSelectionDto | null
  readonly fontFaces?: readonly PdfFontManifestIdentityDto[]
}

/**
 * Builds the closed Rust PDF request from accepted editor/layout projections.
 * It never reads DOM text or derives page geometry in the browser.
 */
export function createPdfExportRequest(
  accepted: EditorAcceptedSnapshot,
  layout: AcceptedLayoutDto,
  options: PdfExportRequestBuilderOptions,
): PdfExportRequestDto | null {
  if (options.requestId.trim().length === 0 || options.requestId.length > 128) return null
  if (
    accepted.session.revision !== layout.request.sourceRevision ||
    accepted.session.canonicalHash !== layout.request.sourceHash ||
    accepted.session.revision !== layout.result.sourceRevision ||
    accepted.session.canonicalHash !== layout.result.sourceHash ||
    layout.request.sourceRevision !== layout.result.sourceRevision ||
    layout.request.sourceHash !== layout.result.sourceHash ||
    layout.request.requestId !== layout.result.requestId ||
    layout.request.fontCatalogIdentity !== layout.result.fontCatalogIdentity ||
    layout.request.hyphenationDataIdentity !== layout.result.hyphenationDataIdentity ||
    (layout.request.expectedLayoutSettingsFingerprint !== null &&
      layout.request.expectedLayoutSettingsFingerprint !==
        layout.result.layoutSettingsFingerprint) ||
    layout.result.resultHash.trim().length === 0 ||
    layout.result.layoutSettingsFingerprint.trim().length === 0 ||
    layout.result.fontCatalogIdentity.trim().length === 0 ||
    layout.result.pages.length === 0
  ) {
    return null
  }

  const formSelection = options.formSelection ?? null
  if (
    formSelection !== null &&
    (validateFormProjectionSelection(formSelection) !== null ||
      formSelection.sourceRevision !== accepted.session.revision ||
      formSelection.sourceHash !== accepted.session.canonicalHash ||
      formSelection.displayListHash !== formSelection.formPlan.displayListHash ||
      formSelection.formPlan.sourceRevision !== accepted.session.revision ||
      formSelection.formPlan.sourceHash !== accepted.session.canonicalHash)
  ) {
    return null
  }

  const request: PdfExportRequestDto = {
    protocolVersion: PDF_PROTOCOL_VERSION,
    requestId: options.requestId,
    sourceRevision: accepted.session.revision,
    sourceHash: accepted.session.canonicalHash,
    layoutSettingsFingerprint: layout.result.layoutSettingsFingerprint,
    layoutResultHash: layout.result.resultHash,
    serializedRequest: JSON.stringify({
      protocolVersion: PDF_PROTOCOL_VERSION,
      requestId: options.requestId,
      sourceRevision: accepted.session.revision,
      sourceHash: accepted.session.canonicalHash,
      layoutSettingsFingerprint: layout.result.layoutSettingsFingerprint,
      layoutResultHash: layout.result.resultHash,
      fontCatalogIdentity: layout.result.fontCatalogIdentity,
      fontFaces: options.fontFaces ?? [],
      hyphenationDataIdentity: layout.result.hyphenationDataIdentity,
      canonicalJson: accepted.session.canonicalJson,
      pages: layout.result.pages.map((page) => ({ bounds: page.bounds })),
      producer: options.producer,
      metadata: null,
      outlines: [],
      internalLinks: [],
      formPlan: formSelection?.formPlan ?? null,
      flattenedFieldIds: formSelection?.flattenedFieldIds ?? [],
    }),
  }
  return validatePdfExportRequest(request) === null ? request : null
}
