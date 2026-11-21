import { createHash } from 'node:crypto'
import { accessSync, constants, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { delimiter, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { isDeepStrictEqual } from 'node:util'
import { pinnedEnvironment, root, runBounded } from './check-phase9.mjs'
import { supportedNodeVersion } from './node-version.mjs'

const corpus = 'fixtures/pdf-native'
const manifestHash = '4a752fdeef4bacb2a336b7dcb3a5c258916bba4bda0c1a8e91d49e7419a20fdc'
const byteLimit = 4 * 1024 * 1024
const wasmLimit = 32 * 1024 * 1024
const toolNames = ['qpdf', 'pdftotext', 'pdftoppm']
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex')
const milli = (points) => points.map((point) => Math.round(point * 1000))
const same = isDeepStrictEqual

class GateError extends Error {
  constructor(status, code) {
    super(code)
    this.status = status
  }
}

function requireInput(condition, code) {
  if (!condition) throw new GateError('unavailable', code)
}

function requireMatch(condition, code) {
  if (!condition) throw new GateError('fail', code)
}

function withCleanup(run, cleanup) {
  let failed = false
  try {
    return run()
  } catch (error) {
    failed = true
    throw error
  } finally {
    try {
      cleanup()
    } catch (error) {
      if (!failed) throw error
    }
  }
}

function boundedRead(path, limit = byteLimit) {
  try {
    const size = statSync(path).size
    requireInput(size > 0 && size <= limit, 'input-size')
    return readFileSync(path)
  } catch (error) {
    if (error instanceof GateError) throw error
    throw new GateError('unavailable', 'input-file')
  }
}

export function loadFixtures() {
  const raw = boundedRead(resolve(root, corpus, 'manifest.json'))
  requireInput(digest(raw) === manifestHash, 'corpus-manifest')
  return JSON.parse(raw).fixtures.map(({ id, source, expected }) => {
    const input = boundedRead(resolve(root, source.path))
    const expectations = expected.inline ? Buffer.from(JSON.stringify(expected.inline)) :
      boundedRead(resolve(root, expected.path))
    requireInput(digest(input) === source.sha256 && digest(expectations) === expected.sha256, 'corpus-hash')
    const spec = JSON.parse(expectations)
    requireInput(spec.fixtureId === id && [1, 2].includes(spec.schemaVersion) &&
      spec.raster.version === spec.schemaVersion && spec.raster.dpi === 144 &&
      spec.raster.annotationLayer === 'excluded' && spec.operations.length > 0, 'expectation-profile')
    return { id, input, spec, sourceHash: source.sha256, expectationHash: expected.sha256 }
  })
}

function resolveExecutable(tool, cwd, path) {
  for (const entry of path?.split(delimiter) ?? []) {
    try {
      const executable = realpathSync(resolve(cwd, entry, tool))
      if (!statSync(executable).isFile()) continue
      accessSync(executable, constants.X_OK)
      return executable
    } catch {
      // @contract: Continue PATH lookup past missing or inaccessible candidates.
    }
  }
  return null
}

export function probeTool(tool, run = runBounded, { cwd = root, env = pinnedEnvironment(process.env, cwd) } = {}) {
  if (!toolNames.includes(tool)) return null
  const executable = resolveExecutable(tool, cwd, env.PATH)
  if (!executable) return null
  const result = run([executable, tool === 'qpdf' ? '--version' : '-v'], { cwd, env })
  if (result.status !== 0 || result.error) return null
  const pattern = new RegExp(`^${tool} version (\\d{1,4}\\.\\d{1,4}\\.\\d{1,4})(?:\\r?\\n|$)`, 'm')
  const version = pattern.exec(`${result.stdout ?? ''}\n${result.stderr ?? ''}`)?.[1]
  return version ? { executable, version } : null
}

export function externalExitCode(status, strict) {
  if (status === 'pass' || (status === 'unavailable' && !strict)) return 0
  return status === 'unavailable' ? 2 : 1
}

export function decodePpm(bytes, width, height) {
  requireMatch(Number.isSafeInteger(width) && width > 0 && Number.isSafeInteger(height) && height > 0 &&
    width * height * 3 <= byteLimit, 'raster-size')
  const header = /^P6\s+(\d+)\s+(\d+)\s+255\r?\n/.exec(bytes.subarray(0, 128).toString('latin1'))
  requireMatch(header && +header[1] === width && +header[2] === height &&
    bytes.length === header[0].length + width * height * 3, 'raster-format')
  return bytes.subarray(header[0].length)
}

function paintBounds({ rectPt }, page, scale) {
  const [left, , , top] = page.cropBoxPt
  const [x0, y0, x1, y1] = rectPt
  return [(x0 - left) * scale, (top - y1) * scale, (x1 - left) * scale, (top - y0) * scale]
}

export function compareWidgetRegion(page, dpi, rectPt, actual, expected) {
  const { widthPx: width, heightPx: height } = page
  const [x0, y0, x1, y1] = paintBounds({ rectPt }, page, dpi / 72)
  const size = width * height * 3
  if (page.rotation !== 0 || ![x0, y0, x1, y1].every(Number.isSafeInteger) ||
    x0 < 0 || y0 < 0 || x0 >= x1 || y0 >= y1 || x1 > width || y1 > height ||
    actual.length !== size || expected.length !== size) return false
  for (let y = y0; y < y1; y++) {
    const start = (y * width + x0) * 3
    const end = (y * width + x1) * 3
    if (!actual.subarray(start, end).equals(expected.subarray(start, end))) return false
  }
  return true
}

export function matchesWidgetBounds(rect, box, matrix = [1, 0, 0, 1, 0, 0]) {
  if (![rect, box].every((bounds) =>
    Array.isArray(bounds) && bounds.length === 4 && Array.from(bounds).every(Number.isFinite))) return false
  const [left, bottom, right, top] = rect
  return right > left && top > bottom && same(box, [0, 0, right - left, top - bottom]) &&
    same(matrix, [1, 0, 0, 1, 0, 0])
}

export function compareCheckboxAppearances(page, dpi, rectPt, actual, expected) {
  return compareWidgetRegion(page, dpi, rectPt, actual.saved, expected.saved) &&
    compareWidgetRegion(page, dpi, rectPt, actual.alternate, expected.alternate) &&
    !compareWidgetRegion(page, dpi, rectPt, actual.saved, actual.alternate)
}

// @contract: Edit-family sources and native rewrites use one dense, generation-zero classic xref table.
function classicLayout(bytes) {
  if (!Buffer.isBuffer(bytes) || bytes.length > byteLimit) return null
  const text = bytes.toString('latin1')
  const header = '%PDF-1.7\n%\xE2\xE3\xCF\xD3\n'
  const footer = /\nstartxref\n(\d{1,7})\n%%EOF\n$/.exec(text)
  if (!text.startsWith(header) || !footer) return null
  const xref = Number(footer[1])
  const table = /^xref\n0 (\d+)\n0000000000 65535 f \n((?:\d{10} 00000 n \n)+)trailer\n(<<[^\n]*>>)\n$/
    .exec(text.slice(xref, footer.index + 1))
  if (!table) return null
  const offsets = table[2].trimEnd().split('\n').map((entry) => Number(entry.slice(0, 10)))
  if (Number(table[1]) !== offsets.length + 1 || offsets[0] !== header.length ||
      !offsets.every((offset, index) => offset < xref && (index === 0 || offset > offsets[index - 1]))) return null
  return {
    size: offsets.length + 1,
    trailer: table[3],
    object(number) {
      const offset = offsets[number - 1]
      if (offset === undefined) return null
      const prefix = `${number} 0 obj\n`
      const end = offsets[number] ?? xref
      const object = text.slice(offset, end)
      if (!object.startsWith(prefix) || !object.endsWith('\nendobj\n')) return null
      return { start: offset + prefix.length, body: object.slice(prefix.length, -'\nendobj\n'.length) }
    },
  }
}

function hasSingleRevision(bytes, { trailer, objects, resolveObject }) {
  const layout = classicLayout(bytes)
  if (!layout || Object.hasOwn(trailer, '/Prev') || Object.hasOwn(trailer, '/XRefStm') ||
      layout.size !== trailer['/Size'] || layout.size !== Object.keys(objects).length ||
      layout.trailer !== `<< /Size ${layout.size} /Root ${trailer['/Root']} >>`) return false
  for (let number = 1; number < layout.size; number++) {
    const object = layout.object(number)
    const inspected = objects[`obj:${number} 0 R`]
    if (!object || !inspected) return false
    if (inspected.stream) {
      const header = /^(<<[^\n]*>>)\nstream\n/.exec(object.body)
      const length = header && / \/Length (\d+)( 0 R)?(?= )/.exec(header[1])
      if (!length) return false
      const size = length[2] ? resolveObject(`${length[1]} 0 R`) : Number(length[1])
      if (!Number.isSafeInteger(size) || size < 0 ||
          object.body.slice(header[0].length + size) !== '\nendstream') return false
    } else if (object.body.includes('\n')) {
      return false
    }
  }
  return true
}

const alternateState = (state) => state === 'Off' ? 'Yes' : 'Off'

export function alternateCheckboxPdf(saved, checkboxes) {
  const layout = classicLayout(saved)
  requireMatch(layout, 'checkbox-source-xref')
  const alternate = Buffer.from(saved)
  for (const { ref, state } of checkboxes) {
    const number = /^([1-9]\d*) 0 R$/.exec(ref)?.[1]
    const object = number && layout.object(Number(number))
    requireMatch(object && !object.body.includes('\n') && ['Off', 'Yes'].includes(state), 'checkbox-source-widget')
    for (const key of ['AS', 'V']) {
      const [selector, duplicate] = object.body.matchAll(new RegExp(` /${key} /${state}(?= )`, 'g'))
      requireMatch(selector && !duplicate, 'checkbox-source-state')
      const start = object.start + selector.index + selector[0].length - state.length
      alternate.write(alternateState(state), start, state.length, 'ascii')
    }
  }
  return alternate
}

function widgetReference(input, page, fields, policy) {
  const trailer = /trailer\s+<<\s*\/Size 28\b[^>]*>>\s+startxref\s+(\d+)\s+%%EOF\s*$/
    .exec(input.subarray(-128).toString('latin1'))
  requireInput(trailer && policy?.version === 1 && policy.annotationLayer === 'included', 'widget-reference')
  const objects = [
    '<< /Type /Catalog /Pages 29 0 R /AcroForm 31 0 R >>',
    '<< /Type /Pages /Kids [30 0 R] /Count 1 >>',
    null,
    null,
  ]
  const widgets = fields.map(({ rectPt, value, appearanceState }) => {
    const [left, bottom, right, top] = rectPt
    const width = right - left
    const height = top - bottom
    const { backgroundGray, inkGray, borderInset: inset, borderWidth } = policy
    const frame = `q ${backgroundGray} g 0 0 ${width} ${height} re f ${inkGray} G ${borderWidth} w ` +
      `${inset} ${inset} ${width - 2 * inset} ${height - 2 * inset} re S\n`
    const stream = (ink, resources = '<<>>') => {
      const content = frame + ink + 'Q\n'
      objects.push(`<< /Type /XObject /Subtype /Form /BBox [0 0 ${width} ${height}] ` +
        `/Resources ${resources} /Length ${Buffer.byteLength(content)} >>\nstream\n${content}endstream`)
      return `${objects.length + 27} 0 R`
    }
    let field
    if (appearanceState) {
      const { inset: mark, width: stroke } = policy.checkmark
      const off = stream('')
      const yes = stream(`${stroke} w ${mark} ${mark} m ${width - mark} ${height - mark} l ` +
        `${mark} ${height - mark} m ${width - mark} ${mark} l S\n`)
      field = `/FT /Btn /V /${appearanceState} /AS /${appearanceState} /AP << /N << /Off ${off} /Yes ${yes} >> >>`
    } else {
      const { offsetPt, sizePt, hexByValue, fixtureFontObject } = policy.text
      requireInput(Object.hasOwn(hexByValue, value), 'widget-reference-text')
      const normal = stream(`BT /F1 ${sizePt} Tf ${inkGray} g ${offsetPt.join(' ')} Td <${hexByValue[value]}> Tj ET\n`,
        `<< /Font << /F1 ${fixtureFontObject} 0 R >> >>`)
      const unicode = Buffer.from(value, 'utf16le').swap16().toString('hex')
      field = `/FT /Tx /V <FEFF${unicode}> /DA (/F1 ${sizePt} Tf ${inkGray} g) /AP << /N ${normal} >>`
    }
    objects.push(`<< /Type /Annot /Subtype /Widget /P 30 0 R /Rect [${rectPt.join(' ')}] ` +
      `/T (Reference${objects.length}) ${field} >>`)
    return `${objects.length + 27} 0 R`
  })
  objects[2] = `<< /Type /Page /Parent 29 0 R /MediaBox [${page.cropBoxPt.join(' ')}] ` +
    `/Resources <<>> /Annots [${widgets.join(' ')}] >>`
  objects[3] = `<< /Fields [${widgets.join(' ')}] /NeedAppearances false ` +
    `/DR << /Font << /F1 ${policy.text.fixtureFontObject} 0 R >> >> /DA (/F1 ${policy.text.sizePt} Tf 0 g) >>`
  let revision = '\n'
  const offsets = objects.map((object, index) => {
    const offset = input.length + revision.length
    revision += `${28 + index} 0 obj\n${object}\nendobj\n`
    return `${String(offset).padStart(10, '0')} 00000 n \n`
  })
  const xref = input.length + revision.length
  revision += `xref\n28 ${objects.length}\n${offsets.join('')}trailer\n` +
    `<< /Size ${28 + objects.length} /Root 28 0 R /Prev ${trailer[1]} >>\nstartxref\n${xref}\n%%EOF\n`
  return Buffer.concat([input, Buffer.from(revision, 'ascii')])
}

export function compareRaster(page, policy, actual, source, glyphReference) {
  const { widthPx: width, heightPx: height, rotation } = page
  const rule = page.raster ?? page
  const size = width * height * 3
  if (![width, height].every((dimension) => Number.isSafeInteger(dimension) && dimension > 0) ||
    size > byteLimit || actual.length !== size) return false
  if (rule.comparison === 'equal-to-source' || rule.comparison === 'source-only') return actual.equals(source)
  if (rule.comparison === 'source-transformed') {
    if (source?.length !== size) return false
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        const index = (y * width + x) * 3
        const previous = ((width - 1 - x) * height + y) * 3
        if (!actual.subarray(index, index + 3).equals(source.subarray(previous, previous + 3))) return false
      }
    }
    return true
  }
  const analytic = rule.comparison === 'analytic-full-page'
  const blank = analytic && rule.fills.length === 0
  if ((!analytic && rule.comparison !== 'source-with-patches') || (!blank && rotation !== 0)) return false
  if (!analytic && source?.length !== size) return false
  const expected = analytic ? Buffer.alloc(size) : Buffer.from(source)
  const paints = (analytic ? rule.fills : rule.patches).map((paint) => ({
    ...paint, bounds: paintBounds(paint, page, policy.dpi / 72),
  }))
  if (paints.some(({ glyphRun }) => glyphRun && glyphReference?.length !== size)) return false
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const index = (y * width + x) * 3
      const paint = paints.findLast(({ bounds: [x0, y0, x1, y1] }) => x >= x0 && x < x1 && y >= y0 && y < y1)
      const rgb = paint?.glyphRun ? glyphReference.subarray(index, index + 3) : paint?.rgb ??
        (analytic ? policy.backgroundRgb : source.subarray(index, index + 3))
      expected.set(rgb, index)
    }
  }
  if (analytic && digest(expected) !== rule.sha256Rgb8) return false
  if (blank) return actual.equals(expected)
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const glyph = paints.find(({ glyphRun, bounds: [x0, y0, x1, y1] }) =>
        glyphRun && x >= x0 && x < x1 && y >= y0 && y < y1)
      const center = expected[(y * width + x) * 3]
      let nearEdge = false
      if (glyph) {
        const band = policy.edgeBandPx
        for (let row = Math.max(0, y - band); row <= Math.min(height - 1, y + band); row++) {
          for (let column = Math.max(0, x - band); column <= Math.min(width - 1, x + band); column++) {
            if ((expected[(row * width + column) * 3] === 255) !== (center === 255)) nearEdge = true
          }
        }
      }
      const index = (y * width + x) * 3
      const tolerance = nearEdge ? policy.edgeChannelTolerance : (policy.flatChannelTolerance ?? 0)
      if ([0, 1, 2].some((channel) => Math.abs(actual[index + channel] - expected[index + channel]) > tolerance)) return false
    }
  }
  return true
}

