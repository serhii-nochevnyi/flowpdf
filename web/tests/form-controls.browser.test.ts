import { expect, test } from 'vitest'

import '../src/styles.css'
import { mountEditorApp, type EditorController } from '../src/editor/editor-app.js'

async function settle(controller: EditorController): Promise<void> {
  await controller.whenIdle()
  await new Promise<void>((resolve) => setTimeout(resolve, 25))
}

async function settleForm(root: HTMLElement, controller: EditorController): Promise<void> {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    await settle(controller)
    if (root.querySelector('[data-form-session-status="loading"]') === null) return
  }
  throw new Error('form session did not settle')
}

function setInputValue(input: HTMLInputElement | HTMLTextAreaElement, value: string): void {
  const prototype = input instanceof HTMLTextAreaElement
    ? HTMLTextAreaElement.prototype
    : HTMLInputElement.prototype
  const setter = Object.getOwnPropertyDescriptor(prototype, 'value')?.set
  setter?.call(input, value)
  input.dispatchEvent(new Event('input', { bubbles: true }))
  input.dispatchEvent(new Event('change', { bubbles: true }))
}

async function openFormEditor(
  locale: 'uk' | 'en',
): Promise<{ readonly root: HTMLElement; readonly controller: EditorController }> {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const controller = await mountEditorApp(root, {
    databaseName: `flowpdf-form-controls-${locale}-${crypto.randomUUID()}`,
    locale,
    clock: () => new Date('2026-09-22T00:00:00Z'),
  })
  await settle(controller)
  root.querySelector<HTMLButtonElement>('[data-action="editor-open-older"]')?.click()
  await settleForm(root, controller)
  return { root, controller }
}

for (const locale of ['uk', 'en'] as const) {
  test(`authored fields expose native controls and Rust-owned session values in ${locale}`, async () => {
    const { root, controller } = await openFormEditor(locale)
    const initialRevision = controller.snapshot().accepted?.session.revision
    if (initialRevision === undefined) throw new Error('accepted revision is required')

    const text = root.querySelector<HTMLInputElement>('[data-form-control-kind="text"] input')
    const checkbox = root.querySelector<HTMLInputElement>(
      '[data-form-control-kind="checkbox"] input',
    )
    const radios = [
      ...root.querySelectorAll<HTMLInputElement>('[data-form-control-kind="radioGroup"] input'),
    ]
    const select = root.querySelector<HTMLSelectElement>('[data-form-control-kind="select"] select')
    if (text === null || checkbox === null || radios.length !== 2 || select === null) {
      throw new Error('all writable field controls are required')
    }
    expect(text.value).toBe('Тест')
    expect(checkbox.checked).toBe(false)
    expect(radios[0]?.checked).toBe(true)
    expect([...select.selectedOptions].map((option) => option.value)).toHaveLength(2)
    expect(root.querySelectorAll('[data-field-noneditable]')).toHaveLength(2)

    setInputValue(text, 'Олена')
    await settle(controller)
    expect(text.value).toBe('Олена')
    expect(root.querySelector('[data-field-value]')?.textContent).toContain('Олена')

    checkbox.click()
    await settle(controller)
    expect(checkbox.checked).toBe(true)

    radios[1]?.click()
    await settle(controller)
    expect(radios[1]?.checked).toBe(true)
    expect(radios[0]?.checked).toBe(false)

    select.options[0]!.selected = false
    select.options[1]!.selected = true
    select.dispatchEvent(new Event('change', { bubbles: true }))
    await settle(controller)
    expect([...select.selectedOptions].map((option) => option.value)).toEqual([
      select.options[1]?.value,
    ])
    expect(controller.snapshot().accepted?.session.revision).toBe(initialRevision)

    const clear = root.querySelector<HTMLButtonElement>(
      '[data-form-control-kind="text"] [data-form-action="clear"]',
    )
    if (clear === null) throw new Error('text clear action is required')
    clear.click()
    await settle(controller)
    expect(text.value).toBe('Тест')
    expect(controller.snapshot().accepted?.session.revision).toBe(initialRevision)
  })

  test(`invalid field values announce the Rust error without publishing canonical state in ${locale}`, async () => {
    const { root, controller } = await openFormEditor(locale)
    const initialRevision = controller.snapshot().accepted?.session.revision
    const text = root.querySelector<HTMLInputElement>('[data-form-control-kind="text"] input')
    if (initialRevision === undefined || text === null) throw new Error('required field is missing')

    setInputValue(text, '')
    await settle(controller)
    const error = root.querySelector<HTMLElement>('[data-form-session-status="error"]')
    expect(error?.getAttribute('role')).toBe('alert')
    expect(error?.textContent).toContain('FLOW_FORM_SESSION_VALUE_INVALID')
    expect(text.value).toBe('Тест')
    expect(controller.snapshot().accepted?.session.revision).toBe(initialRevision)
  })
}
