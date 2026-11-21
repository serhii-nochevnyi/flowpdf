import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, delimiter, join } from 'node:path'
import { test } from 'node:test'
import {
  alternateCheckboxPdf, checkStructure, compareCheckboxAppearances, compareFixtures, compareRaster, compareWidgetRegion,
  comparisonInvoker, decodePpm, externalExitCode, loadFixtures, matchesWidgetBounds,
  nativeSession, operationOutput, pdfStructure, probeTool,
} from '../../scripts/verify-pdf-native-external.mjs'

const fixture = (name) => readFileSync(new URL(`../../fixtures/pdf-native/${name}`, import.meta.url))
const editFamilies = JSON.parse(fixture('edit-families.expected.json'))

function scratchDirectory(t, prefix) {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), prefix)))
  t.after(() => rmSync(directory, { recursive: true, force: true }))
  return directory
}

function nativeResponder(respond) {
  return (json) => {
    const request = JSON.parse(json)
    const { operation, requestId, originStoreHash, binding = null } = request
    return JSON.stringify({ operation, requestId, originStoreHash, binding, ...respond(request) })
  }
}

function openedReply({ originStoreHash, epoch, bytesHex }, details = {}) {
  const originHash = Array.from(Buffer.from(originStoreHash, 'hex'))
  const binding = { epoch, sessionId: 1, originHash, revision: 0 }
  return { status: 'opened', binding, snapshot: {
    originStoreHash, binding, outputHash: originHash, outputHex: bytesHex, ...details,
  } }
}

function assertRejectedComparison(t, source, operation, sourceStructure, wasm, label, reason = 'native-response-binding') {
  const directory = scratchDirectory(t, 'flowpdf-reply-binding-')
  const compared = []
  const evidence = { status: 'unavailable', fixtures: [] }
  compareFixtures([{ ...source, spec: { ...source.spec, operations: [operation] } }], wasm, directory,
    (check, args) => {
      compared.push([check, basename(args.at(-1))])
      return Buffer.from(JSON.stringify(sourceStructure))
    }, evidence)
  assert.equal(evidence.status, 'fail', label)
  assert.equal(evidence.reason, reason, label)
  assert.equal(evidence.fixtures[0].operations[0].status, 'fail', label)
  assert.equal(Object.hasOwn(evidence.fixtures[0].operations[0], 'outputHash'), false, label)
  assert.deepEqual([externalExitCode(evidence.status), externalExitCode(evidence.status, true)], [1, 1], label)
  assert.deepEqual(compared, [['structure', 'source.pdf']], label)
  assert.deepEqual(readdirSync(directory), ['source.pdf'], label)
}

test('the external verifier loads and validates the pinned fixture corpus', () => {
  const fixtures = loadFixtures()
  assert.deepEqual(fixtures.map(({ id }) => id), JSON.parse(fixture('manifest.json')).fixtures.map(({ id }) => id))
  assert.ok(fixtures.some(({ spec }) => spec.operations.some(({ operation }) => operation === 'deletePage')))
})

test('both CLI modes reject failed checks and only strict external rejects unavailable checks', () => {
  assert.equal(externalExitCode('unavailable', true), 2)
  assert.equal(externalExitCode('fail', true), 1)
  assert.equal(externalExitCode('unavailable', false), 0)
  assert.equal(externalExitCode('fail', false), 1)
})

