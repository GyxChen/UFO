import { describe, expect, it } from 'vitest'
import { indentLines, outdentLines } from './js-editor'

/** 断言缩进结果：文本 + 选区一并核对（光标位置错了比文本错更隐蔽）。 */
function expectEdit(
  edit: { value: string; selectionStart: number; selectionEnd: number },
  value: string,
  selectionStart: number,
  selectionEnd: number = selectionStart,
) {
  expect(edit.value).toBe(value)
  expect(edit.selectionStart).toBe(selectionStart)
  expect(edit.selectionEnd).toBe(selectionEnd)
}

describe('indentLines', () => {
  it('indents the caret line and moves the caret with it', () => {
    // 光标在 "bb" 里的下标 4：缩进后应落在 6（自己那行右移了两个空格）。
    expectEdit(indentLines('aa\nbb', 4, 4), 'aa\n  bb', 6)
  })

  it('indents every selected line and keeps the selection covering them', () => {
    expectEdit(indentLines('aa\nbb', 0, 5), '  aa\n  bb', 2, 9)
  })

  it('does not treat a selection that stops at a line start as covering that line', () => {
    // 拖到第二行行首（下标 3）等于没选中第二行，不能顺手把它也缩进。
    expectEdit(indentLines('aa\nbb', 0, 3), '  aa\nbb', 2, 5)
  })

  it('skips blank lines so they get no trailing whitespace', () => {
    expectEdit(indentLines('aa\n\nbb', 0, 6), '  aa\n\n  bb', 2, 10)
  })

  it('is a no-op on an all-blank multi-line block', () => {
    expectEdit(indentLines('\n\n', 0, 2), '\n\n', 0, 2)
  })

  it('still indents when the caret sits on a blank line', () => {
    // 「回车换行 → Tab → 打字」是最常见的写代码动作，光标停在空行时不能被跳过。
    expectEdit(indentLines('aa\n', 3, 3), 'aa\n  ', 5)
  })

  it('only touches the lines the selection spans', () => {
    // 光标停在最后一行行首（下标 6）：只缩进第三行，前两行不动。
    expectEdit(indentLines('aa\nbb\ncc', 6, 6), 'aa\nbb\n  cc', 8)
  })
})

describe('outdentLines', () => {
  it('removes up to two leading spaces from the caret line', () => {
    expectEdit(outdentLines('  aa', 4, 4), 'aa', 2)
  })

  it('removes a single leading tab as one level', () => {
    expectEdit(outdentLines('\taa', 3, 3), 'aa', 2)
  })

  it('leaves a line without indentation untouched', () => {
    expectEdit(outdentLines('aa', 2, 2), 'aa', 2)
  })

  it('outdents every selected line and shrinks the selection accordingly', () => {
    expectEdit(outdentLines('  aa\n  bb', 0, 9), 'aa\nbb', 0, 5)
  })

  it('never pushes the caret before the block start', () => {
    // 光标停在下标 1（第一行内）：去掉两个空格后不能变成负数。
    const edit = outdentLines('  aa', 1, 1)
    expect(edit.value).toBe('aa')
    expect(edit.selectionStart).toBe(0)
  })

  it('handles uneven indentation across a selection', () => {
    // 三行分别缩进 2 / 0 / 1 个空格：只按各自实际缩进量削减。
    expectEdit(outdentLines('  aa\nbb\n cc', 0, 11), 'aa\nbb\ncc', 0, 8)
  })
})