import { expect, test } from 'vitest'

import {
  mountFoundationInspector,
  type FoundationInspectorController,
} from '../src/foundation-inspector'

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
  expect(inspector.snapshot()).toEqual(applied)

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
  expect(redone.audit.map(({ newRevision }) => newRevision)).toEqual([1, 2, 3, 4])

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

  await clickAndWait(inspector, root, 'recover')
  const recovered = inspector.snapshot()
  expect(recovered.revision).toBe(redone.revision)
  expect(recovered.hash).toBe(redone.hash)
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
  expect(migrated.schemaVersion).toBe(1)
  expect(migrated.revisionProvenance.lineage).toMatchObject({
    kind: 'migrated',
    sourceSchemaVersion: 0,
    currentSchemaVersion: 1,
  })
  expect(migrated.audit.map(({ action }) => action.type)).toEqual(['migration'])
  expect(root.querySelector('[data-provenance]')?.textContent).toMatch(/схеми 0.*схеми 1/i)
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
  expect(await remounted.reloadFromStorage()).toEqual(migrated)
})