test('external comparisons use the executables probed before changing directories', (t) => {
  const scratch = scratchDirectory(t, 'flowpdf-executable-contract-')
  const probeDirectory = join(scratch, 'probe')
  const directory = join(scratch, 'comparison')
  const tools = ['qpdf', 'pdftotext', 'pdftoppm']
  for (const cwd of [probeDirectory, directory]) {
    mkdirSync(join(cwd, 'bin'), { recursive: true })
    for (const tool of tools) writeFileSync(join(cwd, 'bin', tool), '', { mode: 0o700 })
  }
  const env = { PATH: ['unavailable', 'bin'].join(delimiter) }
  const options = { cwd: probeDirectory, env }
  const calls = []
  const refused = loadFixtures().find(({ id }) => id === 'refused')
  const [operation] = refused.spec.operations
  const [{ widthPx, heightPx, text }] = operation.pages
  const { json } = structuralReadback(operation.pages)
  const ppm = Buffer.concat([Buffer.from(`P6\n${widthPx} ${heightPx}\n255\n`),
    Buffer.alloc(widthPx * heightPx * 3, 255)])
  const run = ([executable, flag], { cwd, env, encoding, maxBuffer, timeout }) => {
    const tool = basename(executable)
    calls.push([executable, flag, cwd])
    assert.equal(env.PATH, options.env.PATH)
    if (cwd === probeDirectory) {
      return tool === 'qpdf' ? { status: 0, stdout: 'qpdf version 11.9.1\n' } :
        { status: 0, stderr: `${tool} version 25.04.0\nother text` }
    }
    assert.equal(encoding, 'buffer')
    assert.equal(maxBuffer, 4 * 1024 * 1024)
    assert.ok(timeout > 0 && timeout <= 15_000)
    assert.equal(env.TMPDIR, directory)
    if (flag === '--check') return { status: 0, stdout: Buffer.alloc(0) }
    if (flag === '--json=2') return { status: 0, stdout: Buffer.from(JSON.stringify(json)) }
    return { status: 0, stdout: tool === 'pdftotext' ? Buffer.from(text) : ppm }
  }
  const executables = new Map(tools.map((tool) => {
    const probe = probeTool(tool, run, options)
    assert.equal(probe.version, tool === 'qpdf' ? '11.9.1' : '25.04.0')
    return [tool, probe.executable]
  }))
  const wasm = {
    native_pdf_open: nativeResponder(() => ({ status: 'failed', code: operation.outcome, snapshot: null })),
    verify_native_pdf_response: () => true,
  }
  const evidence = { status: 'unavailable', commands: {}, fixtures: [] }
  compareFixtures([refused], wasm, directory, comparisonInvoker(executables, directory, env, evidence.commands, run),
    evidence)
  assert.equal(evidence.status, 'pass', evidence.reason)
  assert.deepEqual(calls, [
    ['qpdf', '--version', probeDirectory],
    ['pdftotext', '-v', probeDirectory],
    ['pdftoppm', '-v', probeDirectory],
    ['qpdf', '--json=2', directory],
    ['qpdf', '--check', directory],
    ['qpdf', '--json=2', directory],
    ['pdftotext', '-f', directory],
    ['pdftoppm', '-r', directory],
    ['pdftoppm', '-r', directory],
  ].map(([tool, flag, cwd]) => [join(probeDirectory, 'bin', tool), flag, cwd]))

  assert.equal(probeTool('qpdf', () => assert.fail('missing executable was invoked'),
    { cwd: probeDirectory, env: { PATH: 'unavailable' } }), null)
  assert.equal(probeTool('qpdf', () => ({ error: { code: 'ENOENT' } }), options), null)
  assert.equal(probeTool('pdftoppm', () => ({ status: 0, stderr: 'private payload' }), options), null)
  const missingDirectory = scratchDirectory(t, 'flowpdf-missing-tool-contract-')
  const unavailable = { status: 'pass', commands: {}, fixtures: [] }
  const missing = comparisonInvoker(executables, missingDirectory, env, unavailable.commands, ([executable]) => {
    assert.equal(executable, join(probeDirectory, 'bin', 'qpdf'))
    return { error: { code: 'ENOENT' } }
  })
  compareFixtures([refused], wasm, missingDirectory, missing, unavailable)
  assert.equal(unavailable.status, 'unavailable')
  assert.equal(unavailable.reason, 'tool-disappeared')
  assert.deepEqual(unavailable.commands.structure, { pass: 0, fail: 0, unavailable: 1 })
  assert.deepEqual(readdirSync(missingDirectory), ['source.pdf'])
  assert.equal(externalExitCode(unavailable.status, false), 0)
  assert.equal(externalExitCode(unavailable.status, true), 2)
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

  const cases = [
    ['matching refusal and readback', {}],
    ['wrong nested revision', { revision: 3 }, 'native-refusal-readback'],
    ['wrong nested session', { sessionId: 2 }, 'native-refusal-readback'],
    ['wrong output hash', { outputHash: Array(32).fill(3) }, 'native-refusal-readback'],
    ['wrong output bytes', { outputHex: input.toString('hex') }, 'native-refusal-readback'],
    ['wrong refusal source', {}, 'native-response-binding', { originStoreHash: '0'.repeat(64) }],
    ['wrong refusal revision', {}, 'native-response-binding', { revision: 1 }],
    ['wrong readback revision', {}, 'native-response-binding', {}, { revision: 1 }],
  ]
  for (const [label, { revision, sessionId, ...change }, reason, refusalChange = {}, readbackChange] of cases) {
    let commands = 0
    let liveChecks = 0
    let base
    const { revision: refusalRevision, ...refusal } = refusalChange
    const wasm = {
      native_pdf_open: nativeResponder((request) => {
        const opened = openedReply(request, { scene: {}, pageState: { order: [1, 2, 3] } })
        base = opened.snapshot
        return opened
      }),
      native_pdf_command: nativeResponder(({ binding, command, outputHash }) => {
        assert.deepEqual(binding, base.binding)
        assert.deepEqual(outputHash, base.outputHash)
        assert.equal(command.kind, 'deletePage')
        commands++
        if (commands < 3) {
          base = { ...base, binding: { ...binding, revision: commands }, outputHash: Array(32).fill(commands),
            outputHex: baseBytes.toString('hex'), pageState: { order: base.pageState.order.slice(1) } }
          return { status: 'applied', snapshot: base }
        }
        return { status: 'failed', code: operation.outcome, snapshot: null, ...refusal,
          binding: { ...binding, revision: refusalRevision ?? binding.revision } }
      }),
      native_pdf_snapshot: nativeResponder(({ originStoreHash, binding, requestId }) => {
        assert.equal(commands, 3)
        assert.equal(requestId, 4)
        assert.equal(originStoreHash, base.originStoreHash)
        assert.deepEqual(binding, base.binding)
        return { status: 'unseen', binding: { ...binding, ...readbackChange }, snapshot: { ...base, ...change,
          binding: { ...binding, revision: revision ?? binding.revision, sessionId: sessionId ?? binding.sessionId },
        } }
      }),
      verify_native_pdf_response: (json) => JSON.parse(json).operation !== 'snapshot',
      verify_native_pdf_worker_response: (json) => {
        liveChecks++
        return JSON.parse(json).operation === 'snapshot'
      },
    }
    const session = nativeSession(wasm, input)
    const run = () => operationOutput(session, { id: 'edit-families', input }, operation)
    if (reason) assert.throws(run, { status: 'fail', message: reason }, label)
    else assert.deepEqual(run(), baseBytes)
    assert.deepEqual(session.current(), base, label)
    assert.equal(liveChecks, Object.keys(refusalChange).length ? 0 : 1, label)
  }
})

test('a refused open retains the source without requesting a session snapshot', () => {
  const input = fixture('refused.pdf')
  const outcome = 'FLOW_PDF_NATIVE_UNSUPPORTED'
  const open = (change = () => ({})) => nativeSession({
    native_pdf_open: nativeResponder((request) => ({
      status: 'failed', code: outcome, snapshot: null, ...change(request),
    })),
    verify_native_pdf_response: () => true,
  }, input)
  const session = open()
  assert.equal(operationOutput(session, { id: 'refused', input }, { outcome }), input)
  assert.throws(() => open(() => ({ originStoreHash: '0'.repeat(64) })),
    { status: 'fail', message: 'native-response-binding' })
  assert.throws(() => open((request) => ({ binding: openedReply(request).binding })),
    { status: 'fail', message: 'native-response-binding' })
})

