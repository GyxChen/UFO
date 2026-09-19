/**
 * 启动期 IPC 重试：窗口的 JS 可能比后端 `setup` 里的 `.manage()` 早一步跑起来。
 *
 * Tauri 先建窗口、后跑 `setup`，所以冷启动时首帧命令会偶发落到「还没注册的状态」上，
 * 报 `state not managed for field \`state\` on command ...`。这类失败都是「再等一下就好」，
 * 不该让调用方退化成空数据，于是统一在这里短暂重试；重试用尽才把最后一次错误抛出去。
 */

/** 默认最多尝试次数（含首次）。 */
export const STARTUP_RETRY_ATTEMPTS = 6

/** 默认每次失败后的等待时长（毫秒）；6 × 250ms ≈ 1.5s，足够跨过 `setup` 的耗时。 */
export const STARTUP_RETRY_DELAY_MS = 250

/**
 * 执行一个启动期的异步动作，失败后按固定节奏重试。
 *
 * @param task 要执行的动作（通常是某个命令的 `invoke` 包装）
 * @param attempts 最多尝试次数（含首次），默认 [`STARTUP_RETRY_ATTEMPTS`]
 * @param delayMs 每次失败后的等待时长，默认 [`STARTUP_RETRY_DELAY_MS`]
 * @returns 首次成功的结果
 * @throws 全部尝试都失败时，抛出最后一次的错误（调用方据此决定如何退化）
 */
export async function retryOnStartupRace<T>(
  task: () => Promise<T>,
  attempts: number = STARTUP_RETRY_ATTEMPTS,
  delayMs: number = STARTUP_RETRY_DELAY_MS,
): Promise<T> {
  let lastError: unknown
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      return await task()
    } catch (error) {
      lastError = error
      // 最后一次失败后不必再等，直接把错误交给调用方。
      if (attempt < attempts - 1) {
        await new Promise(resolve => setTimeout(resolve, delayMs))
      }
    }
  }
  throw lastError
}