function exactSnapshot(snapshot) {
  const hex = snapshot?.outputHex
  requireMatch(typeof hex === 'string' && hex.length <= 2 * byteLimit && /^(?:[0-9a-f]{2})+$/.test(hex), 'native-output')
  return Buffer.from(hex, 'hex')
}

async function loadNative() {
  const binary = boundedRead(resolve(root, 'web/generated/flow_wasm_bg.wasm'), wasmLimit)
  try {
    const wasm = await import(new URL('../web/generated/flow_wasm.js', import.meta.url))
    wasm.initSync({ module: binary })
    return { wasm, wasmHash: digest(binary) }
  } catch {
    throw new GateError('unavailable', 'wasm-module')
  }
}

export function nativeSession(wasm, input) {
  const originStoreHash = digest(input)
  let requestId = 0
  let snapshot
  function exchange(operation, payload, retry = false) {
    const binding = snapshot?.binding
    const response = wasm[`native_pdf_${operation}`](JSON.stringify({
      protocolVersion: 1, requestId: retry ? requestId : ++requestId, operation,
      originStoreHash, ...(binding ? { binding } : {}), ...payload,
    }))
    requireMatch(operation === 'snapshot' ? wasm.verify_native_pdf_worker_response(response) :
      wasm.verify_native_pdf_response(response), 'native-response')
    const reply = JSON.parse(response)
    requireMatch(reply.operation === operation && reply.requestId === requestId &&
      (operation !== 'snapshot' || same(reply.binding, binding)) &&
      (!reply.snapshot || reply.snapshot.originStoreHash === originStoreHash), 'native-response-binding')
    if (reply.snapshot) snapshot = reply.snapshot
    return reply
  }
  const opened = exchange('open', { epoch: Array(16).fill(1), bytesHex: input.toString('hex') })
  if (snapshot) requireMatch(exactSnapshot(snapshot).equals(input), 'native-origin')
  return {
    opened,
    current: () => snapshot,
    command: (command) => exchange('command', { command, outputHash: snapshot.outputHash }),
    redact: (action, retry = false) => exchange('redaction', { action, acceptedHash: snapshot.outputHash }, retry),
    readback: () => exchange('snapshot', {}, true),
    close() {
      if (snapshot) requireMatch(exchange('close', {}).status === 'closed', 'native-close')
    },
  }
}