test('verified replies with mismatched request identity stop external comparison', (t) => {
  const source = loadFixtures().find(({ id }) => id === 'edit-families')
  const operation = source.spec.operations.find(({ operation }) => operation === 'rotatePage')
  const { json } = structuralReadback(operation.pages)
  const outputHex = fixture('notes-preservation.pdf').toString('hex')
  const otherSource = '0'.repeat(64)
  const cases = [
    ['open', 'operation', { operation: 'command' }],
    ['open', 'request ID', { requestId: 2 }],
    ['open', 'source', { originStoreHash: otherSource }],
    ['open', 'epoch', { epoch: Array(16).fill(2) }],
    ['open', 'bytes', { outputHex }, 'native-origin'],
    ['command', 'session ID', { sessionId: 2 }],
    ['command', 'epoch', { epoch: Array(16).fill(2) }],
    ['command', 'native origin', { originHash: Array(32).fill(2) }],
    ['command', 'revision', { revision: 1 }],
    ['close', 'source', { originStoreHash: otherSource }],
    ['close', 'revision', { revision: 0 }],
  ]
  for (const [step, field, { operation: replyOperation, requestId, originStoreHash, outputHex: openHex, ...bindingChange },
    reason = 'native-response-binding'] of cases) {
    const label = `${step} ${field}`
    const verified = []
    let base
    const wasm = {
      native_pdf_open: nativeResponder((request) => {
        const opened = openedReply(request, { scene: {}, pageState: { order: [1, 2, 3] } })
        base = opened.snapshot
        if (step !== 'open') return opened
        const binding = { ...opened.binding, ...bindingChange }
        return { ...opened, operation: replyOperation ?? request.operation, requestId: requestId ?? request.requestId,
          originStoreHash: originStoreHash ?? request.originStoreHash, binding,
          snapshot: { ...base, binding, outputHex: openHex ?? request.bytesHex } }
      }),
      native_pdf_command: nativeResponder(({ binding }) => {
        assert.deepEqual(binding, base.binding, label)
        const replyBinding = step === 'command' ? { ...binding, ...bindingChange } : binding
        const snapshot = { ...base, binding: { ...replyBinding, revision: replyBinding.revision + 1 }, outputHex }
        return { status: 'applied', binding: replyBinding, snapshot }
      }),
      native_pdf_close: nativeResponder((request) => {
        const { binding } = request
        assert.equal(binding.revision, step === 'close' ? 1 : 0, label)
        if (step !== 'close') return { status: 'closed' }
        return { status: 'closed', originStoreHash: originStoreHash ?? request.originStoreHash,
          binding: { ...binding, ...bindingChange } }
      }),
      verify_native_pdf_response: (response) => {
        verified.push(JSON.parse(response).operation)
        return true
      },
    }
    assertRejectedComparison(t, source, operation, json, wasm, label, reason)
    assert.deepEqual(verified, step === 'open' ? [replyOperation ?? 'open'] : ['open', 'command', 'close'], label)
  }
})

test('definitive application refusals fail fixture comparison while missing targets remain unavailable', (t) => {
  const source = loadFixtures().find(({ id }) => id === 'edit-families')
  const cases = [
    ['rotatePage', 'open', 'native-fixture-admission', ['open']],
    ['deleteFinalPage', 'command', 'native-operation-setup', ['open', 'deletePage', 'close']],
    ['rotatePage', 'command', 'native-operation-refused', ['open', 'rotatePage', 'close']],
    ['replaceText', null, 'native-operation-target', ['open', 'close']],
  ]
  for (const [name, failedStep, reason, expectedCalls] of cases) {
    const operation = source.spec.operations.find((entry) => entry.operation === name)
    const directory = scratchDirectory(t, 'flowpdf-application-outcome-')
    const requests = []
    const compared = []
    const refuse = { status: 'failed', code: 'FLOW_PDF_NATIVE_UNSUPPORTED' }
    const respond = (reply) => nativeResponder((request) => {
      requests.push(request.command?.kind ?? request.operation)
      return reply(request)
    })
    const wasm = {
      native_pdf_open: respond((request) => failedStep === 'open' ? refuse :
        openedReply(request, { scene: {}, pageState: { order: [1, 2, 3] } })),
      native_pdf_command: respond(() => {
        assert.equal(failedStep, 'command')
        return refuse
      }),
      native_pdf_close: respond(() => ({ status: 'closed' })),
      verify_native_pdf_response: () => true,
    }
    const evidence = { status: 'unavailable', fixtures: [] }
    const { json } = structuralReadback(source.spec.operations.find((entry) => entry.operation === 'rotatePage').pages)
    compareFixtures([{ ...source, spec: { ...source.spec, operations: [operation] } }], wasm, directory,
      (check, args) => {
        compared.push([check, basename(args.at(-1))])
        return Buffer.from(JSON.stringify(json))
      }, evidence)
    const status = failedStep ? 'fail' : 'unavailable'
    assert.equal(evidence.reason, reason, name)
    assert.equal(evidence.status, status, name)
    assert.equal(evidence.fixtures[0].operations[0].status, status, name)
    assert.deepEqual([externalExitCode(status), externalExitCode(status, true)], failedStep ? [1, 1] : [0, 2])
    assert.deepEqual(requests, expectedCalls, name)
    assert.deepEqual(compared, [['structure', 'source.pdf']], name)
    assert.deepEqual(readdirSync(directory), ['source.pdf'], name)
  }
})

