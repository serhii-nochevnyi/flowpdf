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

  test(`field placement uses the accepted Rust caret in ${locale}`, async () => {
    const { root, controller } = await openFormEditor(locale)
    const initial = controller.snapshot().accepted
    if (initial === null) throw new Error('accepted editor state is required')
    const originalField = initial.editor.view.document.fields[0]?.descriptor
    if (originalField === undefined) throw new Error('authored field is required')
    const targetBlock = initial.editor.view.document.blocks.find(
      (block) => block.kind !== 'atomic' && block.nodeId !== originalField.anchor.original.nodeId,
    )
    if (targetBlock === undefined) throw new Error('second text block is required')
    const target = {
      nodeId: targetBlock.nodeId,
      utf16Offset: 0,
      affinity: 'forward' as const,
    }
    await controller.setEditorSelection({ anchor: target, focus: target })
    await settle(controller)

    const card = root.querySelector<HTMLElement>(`[data-field-id="${originalField.id}"]`)
    const place = card?.querySelector<HTMLButtonElement>('[data-action="editor-field-place"]')
    if (place === null || place === undefined) throw new Error('field placement action is required')
    expect(place.disabled).toBe(false)
    place.click()
    await settle(controller)
    await settleForm(root, controller)

    const moved = controller.snapshot().accepted
    if (moved === null) throw new Error('accepted field placement is required')
    const movedField = moved.editor.view.document.fields.find(
      (field) => field.descriptor.id === originalField.id,
    )?.descriptor
    if (movedField === undefined) throw new Error('moved field is required')
    expect(moved.session.revision).toBe(initial.session.revision + 1)
    expect(movedField.anchor).toEqual({ status: 'graphemeSafe', original: target })
    expect({ ...movedField, anchor: originalField.anchor }).toEqual(originalField)
    expect(root.querySelector<HTMLInputElement>('[data-form-control-kind="text"] input')?.value).toBe(
      'Тест',
    )

    const nonCollapsed = {
      anchor: target,
      focus: { ...target, utf16Offset: 1 },
    }
    await controller.setEditorSelection(nonCollapsed)
    await settle(controller)
    const beforeNoOp = controller.snapshot().accepted
    if (beforeNoOp === null) throw new Error('accepted no-op state is required')
    const disabledPlace = root
      .querySelector<HTMLElement>(`[data-field-id="${originalField.id}"]`)
      ?.querySelector<HTMLButtonElement>('[data-action="editor-field-place"]')
    if (disabledPlace === null || disabledPlace === undefined) {
      throw new Error('field placement fence is required')
    }
    expect(disabledPlace.disabled).toBe(true)
    disabledPlace.click()
    await settle(controller)
    expect(controller.snapshot().accepted?.session.revision).toBe(beforeNoOp.session.revision)
    expect(
      controller.snapshot().accepted?.editor.view.document.fields.find(
        (field) => field.descriptor.id === originalField.id,
      )?.descriptor.anchor,
    ).toEqual(beforeNoOp.editor.view.document.fields.find(
      (field) => field.descriptor.id === originalField.id,
    )?.descriptor.anchor)
  })

  test(`field insertion creates a typed default descriptor at the Rust caret in ${locale}`, async () => {
    const { root, controller } = await openFormEditor(locale)
    const initial = controller.snapshot().accepted
    if (initial === null) throw new Error('accepted editor state is required')
    const textBlock = initial.editor.view.document.blocks.find((block) => block.kind !== 'atomic')
    if (textBlock === undefined) throw new Error('text block is required')
    const target = {
      nodeId: textBlock.nodeId,
      utf16Offset: 0,
      affinity: 'forward' as const,
    }
    await controller.setEditorSelection({ anchor: target, focus: target })
    await settle(controller)

    const insert = root.querySelector<HTMLButtonElement>('[data-action="editor-insert-field"]')
    if (insert === null) throw new Error('field insertion action is required')
    expect(insert.disabled, JSON.stringify({
      phase: controller.snapshot().phase,
      selection: controller.snapshot().accepted?.editor.view.selection,
      capabilities: controller.snapshot().accepted?.editor.view.capabilities,
    })).toBe(false)
    const initialIds = new Set(
      initial.editor.view.document.fields.map((field) => field.descriptor.id),
    )
    insert.click()
    await settle(controller)
    await settleForm(root, controller)

    const inserted = controller.snapshot().accepted
    if (inserted === null) throw new Error('accepted field insertion is required')
    const newFields = inserted.editor.view.document.fields.filter(
      (field) => !initialIds.has(field.descriptor.id),
    )
    expect(newFields).toHaveLength(1)
    const created = newFields[0]?.descriptor
    if (created === undefined) throw new Error('new field descriptor is required')
    expect(inserted.session.revision).toBe(initial.session.revision + 1)
    expect(created.label).toBe(locale === 'uk' ? 'Нове текстове поле' : 'New text field')
    expect(created.kind).toEqual({ type: 'text', multiline: false, inputHint: 'plain' })
    expect(created.required).toBe(false)
    expect(created.readOnly).toBe(false)
    expect(created.defaultValue).toEqual({ type: 'empty' })
    expect(created.options).toEqual([])
    expect(created.anchor).toEqual({ status: 'graphemeSafe', original: target })
    expect(root.querySelectorAll('[data-form-control-kind="text"]')).toHaveLength(2)
    expect(root.querySelector(`[data-field-id="${created.id}"] [data-field-editor]`)).not.toBeNull()

    await controller.undo()
    await settle(controller)
    await settleForm(root, controller)
    expect(controller.snapshot().accepted?.editor.view.document.fields).toHaveLength(
      initial.editor.view.document.fields.length,
    )
    await controller.redo()
    await settle(controller)
    await settleForm(root, controller)
    expect(controller.snapshot().accepted?.editor.view.document.fields).toHaveLength(
      initial.editor.view.document.fields.length + 1,
    )

    const beforeNoOp = controller.snapshot().accepted
    if (beforeNoOp === null) throw new Error('accepted inserted state is required')
    const nonCollapsed = {
      anchor: target,
      focus: { ...target, utf16Offset: 1 },
    }
    await controller.setEditorSelection(nonCollapsed)
    await settle(controller)
    const disabledInsert = root.querySelector<HTMLButtonElement>('[data-action="editor-insert-field"]')
    if (disabledInsert === null) throw new Error('field insertion fence is required')
    expect(disabledInsert.disabled).toBe(true)
    disabledInsert.click()
    await settle(controller)
    expect(controller.snapshot().accepted?.session.revision).toBe(beforeNoOp.session.revision)
    expect(controller.snapshot().accepted?.editor.view.document.fields).toHaveLength(
      beforeNoOp.editor.view.document.fields.length,
    )
  })

  test(`field removal is confirmed, reversible, and source-bound in ${locale}`, async () => {
    const { root, controller } = await openFormEditor(locale)
    const initial = controller.snapshot().accepted
    if (initial === null) throw new Error('accepted editor state is required')
    const textField = initial.editor.view.document.fields.find(
      (field) => field.descriptor.kind.type === 'text',
    )
    if (textField === undefined) throw new Error('text field is required')
    const card = root.querySelector<HTMLElement>(`[data-field-id="${textField.descriptor.id}"]`)
    const text = card?.querySelector<HTMLInputElement>('[data-form-control-kind="text"] input')
    const remove = card?.querySelector<HTMLButtonElement>('[data-action="editor-field-remove"]')
    if (
      card === null ||
      card === undefined ||
      text === null ||
      text === undefined ||
      remove === null ||
      remove === undefined
    ) {
      throw new Error('field removal controls are required')
    }

    setInputValue(text, 'Перед видаленням')
    for (let attempt = 0; attempt < 40 && text.value !== 'Перед видаленням'; attempt += 1) {
      await settle(controller)
    }
    expect(text.value).toBe('Перед видаленням')
    await settleForm(root, controller)
    const beforeRemoval = controller.snapshot().accepted
    if (beforeRemoval === null) throw new Error('accepted pre-removal state is required')

    remove.click()
    await settle(controller)
    const dialog = root.querySelector<HTMLDialogElement>('[data-field-remove-dialog]')
    expect(dialog?.getAttribute('role')).toBe('alertdialog')
    expect(dialog?.textContent).toContain(locale === 'uk' ? 'Видалити поле?' : 'Remove field?')

    dialog
      ?.querySelector<HTMLButtonElement>('[data-action="editor-field-remove-cancel"]')
      ?.click()
    await settle(controller)
    expect(controller.snapshot().accepted?.session.revision).toBe(beforeRemoval.session.revision)
    expect(root.querySelector(`[data-field-id="${textField.descriptor.id}"]`)).not.toBeNull()

    root
      .querySelector<HTMLButtonElement>(
        `[data-field-id="${textField.descriptor.id}"] [data-action="editor-field-remove"]`,
      )
      ?.click()
    await settle(controller)
    root.querySelector<HTMLButtonElement>('[data-action="editor-field-remove-confirm"]')?.click()
    await settle(controller)
    await settleForm(root, controller)

    const removed = controller.snapshot().accepted
    if (removed === null) throw new Error('accepted removed state is required')
    expect(removed.session.revision).toBe(beforeRemoval.session.revision + 1)
    expect(removed.editor.view.document.fields).toHaveLength(
      beforeRemoval.editor.view.document.fields.length - 1,
    )
    expect(root.querySelector(`[data-field-id="${textField.descriptor.id}"]`)).toBeNull()

    await controller.undo()
    await settle(controller)
    await settleForm(root, controller)
    const restored = controller.snapshot().accepted
    if (restored === null) throw new Error('accepted restored state is required')
    expect(
      restored.editor.view.document.fields.some(
        (field) => field.descriptor.id === textField.descriptor.id,
      ),
    ).toBe(true)
    expect(
      root.querySelector<HTMLInputElement>(
        `[data-field-id="${textField.descriptor.id}"] [data-form-control-kind="text"] input`,
      )?.value,
    ).toBe('Тест')

    await controller.redo()
    await settle(controller)
    await settleForm(root, controller)
    expect(controller.snapshot().accepted?.editor.view.document.fields).toHaveLength(
      beforeRemoval.editor.view.document.fields.length - 1,
    )
    expect(root.querySelector(`[data-field-id="${textField.descriptor.id}"]`)).toBeNull()
  })

  test(`field tab order moves through Rust and rebinds form state in ${locale}`, async () => {
    const { root, controller } = await openFormEditor(locale)
    const initial = controller.snapshot().accepted
    if (initial === null) throw new Error('accepted editor state is required')
    const textField = initial.editor.view.document.fields.find(
      (field) => field.descriptor.kind.type === 'text',
    )
    if (textField === undefined) throw new Error('text field is required')
    const textCard = root.querySelector<HTMLElement>(
      `[data-field-id="${textField.descriptor.id}"]`,
    )
    const text = textCard?.querySelector<HTMLInputElement>('[data-form-control-kind="text"] input')
    if (textCard === null || textCard === undefined || text === null || text === undefined) {
      throw new Error('text field card is required')
    }

    setInputValue(text, 'Перед порядком')
    for (let attempt = 0; attempt < 40 && text.value !== 'Перед порядком'; attempt += 1) {
      await settle(controller)
    }
    expect(text.value).toBe('Перед порядком')
    await settleForm(root, controller)

    const beforeMove = controller.snapshot().accepted
    if (beforeMove === null) throw new Error('accepted pre-order state is required')
    const moveEarlier = textField.tabOrder > 0
    const action = textCard.querySelector<HTMLButtonElement>(
      `[data-action="editor-field-move-${moveEarlier ? 'earlier' : 'later'}"]`,
    )
    if (action === null) throw new Error('field order action is required')
    expect(action.disabled).toBe(false)
    action.click()
    await settle(controller)
    await settleForm(root, controller)

    const moved = controller.snapshot().accepted
    if (moved === null) throw new Error('accepted reordered state is required')
    const movedField = moved.editor.view.document.fields.find(
      (field) => field.descriptor.id === textField.descriptor.id,
    )
    if (movedField === undefined) throw new Error('reordered text field is required')
    expect(moved.session.revision).toBe(beforeMove.session.revision + 1)
    expect(movedField.tabOrder).toBe(textField.tabOrder + (moveEarlier ? -1 : 1))
    expect(
      moved.editor.view.document.fields.map((field) => field.tabOrder).sort((a, b) => a - b),
    ).toEqual([...Array(moved.editor.view.document.fields.length).keys()])
    expect(
      root.querySelector<HTMLInputElement>(
        `[data-field-id="${textField.descriptor.id}"] [data-form-control-kind="text"] input`,
      )?.value,
    ).toBe('Тест')

    await controller.undo()
    await settle(controller)
    await settleForm(root, controller)
    const undone = controller.snapshot().accepted
    if (undone === null) throw new Error('accepted undo state is required')
    expect(
      undone.editor.view.document.fields.find(
        (field) => field.descriptor.id === textField.descriptor.id,
      )?.tabOrder,
    ).toBe(textField.tabOrder)
    expect(
      root.querySelector<HTMLInputElement>(
        `[data-field-id="${textField.descriptor.id}"] [data-form-control-kind="text"] input`,
      )?.value,
    ).toBe('Тест')

    await controller.redo()
    await settle(controller)
    await settleForm(root, controller)
    expect(
      controller.snapshot().accepted?.editor.view.document.fields.find(
        (field) => field.descriptor.id === textField.descriptor.id,
      )?.tabOrder,
    ).toBe(textField.tabOrder + (moveEarlier ? -1 : 1))
  })
}
