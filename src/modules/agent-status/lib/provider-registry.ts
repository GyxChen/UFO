import claudeIcon from '@/assets/claude.svg'
import codexIcon from '@/assets/codex.svg'

/**
 * ── provider 展示元信息注册表 ──
 *
 * 「接入一个新平台」在前端只应该改这一张表。此前 Claude / Codex 的名称、图标、是否反相
 * 用三元表达式散在 4 个视图里，且 `isStatusSnapshot` 按字面量白名单校验 provider ——
 * 新平台的每一帧推送都会被整帧丢弃，灯冻结在首帧（首帧走 invoke，不经这道校验）。
 * 收敛成注册表 + 未登记即降级显示，接入面从「改 6 处」降到「加 1 行」。
 * 规范全文见 `reference/doc/agent-status-platform-onboarding.md`。
 */
export interface ProviderMeta {
  /** 可见名称与读屏文案（如 `Claude`）。 */
  label: string
  /** 平台图标；未登记平台为 null，调用方渲染首字符徽标。 */
  icon: string | null
  /** 图标是否需要反相（纯白单色图标在浅色主题下要反相）。 */
  mono: boolean
}

/** 已登记平台的展示元信息；key 与 Rust 侧 `Provider::as_str()` 一致。 */
const PROVIDER_REGISTRY: Record<string, ProviderMeta> = {
  claude: { label: 'Claude', icon: claudeIcon, mono: false },
  // Codex 的图标是纯白单色，浅色主题下要反相；Claude 的橙反相会变青蓝，不套。
  codex: { label: 'Codex', icon: codexIcon, mono: true },
}

/**
 * 取某平台的展示元信息；未登记平台**必须仍能出灯**，因此这里永不返回 undefined。
 *
 * 未知平台回落「原始串当名字 + 无图标」，而不是猜一个已登记平台 ——
 * 猜错会把新平台的会话冒充成 Claude / Codex，比不显示更糟。
 */
export function providerMeta(provider: string): ProviderMeta {
  return PROVIDER_REGISTRY[provider] ?? { label: provider, icon: null, mono: false }
}

/** 已登记的平台 id 清单（护栏测试据此断言注册表与 `Provider` 联合类型同源）。 */
export const KNOWN_PROVIDERS: string[] = Object.keys(PROVIDER_REGISTRY)

/**
 * 未登记平台的首字符徽标文字（无图标时可读）。
 *
 * 按码点取而非 `slice`：emoji 等代理对字符用 `slice(0, 1)` 会切出半个码点，渲染成乱码。
 */
export function providerFallbackMark(provider: string): string {
  const mark = [...provider.trim()][0]
  return mark ? mark.toUpperCase() : '?'
}