function one(targets) {
  requireInput(targets?.length === 1, 'native-operation-target')
  return targets[0]
}

function familyCommand(op, request, snapshot, outcome) {
  const page = request.page - 1
  const { objectPages, textPages } = snapshot.scene
  if (op.startsWith('replaceText')) {
    const { page, sourceText, baselinePt } = request.target
    const textPage = textPages?.find((entry) => entry.page === page - 1)
    requireMatch(outcome !== 'admitted' || !textPage?.refusal, 'native-operation-target-refused')
    const [x, y] = milli(baselinePt)
    const island = one(textPage?.islands.filter(({ text, bounds }) =>
      text === sourceText && bounds[0] >= x - 1000 && bounds[0] <= x && bounds[1] <= y + 1000 && bounds[3] > y))
    return { kind: 'replaceText', island: island.id, text: request.text }
  }
  if (['insertBlankPage', 'reorderPage', 'rotatePage', 'duplicatePage', 'deletePage', 'deleteFinalPage'].includes(op)) {
    const target = { page, stableId: snapshot.pageState.order[page] }
    if (op === 'insertBlankPage') return { kind: op, target, side: request.side }
    if (op === 'reorderPage') return { kind: op, target, position: request.position - 1 }
    if (op === 'rotatePage') return { kind: op, target, degrees: request.degrees }
    return { kind: op === 'deleteFinalPage' ? 'deletePage' : op, target }
  }
  if (op.endsWith('Field') || op.startsWith('setField')) {
    if (op === 'addTextField') return { kind: op, page, rect: request.rectPt, name: request.name, value: request.value }
    if (op === 'addCheckboxField') return { kind: op, page, rect: request.rectPt, name: request.name, checked: request.value === 'Yes' }
    const { id: field } = one(snapshot.fields?.filter(({ name }) => name === request.field))
    if (op.startsWith('setFieldText')) return { kind: 'setFieldText', field, value: request.value }
    if (op === 'setFieldChecked') return { kind: op, field, checked: request.checked }
    if (op === 'moveField') return { kind: op, field, page, rect: request.rectPt }
    return { kind: op, field }
  }
  const graphic = request.graphic && {
    shape: request.graphic.shape, points: request.graphic.pointsPt.map(milli),
    fill: request.graphic.fill, stroke: request.graphic.stroke, strokeWidth: request.graphic.strokeWidth * 1000,
  }
  if (op === 'addGraphic') return { kind: op, page, graphic }
  if (op === 'addImage') return { kind: op, page, bounds: milli(request.rectPt), assetHex: request.assetPngHex }
  const targetPage = request.target.page - 1
  const objectPage = objectPages?.find((entry) => entry.page === targetPage)
  requireMatch(outcome !== 'admitted' || !objectPage?.refusal, 'native-operation-target-refused')
  const { id, profile } = one(objectPage?.objects.filter(
    ({ kind, bounds }) => kind === (request.target.kind === 'path' ? 'graphic' : 'image') &&
      same(bounds, milli(request.target.rectPt)),
  ))
  const target = { page: targetPage, id, profile }
  if (op === 'replaceGraphic') return { kind: op, target, graphic }
  if (op === 'replaceImage') return { kind: op, target, assetHex: request.assetPngHex }
  if (op.startsWith('moveObject')) return { kind: 'moveObject', target,
    transform: request.transform }
  requireInput(op === 'removeObject', 'native-operation')
  return { kind: op, target }
}

