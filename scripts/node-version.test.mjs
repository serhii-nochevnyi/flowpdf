import assert from 'node:assert/strict'
import test from 'node:test'

import {
  assertSupportedNodeVersion,
  supportedNodeVersion,
} from './node-version.mjs'

test('accepts only the pinned Node runtime', () => {
  assert.doesNotThrow(() => assertSupportedNodeVersion(supportedNodeVersion))
  assert.throws(
    () => assertSupportedNodeVersion('20.11.0'),
    /FLOW_UNSUPPORTED_NODE_VERSION: expected 24\.10\.0, received 20\.11\.0/,
  )
})
