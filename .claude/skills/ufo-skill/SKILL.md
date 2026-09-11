---
name: ufo-skill
description: UFO 项目悬浮窗开发约束与交互规范。
---

# UFO Skill

## 悬浮窗独立交互元素

悬浮窗外层使用统一的打开窗口和拖拽交互。以后若在悬浮窗里加入真实按钮、链接或其他独立点击行为，需要在这些元素上阻止事件冒泡，避免触发外层打开主窗口的逻辑。

Vue 示例：

```vue
<button type="button" @click.stop="handleAction">执行操作</button>
<a href="/settings" @click.stop>设置</a>
```