function redact(session, operation) {
  for (const { page, rectPt } of operation.request.marks) {
    const reply = session.redact({ kind: 'addMark', pageId: session.current().pageState.order[page - 1], rect: milli(rectPt) })
    if (reply.code) return reply
  }
  const staged = session.redact({ kind: 'stage' })
  if (staged.code) return staged
  const candidate = staged.redaction?.candidate
  requireInput(candidate, 'native-redaction-candidate')
  let published
  return withCleanup(() => {
    const verified = session.redact({ kind: 'verify', candidate }, true)
    const verdict = verified.redaction?.verification?.status
    requireMatch(verdict !== 'failed', 'native-redaction-verification')
    requireInput(verdict === 'passed', 'native-redaction-verification')
    published = session.redact({ kind: 'publish', candidate }, true)
    requireMatch(published.status === 'published' && same(published.snapshot.outputHash, candidate.hash), 'native-redaction-publication')
    return published
  }, () => {
    if (published?.status !== 'published') session.redact({ kind: 'cancel', candidate }, true)
  })
}

export function operationOutput(session, fixture, operation) {
  const { id, input } = fixture
  const { operation: name, outcome, request, noteReadback } = operation
  if (id === 'refused') {
    requireMatch(session.opened.status === 'failed' && session.opened.code === outcome &&
      !session.opened.snapshot && !session.current(), 'native-refusal')
    return input
  }
  requireMatch(session.opened.status !== 'failed', 'native-fixture-admission')
  requireInput(session.current(), 'native-fixture-admission')
  if (name === 'preserve') return exactSnapshot(session.current())
  for (const { operation: setup, ...parameters } of request?.after ?? []) {
    const result = session.command(familyCommand(setup, parameters, session.current(), 'admitted'))
    requireMatch(result.status !== 'failed', 'native-operation-setup')
    requireInput(result.status === 'applied', 'native-operation-setup')
  }
  const base = session.current()
  let reply
  if (id === 'notes-preservation') {
    const note = session.current().scene.pages.flatMap(({ annotations }) => annotations)
      .find(({ kind }) => kind === 'plainNote')?.id
    const { page, rectPt: rect, contents } = noteReadback[0] ?? {}
    const commands = {
      addNote: { kind: 'add', page: page - 1, rect, contents },
      editNote: { kind: 'edit', note, contents },
      moveNote: { kind: 'move', note, page: page - 1, rect },
      removeNote: { kind: 'remove', note },
    }
    reply = session.command(commands[name])
  } else if (name.startsWith('redact')) {
    reply = redact(session, operation)
  } else {
    reply = session.command(familyCommand(name, request, base, outcome))
  }
  if (outcome !== 'admitted') {
    requireMatch(reply.status === 'failed' && reply.code === outcome && !reply.snapshot &&
      !reply.redaction && !reply.preview, 'native-refusal')
    const { status, snapshot } = session.readback()
    requireMatch(status === 'unseen' && snapshot?.binding.revision === base.binding.revision &&
      same(snapshot.outputHash, base.outputHash), 'native-refusal-readback')
    const output = exactSnapshot(snapshot)
    requireMatch(output.equals(exactSnapshot(base)), 'native-refusal-readback')
    return output
  }
  requireMatch(reply.status !== 'failed', 'native-operation-refused')
  requireInput(['applied', 'published'].includes(reply.status), 'native-operation-unavailable')
  const output = exactSnapshot(reply.snapshot)
  requireMatch(!output.equals(input), 'native-rewrite')
  return output
}