test('a verified text capability refusal contradicts an admitted fixture target', (t) => {
  const source = loadFixtures().find(({ id }) => id === 'edit-families')
  const admitted = source.spec.operations.find(({ operation }) => operation === 'replaceText')
  const expectedRefusal = source.spec.operations.find(({ operation }) => operation === 'replaceTextAbsentCode')
  const { page, sourceText, baselinePt: [x, y] } = admitted.request.target
  const targetPage = page - 1
  const island = {
    id: Array(32).fill(1), text: sourceText, profile: 'identityHCidFontType2', operator: 'Tj', maxAdvance: 20_000,
    bounds: [x * 1000, (y - 1) * 1000, (x + 10) * 1000, (y + 1) * 1000],
  }
  const refused = { page: targetPage, islands: [], refusal: expectedRefusal.outcome }
  const projected = (islands) => [{ page: targetPage, islands, refusal: null }]
  const { json } = structuralReadback(admitted.pages)
  const cases = [
    ['target page refused', admitted, [refused], 'fail', 'closed'],
    ['target page refused with failed close', admitted, [refused], 'fail', 'failed'],
    ['projection omitted', admitted, undefined, 'unavailable', 'closed'],
    ['another page refused', admitted, [{ ...refused, page }], 'unavailable', 'closed'],
    ['no island matched', admitted, projected([{ ...island, text: 'A' }]), 'unavailable', 'closed'],
    ['multiple islands matched', admitted, projected([island, { ...island, id: Array(32).fill(2) }]),
      'unavailable', 'closed'],
    ['projection refusal for an expected command refusal', expectedRefusal, [refused], 'unavailable', 'closed'],
  ]
  for (const [label, operation, textPages, status, closeStatus] of cases) {
    const directory = scratchDirectory(t, 'flowpdf-text-capability-')
    const requests = []
    const verified = []
    const compared = []
    const respond = (reply) => nativeResponder((request) => {
      requests.push(request.operation)
      return reply(request)
    })
    const wasm = {
      native_pdf_open: respond((request) => openedReply(request, { scene: { textPages } })),
      native_pdf_close: respond(() => ({ status: closeStatus })),
      verify_native_pdf_response: (response) => {
        verified.push(JSON.parse(response).operation)
        return true
      },
    }
    const evidence = { status: 'unavailable', fixtures: [] }
    compareFixtures([{ ...source, spec: { ...source.spec, operations: [operation] } }], wasm, directory,
      (check, args) => {
        compared.push([check, basename(args.at(-1))])
        return Buffer.from(JSON.stringify(json))
      }, evidence)
    assert.equal(evidence.status, status, label)
    assert.equal(evidence.reason, status === 'fail' ? 'native-operation-target-refused' : 'native-operation-target', label)
    assert.equal(evidence.fixtures[0].operations[0].status, status, label)
    assert.deepEqual([externalExitCode(evidence.status), externalExitCode(evidence.status, true)],
      status === 'fail' ? [1, 1] : [0, 2], label)
    assert.deepEqual(requests, ['open', 'close'], label)
    assert.deepEqual(verified, requests, label)
    assert.deepEqual(compared, [['structure', 'source.pdf']], label)
    assert.deepEqual(readdirSync(directory), ['source.pdf'], label)
  }
})

test('a verified object capability refusal contradicts an admitted fixture target', (t) => {
  const source = loadFixtures().find(({ id }) => id === 'edit-families')
  const admitted = source.spec.operations.find(({ operation }) => operation === 'replaceImage')
  const expectedRefusal = source.spec.operations.find(({ operation }) => operation === 'moveObjectInvalidGeometry')
  const { page, rectPt } = admitted.request.target
  const image = {
    id: Array(32).fill(1), kind: 'image', profile: 'rasterRgb8',
    bounds: rectPt.map((point) => point * 1000), operations: ['replace', 'move', 'remove'],
  }
  const refused = 'FLOW_PDF_NATIVE_OBJECT_STATE'
  const projected = (objects, refusedPage) => admitted.pages.map(({ page: number }) => number === refusedPage
    ? { page: number - 1, canAdd: false, objects: [], refusal: refused }
    : { page: number - 1, canAdd: true, objects: number === page ? objects : [], refusal: null })
  const { json } = structuralReadback(admitted.pages)
  const cases = [
    ['target page refused', admitted, projected([], page), 'fail'],
    ['projection omitted', admitted, undefined, 'unavailable'],
    ['another page refused', admitted, projected([], 1), 'unavailable'],
    ['no image matched', admitted, projected([{ ...image, bounds: [1000, 1000, 2000, 2000] }]), 'unavailable'],
    ['multiple images matched', admitted, projected([image, { ...image, id: Array(32).fill(2) }]), 'unavailable'],
    ['projection refusal for an expected command refusal', expectedRefusal, projected([], page), 'unavailable'],
  ]
  for (const [label, operation, objectPages, status] of cases) {
    const directory = scratchDirectory(t, 'flowpdf-object-capability-')
    const verified = []
    const compared = []
    const wasm = {
      native_pdf_open: nativeResponder((request) => openedReply(request, { scene: { objectPages } })),
      native_pdf_close: nativeResponder(() => ({ status: 'closed' })),
      verify_native_pdf_response: (response) => {
        verified.push(JSON.parse(response).operation)
        return true
      },
    }
    const evidence = { status: 'unavailable', fixtures: [] }
    compareFixtures([{ ...source, spec: { ...source.spec, operations: [operation] } }], wasm, directory,
      (check, args) => {
        compared.push([check, basename(args.at(-1))])
        return Buffer.from(JSON.stringify(json))
      }, evidence)

    assert.equal(evidence.status, status, label)
    assert.equal(evidence.reason, status === 'fail' ? 'native-operation-target-refused' : 'native-operation-target', label)
    assert.equal(evidence.fixtures[0].operations[0].status, status, label)
    assert.deepEqual([externalExitCode(evidence.status, false), externalExitCode(evidence.status, true)],
      status === 'fail' ? [1, 1] : [0, 2], label)
    assert.deepEqual(verified, ['open', 'close'], label)
    assert.deepEqual(compared, [['structure', 'source.pdf']], label)
    assert.deepEqual(readdirSync(directory), ['source.pdf'], label)
  }
})

