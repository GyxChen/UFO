/**
 * 代码编辑器的交互模型（纯函数 / 纯类型，便于单测）。
 *
 * 两部分内容：
 * 1. Tab / Shift+Tab 的缩进计算 —— 编辑器本体是 textarea（见 `components/JsCodeFrame.vue`），
 *    浏览器默认把 Tab 当「切换焦点」，写代码时不可用，所以手动接管。
 * 2. `EditorCaret` —— 光标与滚动的位置快照，用于「内嵌编辑框 ⇄ 整窗弹窗」之间交接编辑位置。
 */
export const INDENT_UNIT = '  '

/** 一次缩进 / 反缩进的结果：新文本 + 新的选区（光标位置）。 */
export interface IndentEdit {
  /** 处理后的新文本 */
  value: string
  /** 新的选区起点（光标左端） */
  selectionStart: number
  /** 新的选区终点（无选区时等于起点） */
  selectionEnd: number
}

/** 下标所在行的行首位置。 */
function lineStart(value: string, index: number): number {
  return value.lastIndexOf('\n', Math.max(index - 1, 0)) + 1
}

/** 下标所在行的行尾位置（不含换行符）。 */
function lineEnd(value: string, index: number): number {
  const next = value.indexOf('\n', index)
  return next === -1 ? value.length : next
}

/**
 * 本次操作覆盖的文本区块 `[from, to)`。
 *
 * ⚠️ 选区末端正好停在某行行首时**不含**那一行 —— 否则「选中前两行」会顺手把第三行也缩进，
 * 这是编辑器通用约定（拖到行首等于没选中该行）。
 */
function blockRange(value: string, start: number, end: number): { from: number; to: number } {
  const from = lineStart(value, start)
  const lastLineStart = lineStart(value, end)
  const stopsAtLineStart = end > from && end === lastLineStart
  return { from, to: stopsAtLineStart ? lastLineStart : lineEnd(value, end) }
}

/**
 * 对选中行（无选区时为光标所在行）整体加一级缩进。
 *
 * ⚠️ 空行的处理分两种（与 VS Code 一致）：
 * - 多行选区：跳过空行 —— 否则整段缩进会在空行留下尾随空白，作者看不出来但 diff 很脏；
 * - 单行（光标停在空行上）：照常写入一级缩进 —— 否则「回车换行 → Tab → 打字」这个最常见的
 *   写代码动作会失效，光标停在空行时永远缩进不进去。
 */
export function indentLines(value: string, start: number, end: number): IndentEdit {
  const { from, to } = blockRange(value, start, end)
  const multiLine = value.slice(from, to).includes('\n')
  let firstLineIndented = false

  const block = value
    .slice(from, to)
    .split('\n')
    .map((line, index) => {
      if (multiLine && !line.trim()) return line
      if (index === 0) firstLineIndented = true
      return INDENT_UNIT + line
    })
    .join('\n')

  // 实际追加的字符数（空行被跳过，不能按「行数 × 缩进宽度」算）。
  const added = block.length - (to - from)
  return {
    value: value.slice(0, from) + block + value.slice(to),
    // 光标跟着自己那一行右移，保持停在原来的逻辑列上。
    selectionStart: start + (firstLineIndented ? INDENT_UNIT.length : 0),
    selectionEnd: end + added,
  }
}

/** 该行可去掉的缩进量：一个 Tab，或最多两个前导空格。 */
function indentWidth(line: string): number {
  if (line.startsWith('\t')) return 1
  let count = 0
  while (count < INDENT_UNIT.length && line[count] === ' ') count += 1
  return count
}

/** 对选中行（无选区时为光标所在行）整体去一级缩进；本来就没缩进的行原样保留。 */
export function outdentLines(value: string, start: number, end: number): IndentEdit {
  const { from, to } = blockRange(value, start, end)
  let removedFirstLine = 0
  let removed = 0

  const block = value
    .slice(from, to)
    .split('\n')
    .map((line, index) => {
      const width = indentWidth(line)
      if (width === 0) return line
      removed += width
      if (index === 0) removedFirstLine = width
      return line.slice(width)
    })
    .join('\n')

  return {
    value: value.slice(0, from) + block + value.slice(to),
    // 光标最多退到区块起点，不能被推到区块之前。
    selectionStart: Math.max(from, start - removedFirstLine),
    selectionEnd: Math.max(from, end - removed),
  }
}
/**
 * 编辑器光标 / 滚动快照。
 *
 * 用途：整窗弹窗里的编辑框是**另一个** DOM 实例，Vue 写入 value 后浏览器的光标会停在文末，
 * 打开弹窗就会莫名跳到脚本末尾。开弹窗前存一份、渲染完灌回去，编辑位置才连续。
 */
export interface EditorCaret {
  /** 选区起点（光标左端）在文本中的下标 */
  selectionStart: number
  /** 选区终点（无选区时等于起点） */
  selectionEnd: number
  /** 纵向滚动偏移（像素） */
  scrollTop: number
  /** 横向滚动偏移（像素） */
  scrollLeft: number
}