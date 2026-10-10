/**
 * 能力 / 挂载点注册表的前端那一份。
 *
 * 语义只有一份（Rust `src-tauri/src/plugin/capability.rs` 才是权威：装前披露、注入闸门都读它），
 * 这里**手抄一份字面量**专给护栏用 —— `plugin-capabilities.spec.ts` 双向对撞：
 * 名字、徽标文字、配色档、图标键、披露原文逐条比，并且「Rust 新增一条而这里没登记」也红。
 *
 * 界面上真正渲染时读的是命令 `get_plugin_registry` 返回的事实表（见 `stores/plugins.ts`），
 * 所以这份抄本不会被业务代码 import 成「第二个真值」。
 */

/** 一条声明的界面语义。 */
export interface PluginCapabilityRow {
  /** 清单里写的名字。 */
  id: string
  /** `permission` = 宿主能力（要授权）；`surface` = 界面挂载点（只声明插在哪）。 */
  kind: 'permission' | 'surface'
  /** 徽标上的一行文字。 */
  label: string
  /** 图标键（真路径在 `plugin-icons.ts`）。 */
  icon: string
  /** 配色档。 */
  tone: 'danger' | 'caution' | 'local'
  /** 装前确认弹窗里的逐条披露原文。 */
  disclosure: string
}

/** 宿主能力表（顺序 = 徽标与披露的渲染顺序）。 */
export const PLUGIN_PERMISSIONS: readonly PluginCapabilityRow[] = [
  {
    id: 'events:agent',
    kind: 'permission',
    label: '订阅转灯事件',
    icon: 'bell',
    tone: 'danger',
    disclosure:
      '可读取你发给 AI 的提问原文，其中可能含密钥或私有代码；组件可把它发往外网，发出后不受控。',
  },
  {
    id: 'fs',
    kind: 'permission',
    label: '读写文件',
    icon: 'folder',
    tone: 'danger',
    disclosure:
      '读你亲手在对话框里选中的文件（每次都要重新选），并可在组件私有目录里写任意字节；选中的若是压缩包，能读到里面每一项。',
  },
  {
    id: 'http',
    kind: 'permission',
    label: 'ufo.http 出网',
    icon: 'globe',
    tone: 'caution',
    disclosure:
      '由宿主代发网络请求，只能访问清单 hosts 里列出的域名；请求体与响应内容都在组件手里。',
  },
  {
    id: 'secrets',
    kind: 'permission',
    label: '读取密钥',
    icon: 'key',
    tone: 'caution',
    disclosure: '从系统凭据管理器按名字取值；值不落盘、不进日志、界面上不回显。',
  },
  {
    id: 'ble',
    kind: 'permission',
    label: '蓝牙连接与收发',
    icon: 'bluetooth',
    tone: 'caution',
    disclosure:
      '扫描并连接蓝牙设备、收发数据；连接会一直保留到你在管理区断开或卸载组件，期间占用无线电与电量。',
  },
  {
    id: 'serial',
    kind: 'permission',
    label: '串口读写',
    icon: 'plug',
    tone: 'caution',
    disclosure: '打开串口并读写；端口被占用期间其它程序拿不到它。',
  },
  {
    id: 'frame',
    kind: 'permission',
    label: '内嵌网页',
    icon: 'frame',
    tone: 'caution',
    disclosure:
      '在组件界面里嵌入网页（只嵌 https 页面）。嵌入的页面看起来像真站，但可能不是它；多数站点禁止被嵌，显示不出来是正常现象。',
  },
  {
    id: 'storage',
    kind: 'permission',
    label: 'ufo.storage 按 id 隔离',
    icon: 'database',
    tone: 'local',
    disclosure: '在自己的分区里存数据（按包 id 隔离，卸载即删），有容量上限。',
  },
]

/** 界面挂载点表（不授权任何东西，一律 local 档）。 */
export const PLUGIN_SURFACES: readonly PluginCapabilityRow[] = [
  {
    id: 'cell',
    kind: 'surface',
    label: '悬浮窗格子',
    icon: 'grid',
    tone: 'local',
    disclosure: '在悬浮窗里申请格子，与内置组件同级共享同一块窗。',
  },
  {
    id: 'tooltip-own',
    kind: 'surface',
    label: '独立信息卡（tooltip）',
    icon: 'card',
    tone: 'local',
    disclosure: '悬停它自己的格子时弹出这个组件自己的信息卡。',
  },
  {
    id: 'tooltip-extension',
    kind: 'surface',
    label: '信息卡插入块',
    icon: 'insert',
    tone: 'local',
    disclosure: '往别的组件（如状态灯）的信息卡尾部追加一块，最多 3 行。',
  },
  {
    id: 'settings',
    kind: 'surface',
    label: '主窗设置页',
    icon: 'gear',
    tone: 'local',
    disclosure: '在主窗口一级页签里拥有自己的设置页。',
  },
]

/** 两张表拼成一张（徽标渲染时按 id 查这一份）。 */
export const PLUGIN_CAPABILITIES: readonly PluginCapabilityRow[] = [
  ...PLUGIN_PERMISSIONS,
  ...PLUGIN_SURFACES,
]

/** 按 id 查一条声明；查不到返回 undefined（外壳不解释未知名字）。 */
export function findCapability(id: string): PluginCapabilityRow | undefined {
  return PLUGIN_CAPABILITIES.find(row => row.id === id)
}
