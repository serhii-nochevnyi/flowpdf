import { expect, test } from 'vitest'

import { mountFoundationInspector } from '../src/foundation-inspector'

test('walking-skeleton: creates, mutates, commits, reloads, and renders only safe proof metadata', async () => {
  const root = document.createElement('div')
  document.body.replaceChildren(root)

  const inspector = await mountFoundationInspector(root)
  const create = root.querySelector<HTMLButtonElement>('[data-action="create-sample"]')
  expect(create).not.toBeNull()

  create?.click()
  await inspector.whenIdle()

  const beforeSave = inspector.snapshot()
  expect(beforeSave.contentNodeCount).toBeGreaterThan(0)
  expect(beforeSave.fieldCount).toBe(6)
  expect(beforeSave.assetCount).toBeGreaterThan(0)
  expect(beforeSave.revisionProvenance.lineage.kind).toBe('created')
  expect(beforeSave.revision).toBe(1)
  expect(JSON.stringify(beforeSave)).not.toMatch(/Український|English|typed mutation/i)

  root.querySelector<HTMLButtonElement>('[data-action="apply-mutation"]')?.click()
  expect(root.querySelector('[data-durability-status]')?.textContent).not.toContain('Збережено')
  await inspector.whenIdle()
  const committed = inspector.snapshot()
  expect(committed.revision).toBe(2)
  expect(committed.hash).not.toBe(beforeSave.hash)
  expect(root.querySelector('[data-durability-status]')?.textContent).toContain('Збережено локально')

  const recovered = await inspector.reloadFromStorage()
  expect(recovered.revision).toBe(committed.revision)
  expect(recovered.hash).toBe(committed.hash)
  expect(root.querySelector('[data-provenance]')?.textContent).toMatch(/створено/i)
  expect(root.textContent).not.toMatch(/Український|English|typed mutation/i)
})
