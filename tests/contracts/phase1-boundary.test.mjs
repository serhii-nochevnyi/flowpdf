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
  'plan_standalone_audit',
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
  assert.match(diagnostics, /create_sample.*typed WASM signature|duplicate WASM export create_sample/i)
  assert.match(diagnostics, /mutable WASM struct/i)
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
  const { checker, source } = parseTypeScript(path, sourceText)
  if (source.parseDiagnostics.length > 0) {
    diagnostics.push(`${path}: TypeScript source does not parse`)
    return
  }
  const capabilities = createCapabilityResolver(source, checker)

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

function parseTypeScript(path, sourceText) {
  const virtualPath = `/phase-boundary/${path}`
  const scriptKind = /x$/.test(path) ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  const source = ts.createSourceFile(
    virtualPath,
    sourceText,
    ts.ScriptTarget.ESNext,
    true,
    scriptKind,
  )
  const options = {
    module: ts.ModuleKind.ESNext,
    noLib: true,
    noResolve: true,
    target: ts.ScriptTarget.ESNext,
  }
  const host = {
    fileExists: (candidate) => candidate === virtualPath,
    getCanonicalFileName: (candidate) => candidate,
    getCurrentDirectory: () => '/',
    getDefaultLibFileName: () => '/lib.d.ts',
    getDirectories: () => [],
    getNewLine: () => '\n',
    getSourceFile: (candidate) => (candidate === virtualPath ? source : undefined),
    readFile: (candidate) => (candidate === virtualPath ? sourceText : undefined),
    useCaseSensitiveFileNames: () => true,
    writeFile: () => {},
  }
  const program = ts.createProgram([virtualPath], options, host)
  return { checker: program.getTypeChecker(), source: program.getSourceFile(virtualPath) }
}

function createCapabilityResolver(source, checker) {
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
    return ts.isIdentifier(identifier) ? checker.getSymbolAtLocation(identifier) : undefined
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
        /^(?:globalThis\.)?navigator\.mediaDevices$/.test(current.expression.getText(source))
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
    collect(source)
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
    exports.push(item.name)
    if (!allowedWasmExports.has(item.name)) {
      diagnostics.push(`crates/flow-wasm/src/lib.rs: unexpected WASM export ${item.name}`)
    }
    if (!hasTypedWasmSignature(item)) {
      diagnostics.push(
        `crates/flow-wasm/src/lib.rs: ${item.name} has an invalid typed WASM signature`,
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
        items.push(item)
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
  const name = /^[A-Za-z_][A-Za-z0-9_]*$/.test(tokens[cursor + 1] ?? '')
    ? tokens[cursor + 1]
    : '<anonymous>'
  let end = cursor + 1
  while (end < tokens.length && tokens[end] !== '{' && tokens[end] !== ';') end += 1
  return { end, header: tokens.slice(start, end), kind, name }
}

function hasTypedWasmSignature(item) {
  const tokens = item.header
  if (
    tokens[0] !== 'pub' ||
    tokens[1] !== 'fn' ||
    tokens[2] !== item.name ||
    tokens[3] !== '('
  ) {
    return false
  }
  const parametersEnd = matchingRustDelimiter(tokens, 3, '(', ')')
  if (parametersEnd === undefined) return false
  const parameters = tokens.slice(4, parametersEnd)
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