function pdfString(value) {
  if (typeof value !== 'string') return value
  if (value.startsWith('u:')) return value.slice(2)
  if (value.startsWith('/')) return value.slice(1)
  return undefined
}

export function pdfStructure(json) {
  const [{ jsonversion }, objects] = json.qpdf
  requireMatch(jsonversion === 2 && json.pages.length <= 16, 'structure-format')
  const resolveObject = (value) => {
    if (typeof value !== 'string' || !/^\d+ \d+ R$/.test(value)) return value
    return objects[`obj:${value}`]?.value ?? objects[`obj:${value}`]?.stream?.dict
  }
  const trailer = objects.trailer.value
  const catalog = resolveObject(trailer['/Root'])
  const refs = json.pages.map(({ object }) => object)
  const pageNumber = (ref) => refs.indexOf(ref) + 1
  function inherited(dict, key) {
    for (let depth = 0; dict && depth < 16; depth++, dict = resolveObject(dict['/Parent'])) {
      if (dict[key] !== undefined) return resolveObject(dict[key])
    }
    return undefined
  }
  const notes = []
  const fields = []
  const checkboxes = []
  const widgets = []
  const links = []
  const outlines = []
  const pages = refs.map((ref, index) => {
    const dict = resolveObject(ref)
    const page = index + 1
    for (const annotRef of resolveObject(dict['/Annots']) ?? []) {
      const annot = resolveObject(annotRef)
      const subtype = annot['/Subtype']
      const rectPt = resolveObject(annot['/Rect'])
      if (subtype === '/Text') notes.push({ page, rectPt, contents: pdfString(annot['/Contents']) })
      else if (subtype === '/Widget') {
        requireMatch(annot['/P'] === ref, 'widget-page')
        widgets.push(annotRef)
        const normal = resolveObject(annot['/AP'])?.['/N']
        const state = annot['/AS']
        const appearance = resolveObject(normal)
        if (state) {
          requireMatch((state === '/Off' || state === '/Yes') &&
            Object.hasOwn(appearance ?? {}, '/Off') && Object.hasOwn(appearance, '/Yes') &&
            Object.keys(appearance).length === 2, 'widget-appearance-state')
          requireMatch(appearance['/Off'] !== appearance['/Yes'], 'checkbox-shared-appearance')
          checkboxes.push({ page, rectPt, ref: annotRef, state: pdfString(state) })
        }
        const streams = state ? Object.values(appearance) : [normal]
        for (const stream of streams) {
          const form = objects[`obj:${stream}`]?.stream?.dict
          const matrix = form?.['/Matrix']
          requireMatch(form?.['/Subtype'] === '/Form' &&
            matchesWidgetBounds(rectPt, resolveObject(form['/BBox']),
              matrix === undefined ? undefined : resolveObject(matrix) ?? null), 'widget-appearance')
        }
        const fieldType = resolveObject(annot['/FT'])
        const { '/Ff': flags = 0 } = annot
        fields.push({ page, rectPt, fieldType,
          ...(fieldType === '/Btn' ? { fieldFlags: resolveObject(flags) } : {}),
          name: pdfString(annot['/T']), value: pdfString(annot['/V']),
          defaultValue: pdfString(annot['/DV']), ...(state ? { appearanceState: pdfString(state) } : {}) })
      } else if (subtype === '/Link') {
        const destination = resolveObject(annot['/Dest'])
        links.push({ kind: 'link', page, rectPt,
          target: Array.isArray(destination) ? pageNumber(destination[0]) : null,
          view: Array.isArray(destination) ? destination.slice(1) : null })
      } else {
        requireMatch(false, 'unexpected-annotation')
      }
    }
    return {
      page, cropBoxPt: inherited(dict, '/CropBox') ?? inherited(dict, '/MediaBox'),
      rotation: ((inherited(dict, '/Rotate') ?? 0) % 360 + 360) % 360,
      resources: inherited(dict, '/Resources'), contents: json.pages[index].contents,
    }
  })
  const formFields = resolveObject(resolveObject(catalog['/AcroForm'])?.['/Fields']) ?? []
  requireMatch(same([...formFields].sort(), widgets.sort()), 'field-ownership')
  function readOutlines(ref, depth = 0) {
    if (!ref) return
    requireMatch(depth < 16 && outlines.length < 64, 'outline-budget')
    const outline = resolveObject(ref)
    const destination = resolveObject(outline['/Dest'])
    requireMatch(Array.isArray(destination), 'outline-destination')
    outlines.push({ kind: 'outline', title: pdfString(outline['/Title']),
      target: pageNumber(destination[0]), view: destination.slice(1) })
    readOutlines(outline['/First'], depth + 1)
    readOutlines(outline['/Next'], depth + 1)
  }
  readOutlines(resolveObject(catalog['/Outlines'])?.['/First'])
  return { catalog, trailer, resolveObject, objects, pages, notes, fields, checkboxes,
    destinations: [...outlines, ...links] }
}

