import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { access, chmod, mkdir, mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'

import {
  validateWasmBindgenInstallation,
  verifyInstalledTool,
  verifyWasmBindgenTool,
} from './verify-wasm-bindgen-tool.mjs'

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

test('never executes a wasm-bindgen binary before its receipt and checksum are trusted', () => {
  let executions = 0
  assert.throws(
    () => verifyWasmBindgenTool({
      approved,
      report,
      receipt,
      binaryBytes: Buffer.from('untrusted executable bytes'),
      executeVersion: () => {
        executions += 1
        throw new Error('untrusted binary was executed')
      },
    }),
    /checksum mismatch/,
  )
  assert.equal(executions, 0)

  assert.doesNotThrow(() => verifyWasmBindgenTool({
    approved,
    report,
    receipt,
    binaryBytes: trustedBinary,
    executeVersion: () => {
      executions += 1
      return 'wasm-bindgen 0.2.108\n'
    },
  }))
  assert.equal(executions, 1)
})

test('a checksum-mismatched sentinel executable is never invoked by the filesystem verifier', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowpdf-wasm-tool-'))
  const cargoHome = join(root, 'work/toolchains/cargo')
  const binary = join(cargoHome, 'bin/wasm-bindgen')
  const marker = join(root, 'sentinel-was-invoked')
  await Promise.all([
    mkdir(join(root, 'config'), { recursive: true }),
    mkdir(join(root, 'artifacts/provenance'), { recursive: true }),
    mkdir(join(cargoHome, 'bin'), { recursive: true }),
  ])
  await Promise.all([
    writeFile(join(root, 'config/dependency-provenance.json'), JSON.stringify({
      crates: [approved],
    })),
    writeFile(join(root, 'artifacts/provenance/phase1-dependencies.json'), JSON.stringify(report)),
    writeFile(join(cargoHome, '.crates2.json'), JSON.stringify(receipt)),
    writeFile(binary, `#!/bin/sh\ntouch "${marker}"\nprintf 'wasm-bindgen 0.2.108\\n'\n`),
  ])
  await chmod(binary, 0o700)

  assert.throws(() => verifyInstalledTool(root), /checksum mismatch/)
  await assert.rejects(() => access(marker))
})
