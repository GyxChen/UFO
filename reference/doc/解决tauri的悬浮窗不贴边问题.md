# 解决 Tauri 悬浮窗无法贴到屏幕边缘

在 Tauri 中制作无边框悬浮窗时，一个常见现象是：窗口已经计算到了屏幕边缘，但视觉上左右仍然存在几像素的间隙。

如果悬浮窗使用了原生窗口阴影，优先检查窗口配置中的 `shadow`。在 Windows 等桌面平台上，原生阴影可能绘制在窗口外框之外。窗口坐标虽然已经贴到工作区边界，但阴影仍会向外扩展，于是看起来像是窗口没有真正贴边。

对于需要无缝贴边的悬浮窗，关键修复通常是：

```json
"shadow": false
```

## 问题示例

一个典型的 Tauri 悬浮窗配置可能如下：

```json
{
  "label": "floating",
  "width": 56,
  "height": 56,
  "decorations": false,
  "alwaysOnTop": true,
  "skipTaskbar": true,
  "shadow": true
}
```

其中：

- `decorations: false` 隐藏标题栏和系统边框。
- `alwaysOnTop: true` 让窗口保持置顶。
- `skipTaskbar: true` 让窗口不显示在任务栏。
- `shadow: true` 启用原生窗口阴影。

即使应用代码已经把窗口左上角设置为显示器工作区的最左侧或最右侧，启用的阴影仍可能在窗口外侧留下可见间距。

## 最小修复

将 `shadow` 改为 `false`：

```json
{
  "label": "floating",
  "width": 56,
  "height": 56,
  "decorations": false,
  "alwaysOnTop": true,
  "skipTaskbar": true,
  "shadow": false
}
```

修改后需要完全重启 Tauri 应用。窗口配置是在原生窗口创建时读取的，Vite 的热更新不会重新创建已经存在的窗口。

重启后再次将悬浮窗拖到屏幕左侧和右侧，通常即可看到窗口无缝贴到工作区边界。

## 为什么关闭阴影可以解决贴边问题

悬浮窗的贴边过程实际上包含两个边界：

1. **几何边界**：窗口坐标和窗口尺寸决定的原生窗口外框。
2. **视觉边界**：用户实际看到的内容，包括背景、圆角和窗口阴影。

吸附代码通常只计算几何边界。例如，工作区右侧为 `areaRight`，窗口宽度为 `windowWidth`，窗口贴右侧时左坐标应该是：

```ts
const rightPosition = areaRight - windowWidth
```

这段计算可以是正确的，但原生阴影并不属于窗口内容本身，它可能绘制在窗口外框之外。因此会出现：

- 坐标已经等于预期值；
- 窗口外框已经到达工作区边界；
- 但阴影扩展到了边缘之外；
- 用户看到的阴影或阴影留白造成“没有贴边”的错觉。

将 `shadow` 设为 `false` 后，系统不再额外绘制窗口外侧阴影，视觉边界就会与窗口外框更加一致。对于需要贴屏幕边缘的悬浮按钮、侧边工具条、桌面小组件等窗口，这是最直接的处理方式。

## 仍然需要检查的两个尺寸

关闭阴影后，如果仍然存在固定间距，还需要确认以下尺寸是否一致。

### 原生窗口尺寸和可见内容尺寸

例如原生窗口是 `72 × 72`，但内部按钮只有 `56 × 56`：

```json
{
  "width": 72,
  "height": 72
}
```

```css
.floating-launcher__mark {
  width: 56px;
  height: 56px;
}
```

此时即使原生窗口已经贴边，按钮四周仍会有约 8px 的空白。应让窗口尺寸和主要可见内容一致：

```json
{
  "width": 56,
  "height": 56
}
```

或者根据实际设计保留明确的内边距。重点是区分：这类间距来自 WebView 布局，而不是窗口阴影。

### 工作区边界和整个屏幕边界

桌面系统的工作区通常会排除任务栏，因此 `currentMonitor().workArea` 的边界不一定等于显示器完整物理边界。如果代码使用的是工作区边界，窗口会贴到可用工作区边缘，而不是覆盖任务栏区域。

## 圆角窗口的额外处理

关闭 `shadow` 后，有些悬浮窗会发现圆角看起来失效。这通常不是 `shadow: false` 破坏了 CSS 圆角，而是透明窗口的页面根节点仍然有不透明背景。

例如全局样式中存在：

```css
html {
  background: #1f2937;
}
```