function sameRecords(actual, expected) {
  const normalize = (values) => values.map((record) => JSON.stringify(Object.entries(record).sort())).sort()
  return same(normalize(actual), normalize(expected))
}

export function checkStructure(actual, source, operation, rawStream, showObject, inputPath, outputPath, savedOutput) {
  const pages = operation.pages.map(({ page, cropBoxPt, rotation }) => ({ page, cropBoxPt, rotation }))
  requireMatch(same(actual.pages.map(({ page, cropBoxPt, rotation }) => ({ page, cropBoxPt, rotation })), pages),
    'page-structure')
  requireMatch(sameRecords(actual.notes, operation.noteReadback), 'note-structure')
  const fields = (operation.fieldReadback ?? []).map((field) => ({
    ...field, ...(Object.hasOwn(field, 'appearanceState') ? { fieldType: '/Btn', fieldFlags: 0 } : { fieldType: '/Tx' }),
  }))
  requireMatch(sameRecords(actual.fields, fields), 'field-structure')
  requireMatch(sameRecords(actual.destinations, operation.destinations ?? []), 'destination-structure')
  const hasDecoy = Object.values(actual.objects).some(({ value }) => Object.hasOwn(value ?? {}, '/UnreachableDecoy'))
  if (operation.retained?.includes('UnreachableDecoy')) requireMatch(hasDecoy, 'source-decoy')
  if (operation.removed.includes('UnreachableDecoy')) requireMatch(!hasDecoy, 'pruned-decoy')
  if (['editNote', 'removeNote'].includes(operation.operation)) {
    const retired = operation.removed.filter((entry) => entry !== 'UnreachableDecoy')
    const hasRetiredNote = Object.values(actual.objects).some(({ value }) =>
      actual.resolveObject(value?.['/Subtype']) === '/Text' &&
      retired.includes(pdfString(actual.resolveObject(value['/Contents']))))
    requireMatch(!hasRetiredNote, 'retired-note-contents')
  }
  if (operation.retained?.includes('catalog-custom-scalars')) {
    requireMatch(same(actual.catalog['/FixtureValues'], source.catalog['/FixtureValues']) &&
      same(actual.trailer['/ID'], source.trailer['/ID']) &&
      same(actual.resolveObject(actual.trailer['/Info']), source.resolveObject(source.trailer['/Info'])), 'generic-retention')
    requireMatch(actual.pages[0].resources === actual.pages[1].resources, 'shared-resources')
    for (const [index, page] of actual.pages.entries()) {
      const ref = page.contents[0]
      const sourceRef = source.pages[index].contents[0]
      const outputDict = actual.resolveObject(ref)
      const encoded = rawStream(outputPath, ref)
      requireMatch(page.contents.length === 1 && outputDict?.['/Filter'] === '/FlateDecode' &&
        encoded.equals(rawStream(inputPath, sourceRef)), 'encoded-content-retention')
      if (index === 0) {
        const lengthRef = /\/Length\s+(\d+\s+\d+\s+R)\b/.exec(showObject(outputPath, ref).toString())?.[1]
        requireMatch(lengthRef && actual.resolveObject(lengthRef) === encoded.length, 'indirect-stream-length')
      }
      const fontFile = (structure, { resources }) => {
        const font = structure.resolveObject(structure.resolveObject(resources['/Font'])['/F1'])
        return [structure.resolveObject(font['/FontDescriptor'])['/FontFile2'], font['/ToUnicode']]
      }
      const sourceFonts = fontFile(source, source.pages[index])
      requireMatch(fontFile(actual, page).every((font, fontIndex) => rawStream(outputPath, font).equals(
        rawStream(inputPath, sourceFonts[fontIndex]))), 'embedded-font-retention')
    }
  }
  if (operation.operation === 'redact') {
    requireMatch(hasSingleRevision(savedOutput, actual), 'redaction-revision')
    requireMatch(!actual.trailer['/Info'] && !actual.trailer['/ID'] &&
      !actual.catalog['/FlowPDFSource'] && !actual.catalog['/Metadata'], 'redaction-trailer')
    let images = 0
    for (const [key, object] of Object.entries(actual.objects)) {
      const dict = object.value ?? object.stream?.dict
      if (!dict || Array.isArray(dict) || typeof dict !== 'object') continue
      requireMatch(!dict['/Metadata'] && dict['/Type'] !== '/Metadata' &&
        !Object.values(dict).some((value) => typeof value === 'string' && value.startsWith('u:Sensitive ')),
      'redaction-private-storage')
      if (dict['/Subtype'] === '/Image') {
        images++
        const samples = rawStream(outputPath, key.slice(4), false)
        requireMatch(samples.equals(Buffer.from([0, 83])), 'redaction-image-samples')
      }
    }
    requireMatch(images === 1, 'redaction-image-count')
    for (const ref of actual.pages[0].contents) {
      const content = rawStream(outputPath, ref, false).toString('latin1')
      requireMatch(!/(?:^|\s)(?:BT|Tj|TJ)(?:\s|$)/.test(content), 'redaction-content-text')
    }
  }
}

