export const supportedNodeVersion = '24.10.0'

export function assertSupportedNodeVersion(actual = process.versions.node) {
  if (actual !== supportedNodeVersion) {
    throw new Error(
      `FLOW_UNSUPPORTED_NODE_VERSION: expected ${supportedNodeVersion}, received ${actual}`,
    )
  }
}
