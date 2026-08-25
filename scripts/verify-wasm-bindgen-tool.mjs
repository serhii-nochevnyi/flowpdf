#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { assertSupportedNodeVersion } from './node-version.mjs'

assertSupportedNodeVersion()

export function validateWasmBindgenIntegrity({
  approved,
  report,
  receipt,
  binaryBytes,
}) {
  if (approved?.name !== 'wasm-bindgen-cli' || approved.kind !== 'tool') {
    throw new Error('wasm-bindgen-cli approval is missing or not a tool')
  }
  if (report?.status !== 'success') {
    throw new Error('dependency provenance report is not successful')
  }
  const reported = report.crates?.find((entry) => entry.name === approved.name)
  if (reported?.version !== approved.version || reported.kind !== 'tool') {
    throw new Error('wasm-bindgen-cli report identity differs from approval')
  }
  const receiptKey = `wasm-bindgen-cli ${approved.version} (registry+https://github.com/rust-lang/crates.io-index)`
  const install = receipt?.installs?.[receiptKey]
  if (install?.version_req !== `=${approved.version}` || !install.bins?.includes('wasm-bindgen')) {
    throw new Error('wasm-bindgen-cli installation receipt is missing or mismatched')
  }
  const approvedChecksum = approved.binarySha256?.[install.target]
  if (!/^[a-f0-9]{64}$/.test(approvedChecksum ?? '')) {
    throw new Error(`wasm-bindgen binary checksum is not approved for ${install.target ?? 'unknown target'}`)
  }
  const actualChecksum = createHash('sha256').update(binaryBytes).digest('hex')
  if (actualChecksum !== approvedChecksum) {
    throw new Error('wasm-bindgen binary checksum mismatch')
  }
}

export function validateWasmBindgenVersion(approved, versionOutput) {
  if (versionOutput.trim() !== `wasm-bindgen ${approved.version}`) {
    throw new Error(`wasm-bindgen version mismatch: ${versionOutput.trim()}`)
  }
}

export function validateWasmBindgenInstallation(input) {
  validateWasmBindgenIntegrity(input)
  validateWasmBindgenVersion(input.approved, input.versionOutput)
}

export function verifyWasmBindgenTool({ approved, report, receipt, binaryBytes, executeVersion }) {
  validateWasmBindgenIntegrity({ approved, report, receipt, binaryBytes })
  const versionOutput = executeVersion()
  validateWasmBindgenVersion(approved, versionOutput)
}

export function verifyInstalledTool(root) {
  const config = JSON.parse(readFileSync(resolve(root, 'config/dependency-provenance.json'), 'utf8'))
  const report = JSON.parse(readFileSync(resolve(root, 'artifacts/provenance/phase1-dependencies.json'), 'utf8'))
  const cargoHome = resolve(root, 'work/toolchains/cargo')
  const binary = resolve(cargoHome, 'bin/wasm-bindgen')
  const approved = config.crates?.find((entry) => entry.name === 'wasm-bindgen-cli')
  const binaryBytes = readFileSync(binary)
  verifyWasmBindgenTool({
    approved,
    report,
    receipt: JSON.parse(readFileSync(resolve(cargoHome, '.crates2.json'), 'utf8')),
    binaryBytes,
    executeVersion: () => executeTrustedCopy(binaryBytes),
  })
}

function executeTrustedCopy(binaryBytes) {
  const directory = mkdtempSync(join(tmpdir(), 'flowpdf-wasm-bindgen-'))
  const trustedBinary = resolve(directory, 'wasm-bindgen')
  try {
    writeFileSync(trustedBinary, binaryBytes, { mode: 0o700 })
    chmodSync(trustedBinary, 0o700)
    return execFileSync(trustedBinary, ['--version'], { encoding: 'utf8' })
  } finally {
    rmSync(directory, { force: true, recursive: true })
  }
}

if (process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const root = resolve(import.meta.dirname, '..')
  try {
    verifyInstalledTool(root)
    process.stdout.write('verified wasm-bindgen-cli installation\n')
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 1
  }
}
