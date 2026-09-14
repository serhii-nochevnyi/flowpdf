import { expect, test } from 'vitest'

import init, {
  migrate_document,
  query_document,
} from '../generated/flow_wasm.js'
import {
  IndexedDbDocumentStore,
  type MigrationPersistenceCommitDto,
} from '../persistence/indexeddb-store'
import {
  mountFoundationInspector,
  type FoundationInspectorController,
} from '../src/foundation-inspector'
import {
  mountEditorApp,
  type EditorAppSnapshot,
  type EditorController,
} from '../src/editor/editor-app'

const NON_EMPTY_ASSET_BYTES = [97, 98, 99] as const
const NON_EMPTY_ASSET_HASH =
  'blake3:6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85'

interface ApiResponse<T> {
  readonly ok: boolean
  readonly value: T | null
  readonly error: { readonly code: string } | null
}

interface CanonicalAssetDescriptor {
  readonly contentHash: string
  readonly byteLength: number
}

interface CanonicalFixture {
  readonly schemaVersion: number
  readonly assets: readonly CanonicalAssetDescriptor[]
  readonly provenance: unknown
  readonly [key: string]: unknown
}

interface MigrationResultDto {
  readonly canonicalJson: string
  readonly canonicalHash: string
  readonly commit: MigrationPersistenceCommitDto | null
}

interface RecoverResultDto {
  readonly session: {
    readonly canonicalJson: string
    readonly canonicalHash: string
    readonly documentId: string
    readonly revision: number
  }
  readonly view: {
    readonly assetCount: number
  }
}

interface MountOptionsContract {
  readonly databaseName: string
}

const mountWithOptions = mountFoundationInspector as unknown as (
  root: HTMLElement,
  options: MountOptionsContract,
) => Promise<FoundationInspectorController>

function action(root: HTMLElement, name: string): HTMLButtonElement {
  const control = root.querySelector<HTMLButtonElement>(`[data-action="${name}"]`)
  if (control === null) throw new Error(`missing ${name} control`)
  return control
}

async function clickAndWait(
  inspector: FoundationInspectorController,
  root: HTMLElement,
  name: string,
): Promise<void> {
  action(root, name).click()
  await inspector.whenIdle()
}

function unwrap<T>(response: unknown): T {
  const typed = response as ApiResponse<T>
  expect(typed.ok, typed.error?.code).toBe(true)
  expect(typed.value).not.toBeNull()
  if (typed.value === null) throw new Error(typed.error?.code ?? 'missing WASM value')
  return typed.value
}

test('walking-skeleton: completes conflict, undo/redo, save, reload, and recovery through real WASM and IndexedDB', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)

  const inspector = await mountWithOptions(root, {
    databaseName: 'flowpdf-lifecycle-browser-test',
  })

  expect(root.querySelector('[data-empty-document]')?.textContent).toMatch(/не відкрито/i)
  expect(root.querySelector('[data-audit-empty]')?.textContent).toMatch(/аудит/i)
  expect(root.querySelector('[data-provenance-unavailable]')?.textContent).toMatch(
    /наступній фазі/i,
  )

  await clickAndWait(inspector, root, 'create-sample')
  const created = inspector.snapshot()
  expect(created.revision).toBe(1)
  expect(created.audit.map(({ action }) => action.type)).toEqual(['create'])
  expect(JSON.stringify(created)).not.toMatch(/Український|English|typed mutation/i)

  await clickAndWait(inspector, root, 'apply-mutation')
  const applied = inspector.snapshot()
  expect(applied.revision).toBe(2)
  expect(applied.hash).not.toBe(created.hash)

  await clickAndWait(inspector, root, 'stale-command')
  expect(root.querySelector('[role="alert"]')?.textContent).toContain('FLOW_STALE_REVISION')
  const rejected = inspector.snapshot()
  expect(rejected.revision).toBe(applied.revision)
  expect(rejected.hash).toBe(applied.hash)
  expect(rejected.audit.at(-1)).toMatchObject({
    baseRevision: applied.revision,
    newRevision: applied.revision,
    action: { type: 'command', commandKind: 'insertText' },
    outcome: { kind: 'failure', code: 'staleRevision' },
  })
  expect(JSON.stringify(rejected.audit.at(-1))).not.toMatch(
    /stale diagnostic|typed mutation|canonicalJson|commandArguments/i,
  )

  await clickAndWait(inspector, root, 'undo')
  const undone = inspector.snapshot()
  expect(undone.revision).toBe(3)
  expect(undone.audit.at(-1)?.action).toEqual({
    type: 'command',
    commandKind: 'undo',
  })
  expect(action(root, 'redo').disabled).toBe(false)

  await clickAndWait(inspector, root, 'redo')
  const redone = inspector.snapshot()
  expect(redone.revision).toBe(4)
  expect(redone.audit.at(-1)?.action).toEqual({
    type: 'command',
    commandKind: 'redo',
  })
  expect(redone.audit.map(({ newRevision }) => newRevision)).toEqual([1, 2, 2, 3, 4])

  action(root, 'save').click()
  expect(root.querySelector('[data-durability-status]')?.textContent).not.toContain(
    'Збережено локально',
  )
  await inspector.whenIdle()
  expect(root.querySelector('[data-durability-status]')?.textContent).toContain(
    'Збережено локально — ревізія 4',
  )

  await clickAndWait(inspector, root, 'reload')
  const reloaded = inspector.snapshot()
  expect(reloaded.revision).toBe(redone.revision)
  expect(reloaded.hash).toBe(redone.hash)
  expect(reloaded.audit).toHaveLength(redone.audit.length + 1)
  expect(reloaded.audit.some(({ action }) => action.type === 'recovery')).toBe(true)

  await clickAndWait(inspector, root, 'recover')
  const recovered = inspector.snapshot()
  expect(recovered.revision).toBe(redone.revision)
  expect(recovered.hash).toBe(redone.hash)
  expect(recovered.audit).toHaveLength(reloaded.audit.length + 1)
  expect(root.textContent).not.toMatch(/Український|English|typed mutation/i)
})

