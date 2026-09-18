/**
 * 极简 JS 高亮（零依赖）。
 *
 * 编辑器是「透明 textarea 叠在带高亮的 <pre> 上」（见 `components/JsCodeFrame.vue`），
 * 因此这里只需要把源码切成词法片段并渲染成带 span 的 HTML：不需要 AST、补全、折叠，
 * 也就没必要背上 CodeMirror / Monaco 那一整套状态机与主题系统。
 *
 * 代价：只是词法近似 —— 「/ 是正则还是除号」靠前一个片段猜，模板串按 `${}` 递归扫描，
 * 不做语法分析。对「写一小段查余额脚本」这个场景足够，且出错也只影响颜色、不影响内容。
 *
 * ⚠️ 不变量：`tokenizeJs(code).map(t => t.text).join('') === code`。
 * 高亮层与 textarea 必须是**逐字符相同**的两层文本，少一个字符就会整段错位（有单测锁住）。
 */

/** 词法片段种类；决定渲染时的颜色（`plain` / `ident` 不着色，直接输出原文）。 */
export type JsTokenKind =
  /** 空白（空格 / 制表符 / 换行） */
  | 'plain'
  /** 普通标识符（变量名等） */
  | 'ident'
  /** 关键字（const / return / await …） */
  | 'keyword'
  /** 字面量关键字（true / false / null / undefined …） */
  | 'literal'
  /** 宿主注入或 JS 内置的全局对象（http / secrets / Math …） */
  | 'builtin'
  /** 函数调用名（标识符后紧跟 `(`） */
  | 'call'
  /** 数字 */
  | 'number'
  /** 单 / 双引号字符串 */
  | 'string'
  /** 模板字符串（含两侧反引号与 `${` / `}` 外壳） */
  | 'template'
  /** 正则字面量 */
  | 'regex'
  /** 注释（行注释与块注释） */
  | 'comment'
  /** 标点（括号 / 逗号 / 冒号 / 点号 …） */
  | 'punct'
  /** 运算符 */
  | 'op'

/** 一个词法片段；`text` 是**原文**，渲染时才做 HTML 转义。 */
export interface JsToken {
  /** 片段种类，决定着色 */
  kind: JsTokenKind
  /** 原文（未转义） */
  text: string
}

/** JS 关键字（含少量上下文关键字，用于着色足够）。 */
const KEYWORDS = new Set([
  'as', 'async', 'await', 'break', 'case', 'catch', 'class', 'const', 'continue', 'debugger',
  'default', 'delete', 'do', 'else', 'export', 'extends', 'finally', 'for', 'from', 'function',
  'if', 'import', 'in', 'instanceof', 'let', 'new', 'of', 'return', 'static', 'super', 'switch',
  'this', 'throw', 'try', 'typeof', 'var', 'void', 'while', 'with', 'yield',
])

/** 字面量关键字。 */
const LITERALS = new Set(['true', 'false', 'null', 'undefined', 'NaN', 'Infinity'])

/**
 * 内置 / 宿主注入的全局名。
 * 前六个是本项目脚本沙盒注入的宿主 API（http / secrets / cache / format / ctx / console），
 * 其余是常用 JS 内置对象，着色只影响观感。
 */
const BUILTINS = new Set([
  'http', 'secrets', 'cache', 'format', 'ctx', 'console',
  'Math', 'JSON', 'Number', 'String', 'Boolean', 'Object', 'Array', 'Date', 'RegExp', 'Promise',
  'Error', 'TypeError', 'Symbol', 'BigInt', 'Map', 'Set', 'WeakMap', 'WeakSet', 'globalThis',
  'parseInt', 'parseFloat', 'isNaN', 'isFinite', 'encodeURIComponent', 'decodeURIComponent',
  'setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'structuredClone',
])

/** 数字字面量：0x / 0b / 0o、下划线分隔、小数、指数、BigInt 后缀。 */
const NUMBER_RE = /(?:0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.[\d_]*)?(?:[eE][+-]?\d+)?|\.\d[\d_]*(?:[eE][+-]?\d+)?)n?/y

/** 标识符：`$` 与 `_` 也可作起始字符。 */
const IDENT_RE = /[A-Za-z_$][A-Za-z0-9_$]*/y

