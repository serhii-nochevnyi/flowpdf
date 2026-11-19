import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import {
  compareRaster, compareWidgetRegion, decodePpm, externalExitCode, matchesWidgetBounds, nativeSession,
  operationOutput, probeVersion,
} from '../../scripts/verify-pdf-native-external.mjs'

const fixture = (name) => readFileSync(new URL(`../../fixtures/pdf-native/${name}`, import.meta.url))
const editFamilies = JSON.parse(fixture('edit-families.expected.json'))

test('strict external rejects unavailable or failed checks', () => {
  assert.equal(externalExitCode('unavailable', true), 2)
  assert.equal(externalExitCode('fail', true), 1)
  assert.equal(externalExitCode('unavailable', false), 0)
  assert.equal(probeVersion('qpdf', () => ({ error: { code: 'ENOENT' } })), null)
  assert.equal(probeVersion('pdftoppm', () => ({ status: 0, stderr: 'private payload' })), null)
  assert.equal(probeVersion('pdftotext', () => ({ status: 0, stderr: 'pdftotext version 25.04.0\nother text' })), '25.04.0')
})

test('raster parsing and comparison cover the entire page including controls', () => {
  const white = [255, 255, 255]
  const source = Buffer.from([...white, ...white])
  const page = { widthPx: 2, heightPx: 1, cropBoxPt: [0, 0, 2, 1], rotation: 0,
    raster: { comparison: 'source-with-patches', patches: [{ rectPt: [0, 0, 1, 1], rgb: [0, 0, 0] }] } }
  const policy = { dpi: 72, flatChannelTolerance: 0, edgeBandPx: 0, edgeChannelTolerance: 0 }
  const expected = Buffer.from([0, 0, 0, ...white])
  assert.deepEqual(decodePpm(Buffer.concat([Buffer.from('P6\n2 1\n255\n'), expected]), 2, 1), expected)
  assert.throws(() => decodePpm(Buffer.from('P6\n2 1\n255\n'), 2, 1))
  assert.equal(compareRaster(page, policy, expected, source), true)
  assert.equal(compareRaster(page, policy, Buffer.alloc(6), source), false)
  assert.equal(compareRaster(page, policy, source, source), false)
  assert.equal(compareRaster({ ...page, rotation: 90 }, policy, expected, source), false)
})

test('the declared rotated blank page requires exact dimensions, digest and pixels', () => {
  const page = editFamilies.operations.find(({ operation }) => operation === 'insertBlankPage')
    .pages.find(({ sourcePage }) => sourcePage === null)
  const { widthPx, heightPx, raster } = page
  assert.equal(page.rotation, 90)
  assert.deepEqual(raster.fills, [])
  const white = Buffer.alloc(widthPx * heightPx * 3, 255)
  const ppm = Buffer.concat([Buffer.from(`P6\n${widthPx} ${heightPx}\n255\n`), white])
  assert.throws(() => decodePpm(ppm, heightPx, widthPx), /raster-format/)
  const pixels = decodePpm(ppm, widthPx, heightPx)
  assert.equal(compareRaster(page, editFamilies.raster, pixels), true)
  assert.equal(compareRaster({ ...page, raster: { ...raster, sha256Rgb8: '0'.repeat(64) } },
    editFamilies.raster, pixels), false)
  assert.equal(compareRaster({ ...page, raster: { ...raster,
    fills: [{ rectPt: page.cropBoxPt, rgb: [255, 255, 255] }] } }, editFamilies.raster, pixels), false)
  pixels[pixels.length - 1] = 254
  assert.equal(compareRaster(page, editFamilies.raster, pixels), false)
})