test('walking-skeleton: opens the supported older fixture through Rust migration lineage', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)

  const inspector = await mountWithOptions(root, {
    databaseName: 'flowpdf-migration-browser-test',
  })
  await clickAndWait(inspector, root, 'open-older-schema')

  expect(root.querySelector('[role="alert"]')?.textContent).toBe('')

  const migrated = inspector.snapshot()
  expect(migrated.schemaVersion).toBe(2)
  expect(migrated.revisionProvenance.lineage).toMatchObject({
    kind: 'migrated',
    sourceSchemaVersion: 0,
    currentSchemaVersion: 2,
  })
  expect(migrated.audit.map(({ action }) => action.type)).toEqual(['migration'])
  expect(root.querySelector('[data-provenance]')?.textContent).toMatch(/схеми 0.*схеми 2/i)
  expect(root.querySelector('[data-provenance-unavailable]')?.textContent).toMatch(
    /наступній фазі/i,
  )
  expect(action(root, 'reload').disabled).toBe(false)

  const remountRoot = document.createElement('div')
  document.body.replaceChildren(remountRoot)
  const remounted = await mountWithOptions(remountRoot, {
    databaseName: 'flowpdf-migration-browser-test',
  })
  expect(action(remountRoot, 'open-last').disabled).toBe(false)
  const reopened = await remounted.reloadFromStorage()
  expect(reopened.revision).toBe(migrated.revision)
  expect(reopened.hash).toBe(migrated.hash)
  expect(reopened.revisionProvenance).toEqual(migrated.revisionProvenance)
  expect(reopened.audit).toHaveLength(migrated.audit.length + 1)
  expect(reopened.audit.some(({ action }) => action.type === 'recovery')).toBe(true)
})

test('walking-skeleton: preserves a validated non-empty asset through atomic migration commit and cold reopen', async () => {
  await init()

  const fixtureResponse = await fetch(new URL('../../fixtures/flowdoc/older.json', import.meta.url))
  expect(fixtureResponse.ok).toBe(true)
  const historical = (await fixtureResponse.json()) as CanonicalFixture
  const sourceDescriptor = historical.assets[0]
  if (sourceDescriptor === undefined) throw new Error('sample asset descriptor is required')

  const olderFixture = JSON.stringify({
    ...historical,
    schemaVersion: 0,
    assets: [
      {
        ...sourceDescriptor,
        contentHash: NON_EMPTY_ASSET_HASH,
        byteLength: NON_EMPTY_ASSET_BYTES.length,
      },
    ],
    provenance: { createdAt: '2026-08-14T00:00:00Z' },
  })
  const migrated = unwrap<MigrationResultDto>(
    migrate_document({
      canonicalJson: olderFixture,
      migrationId: '00000000-0000-4000-8000-000000009903',
      issuedAt: '2026-08-14T21:00:00Z',
      assets: [
        {
          recordFormatVersion: 1,
          contentHash: NON_EMPTY_ASSET_HASH,
          bytes: NON_EMPTY_ASSET_BYTES,
        },
      ],
    }),
  )
  expect(migrated.commit).not.toBeNull()
  if (migrated.commit === null) throw new Error('migration commit is required')

  const migratedDocument = JSON.parse(migrated.canonicalJson) as CanonicalFixture
  expect(migratedDocument.assets).toEqual([
    expect.objectContaining({
      contentHash: NON_EMPTY_ASSET_HASH,
      byteLength: NON_EMPTY_ASSET_BYTES.length,
    }),
  ])

  const databaseName = 'flowpdf-non-empty-asset-browser-test'
  const initialStore = new IndexedDbDocumentStore(databaseName)
  await initialStore.commitMigration(migrated.commit)

  const reconstructedRoot = document.createElement('div')
  document.body.replaceChildren(reconstructedRoot)
  const reconstructed = await mountWithOptions(reconstructedRoot, { databaseName })
  const reopened = await reconstructed.reloadFromStorage()
  expect(reopened.revision).toBe(migrated.commit.snapshot.revision)
  expect(reopened.hash).toBe(migrated.canonicalHash)
  expect(reopened.assetCount).toBe(1)

  const reconstructedStore = new IndexedDbDocumentStore(databaseName)
  const records = await reconstructedStore.loadRecords()
  expect(records.assets).toEqual([
    {
      recordFormatVersion: 1,
      contentHash: NON_EMPTY_ASSET_HASH,
      bytes: NON_EMPTY_ASSET_BYTES,
    },
  ])

  const queried = unwrap<RecoverResultDto>(query_document(records))
  expect(queried.session.documentId).toBe(migrated.commit.snapshot.documentId)
  expect(queried.session.revision).toBe(migrated.commit.snapshot.revision)
  expect(queried.session.canonicalHash).toBe(migrated.canonicalHash)
  expect(queried.view.assetCount).toBe(1)
  expect((JSON.parse(queried.session.canonicalJson) as CanonicalFixture).assets).toEqual(
    migratedDocument.assets,
  )
})

