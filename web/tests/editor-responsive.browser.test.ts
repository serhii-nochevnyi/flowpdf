import { page } from 'vitest/browser'
import { expect, test } from 'vitest'

import '../src/styles.css'
import '../src/editor/editor.css'
import {
  mountEditorApp,
  type EditorController,
  type EditorLocale,
} from '../src/editor/editor-app.js'

async function settle(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
}

async function openOlder(root: HTMLElement, controller: EditorController): Promise<void> {
  root.querySelector<HTMLButtonElement>('[data-action="editor-open-older"]')?.click()
  await settle(controller)
}

async function insertWideTable(root: HTMLElement, controller: EditorController): Promise<void> {
  const accepted = controller.snapshot().accepted
  if (accepted === null) throw new Error('accepted state is required')
  const capability = accepted.editor.view.capabilities.find((item) => item.name === 'insertTable')
  if (capability?.placement === null || capability?.placement === undefined) {
    throw new Error('table placement capability is required')
  }
  await controller.structuralCommand(
    {
      type: 'insertTable',
      placement: capability.placement,
      rows: 1,
      columns: 20,
      headerRow: true,
    },
    'ui',
  )
  await settle(controller)
  expect(root.querySelector('[data-block-kind="table"]')).not.toBeNull()
}

function assertMinimumTargetSize(root: HTMLElement): void {
  const controls = [
    ...root.querySelectorAll<HTMLElement>(
      'button, select, input, textarea, summary, [role="button"]',
    ),
  ].filter((element) => element.dataset.editorInputHost === undefined)
  for (const control of controls) {
    const style = getComputedStyle(control)
    if (style.display === 'none' || style.visibility === 'hidden') continue
    const rect = control.getBoundingClientRect()
    expect(rect.height, `${control.outerHTML} must have a 44px target`).toBeGreaterThanOrEqual(44)
    expect(rect.width, `${control.outerHTML} must have a 44px target`).toBeGreaterThanOrEqual(44)
  }
}

export async function assertResponsiveOverflow(
  root: HTMLElement,
  controller: EditorController,
  locale: EditorLocale,
): Promise<void> {
  expect(root.querySelector('[data-formatting-toolbar]')?.getAttribute('aria-label')).toBe(
    locale === 'uk' ? 'Форматування' : 'Formatting',
  )
  expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(window.innerWidth + 1)
  expect(document.body.scrollWidth).toBeLessThanOrEqual(window.innerWidth + 1)
  assertMinimumTargetSize(root)
  const table = root.querySelector<HTMLElement>('[data-block-kind="table"]')
  const wrapper = table?.closest<HTMLElement>('.editor-table-wrapper')
  if (wrapper === null || wrapper === undefined) throw new Error('table overflow wrapper is required')
  expect(wrapper.getAttribute('data-block-kind')).toBe('table')
  expect(wrapper.scrollWidth).toBeGreaterThanOrEqual(wrapper.clientWidth)
  expect(wrapper.getAttribute('aria-label')).toBeNull()
  const accessibleTable = wrapper.querySelector('table')
  expect(accessibleTable?.getAttribute('aria-label')).toBe(
    locale === 'uk' ? 'Таблиця документа' : 'Document table',
  )
  expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(window.innerWidth + 1)
  expect(document.body.scrollWidth).toBeLessThanOrEqual(window.innerWidth + 1)
}

for (const viewport of [320, 1280] as const) {
  for (const locale of ['uk', 'en'] as const) {
    test(`responsive editor contract stays bounded at ${viewport}px in ${locale}`, async () => {
      await page.viewport(viewport, 1000)
      const root = document.createElement('div')
      document.body.replaceChildren(root)
      const controller = await mountEditorApp(root, {
        databaseName: `flowpdf-editor-responsive-${viewport}-${locale}-${crypto.randomUUID()}`,
        locale,
        clock: () => new Date('2026-09-15T00:10:00Z'),
      })
      await settle(controller)
      await openOlder(root, controller)
      await insertWideTable(root, controller)
      await assertResponsiveOverflow(root, controller, locale)
      if (viewport === 320) {
        expect(root.querySelector('[data-formatting-more]')?.hasAttribute('open')).toBe(false)
        expect(root.querySelector('[data-formatting-more] summary')?.textContent).toContain(
          locale === 'uk' ? 'Ще форматування' : 'More formatting',
        )
      } else {
        expect(root.querySelector('[data-formatting-more]')?.hasAttribute('open')).toBe(true)
      }
      controller.dispose()
    })
  }
}
