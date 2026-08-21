import assert from 'node:assert/strict'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import test from 'node:test'
import ts from 'typescript'

const projectRoot = resolve(import.meta.dirname, '../..')
const allowedWasmExports = new Set([
  'apply_command',
  'commit_record',
  'create_sample',
  'migrate_document',
  'open_document',
  'plan_persistence_commit',
  'query_document',
  'recover_document',
  'recover_document_audited',
  'redo',
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
const forbiddenWebPathSegment = /(?:^|\/)(?:auth|backend|collaboration|editor|forms?|layout|pdf|voice)(?:\/|\.|$)/i
const semanticOwnerName = /^(?:apply|canonicalize|hash|migrate|mutate|recover|redact|replay|serialize)(?:Flow)?(?:Audit|Document|Revision|Transaction)/i

test('the checked-in Phase 1 workspace preserves deferred scope and Rust semantic ownership', async () => {
  const snapshot = loadWorkspaceSnapshot(projectRoot)
  assert.deepEqual(boundaryDiagnostics(snapshot), [])

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

function loadWorkspaceSnapshot(root) {
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
      .map((name) => `#[wasm_bindgen]\npub fn ${name}(request: JsValue) -> JsValue { request }`)
      .join('\n'),
  }
}

function boundaryDiagnostics(snapshot) {
  const diagnostics = []
  validatePackageManifest(snapshot.packageJson, diagnostics)
  for (const [path, source] of snapshot.cargoManifests) {
    for (const dependency of cargoDependencies(source)) {
      if (forbiddenDirectPackages.has(dependency)) {
        diagnostics.push(`${path}: deferred Rust dependency ${dependency}`)
      }
    }
  }
  for (const [path, source] of snapshot.typescript) {
    if (forbiddenWebPathSegment.test(path)) {
      diagnostics.push(`${path}: deferred web path entered Phase 1`)
    }
    validateTypeScript(path, source, diagnostics)
  }
  validateWasmBoundary(snapshot.wasmSource, diagnostics)
  return diagnostics.sort()
}

function validatePackageManifest(packageJson, diagnostics) {
  const direct = {
    ...packageJson.dependencies,
    ...packageJson.devDependencies,
    ...packageJson.optionalDependencies,
  }
  for (const dependency of Object.keys(direct)) {
    if (forbiddenDirectPackages.has(dependency)) {
      diagnostics.push(`package.json: deferred runtime dependency ${dependency}`)
    }
  }
  for (const [name, command] of Object.entries(packageJson.scripts ?? {})) {
    if (/\b(?:drizzle-kit|next|prisma|react-scripts|schema\s+push|vite)\b/i.test(command)) {
      diagnostics.push(`package.json script ${name}: deferred runtime command`)
    }
    if (/\b(?:--watch|watch)\b/.test(command)) {
      diagnostics.push(`package.json script ${name}: watch mode is not deterministic`)
    }
  }
}

function validateTypeScript(path, sourceText, diagnostics) {
  const source = ts.createSourceFile(path, sourceText, ts.ScriptTarget.ESNext, true)
  if (source.parseDiagnostics.length > 0) {
    diagnostics.push(`${path}: TypeScript source does not parse`)
    return
  }

  function visit(node) {
    if (ts.isImportDeclaration(node) && ts.isStringLiteral(node.moduleSpecifier)) {
      const root = packageRoot(node.moduleSpecifier.text)
      if (forbiddenRuntimeImports.has(root)) {
        diagnostics.push(`${path}: deferred runtime import ${node.moduleSpecifier.text}`)
      }
    }
    if (
      ts.isJsxElement(node) ||
      ts.isJsxSelfClosingElement(node) ||
      ts.isJsxFragment(node)
    ) {
      diagnostics.push(`${path}: JSX/React editor surface is deferred`)
    }
    if (hasDeclarationName(node) && semanticOwnerName.test(node.name.text)) {
      diagnostics.push(`${path}: TypeScript semantic owner ${node.name.text}`)
    }
    if (ts.isCallExpression(node)) {
      validateCall(path, source, node, diagnostics)
    }
    if (ts.isNewExpression(node)) {
      const name = node.expression.getText(source)
      if (/^(?:AudioContext|EventSource|MediaRecorder|RTCPeerConnection|SpeechRecognition|WebSocket|webkitSpeechRecognition)$/.test(name)) {
        diagnostics.push(`${path}: deferred ${name} runtime surface`)
      }
    }
    if (ts.isBinaryExpression(node) && isAssignment(node.operatorToken.kind)) {
      validateAssignment(path, source, node.left, diagnostics)
    }
    ts.forEachChild(node, visit)
  }
  visit(source)
}

function validateCall(path, source, call, diagnostics) {
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
  if (/^(?:fetch|navigator\.mediaDevices\.getUserMedia)$/.test(callee)) {
    diagnostics.push(
      `${path}: deferred ${callee === 'fetch' ? 'backend' : 'voice'} runtime surface`,
    )
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
  const exports = [...source.matchAll(/#\[wasm_bindgen(?:\([^\]]*\))?\]\s*pub\s+(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)/g)].map(
    (match) => match[1],
  )
  for (const name of exports) {
    if (!allowedWasmExports.has(name)) {
      diagnostics.push(`crates/flow-wasm/src/lib.rs: unexpected WASM export ${name}`)
    }
  }
  for (const name of allowedWasmExports) {
    if (!exports.includes(name)) {
      diagnostics.push(`crates/flow-wasm/src/lib.rs: missing typed WASM export ${name}`)
    }
  }
  if (/#\[wasm_bindgen(?:\([^\]]*\))?\]\s*pub\s+struct\b/.test(source)) {
    diagnostics.push('crates/flow-wasm/src/lib.rs: mutable WASM struct export is forbidden')
  }
}

function assertGateContract(gate) {
  const required = [
    'accessibility-focused',
    'boundary-contract',
    'browser-suites',
    'dependency-locks',
    'dependency-provenance',
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