function editorAction(root: HTMLElement, name: string): HTMLButtonElement {
  const control = root.querySelector<HTMLButtonElement>(`[data-action="editor-${name}"]`)
  if (control === null) throw new Error(`missing editor-${name} control`)
  return control
}

async function settleEditor(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
}

function durableEditorState(snapshot: EditorAppSnapshot) {
  if (snapshot.accepted === null) throw new Error('accepted editor state is required')
  return {
    revision: snapshot.accepted.session.revision,
    hash: snapshot.accepted.session.canonicalHash,
    text: snapshot.accepted.editor.view.document.blocks
      .map((block) => block.text ?? block.nodeKind ?? '')
      .join('\n'),
    selection: snapshot.accepted.editor.view.selection,
    history: snapshot.accepted.session.history,
  }
}

test('Phase 2 paragraph tracer crosses migration, ReplaceSelection, persistence, and recovery', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)

  const options = {
    databaseName: 'flowpdf-phase2-paragraph-tracer-browser-test',
    clock: () => new Date('2026-08-14T22:00:00Z'),
  }
  const controller = await mountEditorApp(root, options)
  await settleEditor(controller)
  expect(root.querySelector('[data-empty-document]')?.textContent).toMatch(/не відкрито/i)

  await (editorAction(root, 'open-older').click(), settleEditor(controller))
  const migrated = controller.snapshot()
  expect(migrated.accepted?.session.revision).toBe(1)
  expect(migrated.accepted?.view.revisionProvenance.lineage).toMatchObject({
    kind: 'migrated',
  })
  const migratedState = durableEditorState(migrated)
  expect(migratedState.text).toMatch(/Український/)

  const acceptedBeforeEdit = migrated.accepted
  editorAction(root, 'replace').click()
  await settleEditor(controller)
  const edited = controller.snapshot()
  expect(edited.accepted).not.toBe(acceptedBeforeEdit)
  const editedState = durableEditorState(edited)
  expect(editedState.revision).toBe(2)
  expect(editedState.hash).not.toBe(migratedState.hash)
  expect(editedState.text).toMatch(/зміна \/ edit/)
  expect(editedState.selection.anchor).toEqual(editedState.selection.focus)
  expect(editedState.selection.anchor.utf16Offset).toBeGreaterThan(0)

  editorAction(root, 'undo').click()
  await settleEditor(controller)
  const undone = durableEditorState(controller.snapshot())
  expect(undone.revision).toBe(3)
  expect(undone.hash).not.toBe(editedState.hash)
  expect(undone.text).toBe(migratedState.text)

  editorAction(root, 'redo').click()
  await settleEditor(controller)
  const redoneSnapshot = controller.snapshot()
  const redone = durableEditorState(redoneSnapshot)
  expect(redone.revision).toBe(4)
  expect(redone.text).toMatch(/зміна \/ edit/)

  await controller.reloadFromStorage()
  await settleEditor(controller)
  expect(durableEditorState(controller.snapshot())).toEqual(redone)

  const remountRoot = document.createElement('div')
  document.body.replaceChildren(remountRoot)
  const remounted = await mountEditorApp(remountRoot, options)
  await settleEditor(remounted)
  expect(durableEditorState(remounted.snapshot())).toEqual(redone)

  await remounted.recoverFromStorage()
  await settleEditor(remounted)
  expect(durableEditorState(remounted.snapshot())).toEqual(redone)
  expect(remountRoot.querySelector('.diagnostics-panel > summary')?.textContent).toMatch(
    /Інспектор|Inspector/,
  )
  expect(remountRoot.querySelectorAll('[data-editor-document] [data-text-block]')).toHaveLength(2)
})
