---
description: 总结当前改动为简洁摘要并 git commit
allowed-tools: Bash(git *), Bash(rg *)
---

将当前工作区改动整理成一次 git commit。

## 流程

1. 并行收集仓库状态（不要用 interactive 命令）：
   - `git status`
   - `git diff` 与 `git diff --staged`
   - `git log -5 --oneline`（对齐本仓库提交语气）
2. 若没有任何改动（含未暂存/已暂存），直接告知用户并结束，不要空提交。
3. 用**一段话**总结本次改动意图与效果（不要写成条目清单），**长度按下方「摘要写法」严格收敛**。优先对齐本仓库近期风格，例如：`[u]……`。
4. 若存在相关未跟踪文件，按需 `git add`；不要添加疑似密钥/凭据（`.env`、`*.pem`、`credentials` 等）。若用户给了 `$ARGUMENTS`，把它当作提交重点或补充说明纳入摘要。
5. 用 HEREDOC 提交（Windows Git Bash 可用）：

```bash
git commit -m "$(cat <<'EOF'
<一段话摘要>

@C
EOF
)"
```

6. 提交后跑 `git status` 确认成功。
7. **不要 push**。除非用户在 `$ARGUMENTS` 里明确要求，否则不要 amend。

## 摘要写法（重要）

- **标题行**：不超过 ~72 字符，一句话说清"这次改动的核心是什么"。
- **正文**：紧跟标题行空一行，控制在 **3–4 行以内、总计不超过 400 字**。
  只讲三件事：改了什么、为什么、修了哪些关键问题。
  commit message 只需让人（和 `git log --oneline`）一眼看懂这次为什么动。
- 关键 bug 修复可以点一句（"修了 X 导致 Y 空转"），但不要展开成因与验证过程。

## 约束

- 只做 commit，不做 push / force / reset hard。
- 摘要必须是一段完整中文（可带 `[u]` 前缀），说明「改了什么、为什么」。
- 若 hooks 修改了文件导致 commit 失败，按提示处理后再提交一次；不要反复空转。
- commit message 末尾只加一行 `@C`；不要写 `Co-Authored-By: Claude …` 这类署名。