即使内部按钮设置了 `border-radius`，`html`、`body` 或 `#app` 的矩形背景仍可能覆盖圆角外部区域。

如果要制作透明圆角悬浮窗，可以同时配置原生窗口透明：

```json
{
  "decorations": false,
  "transparent": true,
  "shadow": false
}
```

然后仅对悬浮窗对应的页面设置透明背景：

```css
html.is-floating-window,
html.is-floating-window body,
html.is-floating-window #app {
  background: transparent;
}
```

窗口入口可以根据 Tauri 窗口标签添加这个类名：

```ts
import { getCurrentWindow } from '@tauri-apps/api/window'

const isFloatingWindow = getCurrentWindow().label === 'floating'

if (isFloatingWindow) {
  document.documentElement.classList.add('is-floating-window')
}
```

这样可以达到以下效果：

- `shadow: false` 消除原生阴影造成的边缘留白；
- `transparent: true` 允许窗口外部区域透明；
- `html`、`body` 和 `#app` 透明后，按钮的 CSS 圆角可以正常显示；
- 主窗口仍然可以保留自己的不透明背景。

## 吸附代码的基本写法

如果关闭 `shadow` 后仍然无法贴边，再检查吸附代码本身。左右吸附的核心计算如下：

```ts
const areaLeft = workArea.position.x
const areaRight = areaLeft + workArea.size.width
const maxX = Math.max(areaLeft, areaRight - windowSize.width)

let x = clamp(position.x, areaLeft, maxX)

if (Math.abs(x - areaLeft) <= threshold) {
  x = areaLeft
} else if (Math.abs(x + windowSize.width - areaRight) <= threshold) {
  x = maxX
}
```

这里需要注意：

- 左侧吸附使用 `areaLeft`。
- 右侧吸附使用 `areaRight - windowSize.width`。
- 判断右侧距离时，要比较窗口右边缘 `x + windowSize.width`。
- `threshold` 只是“距离边缘多近时触发吸附”的阈值，例如 `24` 个物理像素。
- 多显示器场景下，工作区坐标可能为负数，不能假定左上角永远是 `(0, 0)`。

窗口移动时建议使用原生窗口的物理坐标和尺寸：

```ts
const [position, size] = await Promise.all([
  window.outerPosition(),
  window.outerSize(),
])
```

这样可以避免浏览器 CSS 像素与 Windows DPI 缩放之间的换算误差。

## 排查顺序

遇到 Tauri 悬浮窗左右无法贴边时，可以按以下顺序排查：

1. 将 `tauri.conf.json` 中悬浮窗的 `shadow` 改为 `false`。
2. 完全重启 Tauri 应用，而不是只等待 Vite 热更新。
3. 检查原生窗口尺寸是否大于实际可见按钮尺寸。
4. 如果需要圆角，配置 `transparent: true`，并让 `html`、`body`、`#app` 的背景透明。
5. 确认吸附代码使用 `outerPosition()`、`outerSize()` 和当前显示器的 `workArea`。
6. 确认右侧位置使用 `areaRight - windowSize.width`，而不是直接使用 `areaRight`。
7. 在多显示器和不同 DPI 缩放比例下重新验证。

## 验证清单

完成修改后，建议验证以下场景：

- 拖到左边缘，悬浮窗是否紧贴左侧。
- 拖到右边缘，悬浮窗是否紧贴右侧。
- 拖到左上、右上、左下、右下四个角，是否可以正确吸附。
- 关闭阴影后，是否仍有来自页面布局的固定空白。
- 透明圆角窗口外部是否不再显示矩形背景。
- 多显示器副屏位于主屏左侧或上方时，负坐标是否仍能正常处理。

项目中还可以使用以下命令进行基础验证：

```bash
pnpm test --run src/lib/snap.spec.ts
pnpm build
pnpm check
```

## 结论

对于 Tauri 无边框悬浮窗无法无缝贴到屏幕边缘的问题，首先检查原生窗口阴影：

```json
"shadow": false
```

`shadow: false` 是解决视觉贴边问题的关键配置。它不会改变吸附算法的坐标计算，而是移除窗口外框之外的原生阴影，让窗口的视觉边界与实际边界保持一致。

如果关闭阴影后仍有间距，再分别检查窗口尺寸、页面内边距、透明背景和工作区坐标。这样可以快速区分是原生窗口阴影问题、WebView 布局问题，还是吸附坐标计算问题。
