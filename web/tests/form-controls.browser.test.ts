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

  test(`descriptor configuration stays Rust-owned in ${locale}`, async () => {
    const { root, controller } = await openFormEditor(locale)
    const initial = controller.snapshot().accepted
    if (initial === null) throw new Error('accepted editor state is required')
    expect(root.querySelectorAll('[data-field-editor]')).toHaveLength(
      initial.editor.view.document.fields.length,
    )

    const textCard = root.querySelector<HTMLElement>('[data-form-control-kind="text"]')?.closest(
      '[data-field-id]',
    )
    const textEditor = textCard?.querySelector<HTMLDetailsElement>('[data-field-editor]')
    if (textEditor === undefined || textEditor === null) throw new Error('text editor is required')
    textEditor.querySelector<HTMLElement>('summary')?.click()
    const label = textEditor.querySelector<HTMLInputElement>('[data-field-editor-label]')
    const defaultValue = textEditor.querySelector<HTMLInputElement>('[data-field-editor-default]')
    const save = textEditor.querySelector<HTMLButtonElement>('[data-action="editor-field-save"]')
    if (label === null || defaultValue === null || save === null) {
      throw new Error('text descriptor controls are required')
    }
    setInputValue(label, 'Заповнювач / Placeholder')
    setInputValue(defaultValue, 'Автор')
    save.click()
    await settle(controller)
    await settleForm(root, controller)

    const accepted = controller.snapshot().accepted
    if (accepted === null) throw new Error('accepted descriptor edit is required')
    expect(accepted.session.revision).toBe(initial.session.revision + 1)
    const updated = accepted.editor.view.document.fields.find(
      (field) => field.descriptor.id === initial.editor.view.document.fields[0]?.descriptor.id,
    )
    if (updated === undefined) throw new Error('updated text descriptor is required')
    expect(updated.descriptor.label).toBe('Заповнювач / Placeholder')
    expect(updated.descriptor.defaultValue).toEqual({ type: 'text', value: 'Автор' })
    expect(updated.descriptor.anchor).toEqual(
      initial.editor.view.document.fields[0]?.descriptor.anchor,
    )
    expect(root.querySelector<HTMLInputElement>('[data-form-control-kind="text"] input')?.value).toBe(
      'Автор',
    )

    const radioCard = root
      .querySelector<HTMLElement>('[data-form-control-kind="radioGroup"]')
      ?.closest('[data-field-id]')
    const radioEditor = radioCard?.querySelector<HTMLDetailsElement>('[data-field-editor]')
    if (radioEditor === undefined || radioEditor === null) throw new Error('radio editor is required')
    radioEditor.querySelector<HTMLElement>('summary')?.click()
    const optionLabel = radioEditor.querySelector<HTMLInputElement>('[data-field-editor-option-label]')
    const radioSave = radioEditor.querySelector<HTMLButtonElement>('[data-action="editor-field-save"]')
    if (optionLabel === null || radioSave === null) throw new Error('radio option editor is required')
    setInputValue(optionLabel, 'Так / Yes')
    radioSave.click()
    await settle(controller)
    const afterOptionEdit = controller.snapshot().accepted
    if (afterOptionEdit === null) throw new Error('accepted option edit is required')
    const radio = afterOptionEdit.editor.view.document.fields.find(
      (field) => field.descriptor.kind.type === 'radioGroup',
    )
    expect(radio?.descriptor.options[0]?.label).toBe('Так / Yes')
    expect(afterOptionEdit.session.revision).toBe(initial.session.revision + 2)
  })

  test(`invalid descriptor drafts remain canonical no-ops in ${locale}`, async () => {
    const { root, controller } = await openFormEditor(locale)
    const initial = controller.snapshot().accepted
    if (initial === null) throw new Error('accepted editor state is required')
    const textEditor = root
      .querySelector<HTMLElement>('[data-form-control-kind="text"]')
      ?.closest('[data-field-id]')
      ?.querySelector<HTMLDetailsElement>('[data-field-editor]')
    if (textEditor === undefined || textEditor === null) throw new Error('text editor is required')
    textEditor.querySelector<HTMLElement>('summary')?.click()
    const name = textEditor.querySelector<HTMLInputElement>('[data-field-editor-name]')
    const save = textEditor.querySelector<HTMLButtonElement>('[data-action="editor-field-save"]')
    if (name === null || save === null) throw new Error('name editor is required')
    setInputValue(name, '')
    save.click()
    await settle(controller)
    expect(controller.snapshot().accepted?.session.revision).toBe(initial.session.revision)
    expect(controller.snapshot().accepted?.editor.view.document.fields[0]?.descriptor.name).toBe(
      initial.editor.view.document.fields[0]?.descriptor.name,
    )
    expect(controller.snapshot().errorCode).toBe('FLOW_INVALID_DOCUMENT')
    expect(root.querySelector('[data-editor-error]')?.textContent).toContain('FLOW_INVALID_DOCUMENT')
  })
}
