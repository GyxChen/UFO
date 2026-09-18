---
name: commit
description: 把当前工作区改动整理成一次简洁的中文 git commit（只提交、不推送、不 amend）。用户说「提交」「commit」「整理成一次提交」「写提交信息」时使用。
---

# commit

把当前工作区改动整理成一次 git commit。要求与 `.claude/commands/commit.md`（Claude Code 版）保持一致；改规则时两边一起改。

## 流程

1. 并行收集仓库状态（不要用 interactive 命令）：
   - `git status`
   - `git diff` 与 `git diff --staged`
   - `git log -5 --oneline`（对齐本仓库提交语气：`[u]……`）
2. 若没有任何改动（含未暂存/已暂存），直接告知用户并结束，不要空提交。
3. 用**一段话**总结本次改动意图与效果（不要写成条目清单），长度按下方「摘要写法」严格收敛。
4. 若存在相关未跟踪文件，按需 `git add`；不要添加疑似密钥/凭据（`.env`、`*.pem`、`credentials` 等）。用户若给了补充说明，把它当作提交重点纳入摘要。
5. 提交。本机 Codex 跑的是 PowerShell，用 here-string 传多行 message（Git Bash 下可换成 `git commit -m "$(cat <<'EOF' … EOF)"`）：

```powershell
git commit -m @'
<一段话摘要>

@C
'@
```

6. 提交后跑 `git status` 确认成功。
7. **不要 push**。除非用户明确要求，否则不要 amend。

## 摘要写法（重要）

- **标题行**：不超过 ~72 字符，一句话说清「这次改动的核心是什么」，带 `[u]` 前缀。
- **正文**：紧跟标题行空一行，控制在 **3–4 行以内、总计不超过 400 字**。只讲三件事：改了什么、为什么、修了哪些关键问题。
  commit message 只需让人（和 `git log --oneline`）一眼看懂这次为什么动。
- 关键 bug 修复可以点一句（「修了 X 导致 Y 空转」），但不要展开成因与验证过程。

## 约束

- 只做 commit，不做 push / force / reset hard。
- commit message 末尾只加一行 `@C`；不要写 `Co-Authored-By: Claude …` 这类署名。
- 摘要必须是一段完整中文（可带 `[u]` 前缀），说明「改了什么、为什么」。
- 若 hooks 修改了文件导致 commit 失败，按提示处理后再提交一次；不要反复空转。