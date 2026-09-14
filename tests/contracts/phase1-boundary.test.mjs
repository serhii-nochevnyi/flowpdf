import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join, posix, relative, resolve } from 'node:path'
import test from 'node:test'
import ts from 'typescript'

const projectRoot = resolve(import.meta.dirname, '../..')
const allowedWasmExports = new Set([
  'apply_command',
  'apply_editor_session',
  'commit_record',
  'create_sample',
  'migrate_document',
  'open_document',
  'plan_persistence_commit',
  'plan_standalone_audit',
  'query_document',
  'query_editor_view',
  'recover_document',
  'recover_document_audited',
  'redo',
  'stage_asset',
  'undo',
])
const forbiddenDirectPackages = new Set([
  '@vitejs/plugin-react',
  'actix-web',
  'axum',
  'diesel',
  'drizzle-orm',
  'express',
  'fastify',
  'lopdf',
  'next',
  'pdf-lib',
  'pdfium-render',
  'pdfjs-dist',
  'prisma',
  'react',
  'react-dom',
  'reqwest',
  'sea-orm',
  'sequelize',
  'socket.io',
  'sqlx',
  'typeorm',
  'vite',
  'warp',
  'ws',
])
const forbiddenRuntimeImports = new Set([
  '@vitejs/plugin-react',
  'express',
  'fastify',
  'next',
  'pdf-lib',
  'pdfjs-dist',
  'react',
  'react-dom',
  'socket.io',
  'vite',
  'ws',
])
const forbiddenPhaseOneWebPathSegment = /(?:^|[\/._-])(?:auth|backend|collaboration|editor|forms?|layout|pdf|voice)(?=[\/._-]|$)/i
const forbiddenPhaseTwoWebPathSegment = /(?:^|[\/._-])(?:auth|backend|collaboration|forms?|layout|pdf|voice)(?=[\/._-]|$)/i
const phaseTwoEditorSource = /^(?:web\/src\/main\.tsx|web\/src\/editor\/[A-Za-z0-9._/-]+\.(?:ts|tsx))$/
const phaseTwoPackagePins = new Map([
  ['react', { section: 'dependencies', version: '19.2.8' }],
  ['react-dom', { section: 'dependencies', version: '19.2.8' }],
  ['@types/react', { section: 'devDependencies', version: '19.2.17' }],
  ['@types/react-dom', { section: 'devDependencies', version: '19.2.3' }],
  ['@vitejs/plugin-react', { section: 'devDependencies', version: '6.0.5' }],
  ['vite', { section: 'devDependencies', version: '8.1.5' }],
])
const phaseTwoViteScripts = new Map([
  ['dev', 'vite'],
  ['build:vite', 'vite build'],
])
const semanticOwnerName = /^(?:apply|canonicalize|hash|migrate|mutate|recover|redact|replay|serialize)(?:Flow)?(?:Audit|Document|Revision|Transaction)/i

