import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'

import {
  validateLinuxReleaseBindgen,
  validateWasmBindgenInstallation,
  verifyInstalledTool,
  verifyWasmBindgenTool,
} from './verify-wasm-bindgen-tool.mjs'

const target = 'test-target'
const trustedBinary = Buffer.from('trusted wasm-bindgen fixture')
const binarySha256 = createHash('sha256').update(trustedBinary).digest('hex')
const approved = {
  name: 'wasm-bindgen-cli',
  version: '0.2.108',
  kind: 'tool',
  binarySha256: {
    [target]: binarySha256,
  },
}
const linuxApproved = {
  ...approved,
  linuxRelease: {
    assetSha256: 'a'.repeat(64),
    binarySha256,
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

test('the filesystem verifier reaches the platform checksum comparison', async (t) => {
  const root = await mkdtemp(join(tmpdir(), 'flowpdf-wasm-tool-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const cargoHome = join(root, 'work/toolchains/cargo')
  const binary = join(cargoHome, 'bin/wasm-bindgen')
  await Promise.all([
    mkdir(join(root, 'config'), { recursive: true }),
    mkdir(join(root, 'artifacts/provenance'), { recursive: true }),
    mkdir(join(cargoHome, 'bin'), { recursive: true }),
  ])
  await Promise.all([
    writeFile(join(root, 'config/dependency-provenance.json'), JSON.stringify({
      crates: [linuxApproved],
    })),
    writeFile(join(root, 'artifacts/provenance/phase1-dependencies.json'), JSON.stringify(report)),
    writeFile(join(cargoHome, '.crates2.json'), JSON.stringify(receipt)),
    writeFile(binary, 'synthetic mismatched bytes', { mode: 0o600 }),
  ])

  const message = process.platform === 'linux' && process.arch === 'x64'
    ? 'wasm-bindgen Linux release binary checksum mismatch'
    : 'wasm-bindgen binary checksum mismatch'
  assert.throws(() => verifyInstalledTool(root), { message })
})

test('accepts the pinned Linux release and rejects a changed executable', () => {
  assert.doesNotThrow(() => validateLinuxReleaseBindgen({ approved: linuxApproved, report, binaryBytes: trustedBinary }))
  assert.throws(() => validateLinuxReleaseBindgen({ approved: linuxApproved, report, binaryBytes: Buffer.from('changed') }),
    /Linux release binary checksum mismatch/)
  assert.throws(() => validateLinuxReleaseBindgen({ approved, report, binaryBytes: trustedBinary }),
    /Linux release checksums are not approved/)
})