test('redaction refusal preserves the verifier classification through bounded cleanup', (t) => {
  const redaction = loadFixtures().find(({ id }) => id === 'redaction')
  const { input, sourceHash, spec } = redaction
  const [operation] = spec.operations
  const originHash = Array.from(Buffer.from(sourceHash, 'hex'))
  const epoch = Array(16).fill(1)
  const binding = { epoch, sessionId: 1, originHash, revision: 0 }
  const outputHex = input.toString('hex')
  const snapshot = { originStoreHash: sourceHash, binding, outputHash: originHash, outputHex,
    pageState: { order: [42, 43] } }
  const candidate = { hash: Array(32).fill(7), validatorVersion: 1,
    validatorProfile: 'classic-xref-owned-144dpi-v1', affectedPages: [42] }
  const { json: sourceStructure } = structuralReadback(operation.pages)
  const stageId = operation.request.marks.length + 2
  const request = (operation, requestId) => ({ protocolVersion: 1, operation, requestId, originStoreHash: sourceHash })
  const redactRequest = (requestId, action) => ({
    ...request('redaction', requestId), binding, acceptedHash: originHash, action,
  })

  for (const [verdict, cleanupFailure, status, exitCodes] of [
    ['failed', 'none', 'fail', [1, 1]],
    ['failed', 'close', 'fail', [1, 1]],
    ['failed', 'cancel', 'fail', [1, 1]],
    ['publish-refused', 'none', 'fail', [1, 1]],
    ['inconclusive', 'none', 'unavailable', [0, 2]],
    ['inconclusive', 'close', 'unavailable', [0, 2]],
    ['inconclusive', 'cancel', 'unavailable', [0, 2]],
    ['unavailable', 'none', 'unavailable', [0, 2]],
    ['missing-verdict', 'none', 'unavailable', [0, 2]],
    ['missing-candidate', 'none', 'unavailable', [0, 2]],
  ]) {
    const label = `${verdict}, ${cleanupFailure} cleanup failure`
    const hasCandidate = verdict !== 'missing-candidate'
    const publishRefused = verdict === 'publish-refused'
    const directory = scratchDirectory(t, 'flowpdf-redaction-contract-')
    const requests = []
    const invocations = []
    let pending = false
    const respond = (reply) => nativeResponder((submitted) => {
      requests.push(submitted)
      return reply(submitted)
    })
    const wasm = {
      native_pdf_open: respond(() => ({ status: 'opened', binding, snapshot })),
      native_pdf_redaction: respond(({ action, acceptedHash }) => {
        const result = (details) => ({ status: 'redaction', redaction: { action: action.kind, acceptedHash, ...details } })
        switch (action.kind) {
          case 'addMark': return result({ candidate: null, verification: null })
          case 'stage':
            if (!hasCandidate) return result({ candidate: null, verification: null })
            pending = true
            return result({ candidate, verification: { status: 'pending' } })
          case 'verify':
            if (verdict === 'missing-verdict') {
              return { status: 'failed', code: 'FLOW_PDF_NATIVE_REDACTION_CANDIDATE_UNAVAILABLE' }
            }
            return result({ candidate, verification: { status: publishRefused ? 'passed' : verdict } })
          case 'publish':
            assert.ok(publishRefused)
            return { status: 'failed', code: 'FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED' }
          case 'cancel':
            if (cleanupFailure === 'cancel') throw new Error('controlled cancellation failure')
            pending = false
            return result({ candidate, verification: null })
          default: assert.fail(`unexpected redaction action: ${action.kind}`)
        }
      }),
      native_pdf_close: respond(() => pending || cleanupFailure === 'close' ?
        { status: 'failed', code: 'FLOW_PDF_NATIVE_RECONCILIATION_REQUIRED' } : { status: 'closed' }),
      verify_native_pdf_response: () => true,
    }
    const evidence = { status: 'unavailable', fixtures: [] }
    compareFixtures([redaction], wasm, directory, (check, argv) => {
      invocations.push([check, argv.at(-1)])
      return Buffer.from(JSON.stringify(sourceStructure))
    }, evidence)

    assert.deepEqual(requests, [
      { ...request('open', 1), epoch, bytesHex: outputHex },
      ...operation.request.marks.map(({ rectPt }, index) =>
        redactRequest(index + 2, { kind: 'addMark', pageId: 42, rect: rectPt.map((point) => point * 1000) })),
      redactRequest(stageId, { kind: 'stage' }),
      ...(hasCandidate ? [redactRequest(stageId, { kind: 'verify', candidate }),
        ...(publishRefused ? [redactRequest(stageId, { kind: 'publish', candidate })] : []),
        redactRequest(stageId, { kind: 'cancel', candidate })] : []),
      { ...request('close', stageId + 1), binding },
    ], label)
    assert.equal(evidence.status, status, label)
    const reason = publishRefused ? 'native-redaction-publication' :
      hasCandidate ? 'native-redaction-verification' : 'native-redaction-candidate'
    assert.equal(evidence.reason, reason, label)
    assert.deepEqual([externalExitCode(evidence.status, false), externalExitCode(evidence.status, true)], exitCodes, label)
    assert.deepEqual(evidence.fixtures[0].operations, [{
      operation: 'redact', status, affectedPages: operation.affectedPages, pageChecks: [], checks: {},
    }], label)
    assert.deepEqual(invocations, [['structure', join(directory, 'source.pdf')]], label)
    assert.deepEqual(readdirSync(directory), ['source.pdf'], label)
  }
})