test('a refused edit confirms the live state against the base after setup', () => {
  const input = fixture('edit-families.pdf')
  const baseBytes = fixture('notes-preservation.pdf')
  const operation = editFamilies.operations.find(({ operation }) => operation === 'deleteFinalPage')

  for (const change of [{}, { binding: { revision: 3 } }, { outputHash: [3] }, { outputHex: input.toString('hex') }]) {
    let commands = 0
    let liveChecks = 0
    let base
    const respond = (handler) => (json) => {
      const { operation, requestId, originStoreHash, binding, ...payload } = JSON.parse(json)
      return JSON.stringify({ operation, requestId, binding, ...handler(originStoreHash, binding, payload, requestId) })
    }
    const snapshot = (originStoreHash, revision, bytes) => ({
      originStoreHash, binding: { revision }, outputHash: [revision], outputHex: bytes.toString('hex'),
      scene: {}, pageState: { order: [1, 2, 3].slice(revision) },
    })
    const wasm = {
      native_pdf_open: respond((originStoreHash) => ({ status: 'opened', snapshot: snapshot(originStoreHash, 0, input) })),
      native_pdf_command: respond((originStoreHash, binding, { command, outputHash }) => {
        assert.equal(binding.revision, commands)
        assert.deepEqual(outputHash, [commands])
        assert.equal(command.kind, 'deletePage')
        commands++
        if (commands < 3) {
          base = snapshot(originStoreHash, commands, baseBytes)
          return { status: 'applied', snapshot: base }
        }
        return { status: 'failed', code: operation.outcome, snapshot: null }
      }),
      native_pdf_snapshot: respond((originStoreHash, binding, _payload, requestId) => {
        assert.equal(commands, 3)
        assert.equal(requestId, 4)
        assert.equal(originStoreHash, base.originStoreHash)
        assert.deepEqual(binding, base.binding)
        return { status: 'unseen', snapshot: { ...base, ...change } }
      }),
      verify_native_pdf_response: (json) => JSON.parse(json).operation !== 'snapshot',
      verify_native_pdf_worker_response: (json) => {
        liveChecks++
        return JSON.parse(json).operation === 'snapshot'
      },
    }
    const session = nativeSession(wasm, input)
    const run = () => operationOutput(session, { id: 'edit-families', input }, operation)
    if (Object.keys(change).length) assert.throws(run, /native-refusal-readback/)
    else assert.deepEqual(run(), baseBytes)
    assert.equal(liveChecks, 1)
  }
})

test('a refused open retains the source without requesting a session snapshot', () => {
  const input = fixture('refused.pdf')
  const outcome = 'FLOW_PDF_NATIVE_UNSUPPORTED'
  const session = nativeSession({
    native_pdf_open: (json) => {
      const { operation, requestId } = JSON.parse(json)
      return JSON.stringify({ operation, requestId, status: 'failed', code: outcome, snapshot: null })
    },
    verify_native_pdf_response: () => true,
  }, input)
  assert.equal(operationOutput(session, { id: 'refused', input }, { outcome }), input)
})


test('a glyph patch needs a reference raster and still preserves outside pixels', () => {
  const source = Buffer.alloc(12, 255)
  const reference = Buffer.from([0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255])
  const page = { widthPx: 4, heightPx: 1, cropBoxPt: [0, 0, 4, 1], rotation: 0,
    raster: { comparison: 'source-with-patches',
      patches: [{ rectPt: [0, 0, 2, 1], rgb: [255, 255, 255], glyphRun: { cids: [3, 1] } }] } }
  const policy = { dpi: 72, flatChannelTolerance: 0, edgeBandPx: 1, edgeChannelTolerance: 64 }
  assert.equal(compareRaster(page, policy, reference, source), false)
  const softened = Buffer.from(reference)
  softened.fill(48, 0, 3)
  assert.equal(compareRaster(page, policy, softened, source, reference), true)
  softened[6] = 254
  assert.equal(compareRaster(page, policy, softened, source, reference), false)
})

test('widget comparison checks every pixel within the crop-relative rectangle', () => {
  const page = { widthPx: 4, heightPx: 4, cropBoxPt: [10, 20, 12, 22], rotation: 0 }
  const rect = [10.5, 20.5, 11.5, 21.5]
  const expected = Buffer.alloc(48, 255)
  const actual = Buffer.from(expected)
  actual[0] = 0
  assert.equal(compareWidgetRegion(page, 144, rect, actual, expected), true)
  for (const pixel of [5, 6, 9, 10]) {
    actual[pixel * 3 + 2] = 0
    assert.equal(compareWidgetRegion(page, 144, rect, actual, expected), false)
    actual[pixel * 3 + 2] = 255
  }
  assert.equal(compareWidgetRegion(page, 144, [9.5, 20.5, 11.5, 21.5], actual, expected), false)
  assert.equal(compareWidgetRegion(page, 144, rect, actual.subarray(1), expected), false)
})

test('widget Forms need a finite full-size local box and an identity matrix', () => {
  const rect = [16, 12, 64, 32]
  assert.equal(matchesWidgetBounds(rect, [0, 0, 48, 20]), true)
  for (const box of [[0, 0, 48], [0, 0, 48, 19], [0, 0, 48, 20, 0], [1, 0, 49, 20], [0, 0, 48, Infinity]]) {
    assert.equal(matchesWidgetBounds(rect, box), false)
  }
  assert.equal(matchesWidgetBounds(rect, [0, 0, 48, 20], [1, 0, 0, 1, 1, 0]), false)
  assert.equal(matchesWidgetBounds([64, 12, 16, 32], [0, 0, -48, 20]), false)
})
