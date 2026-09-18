import { describe, expect, it } from 'vitest'
import { escapeHtml, renderJsHighlight, tokenizeJs, type JsToken } from './js-highlight'

/** 取某个片段种类的全部文本，断言着色时比逐个查表更直观。 */
function textsOfKind(tokens: JsToken[], kind: string): string[] {
  return tokens.filter((token) => token.kind === kind).map((token) => token.text)
}

/** 一组「必须能原样拼回去」的样本：含各种容易漏字符的写法。 */
const SAMPLES = [
  '',
  'return [{ label: "余额", value: "¥12.30", color: "green" }]',
  '// 行注释\nconst a = 1 /* 块注释 */ + 2\n',
  'const res = await http.get(`https://x.dev/${id}?a=1`, { headers: { Authorization: `Bearer ${secrets.get(\'K\')}` } })',
  'if (!res.ok) return [{ label: "失败", value: `HTTP ${res.status}`, color: "red" }]',
  'const re = /ab+c/gi\ntext.replace(/[a-z/]+/g, "")',
  'const half = total / 2 / 3\nconst ratio = (a + b) / c',
  'const big = 1_000.5e3n, hex = 0xFF, oct = 0o17, bin = 0b1010',
  'const s = \'it\\\'s\'\nconst t = "line\\nbreak"',
  'const broken = "未闭合的字符串\nconst next = 1',
  'const unfinished = `模板没写完 ${a + 1',
  'const nested = `a${`b${c}d`}e`',
  'const obj = { a: { b: 1 } }\nconst spread = { ...obj, ["k"]: 1 }',
  'a?.b ?? c; d **= 2; e >>>= 1; f = g => g + 1',
  '中文标识符不合法，但字符串里的中文要原样保留',
  'const 未知 = 1',
  'const x = /\\//; const y = a / b;',
  'function f() {}; class C extends Object {}',
  'const emoji = "🙂"\n',
]

describe('tokenizeJs', () => {
  it('keeps every character of the source (highlight layer must match the textarea)', () => {
    for (const sample of SAMPLES) {
      expect(tokenizeJs(sample).map((token) => token.text).join('')).toBe(sample)
    }
  })

  it('never loops forever on unfinished source', () => {
    // 未闭合的模板串 / 字符串 / 块注释：扫描必须收敛（用「能跑完」本身当断言）。
    for (const sample of ['`', '${', '"', "'", '/*', '`${`', '/*/']) {
      expect(tokenizeJs(sample).map((token) => token.text).join('')).toBe(sample)
    }
  })

  it('tags keywords, literals, builtins and call names', () => {
    const tokens = tokenizeJs('const ok = await http.get(url); return null')
    expect(textsOfKind(tokens, 'keyword')).toContain('const')
    expect(textsOfKind(tokens, 'keyword')).toContain('await')
    expect(textsOfKind(tokens, 'keyword')).toContain('return')
    expect(textsOfKind(tokens, 'builtin')).toContain('http')
    expect(textsOfKind(tokens, 'call')).toContain('get')
    expect(textsOfKind(tokens, 'literal')).toContain('null')
  })

  it('tags sandbox host objects as builtins', () => {
    const tokens = tokenizeJs('secrets.get("K"); cache.set("k", 1); format.pct(0.4); ctx.endpoint')
    expect(textsOfKind(tokens, 'builtin')).toEqual(['secrets', 'cache', 'format', 'ctx'])
  })

  it('tags comments, strings, templates, numbers and regexes', () => {
    const tokens = tokenizeJs('// c\nconst s = "x"\nconst t = `a${1}b`\nconst n = 0x1F\nconst r = /a.c/g')
    expect(textsOfKind(tokens, 'comment')).toEqual(['// c'])
    expect(textsOfKind(tokens, 'string')).toEqual(['"x"'])
    expect(textsOfKind(tokens, 'number')).toEqual(['1', '0x1F'])
    expect(textsOfKind(tokens, 'regex')).toEqual(['/a.c/g'])
    // 模板串拆成「反引号 / 字面量 / ${ / } 」多段，拼起来仍是原文。
    expect(textsOfKind(tokens, 'template').join('')).toBe('`a${}b`')
  })

  it('highlights the whole template literal including interpolated code', () => {
    const tokens = tokenizeJs('`Bearer ${secrets.get("K")}`')
    // `${}` 里的标识符按宿主 API 着色，说明交叠扫描没把整串当纯文本吞掉。
    expect(textsOfKind(tokens, 'builtin')).toEqual(['secrets'])
    expect(textsOfKind(tokens, 'call')).toEqual(['get'])
  })

  it('treats a slash after a value as division and after an operator as a regex', () => {
    const division = tokenizeJs('const half = total / 2')
    expect(textsOfKind(division, 'regex')).toEqual([])

    const regex = tokenizeJs('const re = /ab+/g')
    expect(textsOfKind(regex, 'regex')).toEqual(['/ab+/g'])
  })

  it('does not treat a line-crossing slash as a regex', () => {
    // `a / b` 后面那行开头是注释：正则扫描失败必须退回运算符，不能把两行吞进一个片段。
    const tokens = tokenizeJs('const a = b / c\n// 注释')
    expect(textsOfKind(tokens, 'regex')).toEqual([])
    expect(textsOfKind(tokens, 'comment')).toEqual(['// 注释'])
  })
})

describe('escapeHtml', () => {
  it('escapes tag delimiters so scripts cannot inject markup', () => {
    expect(escapeHtml('<script>alert("x")</script>')).toBe(
      '&lt;script&gt;alert("x")&lt;/script&gt;',
    )
    expect(escapeHtml('a & b')).toBe('a &amp; b')
  })
})

describe('renderJsHighlight', () => {
  it('renders spans for coloured kinds and plain text for the rest', () => {
    const html = renderJsHighlight('const a = 1')
    expect(html).toContain('<span class="jse-tok jse-tok--keyword">const</span>')
    expect(html).toContain('<span class="jse-tok jse-tok--number">1</span>')
    // 标识符与空白不着色：不加 span，省 DOM。
    expect(html).toContain(' a ')
  })

  it('always ends with a newline so the highlight layer can scroll as far as the textarea', () => {
    expect(renderJsHighlight('')).toBe('\n')
    expect(renderJsHighlight('a\n')).toBe('a\n\n')
  })

  it('keeps the rendered text identical to the source', () => {
    // 剥掉 span 标签、还原 HTML 实体后必须与原文一致（这是两层不错位的根据）。
    for (const sample of SAMPLES) {
      const stripped = renderJsHighlight(sample)
        .replace(/<span class="jse-tok [^"]+">/g, '')
        .replace(/<\/span>/g, '')
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&amp;/g, '&')
      expect(stripped).toBe(`${sample}\n`)
    }
  })
})