test('redaction replies match the accepted PDF, action and staged candidate before comparison', (t) => {
  const source = loadFixtures().find(({ id }) => id === 'redaction')
  const [operation] = source.spec.operations
  const { json } = structuralReadback(operation.pages)
  const output = fixture('notes-preservation.pdf')
  const candidate = { hash: Array(32).fill(7), validatorVersion: 1,
    validatorProfile: 'classic-xref-owned-144dpi-v1', affectedPages: [42, 43] }
  const cases = [
    ['addMark', 'action', { action: 'removeMark' }],
    ['stage', 'accepted hash', { acceptedHash: candidate.hash }],
    ['verify', 'candidate hash', { candidate: { ...candidate, hash: Array(32).fill(8) } }],
    ['verify', 'validator version', { candidate: { ...candidate, validatorVersion: 2 } }],
    ['verify', 'validator profile', { candidate: { ...candidate, validatorProfile: 'other' } }],
    ['verify', 'candidate page order', { candidate: { ...candidate, affectedPages: [43, 42] } }],
    ['publish', 'candidate hash', { candidate: { ...candidate, hash: Array(32).fill(8) } }],
    ['publish', 'accepted hash', { acceptedHash: candidate.hash }],
    ['cancel', 'missing candidate', { candidate: null }],
    ['publish', 'matching publication'],
  ]
  for (const [step, field, change] of cases) {
    const label = `${step} ${field}`
    const calls = []
    let base
    const wasm = {
      native_pdf_open: nativeResponder((request) => {
        const opened = openedReply(request, { pageState: { order: [42, 43] } })
        base = opened.snapshot
        return opened
      }),
      native_pdf_redaction: nativeResponder(({ action, acceptedHash, binding, requestId }) => {
        assert.deepEqual(acceptedHash, base.outputHash, label)
        assert.deepEqual(binding, base.binding, label)
        if (Object.hasOwn(action, 'candidate')) assert.deepEqual(action.candidate, candidate, label)
        calls.push([action.kind, requestId])
        const redaction = {
          action: action.kind, acceptedHash, candidate: action.kind === 'addMark' ? null : candidate,
          verification: action.kind === 'addMark' || action.kind === 'cancel' ? null :
            { status: action.kind === 'stage' ? 'pending' : 'passed' },
          ...(action.kind === step ? change : {}),
        }
        if (action.kind !== 'publish') return { status: 'redaction', redaction }
        return { status: 'published', redaction, snapshot: { ...base, binding: { ...binding, revision: 1 },
          outputHash: redaction.candidate.hash, outputHex: output.toString('hex') } }
      }),
      native_pdf_close: nativeResponder(({ binding }) => {
        calls.push(['close', binding.revision])
        return { status: 'closed' }
      }),
      verify_native_pdf_response: () => true,
    }
    const stageId = operation.request.marks.length + 2
    if (!change) {
      const session = nativeSession(wasm, source.input)
      assert.deepEqual(operationOutput(session, source, operation), output)
      assert.deepEqual(session.current().outputHash, candidate.hash)
      session.close()
      assert.deepEqual(calls.slice(-4), [
        ['stage', stageId], ['verify', stageId], ['publish', stageId], ['close', 1],
      ])
      continue
    }
    if (step === 'cancel') {
      const session = nativeSession(wasm, source.input)
      const [{ rectPt }] = operation.request.marks
      session.redact({ kind: 'addMark', pageId: base.pageState.order[0], rect: rectPt.map((point) => point * 1000) })
      session.redact({ kind: 'stage' })
      assert.throws(() => session.redact({ kind: 'cancel', candidate }, true),
        { status: 'fail', message: 'native-response-binding' })
      assert.equal(session.current().binding.revision, 0)
      session.close()
      continue
    }
    assertRejectedComparison(t, source, operation, json, wasm, label)
    const cleanup = step === 'verify' || step === 'publish' ? [['cancel', stageId]] : []
    assert.deepEqual(calls.slice(-2 - cleanup.length),
      [[step, step === 'addMark' ? 2 : stageId], ...cleanup, ['close', 0]], label)
  }
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

test('checkbox comparison requires the declared artwork in both states within the widget rectangle', () => {
  const page = { widthPx: 2, heightPx: 1, cropBoxPt: [0, 0, 2, 1], rotation: 0 }
  const rect = [0, 0, 1, 1]
  const off = Buffer.alloc(6, 255)
  const yes = Buffer.from([0, 0, 0, 255, 255, 255])
  const wrong = Buffer.from([80, 80, 80, 255, 255, 255])
  const outsideOnly = Buffer.from([255, 255, 255, 0, 0, 0])
  const check = (actual, expected) => compareCheckboxAppearances(page, 72, rect, actual, expected)
  for (const [saved, alternate] of [[off, yes], [yes, off]]) {
    const expected = { saved, alternate }
    assert.equal(check(expected, expected), true)
    assert.equal(check({ saved, alternate: wrong }, expected), false)
    assert.equal(check({ saved: wrong, alternate }, expected), false)
    assert.equal(check({ saved, alternate: saved }, expected), false)
  }
  const sameRegion = { saved: off, alternate: outsideOnly }
  assert.equal(check(sameRegion, sameRegion), false)
})

test('the alternate raster input changes only the saved checkbox state selectors', () => {
  const saved = fixture('edit-families.pdf')
  const checkboxes = [{ ref: '19 0 R', state: 'Off' }]
  const alternate = alternateCheckboxPdf(saved, checkboxes)
  const changes = Array.from(alternate.keys()).filter((index) => saved[index] !== alternate[index])
  assert.equal(alternate.length, saved.length)
  assert.equal(changes.length, 6)
  for (const [index, selector] of [[changes[0], '/V /'], [changes[3], '/AS /']]) {
    assert.equal(saved.toString('latin1', index - selector.length, index + 3), selector + 'Off')
    assert.equal(alternate.toString('latin1', index - selector.length, index + 3), selector + 'Yes')
  }
  assert.deepEqual(alternateCheckboxPdf(alternate, [{ ref: '19 0 R', state: 'Yes' }]), saved)
  assert.throws(() => alternateCheckboxPdf(saved, [{ ref: '19 0 R', state: 'Yes' }]), /checkbox-source-state/)
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

function structuralReadback(pages) {
  const objects = {
    trailer: { value: { '/Root': '1 0 R' } },
    'obj:1 0 R': { value: {} },
  }
  const json = { qpdf: [{ jsonversion: 2 }, objects], pages: [] }
  for (const { page, cropBoxPt, rotation } of pages) {
    const object = `${page + 1} 0 R`
    objects[`obj:${object}`] = { value: { '/CropBox': cropBoxPt, '/Rotate': rotation } }
    json.pages.push({ object, contents: [] })
  }
  return { json, objects }
}

test('editing and removing notes reject exact retired contents in unlinked saved Text annotations', () => {
  const { spec } = loadFixtures().find(({ id }) => id === 'notes-preservation')
  for (const name of ['editNote', 'removeNote']) {
    const operation = { ...spec.operations.find(({ operation }) => operation === name), retained: [] }
    const retired = operation.removed.find((entry) => entry !== 'UnreachableDecoy')
    const { json, objects } = structuralReadback(operation.pages)
    for (const { page, rectPt, contents } of operation.noteReadback) {
      objects[`obj:${page + 1} 0 R`].value['/Annots'] = ['10 0 R']
      objects['obj:10 0 R'] = { value: { '/Subtype': '/Text', '/Rect': rectPt, '/Contents': `u:${contents}` } }
    }
    const check = () => checkStructure(pdfStructure(json), null, operation)
    check()
    const annotation = { '/Subtype': '/Text', '/Contents': '12 0 R' }
    objects['obj:11 0 R'] = { value: annotation }
    objects['obj:12 0 R'] = { value: `u:${retired} draft` }
    check()
    objects['obj:12 0 R'].value = `u:${retired}`
    assert.throws(check, /retired-note-contents/)
    annotation['/Contents'] = `u:${retired}`
    assert.throws(check, /retired-note-contents/)
    annotation['/Subtype'] = '/Link'
    check()
    objects['obj:13 0 R'] = { value: { '/UnreachableDecoy': 'u:Unused sample' } }
    assert.throws(check, /pruned-decoy/)
  }
})

test('widget readback compares field types and checkbox flags and requires distinct Off and Yes Forms', () => {
  const [source] = editFamilies.operations
  const operation = { pages: source.pages.slice(0, 1), fieldReadback: source.fieldReadback,
    noteReadback: [], removed: [] }
  const { json, objects } = structuralReadback(operation.pages)
  objects['obj:1 0 R'].value['/AcroForm'] = { '/Fields': ['10 0 R', '11 0 R'] }
  objects['obj:2 0 R'].value['/Annots'] = ['10 0 R', '11 0 R']
  const textWidget = {
    '/Subtype': '/Widget', '/P': '2 0 R', '/Rect': [16, 12, 64, 32], '/FT': '/Tx',
    '/T': 'u:recipient', '/V': 'u:', '/DV': 'u:', '/AP': { '/N': '12 0 R' },
  }
  const appearance = { '/Off': '13 0 R', '/Yes': '14 0 R' }
  const checkbox = {
    '/Subtype': '/Widget', '/P': '2 0 R', '/Rect': [80, 12, 100, 32], '/FT': '/Btn',
    '/T': 'u:accepted', '/V': '/Off', '/DV': '/Off', '/AS': '/Off', '/AP': { '/N': appearance },
  }
  objects['obj:10 0 R'] = { value: textWidget }
  objects['obj:11 0 R'] = { value: checkbox }
  for (const [ref, width] of [['12 0 R', 48], ['13 0 R', 20], ['14 0 R', 20]]) {
    objects[`obj:${ref}`] = { stream: { dict: { '/Subtype': '/Form', '/BBox': [0, 0, width, 20] } } }
  }
  const check = (expected = operation) => checkStructure(pdfStructure(json), null, expected)
  check()
  checkbox['/Ff'] = 0
  check()
  checkbox['/Ff'] = '15 0 R'
  objects['obj:15 0 R'] = { value: 0 }
  check()
  for (const flags of [1 << 15, 1 << 16]) {
    checkbox['/Ff'] = flags
    assert.throws(() => check(), /field-structure/)
    objects['obj:15 0 R'].value = flags
    checkbox['/Ff'] = '15 0 R'
    assert.throws(() => check(), /field-structure/)
  }
  delete objects['obj:15 0 R']
  assert.throws(() => check(), /field-structure/)
  delete checkbox['/Ff']
  for (const widget of [textWidget, checkbox]) {
    const fieldType = widget['/FT']
    widget['/FT'] = '/Ch'
    assert.throws(() => check(), /field-structure/)
    widget['/FT'] = fieldType
  }
  appearance['/On'] = appearance['/Yes']
  delete appearance['/Yes']
  assert.throws(() => check(), /widget-appearance-state/)
  appearance['/Yes'] = appearance['/On']
  assert.throws(() => check(), /widget-appearance-state/)
  delete appearance['/On']
  appearance['/Yes'] = appearance['/Off']
  assert.throws(() => check(), /checkbox-shared-appearance/)
  appearance['/Yes'] = '14 0 R'
  checkbox['/AS'] = checkbox['/V'] = '/Yes'
  const [textField, checkboxField] = operation.fieldReadback
  check({ ...operation, fieldReadback: [textField, { ...checkboxField, value: 'Yes', appearanceState: 'Yes' }] })
})

test('redaction structure requires one complete revision of the saved bytes', () => {
  const samples = Buffer.from([0, 83]).toString('latin1')
  const bodies = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Count 1 /Kids [3 0 R] >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1 1] /Resources << /XObject << /Im 4 0 R >> >> >>',
    '<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 2 >>' +
      '\nstream\n' + samples + '\nendstream',
  ]
  let pdf = '%PDF-1.7\n%\xE2\xE3\xCF\xD3\n'
  let entries = '0000000000 65535 f \n'
  for (const [index, body] of bodies.entries()) {
    entries += String(pdf.length).padStart(10, '0') + ' 00000 n \n'
    pdf += `${index + 1} 0 obj\n${body}\nendobj\n`
  }
  const xref = pdf.length
  const trailer = '<< /Size 5 /Root 1 0 R >>'
  const table = `xref\n0 5\n${entries}trailer\n${trailer}\n`
  const footer = (offset) => `startxref\n${offset}\n%%EOF\n`
  const saved = pdf + table + footer(xref)
  const operation = { operation: 'redact', pages: [{ page: 1, cropBoxPt: [0, 0, 1, 1], rotation: 0 }],
    noteReadback: [], removed: [] }
  const { json, objects } = structuralReadback(operation.pages)
  json.pages[0].object = '3 0 R'
  objects.trailer.value['/Size'] = 5
  objects['obj:1 0 R'].value['/Pages'] = '2 0 R'
  objects['obj:3 0 R'] = objects['obj:2 0 R']
  objects['obj:2 0 R'] = { value: { '/Kids': ['3 0 R'], '/Count': 1 } }
  objects['obj:4 0 R'] = { stream: { dict: { '/Subtype': '/Image' } } }
  const check = (bytes) => {
    const rawStream = (_path, ref) => {
      assert.equal(ref, '4 0 R')
      const sampleOffset = bytes.indexOf('\nstream\n') + '\nstream\n'.length
      return bytes.subarray(sampleOffset, sampleOffset + samples.length)
    }
    return checkStructure(pdfStructure(json), null, operation, rawStream, null, null, 'saved.pdf', bytes)
  }
  assert.throws(() => check(), /redaction-revision/)
  const laterXref = `xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 5 /Root 1 0 R /Prev ${xref} >>\n`
  for (const invalid of [
    saved.replace(footer(xref), footer(xref + 1)),
    saved + 'trailing bytes',
    saved + laterXref + footer(saved.length),
    saved + table + footer(saved.length),
    saved.replace('/Length 2', '/Length 1'),
  ]) {
    assert.throws(() => check(Buffer.from(invalid, 'latin1')), /redaction-revision/)
  }
  const bytes = Buffer.from(saved, 'latin1')
  for (const key of ['/Prev', '/XRefStm']) {
    objects.trailer.value[key] = null
    assert.throws(() => check(bytes), /redaction-revision/)
    delete objects.trailer.value[key]
  }
  check(bytes)
})

test('preserved destinations compare link rectangles and complete views', () => {
  const [source] = editFamilies.operations
  const rectPt = [120, 12, 144, 32]
  const linkView = ['/XYZ', 20, null, 1.25]
  const outlineView = ['/FitR', 12, 16, 144, 160]
  const operation = { pages: source.pages, noteReadback: [], removed: [], destinations: [
    { kind: 'outline', title: 'Second page', target: 2, view: outlineView },
    { kind: 'link', page: 1, rectPt, target: 2, view: linkView },
  ] }
  const { json, objects } = structuralReadback(operation.pages)
  const link = { '/Subtype': '/Link', '/Rect': '11 0 R', '/Dest': '12 0 R' }
  const outline = { '/Title': 'u:Second page', '/Dest': ['3 0 R', ...outlineView] }
  objects['obj:1 0 R'].value['/Outlines'] = { '/First': '13 0 R' }
  objects['obj:2 0 R'].value['/Annots'] = ['10 0 R']
  objects['obj:10 0 R'] = { value: link }
  objects['obj:11 0 R'] = { value: [...rectPt] }
  objects['obj:12 0 R'] = { value: ['3 0 R', ...linkView] }
  objects['obj:13 0 R'] = { value: outline }
  const check = () => checkStructure(pdfStructure(json), null, operation)
  check()
  objects['obj:11 0 R'].value = [120, 12, 145, 32]
  assert.throws(check, /destination-structure/)
  objects['obj:11 0 R'].value = [...rectPt]
  for (const view of [['/Fit'], ['/XYZ', 20, 0, 1.25], ['/XYZ', null, 20, 1.25], ['/XYZ', 20, null, 1.5]]) {
    objects['obj:12 0 R'].value = ['3 0 R', ...view]
    assert.throws(check, /destination-structure/)
  }
  objects['obj:12 0 R'].value = ['3 0 R', ...linkView]
  outline['/Dest'] = ['3 0 R', '/FitR', 12, 16, 144, 161]
  assert.throws(check, /destination-structure/)
})

test('deleting a destination page requires removing its inbound link annotation', () => {
  const operation = editFamilies.operations.find(({ operation }) => operation === 'deletePage')
  const { json, objects } = structuralReadback(operation.pages)
  const page = objects['obj:2 0 R'].value
  const check = (expected = operation) => checkStructure(pdfStructure(json), null, expected)
  check()
  page['/Annots'] = ['10 0 R']
  const link = { '/Subtype': '/Link', '/Rect': [120, 12, 144, 32], '/Dest': ['3 0 R', '/Fit'] }
  objects['obj:10 0 R'] = { value: link }
  check({ ...operation, destinations: [
    { kind: 'link', page: 1, rectPt: [120, 12, 144, 32], target: 2, view: ['/Fit'] },
  ] })
  delete link['/Dest']
  assert.throws(() => check(), /destination-structure/)
  delete page['/Annots']
  check()
})
