import { getAllWindows } from '@tauri-apps/api/window'

export async function showMainWindow() {
  const main = (await getAllWindows()).find(({ label }) => label === 'main')
  if (!main) {
    return
  }

  await main.unminimize()
  await main.show()
  await main.setFocus()
}
