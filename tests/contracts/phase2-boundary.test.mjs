import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import test from 'node:test'

import {
  assertPhaseTwoParityBoundary,
  boundaryDiagnostics,
  loadWorkspaceSnapshot,
} from './phase1-boundary.test.mjs'

const projectRoot = resolve(import.meta.dirname, '../..')

test('Phase 2 admits the checked-in editor and parity contract without opening ownership gaps', () => {
  const snapshot = loadWorkspaceSnapshot(projectRoot)
  assert.deepEqual(boundaryDiagnostics(snapshot, { phase: 2 }), [])
  assertPhaseTwoParityBoundary(projectRoot, snapshot)
})

test('Phase 2 rejects semantic JSON, unsafe DOM, deferred scope, and mutable WASM fixtures', () => {
  const fixture = loadWorkspaceSnapshot(projectRoot)
  fixture.typescript = new Map(fixture.typescript)
  fixture.typescript.set(
    'web/src/editor/unsafe-contract.tsx',
    `
      import React from 'react'
      export function unsafeEditor(canonicalJson, root) {
        const documentState = JSON.parse(canonicalJson)
        documentState.revision += 1
        root.innerHTML = canonicalJson
        document.createElement('canvas')
        navigator.mediaDevices.getUserMedia({ audio: true })
        fetch('/api/documents')
        return <form contentEditable dangerouslySetInnerHTML={{ __html: canonicalJson }} />
      }
    `,
  )
  fixture.typescript.set('web/src/editor/layout-engine.ts', 'export const deferredLayout = true')
  fixture.wasmSource += `
    #[wasm_bindgen]
    pub struct MutableFlowDocument { revision: u64 }
    #[wasm_bindgen]
    pub fn mutable_document_handle() -> JsValue { JsValue::NULL }
  `

  const diagnostics = boundaryDiagnostics(fixture, { phase: 2 }).join('\n')
  assert.match(diagnostics, /semantic owner|semantic JSON|semantic state/i)
  assert.match(diagnostics, /innerHTML|unsafe or deferred DOM|contenteditable/i)
  assert.match(diagnostics, /canvas|form|voice|backend/i)
  assert.match(diagnostics, /deferred web path/i)
  assert.match(diagnostics, /mutable WASM struct|unexpected WASM export/i)
})

test('the checked-in parity artifact explicitly covers the complete image and table lifecycle', () => {
  const contract = JSON.parse(
    readFileSync(resolve(projectRoot, 'tests/contracts/phase2-command-parity.json'), 'utf8'),
  )
  const commandTypes = new Set(contract.commands.map(({ commandType }) => commandType))
  for (const commandType of [
    'insertImage',
    'replaceImage',
    'setImageAccessibility',
    'removeImage',
    'insertTable',
    'addTableRow',
    'removeTableRow',
    'addTableColumn',
    'removeTableColumn',
    'setTableHeaderRow',
    'removeTable',
    'insertPageBreak',
    'removePageBreak',
  ]) {
    assert.equal(commandTypes.has(commandType), true, `${commandType} must be catalogued`)
  }
})
