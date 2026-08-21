import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import test from 'node:test'

import { validateWasmBindgenInstallation } from './verify-wasm-bindgen-tool.mjs'

const target = 'test-target'
const trustedBinary = Buffer.from('trusted wasm-bindgen fixture')
const approved = {
  name: 'wasm-bindgen-cli',
  version: '0.2.108',
  kind: 'tool',
  binarySha256: {
    [target]: createHash('sha256').update(trustedBinary).digest('hex'),
  },
}
const report = {
  status: 'success',
  crates: [{ name: 'wasm-bindgen-cli', version: '0.2.108', kind: 'tool' }],
}
const receipt = {
  installs: {
    'wasm-bindgen-cli 0.2.108 (registry+https://github.com/rust-lang/crates.io-index)': {
      version_req: '=0.2.108',
      bins: ['wasm-bindgen'],
      target,
    },
  },
}

test('accepts the approved version, receipt, and binary checksum', () => {
  assert.doesNotThrow(() => validateWasmBindgenInstallation({
    approved,
    report,
    receipt,
    versionOutput: 'wasm-bindgen 0.2.108\n',
    binaryBytes: trustedBinary,
  }))
})

test('rejects wrong-version and replaced wasm-bindgen binaries', () => {
  assert.throws(
    () => validateWasmBindgenInstallation({
      approved,
      report,
      receipt,
      versionOutput: 'wasm-bindgen 0.2.107\n',
      binaryBytes: trustedBinary,
    }),
    /version mismatch/,
  )
  assert.throws(
    () => validateWasmBindgenInstallation({
      approved,
      report,
      receipt,
      versionOutput: 'wasm-bindgen 0.2.108\n',
      binaryBytes: Buffer.from('replaced binary with spoofed version output'),
    }),
    /checksum mismatch/,
  )
})
