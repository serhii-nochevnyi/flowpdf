import type { PdfFontManifestIdentityDto } from '../pdf/pdf-protocol.js'

export const FONT_CATALOG_PROTOCOL_VERSION = 1 as const
const MAX_FONT_FACE_BYTES = 16 * 1024 * 1024

export interface FontCatalogIdentityWasmBoundary {
  readonly font_catalog_identity: (requestJson: string) => string
  readonly hyphenation_data_identity: (requestJson: string) => string
}

export interface RuntimeFontFace {
  readonly id: string
  readonly family: string
  readonly faceIndex: number
  readonly bytes: Uint8Array
  readonly manifest: PdfFontManifestIdentityDto
}

export interface RuntimeFontCatalog {
  readonly identity: string
  readonly faces: readonly RuntimeFontFace[]
  readonly hyphenation: {
    readonly identity: string
    readonly bytes: Uint8Array
  }
}

interface FontCatalogIdentityResponse {
  readonly protocolVersion: number
  readonly ok: boolean
  readonly identity: string | null
  readonly faces: readonly {
    readonly id: string
    readonly family: string
    readonly faceIndex: number
    readonly contentHash: string
  }[] | null
  readonly error: { readonly code: string } | null
}

interface HyphenationIdentityResponse {
  readonly protocolVersion: number
  readonly ok: boolean
  readonly identity: string | null
  readonly error: { readonly code: string } | null
}

/** Loads the checked-in font bytes without consulting host fonts or a network font service. */
export async function loadBundledFontCatalog(
  wasm: FontCatalogIdentityWasmBoundary,
): Promise<RuntimeFontCatalog> {
  const bytes = await loadBoundedAsset(
    new URL('./assets/NotoSans-Regular.ttf', import.meta.url),
    'FLOW_FONT_CATALOG',
  )
  const faces = [{
    id: 'noto-sans',
    family: 'Noto Sans',
    faceIndex: 0,
    bytes,
  }]
  let parsed: unknown
  try {
    parsed = JSON.parse(wasm.font_catalog_identity(JSON.stringify({
      protocolVersion: FONT_CATALOG_PROTOCOL_VERSION,
      fonts: faces.map((face) => ({
        id: face.id,
        family: face.family,
        faceIndex: face.faceIndex,
        bytes: Array.from(face.bytes),
      })),
    }))) as unknown
  } catch {
    throw new FontCatalogError('FLOW_FONT_CATALOG_RESPONSE_DECODE')
  }
  if (!isIdentityResponse(parsed)) {
    throw new FontCatalogError('FLOW_FONT_CATALOG_RESPONSE_DECODE')
  }
  if (!parsed.ok || parsed.identity === null || parsed.faces === null) {
    throw new FontCatalogError(parsed.error?.code ?? 'FLOW_FONT_CATALOG_UNAVAILABLE')
  }
  const runtimeFaces = faces.map((face, index) => {
    const identity = parsed.faces?.[index]
    if (
      identity === undefined ||
      identity.id !== face.id ||
      identity.family !== face.family ||
      identity.faceIndex !== face.faceIndex ||
      identity.contentHash.trim().length === 0
    ) {
      throw new FontCatalogError('FLOW_FONT_CATALOG_IDENTITY_MISMATCH')
    }
    return {
      ...face,
      manifest: { faceId: identity.id, contentHash: identity.contentHash },
    }
  })
  const hyphenationBytes = await loadBoundedAsset(
    new URL('./assets/uk.standard.bincode', import.meta.url),
    'FLOW_HYPHENATION',
  )
  let hyphenationParsed: unknown
  try {
    hyphenationParsed = JSON.parse(wasm.hyphenation_data_identity(JSON.stringify({
      protocolVersion: FONT_CATALOG_PROTOCOL_VERSION,
      bytes: Array.from(hyphenationBytes),
    }))) as unknown
  } catch {
    throw new FontCatalogError('FLOW_HYPHENATION_RESPONSE_DECODE')
  }
  if (!isHyphenationResponse(hyphenationParsed)) {
    throw new FontCatalogError('FLOW_HYPHENATION_RESPONSE_DECODE')
  }
  if (!hyphenationParsed.ok || hyphenationParsed.identity === null) {
    throw new FontCatalogError(
      hyphenationParsed.error?.code ?? 'FLOW_HYPHENATION_UNAVAILABLE',
    )
  }
  return {
    identity: parsed.identity,
    faces: runtimeFaces,
    hyphenation: { identity: hyphenationParsed.identity, bytes: hyphenationBytes },
  }
}

export class FontCatalogError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'FontCatalogError'
  }
}

function isIdentityResponse(value: unknown): value is FontCatalogIdentityResponse {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return false
  const candidate = value as Partial<FontCatalogIdentityResponse>
  return (
    candidate.protocolVersion === FONT_CATALOG_PROTOCOL_VERSION &&
    typeof candidate.ok === 'boolean' &&
    (candidate.identity === null || typeof candidate.identity === 'string') &&
    (candidate.faces === null || Array.isArray(candidate.faces)) &&
    (candidate.error === null ||
      (candidate.error !== undefined &&
        typeof candidate.error === 'object' &&
        typeof candidate.error.code === 'string'))
  )
}

function isHyphenationResponse(value: unknown): value is HyphenationIdentityResponse {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return false
  const candidate = value as Partial<HyphenationIdentityResponse>
  return (
    candidate.protocolVersion === FONT_CATALOG_PROTOCOL_VERSION &&
    typeof candidate.ok === 'boolean' &&
    (candidate.identity === null || typeof candidate.identity === 'string') &&
    (candidate.error === null ||
      (candidate.error !== undefined &&
        typeof candidate.error === 'object' &&
        typeof candidate.error.code === 'string'))
  )
}

async function loadBoundedAsset(url: URL, prefix: string): Promise<Uint8Array> {
  const response = await fetch(url)
  if (!response.ok) throw new FontCatalogError(`${prefix}_ASSET_UNAVAILABLE`)
  const declaredLength = response.headers.get('content-length')
  if (declaredLength !== null && Number(declaredLength) > MAX_FONT_FACE_BYTES) {
    throw new FontCatalogError(`${prefix}_ASSET_LIMIT`)
  }
  const bytes = new Uint8Array(await response.arrayBuffer())
  if (bytes.length === 0 || bytes.length > MAX_FONT_FACE_BYTES) {
    throw new FontCatalogError(`${prefix}_ASSET_LIMIT`)
  }
  return bytes
}