/**
 * 运算符表。
 * 只列真的运算符：括号 / 逗号 / 冒号 / 点号等走标点分支，`{` `}` 有专门处理（要配平）。
 */
const OPERATORS = [
  '>>>=', '...', '===', '!==', '**=', '<<=', '>>=', '>>>', '&&=', '||=', '??=',
  '==', '!=', '<=', '>=', '&&', '||', '??', '?.', '**', '++', '--',
  '+=', '-=', '*=', '/=', '%=', '&=', '|=', '^=', '=>', '<<', '>>',
  '+', '-', '*', '/', '%', '<', '>', '=', '!', '&', '|', '^', '~', '?',
]

/** 运算符按首字符分组、组内长的优先（`?.` 不能被拆成 `?` + `.`）。 */
const OPERATORS_BY_FIRST = (() => {
  const grouped = new Map<string, string[]>()
  for (const operator of [...OPERATORS].sort((a, b) => b.length - a.length)) {
    const list = grouped.get(operator[0]) ?? []
    list.push(operator)
    grouped.set(operator[0], list)
  }
  return grouped
})()

/** 扫描过程中的可变状态；递归扫模板串的 `${}` 时要共享同一份。 */
interface ScanState {
  /** 当前扫描下标 */
  i: number
  /** 最近一个「有意义」的片段（空白 / 注释不算），用于判定 `/` 是正则还是除号 */
  prev: JsToken | null
}

/** 空白字符判定。 */
function isSpace(ch: string): boolean {
  return ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r'
}

/** 数字字符判定。 */
function isDigit(ch: string): boolean {
  return ch >= '0' && ch <= '9'
}

