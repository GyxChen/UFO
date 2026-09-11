import { getAllWindows, getCurrentWindow } from '@tauri-apps/api/window'

/** 显示并聚焦主窗口。 */
export async function showMainWindow() {
  const main = (await getAllWindows()).find(({ label }) => label === 'main')
  if (!main) {
    return
  }

  await main.unminimize()
  await main.show()
  await main.setFocus()
}

/** 隐藏主窗口（关闭按钮走托盘隐藏，不退出进程）。 */
export async function hideMainWindow() {
  const main = (await getAllWindows()).find(({ label }) => label === 'main')
  if (!main) {
    return
  }
  await main.hide()
}

/** 开始拖拽当前窗口（自绘标题栏用）。 */
export async function startCurrentWindowDrag() {
  await getCurrentWindow().startDragging()
}