test('the checked-in Phase 1 workspace preserves deferred scope and Rust semantic ownership', async () => {
  const snapshot = loadWorkspaceSnapshot(projectRoot)
  assert.deepEqual(boundaryDiagnostics(snapshot, { phase: 2 }), [])
  assertPhaseTwoParityBoundary(projectRoot, snapshot)

  const gatePath = resolve(projectRoot, 'scripts/check-phase1.mjs')
  assert.equal(existsSync(gatePath), true, 'scripts/check-phase1.mjs must close the phase')
  const gate = await import(`${new URL(`file://${gatePath}`).href}?contract=${Date.now()}`)
  assertGateContract(gate)
})

test('boundary fixture rejects TypeScript semantic mutation, recovery, and redaction ownership', () => {
  const fixture = validFixture()
  fixture.typescript.set(
    'web/src/semantic-owner.ts',
    `
      export function recoverFlowDocument(canonicalJson) {
        const document = JSON.parse(canonicalJson)
        document.revision += 1
        return redactDocumentAudit(document)
      }
      function redactDocumentAudit(document) { return document.audit.filter(Boolean) }
    `,
  )
  assert.match(boundaryDiagnostics(fixture).join('\n'), /semantic owner|semantic JSON|semantic state/i)
})

test('boundary fixture rejects representative editor, canvas, voice, and backend surfaces', () => {
  const fixture = validFixture()
  fixture.packageJson.dependencies = { react: '19.0.0' }
  fixture.typescript.set(
    'web/src/editor.ts',
    `
      import React from 'react'
      const canvas = document.createElement('canvas')
      canvas.setAttribute('contenteditable', 'true')
      navigator.mediaDevices.getUserMedia({ audio: true })
      fetch('/api/documents')
    `,
  )
  const diagnostics = boundaryDiagnostics(fixture).join('\n')
  assert.match(diagnostics, /react/i)
  assert.match(diagnostics, /deferred web path|canvas|contenteditable|voice|backend/i)
})

test('boundary fixture rejects Rust deferred dependencies and mutable WASM exports', () => {
  const fixture = validFixture()
  fixture.cargoManifests.set(
    'Cargo.toml',
    '[workspace.dependencies]\naxum = "=1.0.0"\nserde = "=1.0.228"\n',
  )
  fixture.wasmSource += `
    #[wasm_bindgen]
    pub struct MutableFlowDocument { revision: u64 }
    #[wasm_bindgen]
    pub fn mutable_document_handle() -> JsValue { JsValue::NULL }
  `
  const diagnostics = boundaryDiagnostics(fixture).join('\n')
  assert.match(diagnostics, /axum/i)
  assert.match(diagnostics, /mutable WASM struct|unexpected WASM export/i)
})

test('boundary fixture resolves deferred browser capabilities through aliases', () => {
  const fixture = validFixture()
  fixture.typescript.set(
    'web/src/inspector-capabilities.ts',
    `
      const directRequest = globalThis.fetch
      const { fetch: destructuredRequest } = globalThis
      const RequestTransport = globalThis.XMLHttpRequest
      const assignSemanticState = Object.assign
      globalThis.fetch('/api/global')
      directRequest('/api/direct')
      destructuredRequest('/api/destructured')
      new RequestTransport()
      assignSemanticState(flowDocument, { revision: 2 })
    `,
  )

  const diagnostics = boundaryDiagnostics(fixture).join('\n')
  assert.match(diagnostics, /deferred backend runtime surface/i)
  assert.match(diagnostics, /XMLHttpRequest/i)
  assert.match(diagnostics, /Object\.assign|mutates semantic state/i)
})

test('boundary fixture resolves deferred capabilities through cross-module export aliases', () => {
  const fixture = validFixture()
  fixture.typescript.set(
    'web/src/capabilities.ts',
    'export const remoteRequest = globalThis.fetch',
  )
  fixture.typescript.set(
    'web/src/use-capability.ts',
    `
      import { remoteRequest as request } from './capabilities.js'
      request('/api/documents')
    `,
  )

  assert.match(boundaryDiagnostics(fixture).join('\n'), /deferred backend runtime surface/i)
})

test('boundary fixture parses annotated WASM items and enforces typed signatures', () => {
  const fixture = validFixture()
  fixture.wasmSource += `
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    #[allow(clippy::needless_pass_by_value)]
    pub fn annotated_escape(request: JsValue) -> JsValue { request }

    #[wasm_bindgen(js_name = invalidCreateSample)]
    #[inline]
    pub fn create_sample(request: String) -> JsValue { JsValue::from_str(&request) }

    #[wasm_bindgen]
    #[derive(Clone)]
    pub struct AnnotatedMutableDocument { revision: u64 }
  `

  const diagnostics = boundaryDiagnostics(fixture).join('\n')
  assert.match(diagnostics, /unexpected WASM export annotated_escape/i)
  assert.match(diagnostics, /unexpected WASM export invalidCreateSample/i)
  assert.match(diagnostics, /invalidCreateSample.*typed WASM signature/i)
  assert.match(diagnostics, /mutable WASM struct/i)
})

test('boundary fixture validates the effective js_name instead of the Rust function name', () => {
  const fixture = validFixture()
  fixture.wasmSource = fixture.wasmSource.replace(
    '#[wasm_bindgen]\npub fn create_sample',
    '#[wasm_bindgen(js_name = hiddenCreate)]\npub fn create_sample',
  )

  const diagnostics = boundaryDiagnostics(fixture).join('\n')
  assert.match(diagnostics, /unexpected WASM export hiddenCreate/i)
  assert.match(diagnostics, /missing typed WASM export create_sample/i)
})

test('Phase 2 admits only exact React and Vite pins plus a safe editor TSX path', () => {
  const fixture = validFixture()
  fixture.packageJson.dependencies = {
    react: '19.2.8',
    'react-dom': '19.2.8',
  }
  fixture.packageJson.devDependencies = {
    ...fixture.packageJson.devDependencies,
    '@types/react': '19.2.17',
    '@types/react-dom': '19.2.3',
    '@vitejs/plugin-react': '6.0.5',
    vite: '8.1.5',
  }
  fixture.packageJson.scripts = { dev: 'vite', 'build:vite': 'vite build' }
  fixture.typescript.set(
    'web/src/editor/editor-shell.tsx',
    `
      import React from 'react'
      export function EditorShell({ label }) {
        return <article aria-label={label}><p>{label}</p></article>
      }
    `,
  )

  assert.deepEqual(boundaryDiagnostics(fixture, { phase: 2 }), [])

  fixture.packageJson.dependencies.react = '19.2.9'
  assert.match(
    boundaryDiagnostics(fixture, { phase: 2 }).join('\n'),
    /react.*exact approved Phase 2 version/i,
  )
})

test('Phase 2 editor allowance retains semantic, unsafe DOM, layout, voice, and backend bans', () => {
  const fixture = validFixture()
  fixture.packageJson.dependencies = { react: '19.2.8', 'react-dom': '19.2.8' }
  fixture.packageJson.devDependencies = {
    ...fixture.packageJson.devDependencies,
    '@types/react': '19.2.17',
    '@types/react-dom': '19.2.3',
    '@vitejs/plugin-react': '6.0.5',
    vite: '8.1.5',
  }
  fixture.typescript.set(
    'web/src/editor/unsafe-editor.tsx',
    `
      import React from 'react'
      export function mutateFlowDocument(canonicalJson, root) {
        const documentState = JSON.parse(canonicalJson)
        documentState.revision += 1
        root.innerHTML = canonicalJson
        document.createElement('canvas')
        navigator.mediaDevices.getUserMedia({ audio: true })
        fetch('/api/documents')
        return <form contentEditable dangerouslySetInnerHTML={{ __html: canonicalJson }}><canvas /></form>
      }
    `,
  )

  const diagnostics = boundaryDiagnostics(fixture, { phase: 2 }).join('\n')
  assert.match(diagnostics, /semantic owner|semantic JSON|semantic state/i)
  assert.match(diagnostics, /innerHTML|unsafe or deferred DOM/i)
  assert.match(diagnostics, /canvas/i)
  assert.match(diagnostics, /form/i)
  assert.match(diagnostics, /contenteditable/i)
  assert.match(diagnostics, /dangerouslySetInnerHTML/i)
  assert.match(diagnostics, /voice/i)
  assert.match(diagnostics, /backend/i)
})

test('Phase 2 rejects deferred filename tokens nested under the editor allowance', () => {
  const fixture = validFixture()
  for (const path of [
    'web/src/editor/pdf-export.ts',
    'web/src/editor/layout_engine.ts',
    'web/src/editor/voice-controller.ts',
    'web/src/editor/backend.client.ts',
  ]) {
    fixture.typescript.set(path, 'export const deferredSurface = true')
  }

  const diagnostics = boundaryDiagnostics(fixture, { phase: 2 }).join('\n')
  assert.match(diagnostics, /pdf-export\.ts: deferred web path/i)
  assert.match(diagnostics, /layout_engine\.ts: deferred web path/i)
  assert.match(diagnostics, /voice-controller\.ts: deferred web path/i)
  assert.match(diagnostics, /backend\.client\.ts: deferred web path/i)
})

test('WASM size report rejects forged measurements, stale inputs, and either exceeded budget', async () => {
  const { validateWasmSizeReport, WASM_SIZE_LIMITS } = await import('../../scripts/verify-wasm-size.mjs')
  const validReport = wasmSizeReportFixture(WASM_SIZE_LIMITS)
  const expected = expectedWasmSizeContext(validReport)
  assert.doesNotThrow(() => validateWasmSizeReport(validReport, expected))

  const adversarialCases = [
    ['forged baseline total', (report) => { report.baseline.rawBytes += 1 }],
    ['forged raw delta', (report) => { report.delta.rawBytes += 1 }],
    ['stale source manifest', (report) => { report.sourceManifest.digest = `sha256:${'f'.repeat(64)}` }],
    ['changed build inputs', (report) => { report.buildConfiguration.candidateFeatures.push('auto') }],
  ]
  for (const [name, mutate] of adversarialCases) {
    const report = structuredClone(validReport)
    mutate(report)
    assert.throws(
      () => validateWasmSizeReport(report, expected),
      undefined,
      name,
    )
  }

  const rawOverBudget = structuredClone(validReport)
  rawOverBudget.candidate.rawBytes = rawOverBudget.baseline.rawBytes + WASM_SIZE_LIMITS.rawDeltaBytes + 1
  rawOverBudget.delta.rawBytes = WASM_SIZE_LIMITS.rawDeltaBytes + 1
  assert.throws(
    () => validateWasmSizeReport(rawOverBudget, expectedWasmSizeContext(rawOverBudget)),
    /raw.*budget/i,
  )

  const gzipOverBudget = structuredClone(validReport)
  gzipOverBudget.candidate.gzipBytes = gzipOverBudget.baseline.gzipBytes + WASM_SIZE_LIMITS.gzipDeltaBytes + 1
  gzipOverBudget.delta.gzipBytes = WASM_SIZE_LIMITS.gzipDeltaBytes + 1
  assert.throws(
    () => validateWasmSizeReport(gzipOverBudget, expectedWasmSizeContext(gzipOverBudget)),
    /gzip.*budget/i,
  )
})

test('WASM size verifier rejects a same-version substituted bindgen binary before execution', async () => {
  const { materializeVerifiedWasmBindgen } = await import('../../scripts/verify-wasm-size.mjs')
  const root = mkdtempSync(join(tmpdir(), 'flowpdf-size-integrity-test-'))
  const cargoHome = join(root, 'work/toolchains/cargo')
  const binaryPath = join(cargoHome, 'bin/wasm-bindgen')
  const markerPath = join(root, 'substituted-binary-executed')
  const target = 'test-target'
  const approvedBytes = Buffer.from('approved wasm-bindgen fixture bytes')
  const approvedChecksum = createHash('sha256').update(approvedBytes).digest('hex')
  const receiptKey = 'wasm-bindgen-cli 0.2.108 (registry+https://github.com/rust-lang/crates.io-index)'
  try {
    mkdirSync(join(root, 'config'), { recursive: true })
    mkdirSync(join(root, 'artifacts/provenance'), { recursive: true })
    mkdirSync(join(cargoHome, 'bin'), { recursive: true })
    writeFileSync(join(root, 'config/dependency-provenance.json'), JSON.stringify({
      crates: [{
        name: 'wasm-bindgen-cli',
        version: '0.2.108',
        kind: 'tool',
        binarySha256: { [target]: approvedChecksum },
      }],
    }))
    writeFileSync(join(root, 'artifacts/provenance/phase1-dependencies.json'), JSON.stringify({
      status: 'success',
      crates: [{ name: 'wasm-bindgen-cli', version: '0.2.108', kind: 'tool' }],
    }))
    writeFileSync(join(cargoHome, '.crates2.json'), JSON.stringify({
      installs: {
        [receiptKey]: {
          version_req: '=0.2.108',
          bins: ['wasm-bindgen'],
          target,
        },
      },
    }))
    writeFileSync(
      binaryPath,
      `#!/bin/sh\ntouch ${JSON.stringify(markerPath)}\nprintf 'wasm-bindgen 0.2.108\\n'\n`,
      { mode: 0o700 },
    )
    chmodSync(binaryPath, 0o700)

    assert.throws(
      () => materializeVerifiedWasmBindgen(root),
      /checksum mismatch/,
    )
    assert.equal(existsSync(markerPath), false, 'substituted executable must never run')
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

function wasmSizeReportFixture(limits) {
  return {
    formatVersion: 1,
    status: 'passed',
    sourceManifest: {
      formatVersion: 1,
      algorithm: 'sha256',
      files: [{ path: 'crates/flow-wasm/src/lib.rs', sha256: `sha256:${'a'.repeat(64)}` }],
      digest: `sha256:${'b'.repeat(64)}`,
    },
    toolchain: {
      cargoVersion: 'cargo 1.97.1',
      rustcVersion: 'rustc 1.97.1',
      wasmBindgenVersion: 'wasm-bindgen 0.2.108',
      wasmBindgenBinarySha256: `sha256:${'e'.repeat(64)}`,
      wasmBindgenReceiptTarget: 'aarch64-apple-darwin',
      wasmBindgenProvenanceIdentity: {
        name: 'wasm-bindgen-cli',
        version: '0.2.108',
        kind: 'tool',
      },
      nodeVersion: 'v24.10.0',
      zlibVersion: '1.2.12',
      target: 'wasm32-unknown-unknown',
    },
    buildConfiguration: {
      profile: 'release',
      locked: true,
      target: 'wasm32-unknown-unknown',
      package: 'flow-wasm',
      baselineFeatures: [],
      candidateFeatures: ['icu-segmenter'],
      rustFlags: '-C debuginfo=0',
      bindgenTarget: 'web',
      measurementMode: 'current-source-probe-delta',
      onlyVariant: 'measurement-only ICU probe activation against current Phase 2 source',
    },
    compression: { algorithm: 'gzip', level: 9, mtime: 0 },
    limits: { ...limits },
    baseline: {
      label: 'phase2-production',
      rawBytes: 100_000,
      gzipBytes: 30_000,
      sha256: `sha256:${'c'.repeat(64)}`,
    },
    candidate: {
      label: 'phase2-production-with-icu-probe',
      rawBytes: 110_000,
      gzipBytes: 35_000,
      sha256: `sha256:${'d'.repeat(64)}`,
    },
    delta: { rawBytes: 10_000, gzipBytes: 5_000 },
    passed: true,
    blocked: false,
  }
}

function expectedWasmSizeContext(report) {
  return structuredClone({
    sourceManifest: report.sourceManifest,
    toolchain: report.toolchain,
    buildConfiguration: report.buildConfiguration,
    compression: report.compression,
    baseline: report.baseline,
    candidate: report.candidate,
  })
}

export function loadWorkspaceSnapshot(root) {
  const typescript = new Map()
  for (const directory of ['web/src', 'web/persistence']) {
    for (const path of productionFiles(resolve(root, directory))) {
      typescript.set(relative(root, path), readFileSync(path, 'utf8'))
    }
  }
  const cargoManifests = new Map()
  for (const path of [
    'Cargo.toml',
    'crates/flow-core/Cargo.toml',
    'crates/flow-wasm/Cargo.toml',
  ]) {
    cargoManifests.set(path, readFileSync(resolve(root, path), 'utf8'))
  }
  return {
    packageJson: JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8')),
    cargoManifests,
    typescript,
    wasmSource: readFileSync(resolve(root, 'crates/flow-wasm/src/lib.rs'), 'utf8'),
  }
}

function validFixture() {
  return {
    packageJson: { scripts: {}, dependencies: {}, devDependencies: { typescript: '5.9.3' } },
    cargoManifests: new Map([
      ['Cargo.toml', '[workspace.dependencies]\nserde = "=1.0.228"\n'],
    ]),
    typescript: new Map([
      [
        'web/src/inspector.ts',
        `
          export function renderInspector(root, dto) {
            const output = document.createElement('p')
            output.textContent = String(dto.revision)
            root.replaceChildren(output)
          }
        `,
      ],
    ]),
    wasmSource: [...allowedWasmExports]
      .map((name) =>
        name === 'stage_asset'
          ? '#[wasm_bindgen]\npub fn stage_asset(bytes: &[u8], request: JsValue) -> JsValue { request }'
          : `#[wasm_bindgen]\npub fn ${name}(request: JsValue) -> JsValue { request }`,
      )
      .join('\n'),
  }
}

export function boundaryDiagnostics(snapshot, options = {}) {
  const policy = boundaryPolicy(options)
  const diagnostics = []
  validatePackageManifest(snapshot.packageJson, diagnostics, policy)
  for (const [path, source] of snapshot.cargoManifests) {
    for (const dependency of cargoDependencies(source)) {
      if (forbiddenDirectPackages.has(dependency)) {
        diagnostics.push(`${path}: deferred Rust dependency ${dependency}`)
      }
    }
  }
  const typescriptProgram = parseTypeScriptProgram(snapshot.typescript)
  const capabilities = createCapabilityResolver(
    [...typescriptProgram.sources.values()],
    typescriptProgram.checker,
  )
  for (const [path] of snapshot.typescript) {
    if (policy.forbiddenWebPathSegment.test(path)) {
      diagnostics.push(`${path}: deferred web path entered Phase 1`)
    }
    validateTypeScript(
      path,
      typescriptProgram.sources.get(path),
      diagnostics,
      capabilities,
      policy,
    )
  }
  validateWasmBoundary(snapshot.wasmSource, diagnostics)
  return diagnostics.sort()
}

function boundaryPolicy(options) {
  const phase = options.phase ?? 1
  if (phase === 1) {
    return {
      phase,
      forbiddenWebPathSegment: forbiddenPhaseOneWebPathSegment,
      allowsEditorSource: () => false,
    }
  }
  if (phase === 2) {
    return {
      phase,
      forbiddenWebPathSegment: forbiddenPhaseTwoWebPathSegment,
      allowsEditorSource: (path) => phaseTwoEditorSource.test(path),
    }
  }
  throw new RangeError(`unsupported boundary policy phase ${phase}`)
}

export function assertPhaseTwoParityBoundary(root, snapshot = loadWorkspaceSnapshot(root)) {
  const contract = JSON.parse(
    readFileSync(resolve(root, 'tests/contracts/phase2-command-parity.json'), 'utf8'),
  )
  const expectedCommandTypes = [
    'insertText',
    'replaceText',
    'replaceSelection',
    'deleteText',
    'setNodeStyle',
    'insertNode',
    'deleteNode',
    'splitTextBlock',
    'mergeTextBlocks',
    'deleteSubtree',
    'setInlineMarks',
    'setInlineMark',
    'setBlockAttributes',
    'setBlockStyle',
    'setListKind',
    'continueListItem',
    'exitListItem',
    'indentListItem',
    'outdentListItem',
    'insertPageBreak',
    'removePageBreak',
    'insertTable',
    'insertImage',
    'replaceImage',
    'setImageAccessibility',
    'removeImage',
    'addTableRow',
    'removeTableRow',
    'addTableColumn',
    'removeTableColumn',
    'setTableHeaderRow',
    'removeTable',
    'setField',
  ]
  assert.equal(contract.formatVersion, 1)
  assert.equal(contract.phase, 'FLOWPDF-02-accessible-rich-text-editing')
  assert.equal(contract.mutationCount, expectedCommandTypes.length)
  assert.deepEqual(
    contract.commands.map(({ commandType }) => commandType),
    expectedCommandTypes,
  )
  assert.equal(
    new Set(contract.commands.map(({ commandType }) => commandType)).size,
    expectedCommandTypes.length,
  )
  for (const capability of contract.commands) {
    assert.match(capability.intent, /^editor\.intent\.[A-Za-z]+$/)
    assert.match(capability.visible?.labelKey ?? '', /^editor\.parity\.visible\./)
    assert.ok(capability.visible?.route, `${capability.commandType}: visible route required`)
    assert.match(capability.keyboard?.labelKey ?? '', /^editor\.parity\.keyboard\./)
    assert.ok(capability.keyboard?.route, `${capability.commandType}: keyboard route required`)
    assert.equal(capability.futureVoice?.commandType, capability.commandType)
    assert.equal(capability.futureVoice?.intent, capability.intent)
  }

  const controller = snapshot.typescript.get('web/src/editor/editor-controller.ts')
  const editorStore = snapshot.typescript.get('web/src/editor/editor-store.ts')
  assert.ok(controller, 'editor controller must be part of the Phase 2 boundary')
  assert.ok(editorStore, 'editor store must be part of the Phase 2 boundary')
  const structuralStart = controller.indexOf('export type StructuralCommandDto')
  const formattingStart = controller.indexOf('export type FormattingCommandDto')
  assert.ok(structuralStart >= 0 && formattingStart > structuralStart)
  assert.doesNotMatch(controller.slice(structuralStart, formattingStart), /\bbytes\s*:/)
  const requestStart = controller.indexOf('interface ApplyCommandRequestDto')
  const recoveryStart = controller.indexOf('interface RecoveryAuditContextDto')
  assert.ok(requestStart >= 0 && recoveryStart > requestStart)
  assert.doesNotMatch(controller.slice(requestStart, recoveryStart), /\bbytes\s*:/)
  assert.match(controller, /stage_asset/)
  assert.match(controller, /receipt:\s*staged\.receipt/)
  assert.doesNotMatch(controller, /(?:crypto\.subtle|createHash|blake3\s*\()/)
  assert.doesNotMatch(editorStore, /\bbytes\s*:/)
}

function validatePackageManifest(packageJson, diagnostics, policy) {
  for (const section of ['dependencies', 'devDependencies', 'optionalDependencies']) {
    for (const [dependency, version] of Object.entries(packageJson[section] ?? {})) {
      const phaseTwoPin = policy.phase === 2 ? phaseTwoPackagePins.get(dependency) : undefined
      if (phaseTwoPin !== undefined) {
        if (section !== phaseTwoPin.section || version !== phaseTwoPin.version) {
          diagnostics.push(
            `package.json: ${dependency} must use exact approved Phase 2 version ${phaseTwoPin.version} in ${phaseTwoPin.section}`,
          )
        }
      } else if (forbiddenDirectPackages.has(dependency)) {
        diagnostics.push(`package.json: deferred runtime dependency ${dependency}`)
      }
    }
  }
  for (const [name, command] of Object.entries(packageJson.scripts ?? {})) {
    const exactPhaseTwoViteCommand =
      policy.phase === 2 && phaseTwoViteScripts.get(name) === command
    if (
      /\b(?:drizzle-kit|next|prisma|react-scripts|schema\s+push|vite)\b/i.test(command) &&
      !exactPhaseTwoViteCommand
    ) {
      diagnostics.push(`package.json script ${name}: deferred runtime command`)
    }
    if (/\b(?:--watch|watch)\b/.test(command)) {
      diagnostics.push(`package.json script ${name}: watch mode is not deterministic`)
    }
  }
}

function validateTypeScript(path, source, diagnostics, capabilities, policy) {
  if (source === undefined) {
    diagnostics.push(`${path}: TypeScript source could not be loaded`)
    return
  }
  if (source.parseDiagnostics.length > 0) {
    diagnostics.push(`${path}: TypeScript source does not parse`)
    return
  }

  function visit(node) {
    if (ts.isImportDeclaration(node) && ts.isStringLiteral(node.moduleSpecifier)) {
      const root = packageRoot(node.moduleSpecifier.text)
      const exactPhaseTwoReactImport =
        policy.phase === 2 &&
        /^(?:react|react-dom)$/.test(root) &&
        policy.allowsEditorSource(path)
      if (forbiddenRuntimeImports.has(root) && !exactPhaseTwoReactImport) {
        diagnostics.push(`${path}: deferred runtime import ${node.moduleSpecifier.text}`)
      }
    }
    if (
      ts.isJsxElement(node) ||
      ts.isJsxSelfClosingElement(node) ||
      ts.isJsxFragment(node)
    ) {
      if (policy.phase !== 2 || !policy.allowsEditorSource(path)) {
        diagnostics.push(`${path}: JSX/React editor surface is deferred`)
      } else {
        validateJsxElement(path, node, diagnostics)
      }
    }
    if (hasDeclarationName(node) && semanticOwnerName.test(node.name.text)) {
      diagnostics.push(`${path}: TypeScript semantic owner ${node.name.text}`)
    }
    if (ts.isCallExpression(node)) {
      validateCall(path, source, node, diagnostics, capabilities)
    }
    if (ts.isNewExpression(node)) {
      const capability = capabilities.of(node.expression)
      if (capability !== undefined) {
        diagnostics.push(`${path}: deferred ${capabilityLabel(capability)} runtime surface`)
      }
    }
    if (ts.isBinaryExpression(node) && isAssignment(node.operatorToken.kind)) {
      validateAssignment(path, source, node.left, diagnostics)
    }
    ts.forEachChild(node, visit)
  }
  visit(source)
}

function validateJsxElement(path, node, diagnostics) {
  const tagName = ts.isJsxElement(node)
    ? node.openingElement.tagName
    : ts.isJsxSelfClosingElement(node) ? node.tagName : undefined
  if (tagName && ts.isIdentifier(tagName) && /^(?:canvas|form)$/.test(tagName.text)) {
    diagnostics.push(`${path}: deferred ${tagName.text} UI surface`)
  }
  const attributes =
    ts.isJsxElement(node) ? node.openingElement.attributes :
      ts.isJsxSelfClosingElement(node) ? node.attributes : undefined
  for (const attribute of attributes?.properties ?? []) {
    if (!ts.isJsxAttribute(attribute)) continue
    const name = attribute.name.getText()
    if (name === 'dangerouslySetInnerHTML') {
      diagnostics.push(`${path}: unsafe JSX dangerouslySetInnerHTML surface`)
    }
    if (name.toLowerCase() === 'contenteditable' && jsxAttributeIsActive(attribute)) {
      diagnostics.push(`${path}: deferred contenteditable editor surface`)
    }
  }
}

function jsxAttributeIsActive(attribute) {
  if (attribute.initializer === undefined) return true
  if (ts.isStringLiteral(attribute.initializer)) {
    return attribute.initializer.text.toLowerCase() !== 'false'
  }
  if (!ts.isJsxExpression(attribute.initializer)) return true
  const expression = attribute.initializer.expression
  return expression?.kind !== ts.SyntaxKind.FalseKeyword
}

function validateCall(path, source, call, diagnostics, capabilities) {
  const callee = call.expression.getText(source)
  const first = call.arguments[0]
  const second = call.arguments[1]
  if (
    /(?:^|\.)createElement$/.test(callee) &&
    first &&
    ts.isStringLiteral(first) &&
    /^(?:canvas|form)$/.test(first.text)
  ) {
    diagnostics.push(`${path}: deferred ${first.text} UI surface`)
  }
  if (
    callee.endsWith('.setAttribute') &&
    first &&
    ts.isStringLiteral(first) &&
    first.text.toLowerCase() === 'contenteditable' &&
    second &&
    (!ts.isStringLiteral(second) || second.text.toLowerCase() !== 'false')
  ) {
    diagnostics.push(`${path}: deferred contenteditable editor surface`)
  }
  const capability = capabilities.of(call.expression)
  if (capability === 'Object.assign') {
    if (first && isSemanticTarget(first)) {
      diagnostics.push(`${path}: Object.assign mutates semantic state ${first.getText(source)}`)
    }
  } else if (capability !== undefined) {
    diagnostics.push(`${path}: deferred ${capabilityLabel(capability)} runtime surface`)
  }
  if (
    callee === 'JSON.parse' &&
    first &&
    /canonical|flowDocument|snapshot|transaction|audit/i.test(first.getText(source))
  ) {
    diagnostics.push(`${path}: TypeScript interprets semantic JSON`)
  }
  if (/^(?:crypto\.subtle\.digest|createHash)$/.test(callee)) {
    diagnostics.push(`${path}: TypeScript owns canonical hashing`)
  }
}

function parseTypeScriptProgram(files) {
  const virtualFiles = new Map()
  const sourceByPath = new Map()
  for (const [path, sourceText] of files) {
    const virtualPath = `/phase-boundary/${path}`
    const scriptKind = /x$/.test(path) ? ts.ScriptKind.TSX : ts.ScriptKind.TS
    const source = ts.createSourceFile(
      virtualPath,
      sourceText,
      ts.ScriptTarget.ESNext,
      true,
      scriptKind,
    )
    virtualFiles.set(virtualPath, { source, sourceText })
    sourceByPath.set(path, source)
  }
  const options = {
    module: ts.ModuleKind.ESNext,
    noLib: true,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    target: ts.ScriptTarget.ESNext,
  }
  const host = {
    fileExists: (candidate) => virtualFiles.has(candidate),
    getCanonicalFileName: (candidate) => candidate,
    getCurrentDirectory: () => '/',
    getDefaultLibFileName: () => '/lib.d.ts',
    getDirectories: () => [],
    getNewLine: () => '\n',
    getSourceFile: (candidate) => virtualFiles.get(candidate)?.source,
    readFile: (candidate) => virtualFiles.get(candidate)?.sourceText,
    resolveModuleNames: (moduleNames, containingFile) =>
      moduleNames.map((specifier) => resolveVirtualModule(specifier, containingFile, virtualFiles)),
    useCaseSensitiveFileNames: () => true,
    writeFile: () => {},
  }
  const program = ts.createProgram([...virtualFiles.keys()], options, host)
  return { checker: program.getTypeChecker(), sources: sourceByPath }
}

function resolveVirtualModule(specifier, containingFile, virtualFiles) {
  if (!specifier.startsWith('.')) return undefined
  const base = posix.resolve(posix.dirname(containingFile), specifier)
  const withoutJavaScriptExtension = base.replace(/\.(?:mjs|cjs|js|jsx)$/, '')
  for (const candidate of [
    base,
    `${withoutJavaScriptExtension}.ts`,
    `${withoutJavaScriptExtension}.tsx`,
    `${withoutJavaScriptExtension}/index.ts`,
    `${withoutJavaScriptExtension}/index.tsx`,
  ]) {
    if (!virtualFiles.has(candidate)) continue
    return {
      resolvedFileName: candidate,
      extension: candidate.endsWith('.tsx') ? ts.Extension.Tsx : ts.Extension.Ts,
      isExternalLibraryImport: false,
    }
  }
  return undefined
}

function createCapabilityResolver(sources, checker) {
  const aliases = new Map()
  const globalAliases = new Set()
  const globalCapabilities = new Map([
    ['AudioContext', 'AudioContext'],
    ['EventSource', 'EventSource'],
    ['MediaRecorder', 'MediaRecorder'],
    ['RTCPeerConnection', 'RTCPeerConnection'],
    ['SpeechRecognition', 'SpeechRecognition'],
    ['WebSocket', 'WebSocket'],
    ['XMLHttpRequest', 'XMLHttpRequest'],
    ['fetch', 'fetch'],
    ['webkitSpeechRecognition', 'webkitSpeechRecognition'],
  ])

  function symbolOf(identifier) {
    if (!ts.isIdentifier(identifier)) return undefined
    let symbol = checker.getSymbolAtLocation(identifier)
    const visited = new Set()
    while (symbol !== undefined && (symbol.flags & ts.SymbolFlags.Alias) !== 0) {
      if (visited.has(symbol)) break
      visited.add(symbol)
      const target = checker.getAliasedSymbol(symbol)
      if (target === symbol) break
      symbol = target
    }
    return symbol
  }

  function unwrap(expression) {
    let current = expression
    while (
      ts.isAsExpression(current) ||
      ts.isNonNullExpression(current) ||
      ts.isParenthesizedExpression(current) ||
      ts.isSatisfiesExpression(current) ||
      ts.isTypeAssertionExpression(current)
    ) {
      current = current.expression
    }
    return current
  }

  function isUnshadowedGlobal(identifier, allowedNames) {
    const symbol = symbolOf(identifier)
    return (
      allowedNames.has(identifier.text) &&
      (symbol === undefined || (symbol.declarations?.length ?? 0) === 0)
    )
  }

  function isGlobalObject(expression) {
    const current = unwrap(expression)
    if (!ts.isIdentifier(current)) return false
    const symbol = symbolOf(current)
    return (
      (/^(?:globalThis|self|window)$/.test(current.text) &&
        (symbol === undefined || (symbol.declarations?.length ?? 0) === 0)) ||
      (symbol !== undefined && globalAliases.has(symbol))
    )
  }

  function isObjectConstructor(expression) {
    const current = unwrap(expression)
    if (ts.isIdentifier(current)) {
      return isUnshadowedGlobal(current, new Set(['Object']))
    }
    return (
      propertyNameOf(current) === 'Object' &&
      (ts.isPropertyAccessExpression(current) || ts.isElementAccessExpression(current)) &&
      isGlobalObject(current.expression)
    )
  }

  function propertyNameOf(expression) {
    if (ts.isPropertyAccessExpression(expression)) return expression.name.text
    if (
      ts.isElementAccessExpression(expression) &&
      expression.argumentExpression &&
      (ts.isStringLiteral(expression.argumentExpression) ||
        ts.isNoSubstitutionTemplateLiteral(expression.argumentExpression))
    ) {
      return expression.argumentExpression.text
    }
    return undefined
  }

  function of(expression) {
    const current = unwrap(expression)
    if (ts.isIdentifier(current)) {
      const symbol = symbolOf(current)
      if (symbol !== undefined && aliases.has(symbol)) return aliases.get(symbol)
      if (symbol === undefined || (symbol.declarations?.length ?? 0) === 0) {
        return globalCapabilities.get(current.text)
      }
      return undefined
    }
    if (ts.isPropertyAccessExpression(current) || ts.isElementAccessExpression(current)) {
      const property = propertyNameOf(current)
      if (property !== undefined && isGlobalObject(current.expression)) {
        return globalCapabilities.get(property)
      }
      if (property === 'assign' && isObjectConstructor(current.expression)) {
        return 'Object.assign'
      }
      if (
        property === 'getUserMedia' &&
        /^(?:globalThis\.)?navigator\.mediaDevices$/.test(current.expression.getText())
      ) {
        return 'getUserMedia'
      }
      return undefined
    }
    if (
      ts.isCallExpression(current) &&
      (ts.isPropertyAccessExpression(current.expression) ||
        ts.isElementAccessExpression(current.expression)) &&
      propertyNameOf(current.expression) === 'bind'
    ) {
      return of(current.expression.expression)
    }
    if (ts.isBinaryExpression(current) && current.operatorToken.kind === ts.SyntaxKind.EqualsToken) {
      return of(current.right)
    }
    return undefined
  }

  function recordIdentifier(identifier, capability) {
    const symbol = symbolOf(identifier)
    if (symbol === undefined || aliases.has(symbol)) return false
    aliases.set(symbol, capability)
    return true
  }

  function recordGlobalAlias(identifier) {
    const symbol = symbolOf(identifier)
    if (symbol === undefined || globalAliases.has(symbol)) return false
    globalAliases.add(symbol)
    return true
  }

  function recordBinding(name, initializer) {
    if (!initializer) return false
    if (ts.isIdentifier(name)) {
      let changed = false
      const capability = of(initializer)
      if (capability !== undefined) changed = recordIdentifier(name, capability) || changed
      if (isGlobalObject(initializer)) changed = recordGlobalAlias(name) || changed
      return changed
    }
    if (!ts.isObjectBindingPattern(name)) return false

    let changed = false
    for (const element of name.elements) {
      if (!ts.isIdentifier(element.name)) continue
      const property = element.propertyName
        ? ts.isIdentifier(element.propertyName) || ts.isStringLiteral(element.propertyName)
          ? element.propertyName.text
          : undefined
        : element.name.text
      if (property === undefined) continue
      let capability
      if (isGlobalObject(initializer)) capability = globalCapabilities.get(property)
      if (isObjectConstructor(initializer) && property === 'assign') capability = 'Object.assign'
      if (capability !== undefined) {
        changed = recordIdentifier(element.name, capability) || changed
      }
    }
    return changed
  }

  let changed
  do {
    changed = false
    function collect(node) {
      if (ts.isVariableDeclaration(node)) {
        changed = recordBinding(node.name, node.initializer) || changed
      } else if (
        ts.isBinaryExpression(node) &&
        node.operatorToken.kind === ts.SyntaxKind.EqualsToken &&
        ts.isIdentifier(node.left)
      ) {
        const capability = of(node.right)
        if (capability !== undefined) {
          changed = recordIdentifier(node.left, capability) || changed
        }
        if (isGlobalObject(node.right)) changed = recordGlobalAlias(node.left) || changed
      }
      ts.forEachChild(node, collect)
    }
    for (const source of sources) collect(source)
  } while (changed)

  return { of }
}

function capabilityLabel(capability) {
  if (capability === 'fetch') return 'backend'
  if (capability === 'getUserMedia') return 'voice'
  return capability
}

function isSemanticTarget(expression) {
  const target = expression.getText()
  return /(?:^|\.)(?:audit|document|flowDocument|snapshot|transaction)$/i.test(target)
}

function validateAssignment(path, source, left, diagnostics) {
  if (!ts.isPropertyAccessExpression(left)) return
  const property = left.name.text
  const base = left.expression.getText(source)
  if (/^(?:contentEditable|innerHTML|outerHTML)$/.test(property)) {
    diagnostics.push(`${path}: unsafe or deferred DOM assignment ${property}`)
  }
  if (
    /^(?:audit|canonicalHash|content|history|revision|schemaVersion)$/.test(property) &&
    /(?:^|\.)(?:audit|document|flowDocument|snapshot|transaction)$/i.test(base)
  ) {
    diagnostics.push(`${path}: TypeScript mutates semantic state ${base}.${property}`)
  }
}

function validateWasmBoundary(source, diagnostics) {
  const items = rustWasmItems(source)
  const exports = []
  for (const item of items) {
    if (item.kind === 'struct') {
      diagnostics.push('crates/flow-wasm/src/lib.rs: mutable WASM struct export is forbidden')
      continue
    }
    if (item.kind !== 'fn') {
      diagnostics.push(
        `crates/flow-wasm/src/lib.rs: unsupported annotated WASM ${item.kind} item`,
      )
      continue
    }
    exports.push(item.exportName)
    if (!allowedWasmExports.has(item.exportName)) {
      diagnostics.push(`crates/flow-wasm/src/lib.rs: unexpected WASM export ${item.exportName}`)
    }
    if (!hasTypedWasmSignature(item)) {
      diagnostics.push(
        `crates/flow-wasm/src/lib.rs: ${item.exportName} has an invalid typed WASM signature`,
      )
    }
  }
  for (const name of allowedWasmExports) {
    const count = exports.filter((candidate) => candidate === name).length
    if (count === 0) {
      diagnostics.push(`crates/flow-wasm/src/lib.rs: missing typed WASM export ${name}`)
    } else if (count > 1) {
      diagnostics.push(`crates/flow-wasm/src/lib.rs: duplicate WASM export ${name}`)
    }
  }
}

function rustWasmItems(source) {
  const tokens = rustTokens(source)
  const items = []
  const attributes = []
  let braceDepth = 0
  let index = 0

  while (index < tokens.length) {
    const token = tokens[index]
    if (token === '{') {
      braceDepth += 1
      index += 1
      continue
    }
    if (token === '}') {
      braceDepth = Math.max(0, braceDepth - 1)
      index += 1
      continue
    }
    if (braceDepth > 0) {
      index += 1
      continue
    }
    if (token === '#' && tokens[index + 1] === '[') {
      const end = matchingRustDelimiter(tokens, index + 1, '[', ']')
      if (end === undefined) break
      attributes.push(tokens.slice(index + 2, end))
      index = end + 1
      continue
    }

    const item = parseRustItem(tokens, index)
    if (item !== undefined) {
      if (attributes.some((attribute) => attribute.includes('wasm_bindgen'))) {
        items.push({
          ...item,
          exportName: wasmExportName(attributes, item.rustName),
        })
      }
      attributes.length = 0
      index = item.end
      continue
    }

    attributes.length = 0
    index += 1
  }

  return items
}

function parseRustItem(tokens, start) {
  let cursor = start
  if (tokens[cursor] === 'pub') {
    cursor += 1
    if (tokens[cursor] === '(') {
      const visibilityEnd = matchingRustDelimiter(tokens, cursor, '(', ')')
      if (visibilityEnd === undefined) return undefined
      cursor = visibilityEnd + 1
    }
  }
  while (/^(?:async|const|default|unsafe)$/.test(tokens[cursor] ?? '')) cursor += 1
  if (tokens[cursor] === 'extern' && tokens[cursor + 1] === '<literal>') cursor += 2

  const kind = tokens[cursor]
  if (!/^(?:enum|fn|impl|mod|static|struct|trait|type|union)$/.test(kind ?? '')) {
    return undefined
  }
  const rustName = /^[A-Za-z_][A-Za-z0-9_]*$/.test(tokens[cursor + 1] ?? '')
    ? tokens[cursor + 1]
    : '<anonymous>'
  let end = cursor + 1
  while (end < tokens.length && tokens[end] !== '{' && tokens[end] !== ';') end += 1
  return { end, header: tokens.slice(start, end), kind, rustName }
}

function wasmExportName(attributes, rustName) {
  for (const attribute of attributes) {
    const wasmIndex = attribute.indexOf('wasm_bindgen')
    if (wasmIndex < 0) continue
    for (let index = wasmIndex + 1; index < attribute.length - 2; index += 1) {
      if (attribute[index] !== 'js_name' || attribute[index + 1] !== '=') continue
      const candidate = attribute[index + 2]
      return /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(candidate)
        ? candidate
        : '<invalid-js-name>'
    }
  }
  return rustName
}

function hasTypedWasmSignature(item) {
  const tokens = item.header
  if (
    tokens[0] !== 'pub' ||
    tokens[1] !== 'fn' ||
    tokens[2] !== item.rustName ||
    tokens[3] !== '('
  ) {
    return false
  }
  const parametersEnd = matchingRustDelimiter(tokens, 3, '(', ')')
  if (parametersEnd === undefined) return false
  const parameters = tokens.slice(4, parametersEnd)
  if (item.exportName === 'stage_asset') {
    return parameters.join('') === 'bytes:&[u8],request:JsValue'
  }
  if (
    parameters[0] !== 'request' ||
    parameters[1] !== ':' ||
    !isJsValueType(parameters.slice(2))
  ) {
    return false
  }
  return (
    tokens[parametersEnd + 1] === '->' &&
    isJsValueType(tokens.slice(parametersEnd + 2))
  )
}

function isJsValueType(tokens) {
  return new Set([
    'JsValue',
    'wasm_bindgen::JsValue',
    'wasm_bindgen::prelude::JsValue',
  ]).has(tokens.join(''))
}

function matchingRustDelimiter(tokens, start, open, close) {
  let depth = 0
  for (let index = start; index < tokens.length; index += 1) {
    if (tokens[index] === open) depth += 1
    if (tokens[index] === close) {
      depth -= 1
      if (depth === 0) return index
    }
  }
  return undefined
}

function rustTokens(source) {
  const tokens = []
  let index = 0
  while (index < source.length) {
    if (/\s/.test(source[index])) {
      index += 1
      continue
    }
    if (source.startsWith('//', index)) {
      index = source.indexOf('\n', index + 2)
      if (index < 0) break
      continue
    }
    if (source.startsWith('/*', index)) {
      index = skipRustBlockComment(source, index)
      continue
    }

    const rawString = source.slice(index).match(/^(?:b|c)?r(#+)?"/)
    if (rawString) {
      const hashes = rawString[1] ?? ''
      const terminator = `"${hashes}`
      const contentStart = index + rawString[0].length
      const end = source.indexOf(terminator, contentStart)
      tokens.push('<literal>')
      index = end < 0 ? source.length : end + terminator.length
      continue
    }
    if (source[index] === '"' || /^[bc]"/.test(source.slice(index, index + 2))) {
      index = skipRustQuoted(source, source[index] === '"' ? index : index + 1)
      tokens.push('<literal>')
      continue
    }

    const identifier = source.slice(index).match(/^[A-Za-z_][A-Za-z0-9_]*/)?.[0]
    if (identifier) {
      tokens.push(identifier)
      index += identifier.length
      continue
    }
    const pair = source.slice(index, index + 2)
    if (/^(?:::|->|=>)$/.test(pair)) {
      tokens.push(pair)
      index += 2
      continue
    }
    tokens.push(source[index])
    index += 1
  }
  return tokens
}

function skipRustBlockComment(source, start) {
  let depth = 1
  let index = start + 2
  while (index < source.length && depth > 0) {
    if (source.startsWith('/*', index)) {
      depth += 1
      index += 2
    } else if (source.startsWith('*/', index)) {
      depth -= 1
      index += 2
    } else {
      index += 1
    }
  }
  return index
}

function skipRustQuoted(source, quoteIndex) {
  let index = quoteIndex + 1
  while (index < source.length) {
    if (source[index] === '\\') {
      index += 2
    } else if (source[index] === '"') {
      return index + 1
    } else {
      index += 1
    }
  }
  return source.length
}

function assertGateContract(gate) {
  const required = [
    'accessibility-focused',
    'boundary-contract',
    'browser-suites',
    'dependency-locks',
    'dependency-provenance',
    'dependency-provenance-live',
    'node-regressions',
    'recovery-benchmark',
    'rust-clippy',
    'rust-format',
    'rust-tests',
    'typescript',
    'unit-suites',
    'wasm-build',
    'wasm-target',
  ]
  assert.deepEqual(gate.phaseOneSteps.map(({ id }) => id).sort(), required)
  assert.equal(gate.deterministicReplayRounds, 2)
  assert.deepEqual(
    gate.deterministicReplaySteps.map(({ id }) => id).sort(),
    ['canonical-replay', 'migration-replay'],
  )
  for (const step of [...gate.phaseOneSteps, ...gate.deterministicReplaySteps]) {
    assert.equal(step.shell, undefined)
    assert.equal(step.args.some((argument) => /(?:^|-)watch$/.test(argument)), false)
  }
  assert.equal(gate.resolveChildExitCode({ status: 17 }), 17)
  assert.equal(gate.resolveChildExitCode({ status: null, error: new Error('missing') }), 1)
  assert.doesNotThrow(() =>
    gate.validateTerminalBenchmarkEvidence({
      passedExists: true,
      blockerExists: false,
      report: { passed: true, blocked: false },
    }),
  )
  assert.throws(() =>
    gate.validateTerminalBenchmarkEvidence({
      passedExists: true,
      blockerExists: true,
      report: { passed: true, blocked: false },
    }),
  )
  assert.throws(() =>
    gate.validateTerminalBenchmarkEvidence({
      passedExists: false,
      blockerExists: false,
      report: null,
    }),
  )
}

function cargoDependencies(source) {
  let dependencySection = false
  const dependencies = []
  for (const line of source.split('\n')) {
    const section = line.match(/^\s*\[([^\]]+)\]\s*$/)?.[1]
    if (section !== undefined) {
      dependencySection = /(?:^|\.)dependencies$/.test(section)
      continue
    }
    if (!dependencySection) continue
    const name = line.match(/^\s*([A-Za-z0-9_-]+)\s*=/)?.[1]
    if (name !== undefined) dependencies.push(name)
  }
  return dependencies
}

function productionFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) return productionFiles(path)
    return entry.isFile() && /\.[cm]?tsx?$/.test(entry.name) ? [path] : []
  })
}

function packageRoot(specifier) {
  if (specifier.startsWith('@')) return specifier.split('/').slice(0, 2).join('/')
  return specifier.split('/')[0]
}

function hasDeclarationName(node) {
  return (
    (ts.isFunctionDeclaration(node) ||
      ts.isMethodDeclaration(node) ||
      ts.isClassDeclaration(node) ||
      ts.isVariableDeclaration(node)) &&
    node.name !== undefined &&
    ts.isIdentifier(node.name)
  )
}

function isAssignment(kind) {
  return kind >= ts.SyntaxKind.FirstAssignment && kind <= ts.SyntaxKind.LastAssignment
}
