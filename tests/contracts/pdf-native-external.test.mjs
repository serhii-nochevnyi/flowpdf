import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  compareRaster, compareWidgetRegion, decodePpm, externalExitCode, matchesWidgetBounds, probeVersion,
} from '../../scripts/verify-pdf-native-external.mjs'

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
