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
  expect(beforeSave.documentSummary).toContain('Український')
  expect(beforeSave.documentSummary).toContain('English')
  expect(beforeSave.revision).toBe(1)
  expect(beforeSave.audit.every((entry) => !JSON.stringify(entry).match(/Український|English|typed mutation/i))).toBe(true)

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
  expect(root.querySelector('[data-audit]')?.textContent).not.toContain('Український')
})
