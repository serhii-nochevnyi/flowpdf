import { page } from 'vitest/browser'
import { expect, test } from 'vitest'
import { createElement } from 'react'
import { createRoot } from 'react-dom/client'

import '../src/styles.css'
import {
  mountEditorApp,
  type EditorController,
} from '../src/editor/editor-app.js'
import { FieldNavigation } from '../src/editor/field-navigation.js'
import type { EditorFieldReviewDto } from '../src/editor/editor-store.js'

async function settle(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
}

for (const locale of ['uk', 'en'] as const) {
  test(`editor accessibility exposes fields and shell semantics in ${locale}`, async () => {
    await page.viewport(1280, 1000)
    const root = document.createElement('div')
    document.body.replaceChildren(root)
    const controller = await mountEditorApp(root, {
      databaseName: `flowpdf-editor-accessibility-${locale}-${crypto.randomUUID()}`,
      locale,
      clock: () => new Date('2026-09-15T00:00:00Z'),
    })
    await settle(controller)

    root.querySelector<HTMLButtonElement>('[data-action="editor-open-older"]')?.click()
    await settle(controller)
    const accepted = controller.snapshot().accepted
    if (accepted === null) throw new Error('accepted editor state is required')

    const shell = root.querySelector<HTMLElement>('.editor-shell')
    expect(shell?.dataset.editorPhase).toBe('ready')
    expect(root.querySelectorAll('main')).toHaveLength(1)
    expect(root.querySelectorAll('[data-editor-document]')).toHaveLength(1)
    expect(root.querySelector('[data-editor-status]')?.getAttribute('role')).toBe('status')
    expect(root.querySelector('[data-editor-status]')?.getAttribute('aria-live')).toBe('polite')
    expect(root.querySelector('[data-editor-status]')?.getAttribute('aria-atomic')).toBe('true')
    expect(root.querySelector('[data-editor-error]')).toBeNull()
    expect(root.querySelector('[role="alert"]')).toBeNull()

    const documentRegion = root.querySelector<HTMLElement>('[data-editor-document]')
    if (documentRegion === null) throw new Error('semantic document region is required')
    expect(documentRegion.getAttribute('aria-label')).toBe(
      locale === 'uk' ? 'Редактор документа' : 'Document editor',
    )
    expect(documentRegion.querySelectorAll('[data-text-block]').length).toBeGreaterThan(0)
    expect(documentRegion.querySelector('[data-block-kind="image"]')).not.toBeNull()
    expect(documentRegion.querySelector('img')?.getAttribute('alt')).not.toBeNull()

    const fieldsRegion = root.querySelector<HTMLElement>('[data-fields-region]')
    if (fieldsRegion === null) throw new Error('field region is required')
    expect(fieldsRegion.getAttribute('aria-labelledby')).toBe('flowpdf-fields-heading')
    expect(fieldsRegion.querySelector('h2')?.textContent).toBe(
      locale === 'uk' ? 'Поля документа' : 'Document fields',
    )
    const cards = [...fieldsRegion.querySelectorAll<HTMLElement>('[data-field-id]')]
    expect(cards).toHaveLength(accepted.editor.view.document.fields.length)
    expect(cards.map((card) => card.dataset.fieldId)).toEqual(
      accepted.editor.view.document.fields.map((field) => field.descriptor.id),
    )
    for (const [index, card] of cards.entries()) {
      expect(card.getAttribute('role')).toBe('listitem')
      expect(card.tabIndex).toBe(0)
      expect(card.getAttribute('aria-labelledby')).toBeTruthy()
      expect(card.textContent?.trim()).not.toBe('')
      expect(card.dataset.fieldNodeId).toBe(
        accepted.editor.view.document.fields[index]?.descriptor.anchor.original.nodeId,
      )
      expect(card.querySelectorAll('input, select, textarea, button')).toHaveLength(0)
      expect(getComputedStyle(card).minHeight).toBe('44px')
    }
    expect(fieldsRegion.querySelectorAll('[data-field-option-id]').length).toBeGreaterThan(0)

    const fieldActions = [...root.querySelectorAll<HTMLElement>('[data-action]')]
      .map((element) => element.dataset.action ?? '')
      .filter((action) => /field|fill|form/i.test(action))
    expect(fieldActions).toEqual([])
    expect(root.querySelector('[data-field-review]')).toBeNull()

    const first = accepted.editor.view.document.fields[0]
    const second = accepted.editor.view.document.fields[1]
    if (first === undefined || second === undefined) {
      throw new Error('sample fields are required')
    }
    const reviewFields: EditorFieldReviewDto[] = [
      {
        descriptor: {
          ...first.descriptor,
          anchor: {
            status: 'legacyInvalid',
            original: first.descriptor.anchor.original,
            reason: 'nonGraphemeBoundary',
          },
        },
        valueSummary: first.valueSummary,
        status: { kind: 'legacyInvalid', reason: 'nonGraphemeBoundary' },
      },
      {
        descriptor: {
          ...second.descriptor,
          anchor: {
            status: 'targetDeleted',
            original: second.descriptor.anchor.original,
            tombstone: { commandId: second.descriptor.id, slot: 0 },
          },
        },
        valueSummary: second.valueSummary,
        status: {
          kind: 'targetDeleted',
          tombstone: { commandId: second.descriptor.id, slot: 0 },
        },
      },
    ]
    const reviewRoot = document.createElement('div')
    root.append(reviewRoot)
    const reviewReactRoot = createRoot(reviewRoot)
    reviewReactRoot.render(
      createElement(FieldNavigation, { fields: [], fieldReview: reviewFields, locale }),
    )
    await new Promise<void>((resolve) => setTimeout(resolve, 50))
    expect(reviewRoot.querySelector('[data-field-review]')).not.toBeNull()
    expect(reviewRoot.querySelectorAll('[data-field-review] [data-field-id]')).toHaveLength(2)
    expect(reviewRoot.querySelectorAll('[data-field-review] [tabindex="0"]')).toHaveLength(2)
    expect(reviewRoot.textContent).toContain(
      locale === 'uk' ? 'Старий якір поля потребує перегляду' : 'Legacy field anchor needs review',
    )
    expect(reviewRoot.textContent).toContain(
      locale === 'uk' ? 'Ціль поля видалена' : 'The field target was deleted',
    )
    reviewReactRoot.unmount()

    cards[0]?.focus()
    expect(document.activeElement).toBe(cards[0])
    const focusStyle = cards[0] === undefined ? null : getComputedStyle(cards[0])
    expect(focusStyle?.outlineStyle).toBe('solid')
    expect(focusStyle?.outlineWidth).toBe('3px')

    const tabStops = [...root.querySelectorAll<HTMLElement>('[tabindex="0"]')]
    expect(tabStops).toContain(cards[0])
    expect(tabStops.every((element) => accessibleName(element).length > 0)).toBe(true)
    expect(root.querySelectorAll('[aria-live="polite"]')).toHaveLength(1)

    controller.dispose()
  })
}

function accessibleName(element: HTMLElement): string {
  const label = element.getAttribute('aria-label')?.trim()
  if (label !== undefined && label !== null && label !== '') return label
  const labelledBy = element.getAttribute('aria-labelledby')
  if (labelledBy !== null) {
    const text = labelledBy
      .split(/\s+/)
      .map((id) => document.getElementById(id)?.textContent ?? '')
      .join(' ')
      .trim()
    if (text !== '') return text
  }
  return element.textContent?.trim() ?? ''
}