function compareFixture(fixture, wasm, directory, invoke, observations) {
  const inputPath = join(directory, 'source.pdf')
  const outputPath = join(directory, 'output.pdf')
  const glyphPath = join(directory, 'glyph-reference.pdf')
  const widgetPath = join(directory, 'widget-reference.pdf')
  const alternatePath = join(directory, 'checkbox-alternate.pdf')
  writeFileSync(inputPath, fixture.input, { mode: 0o600 })
  const { spec } = fixture
  const observation = { fixture: fixture.id, sourceHash: fixture.sourceHash,
    expectationHash: fixture.expectationHash, rasterVersion: spec.raster.version, dpi: spec.raster.dpi, operations: [] }
  observations.push(observation)
  const inspect = (path) => pdfStructure(JSON.parse(invoke('structure', ['qpdf', '--json=2', '--json-stream-data=none',
    '--json-key=qpdf', '--json-key=pages', '--decode-level=none', path])))
  const source = inspect(inputPath)
  const showObject = (path, ref, ...args) => {
    requireMatch(/^\d+ \d+ R$/.test(ref), 'stream-reference')
    const [object, generation] = ref.split(' ')
    return invoke('object', ['qpdf', `--show-object=${object},${generation}`, ...args, path])
  }
  const rawStream = (path, ref, raw = true) =>
    showObject(path, ref, raw ? '--raw-stream-data' : '--filtered-stream-data')
  const raster = (path, page, width, height, annotations = false) => decodePpm(invoke(
    annotations ? 'widget-raster' : 'raster', ['pdftoppm',
    '-r', String(spec.raster.dpi), '-cropbox', ...(annotations ? [] : ['-hide-annotations']), '-f', String(page), '-l', String(page),
    '-singlefile', path]), width, height)
  const sources = new Map()
  const ordered = fixture.id === 'notes-preservation' ? nativeSession(wasm, fixture.input) : null
  withCleanup(() => {
    for (const operation of spec.operations) {
      const evidence = { operation: operation.operation, status: 'unavailable',
        affectedPages: operation.affectedPages, pageChecks: [], checks: {} }
      observation.operations.push(evidence)
      const session = ordered ?? nativeSession(wasm, fixture.input)
      const output = withCleanup(() => operationOutput(session, fixture, operation), () => {
        if (!ordered) session.close()
      })
      evidence.outputHash = digest(output)
      writeFileSync(outputPath, output, { mode: 0o600 })
      invoke('qpdf-check', ['qpdf', '--check', outputPath])
      evidence.checks.qpdf = 'pass'
      const actual = inspect(outputPath)
      const savedOutput = boundedRead(outputPath)
      requireMatch(digest(savedOutput) === evidence.outputHash, 'output-changed')
      checkStructure(actual, source, operation, rawStream, showObject, inputPath, outputPath, savedOutput)
      if (operation.operation === 'redact') evidence.checks.singleRevision = 'pass'
      evidence.checks.structure = 'pass'
      for (const page of operation.pages) {
        const num = String(page.page)
        const pageEvidence = { page: page.page }
        evidence.pageChecks.push(pageEvidence)
        const text = invoke('text', ['pdftotext', '-f', num, '-l', num, '-enc', 'UTF-8', '-nopgbrk', '-raw',
          outputPath, '-']).toString('utf8').replace(/\s+/gu, ' ').trim()
        requireMatch(text === page.text, 'page-text')
        pageEvidence.text = 'pass'
        const sourcePage = page.sourcePage === undefined ? page.page : page.sourcePage
        const comparison = page.raster?.comparison ?? page.comparison
        const turned = comparison === 'source-transformed'
        let reference
        if (sourcePage !== null && comparison !== 'analytic-full-page') {
          if (!sources.has(sourcePage)) {
            sources.set(sourcePage, raster(inputPath, sourcePage, turned ? page.heightPx : page.widthPx,
              turned ? page.widthPx : page.heightPx))
          }
          reference = sources.get(sourcePage)
        }
        let glyphReference
        const glyph = page.raster?.patches?.find(({ glyphRun }) => glyphRun)?.glyphRun
        if (glyph) {
          const sourceText = '<000300010001>'
          const expectedText = '<' + glyph.cids.map((cid) => cid.toString(16).padStart(4, '0')).join('') + '>'
          const bytes = fixture.input.toString('latin1')
          requireInput(same(glyph.baselinePt, [16, 76]) && glyph.sizePt === 12 &&
            bytes.split(sourceText).length === 2 && expectedText.length <= sourceText.length, 'glyph-reference')
          writeFileSync(glyphPath, Buffer.from(bytes.replace(sourceText, expectedText.padEnd(sourceText.length)), 'latin1'),
            { mode: 0o600 })
          glyphReference = raster(glyphPath, sourcePage, page.widthPx, page.heightPx)
        }
        const contentRaster = raster(outputPath, page.page, page.widthPx, page.heightPx)
        requireMatch(compareRaster(page, spec.raster, contentRaster, reference, glyphReference), 'page-raster')
        pageEvidence.raster = 'pass'
        const widgets = (operation.fieldReadback ?? []).filter((field) => field.page === page.page)
        const vacated = source.fields.filter((field) => field.page === sourcePage &&
          !widgets.some(({ rectPt }) => same(rectPt, field.rectPt)))
        if (widgets.length || vacated.length) {
          const rendered = raster(outputPath, page.page, page.widthPx, page.heightPx, true)
          if (widgets.length) {
            writeFileSync(widgetPath, widgetReference(fixture.input, page, widgets, spec.widgetRaster), { mode: 0o600 })
            const expected = raster(widgetPath, 1, page.widthPx, page.heightPx, true)
            requireMatch(widgets.filter(({ appearanceState }) => !appearanceState).every(({ rectPt }) =>
              compareWidgetRegion(page, spec.raster.dpi, rectPt, rendered, expected)), 'widget-raster')
            const checkboxes = actual.checkboxes.filter((field) => field.page === page.page)
            if (checkboxes.length) {
              writeFileSync(alternatePath, alternateCheckboxPdf(savedOutput, checkboxes), { mode: 0o600 })
              const alternate = raster(alternatePath, page.page, page.widthPx, page.heightPx, true)
              const alternateWidgets = widgets.map((field) => {
                if (!field.appearanceState) return field
                const state = alternateState(field.appearanceState)
                return { ...field, appearanceState: state, value: state }
              })
              writeFileSync(widgetPath, widgetReference(fixture.input, page, alternateWidgets, spec.widgetRaster),
                { mode: 0o600 })
              const expectedAlternate = raster(widgetPath, 1, page.widthPx, page.heightPx, true)
              requireMatch(checkboxes.every(({ rectPt }) => compareCheckboxAppearances(page, spec.raster.dpi, rectPt,
                { saved: rendered, alternate }, { saved: expected, alternate: expectedAlternate })), 'checkbox-raster')
              pageEvidence.checkboxAppearances = 'pass'
            }
          }
          requireMatch(vacated.every(({ rectPt }) => compareWidgetRegion(page, spec.raster.dpi, rectPt,
            rendered, contentRaster)), 'widget-raster-vacated')
          pageEvidence.widgetRaster = 'pass'
        }
      }
      requireMatch(digest(boundedRead(outputPath)) === evidence.outputHash, 'output-changed')
      evidence.status = 'pass'
    }
    requireMatch(digest(boundedRead(inputPath)) === fixture.sourceHash, 'source-changed')
  }, () => ordered?.close())
}

