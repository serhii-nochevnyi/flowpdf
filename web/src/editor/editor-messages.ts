import type { EditorLocale } from './editor-store.js'

const uk = {
  toolbar: 'Форматування',
  blockStyle: 'Стиль блока',
  paragraph: 'Основний текст',
  mixedBlockStyle: 'Змішані стилі',
  heading: 'Заголовок',
  bold: 'Жирний',
  italic: 'Курсив',
  underline: 'Підкреслення',
  moreFormatting: 'Ще форматування',
  fontFamily: 'Шрифт',
  fontSize: 'Розмір шрифту',
  fontSizeUnit: 'мілліпойнти',
  textColor: 'Колір тексту',
  language: 'Мова тексту',
  ukrainian: 'Українська',
  english: 'English',
  alignment: 'Вирівнювання',
  alignStart: 'Ліворуч',
  alignCenter: 'По центру',
  alignEnd: 'Праворуч',
  alignJustify: 'За шириною',
  paragraphSpacing: 'Інтервали абзацу',
  spacingBefore: 'Перед',
  spacingAfter: 'Після',
  list: 'Списки',
  unorderedList: 'Маркований список',
  orderedList: 'Нумерований список',
  indentListItem: 'Збільшити відступ списку',
  outdentListItem: 'Зменшити відступ списку',
  plainFont: 'Noto Sans',
  serifFont: 'Noto Serif',
  monoFont: 'Noto Sans Mono',
  mixedValues: 'Змішані значення',
  unavailable: 'Недоступно для поточного виділення',
  textBlockRequired: 'Потрібен текстовий блок',
  formattingRangeRequired: 'Виділіть текст для форматування',
  listItemRequired: 'Потрібен елемент списку',
  emptyListItemRequired: 'Потрібен порожній елемент списку',
  listSiblingRequired: 'Потрібен попередній елемент списку',
  nestedListItemRequired: 'Потрібен вкладений елемент списку',
  pending: 'Застосовуємо форматування…',
  colorHint: 'Введіть колір у форматі #RRGGBB',
  clearValue: 'Без значення',
} as const

export type EditorMessageKey = keyof typeof uk

const en: { readonly [Key in EditorMessageKey]: string } = {
  toolbar: 'Formatting',
  blockStyle: 'Block style',
  paragraph: 'Body text',
  mixedBlockStyle: 'Mixed styles',
  heading: 'Heading',
  bold: 'Bold',
  italic: 'Italic',
  underline: 'Underline',
  moreFormatting: 'More formatting',
  fontFamily: 'Font',
  fontSize: 'Font size',
  fontSizeUnit: 'millipoints',
  textColor: 'Text color',
  language: 'Text language',
  ukrainian: 'Ukrainian',
  english: 'English',
  alignment: 'Alignment',
  alignStart: 'Left',
  alignCenter: 'Center',
  alignEnd: 'Right',
  alignJustify: 'Justify',
  paragraphSpacing: 'Paragraph spacing',
  spacingBefore: 'Before',
  spacingAfter: 'After',
  list: 'Lists',
  unorderedList: 'Bulleted list',
  orderedList: 'Numbered list',
  indentListItem: 'Indent list item',
  outdentListItem: 'Outdent list item',
  plainFont: 'Noto Sans',
  serifFont: 'Noto Serif',
  monoFont: 'Noto Sans Mono',
  mixedValues: 'Mixed values',
  unavailable: 'Unavailable for the current selection',
  textBlockRequired: 'A text block is required',
  formattingRangeRequired: 'Select text to format it',
  listItemRequired: 'A list item is required',
  emptyListItemRequired: 'An empty list item is required',
  listSiblingRequired: 'A previous list item is required',
  nestedListItemRequired: 'A nested list item is required',
  pending: 'Applying formatting…',
  colorHint: 'Enter a color in #RRGGBB format',
  clearValue: 'No value',
}

export const editorMessages: {
  readonly uk: typeof uk
  readonly en: typeof en
} = { uk, en }

export function editorMessage(
  locale: EditorLocale,
  key: EditorMessageKey,
): string {
  return editorMessages[locale][key]
}

export function capabilityReason(
  locale: EditorLocale,
  reasonKey: string | null | undefined,
): string {
  if (reasonKey === undefined || reasonKey === null) return editorMessage(locale, 'unavailable')
  if (reasonKey in editorMessages[locale]) {
    return editorMessage(locale, reasonKey as EditorMessageKey)
  }
  return editorMessage(locale, 'unavailable')
}
