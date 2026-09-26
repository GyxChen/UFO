/**
 * 推送服务的展示元信息：全模块只在这里认「有哪些服务」。
 *
 * ⚠️ 结构不在这里写：服务级只有一个服务器地址，它的标签 / 默认值 / 说明由后端 provider 的
 * `ServerSpec` 随快照送上来；收件方固定是「名字 + 主题」两列。所以加一个推送服务时，
 * 前端只需要在本表加一行文案，不必再抄一遍字段清单（抄了就会有两处真相）。
 * 同源性由 `push-providers.spec.ts` 读 Rust 源码断言。
 */

/** 一个推送服务的展示信息。 */
export interface PushProviderMeta {
  /** 显示名（后端也送 label，这里用于「后端还没回话」时的界面占位）。 */
  label: string
  /** 一句话说明这服务是什么，显示在服务行下方。 */
  blurb: string
  /** 该服务下「一行主题」的量词（Bark 那串叫设备密钥）。只影响文案，不影响控制流。 */
  topicNoun: string
}

/** 已登记服务的展示元信息；key = 后端 provider id。 */
const PUSH_PROVIDER_META: Record<string, PushProviderMeta> = {
  ntfy: {
    label: 'ntfy',
    blurb: '订阅式公开主题，可自建服务器',
    topicNoun: '主题',
  },
  bark: {
    label: 'Bark',
    blurb: '一串密钥一台设备，可加多行并命名',
    topicNoun: '设备密钥',
  },
}

/**
 * 取某服务的展示信息；未登记服务**必须仍然可用**，因此永不返回 undefined。
 *
 * 未登记时回落「原始 id 当名字 + 通用量词」：后端加了服务而这里漏一行，
 * 面板照样能配、照样能发，只是文案糙一点。
 */
export function pushProviderMeta(id: string): PushProviderMeta {
  return PUSH_PROVIDER_META[id] ?? {
    label: id,
    blurb: '未登记展示信息，服务器地址按后端送来的说明填',
    topicNoun: '主题',
  }
}

/** 已登记的服务 id（护栏测试据此与 Rust `build_providers()` 比对）。 */
export const KNOWN_PUSH_PROVIDERS: string[] = Object.keys(PUSH_PROVIDER_META)