function recordComparisonError(evidence, error) {
  evidence.status = error instanceof GateError ? error.status : 'fail'
  evidence.reason = error instanceof GateError ? error.message : 'comparison-error'
  const last = evidence.fixtures.at(-1)?.operations.at(-1)
  if (last && last.status !== 'pass') last.status = evidence.status
}

export function compareFixtures(fixtures, wasm, directory, invoke, evidence) {
  try {
    for (const fixture of fixtures) compareFixture(fixture, wasm, directory, invoke, evidence.fixtures)
    evidence.status = 'pass'
  } catch (error) {
    recordComparisonError(evidence, error)
  }
}

export function comparisonInvoker(executables, directory, env, commands, run = runBounded) {
  const toolEnv = { ...env, TMPDIR: directory, XDG_CACHE_HOME: directory, LC_ALL: 'C' }
  const deadline = Date.now() + 600_000
  return (check, [tool, ...args]) => {
    const remaining = deadline - Date.now()
    requireMatch(remaining > 0, 'comparison-deadline')
    const executable = executables.get(tool)
    const stats = commands[check] ??= { pass: 0, fail: 0, unavailable: 0 }
    const result = executable && run([executable, ...args], { cwd: directory, env: toolEnv, encoding: 'buffer',
      maxBuffer: byteLimit, timeout: Math.min(remaining, 15_000) })
    if (!result || result.error?.code === 'ENOENT') {
      stats.unavailable++
      throw new GateError('unavailable', 'tool-disappeared')
    }
    if (result.status !== 0 || result.error) {
      stats.fail++
      throw new GateError('fail', check)
    }
    stats.pass++
    return result.stdout
  }
}

export async function verifyExternal() {
  const evidence = { status: 'unavailable', observedAt: new Date().toISOString(), manifestHash,
    tools: {}, commands: {}, fixtures: [] }
  let directory
  try {
    const env = pinnedEnvironment()
    const executables = new Map()
    for (const tool of toolNames) {
      const probe = probeTool(tool, runBounded, { cwd: root, env })
      evidence.tools[tool] = probe?.version ?? null
      if (probe) executables.set(tool, probe.executable)
    }
    const missing = toolNames.filter((tool) => !evidence.tools[tool])
    evidence.commands.probe = { pass: toolNames.length - missing.length, fail: 0, unavailable: missing.length }
    if (process.versions.node !== supportedNodeVersion) missing.push('node-24.10.0')
    if (missing.length) {
      evidence.missing = missing
      return evidence
    }
    requireInput(Number(evidence.tools.qpdf.split('.')[0]) >= 11, 'qpdf-json-v2')
    requireInput(evidence.tools.pdftotext === evidence.tools.pdftoppm, 'poppler-version-mismatch')
    const fixtures = loadFixtures()
    const npm = runBounded(['npm', '--version'], { env })
    requireInput(npm.status === 0 && npm.stdout.trim() === '11.9.0', 'npm-11.9.0')
    const rust = runBounded(['rustc', '--version'], { env })
    requireInput(rust.status === 0 && /^rustc 1\.97\.1 /.test(rust.stdout), 'rust-1.97.1')
    const target = runBounded(['rustup', 'target', 'list', '--installed'], { env })
    requireInput(target.status === 0 && target.stdout.split(/\s+/).includes('wasm32-unknown-unknown'), 'rust-wasm-target')
    const bindgen = runBounded(['node', 'scripts/verify-wasm-bindgen-tool.mjs'], { env })
    requireInput(bindgen.status === 0, 'wasm-bindgen')
    const build = runBounded(['npm', 'run', 'build:wasm'], { env, timeout: 1_200_000 })
    requireInput(build.status === 0, 'wasm-build')
    const { wasm, wasmHash } = await loadNative()
    evidence.runtime = { node: process.versions.node, npm: '11.9.0', rust: '1.97.1', wasmHash }
    const work = join(root, 'work')
    mkdirSync(work, { recursive: true })
    directory = mkdtempSync(join(work, 'flowpdf-phase9-'))
    compareFixtures(fixtures, wasm, directory, comparisonInvoker(executables, directory, env, evidence.commands), evidence)
  } catch (error) {
    recordComparisonError(evidence, error)
  } finally {
    if (directory) {
      try {
        rmSync(directory, { recursive: true, force: true })
      } catch (error) {
        if (evidence.status === 'pass') recordComparisonError(evidence, error)
      }
    }
  }
  return evidence
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2)
  let evidence
  try {
    evidence = args.length === 0 || (args.length === 1 && args[0] === '--strict-external') ?
      await verifyExternal() : { status: 'fail', reason: 'invalid-arguments' }
  } catch {
    evidence = { status: 'fail', reason: 'comparison-error' }
  }
  console.log(JSON.stringify(evidence))
  process.exitCode = externalExitCode(evidence.status, args.includes('--strict-external'))
}