/** 标识符起始字符判定。 */
function isIdentStart(ch: string): boolean {
  return (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || ch === '_' || ch === '$'
}

/** 用 sticky 正则从 index 处取匹配；不匹配返回 null。 */
function matchSticky(re: RegExp, text: string, index: number): string | null {
  re.lastIndex = index
  const match = re.exec(text)
  return match ? match[0] : null
}

/** 取 index 处最长的运算符；没有则 null。 */
function matchOperator(code: string, index: number): string | null {
  for (const operator of OPERATORS_BY_FIRST.get(code[index]) ?? []) {
    if (code.startsWith(operator, index)) return operator
  }
  return null
}

/**
 * 追加一个片段。
 * 空白 / 注释只做合并（减少 span 数量），其余一律新起片段 —— 保证 `prev.text` 是单个记号，
 * 正则判定才不会被 `))` 这种合并结果干扰。
 */
function push(tokens: JsToken[], state: ScanState, kind: JsTokenKind, text: string): void {
  if (!text) return
  const last = tokens[tokens.length - 1]
  const mergeable = kind === 'plain' || kind === 'comment'
  if (last && last.kind === kind && mergeable) {
    last.text += text
  } else {
    tokens.push({ kind, text })
  }
  // 空白与注释不参与「正则还是除号」的判定（`a / b` 里靠 `a` 判定）。
  if (!mergeable) state.prev = tokens[tokens.length - 1]
}

/**
 * 扫描一段「代码」（顶层，或模板串 `${}` 内部）。
 *
 * `stopAtBrace` 为 true 时表示正处于 `${}` 中：遇到与外层配平的 `}` 就返回且**不消费**它，
 * 由模板分支负责消费（那个 `}` 属于模板串外壳，着色与字面量一致）。
 */
function scanCode(code: string, tokens: JsToken[], state: ScanState, stopAtBrace: boolean): void {
  let braceDepth = 0

  while (state.i < code.length) {
    const index = state.i
    const ch = code[index]

    // 空白：整段合并成一个 plain 片段。
    if (isSpace(ch)) {
      let end = index + 1
      while (end < code.length && isSpace(code[end])) end += 1
      push(tokens, state, 'plain', code.slice(index, end))
      state.i = end
      continue
    }

    // 行注释：到行尾（不含换行，换行留给空白分支）。
    if (ch === '/' && code[index + 1] === '/') {
      const stop = code.indexOf('\n', index)
      const end = stop === -1 ? code.length : stop
      push(tokens, state, 'comment', code.slice(index, end))
      state.i = end
      continue
    }

    // 块注释：到 `*/`（未闭合则到文件末尾）。
    if (ch === '/' && code[index + 1] === '*') {
      const stop = code.indexOf('*/', index + 2)
      const end = stop === -1 ? code.length : stop + 2
      push(tokens, state, 'comment', code.slice(index, end))
      state.i = end
      continue
    }

    // 字符串与模板串。
    if (ch === '"' || ch === "'") {
      state.i = scanQuoted(code, tokens, state, index, ch)
      continue
    }
    if (ch === '`') {
      scanTemplate(code, tokens, state)
      continue
    }

    // 数字（`.5` 这种前导点号也要认）。
    if (isDigit(ch) || (ch === '.' && isDigit(code[index + 1] ?? ''))) {
      const match = matchSticky(NUMBER_RE, code, index) ?? ch
      push(tokens, state, 'number', match)
      state.i = index + match.length
      continue
    }

    // 标识符 / 关键字 / 内置名；后面紧跟 `(` 的按「函数调用」着色。
    if (isIdentStart(ch)) {
      const match = matchSticky(IDENT_RE, code, index) ?? ch
      const kind: JsTokenKind = KEYWORDS.has(match)
        ? 'keyword'
        : LITERALS.has(match)
          ? 'literal'
          : code[index + match.length] === '('
            ? 'call'
            : BUILTINS.has(match)
              ? 'builtin'
              : 'ident'
      push(tokens, state, kind, match)
      state.i = index + match.length
      continue
    }

    // 正则字面量：只有「前面不是值」时 `/` 才可能是正则，否则是除号。
    if (ch === '/' && regexAllowed(state.prev)) {
      const end = scanRegexEnd(code, index)
      if (end > 0) {
        push(tokens, state, 'regex', code.slice(index, end))
        state.i = end
        continue
      }
    }

    // 花括号要配平：`${ a: { b } }` 里第一个 `}` 属于内层对象。
    if (ch === '{') {
      braceDepth += 1
      push(tokens, state, 'punct', ch)
      state.i = index + 1
      continue
    }
    if (ch === '}') {
      if (braceDepth === 0 && stopAtBrace) return
      if (braceDepth > 0) braceDepth -= 1
      push(tokens, state, 'punct', ch)
      state.i = index + 1
      continue
    }

    // 运算符。
    const operator = matchOperator(code, index)
    if (operator) {
      push(tokens, state, 'op', operator)
      state.i = index + operator.length
      continue
    }

    // 兜底：其余字符一律当标点原样收录 —— 不丢字符才能保住「两层文本一致」的不变量。
    push(tokens, state, 'punct', ch)
    state.i = index + 1
  }
}

/**
 * 扫描单 / 双引号字符串，返回下一个下标。
 * 未闭合时读到行尾为止（不跨行、也不吞掉换行），源码写到一半时后面的内容仍能被正常扫描。
 */
function scanQuoted(
  code: string,
  tokens: JsToken[],
  state: ScanState,
  start: number,
  quote: string,
): number {
  let end = start + 1
  while (end < code.length) {
    const ch = code[end]
    if (ch === '\\') {
      end += 2
      continue
    }
    if (ch === quote) {
      end += 1
      break
    }
    if (ch === '\n') break
    end += 1
  }
  const stop = Math.min(end, code.length)
  push(tokens, state, 'string', code.slice(start, stop))
  return stop
}

/**
 * 扫描模板字符串（含 `${}` 里的表达式，可再嵌套模板串）。
 *
 * 进入时 `state.i` 指向起始反引号；返回时指向闭合反引号之后（未闭合则到文件末尾）。
 * 反引号与 `${` / `}` 都算 template 片段，只有 `${}` 内部才是代码，交给 `scanCode` 递归。
 */
function scanTemplate(code: string, tokens: JsToken[], state: ScanState): void {
  push(tokens, state, 'template', '`')
  state.i += 1

  while (state.i < code.length) {
    const ch = code[state.i]

    // 转义序列整体收进字面量（反引号 / `${` 被转义时不算边界）。
    if (ch === '\\') {
      push(tokens, state, 'template', code.slice(state.i, state.i + 2))
      state.i += 2
      continue
    }

    if (ch === '`') {
      push(tokens, state, 'template', '`')
      state.i += 1
      return
    }

    if (ch === '$' && code[state.i + 1] === '{') {
      push(tokens, state, 'template', '${')
      state.i += 2
      scanCode(code, tokens, state, true)
      // 消费掉配平的那个 `}`（没配平说明源码写到一半，外层循环自然收尾）。
      if (code[state.i] === '}') {
        push(tokens, state, 'template', '}')
        state.i += 1
      }
      continue
    }

    // 普通字面量文本：一次读到下一个特殊字符，减少片段数量。
    let end = state.i + 1
    while (end < code.length && code[end] !== '`' && code[end] !== '\\' && code[end] !== '$') {
      end += 1
    }
    push(tokens, state, 'template', code.slice(state.i, end))
    state.i = end
  }
}

/**
 * 从 `/` 开始找正则字面量的结尾；找不到（跨行 / 未闭合）返回 0，表示应按除号处理。
 * 字符类 `[]` 内的 `/` 不算结束；结尾的 `gimsuy` 标志位一并吃进片段。
 */
function scanRegexEnd(code: string, start: number): number {
  let i = start + 1
  let inClass = false
  while (i < code.length) {
    const ch = code[i]
    if (ch === '\\') {
      i += 2
      continue
    }
    if (ch === '\n') return 0
    if (ch === '[') inClass = true
    else if (ch === ']') inClass = false
    else if (ch === '/' && !inClass) {
      i += 1
      while (i < code.length && /[a-z]/i.test(code[i])) i += 1
      return i
    }
    i += 1
  }
  return 0
}

/**
 * `/` 是正则还是除号：前面是「值」（标识符 / 字面量 / `)` `]` `}`）就是除号，否则是正则。
 * 拿不准时倾向正则，因为正则扫描失败会退回运算符分支，反之则不会。
 */
function regexAllowed(prev: JsToken | null): boolean {
  if (!prev) return true
  if (prev.kind === 'op') return true
  if (prev.kind === 'keyword') return true
  if (prev.kind === 'punct') return !(prev.text === ')' || prev.text === ']' || prev.text === '}')
  // ident / builtin / call / number / string / template / regex / literal 都是「值」。
  return false
}

/**
 * 把源码切成词法片段（纯函数）。
 * 不变量：把所有片段的 `text` 顺序拼回来必须与原源码完全一致。
 */
export function tokenizeJs(code: string): JsToken[] {
  const tokens: JsToken[] = []
  scanCode(code, tokens, { i: 0, prev: null }, false)
  return tokens
}

/** HTML 转义表。 */
const HTML_ESCAPES: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;' }

/**
 * HTML 转义。
 * 高亮结果走 `v-html`，而脚本正文完全由用户输入，任何 `<` 都必须先转义，否则会被当标签解析。
 */
export function escapeHtml(text: string): string {
  return text.replace(/[&<>]/g, (ch) => HTML_ESCAPES[ch] ?? ch)
}

/** 片段种类 → 颜色修饰类；未列出的（plain / ident）直接输出原文，少一层 DOM。 */
const TOKEN_CLASS: Partial<Record<JsTokenKind, string>> = {
  comment: 'jse-tok--comment',
  string: 'jse-tok--string',
  template: 'jse-tok--template',
  regex: 'jse-tok--regex',
  number: 'jse-tok--number',
  keyword: 'jse-tok--keyword',
  literal: 'jse-tok--literal',
  builtin: 'jse-tok--builtin',
  call: 'jse-tok--call',
  punct: 'jse-tok--punct',
  op: 'jse-tok--op',
}

/**
 * 渲染高亮 HTML（供高亮层 `<pre>` 使用）。
 *
 * 只在原文外包 span，**不增删任何字符**：与上层 textarea 是逐字符对齐的两层文本。
 * 结尾补一个换行是有意的 —— 源码以换行结尾时 `<pre>` 会比 textarea 少一行，
 * 滚到底部会错位一行；补上后高亮层永远「够高」，两层 scrollTop 才能对齐。
 */
export function renderJsHighlight(code: string): string {
  let html = ''
  for (const token of tokenizeJs(code)) {
    const escaped = escapeHtml(token.text)
    const modifier = TOKEN_CLASS[token.kind]
    html += modifier ? `<span class="jse-tok ${modifier}">${escaped}</span>` : escaped
  }
  return `${html}\n`
}
