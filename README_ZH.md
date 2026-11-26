# Open PPTD

[English](README.md) | [日本語](README_JA.md) | [简体中文](README_ZH.md)

通过 AI 助手在本地离线制作和修改原生 PowerPoint（`.pptx`）演示文稿的工具。

生成的每一页都是可在 PowerPoint 中自由编辑的真实文字、表格、图形与演讲者备注，而不是整页图片。整个过程都在本地电脑完成，无需把业务资料上传云端；也可以导入已有的 `.pptx` 文件，让 AI 协助更新内容或整理排版。

## 安装

您不需要自己敲命令。直接复制下面这段话，发给可以在本地执行操作的 AI 助手（如 Claude Code、Codex、Cursor、WorkBuddy 等）即可：

```text
请帮我安装并配置好 open-pptd 技能：
1. 执行：npx github:Sollomy/open-pptd-skill install -y
2. 执行：python3 ~/.agents/skills/open-pptd/scripts/install_binary.py
3. 用 `pptd --version` 确认可用（若提示找不到命令，请将 `$HOME/.local/bin` 加入 `PATH`）。
4. 全部准备好后告诉我。
```

仅首次安装需要 Node.js、Python 3 和网络连接。如果电脑里还没有装这两项，可以直接请 AI 助手先帮您装好。安装完成后，日常制作与修改 PPT 均可完全离线运行。

## 用法

像平时安排工作一样直接说明需求，或参考下方模板按需替换内容即可。AI 会交付一份可以直接打开的 `.pptx` 文件，并保留一份源文件目录，方便您日后随时微调而无需重做。

**新建一份 PPT：**

```text
使用 open-pptd 为我制作一份关于「分布式架构演进」的 8 页商业技术演讲 PPT：
- 风格主题参考：深色金融科技风（ebony-ledger）或深蓝学术风（deep-blue-atlas）
- 核心内容要求：
  1. 包含封面、架构演进痛点、微服务架构蓝图（必须包含带箭头的连接线与矢量容器框）、核心技术指标对比表格；
  2. 每页均需包含完整的演讲者备注（notes: "..."），供演练参考；
  3. 严格使用 LaTeX 原生数学公式呈现核心算法；
- 交付标准：生成完整的 PPTD 工程及编译好的 PPTX 文件，并在完成后运行 validate_pptx.py 质检通过。
```

**修改已有的 PPT 文件：**

```text
用 open-pptd 打开 report.pptx，把第 4 页的第三季度数据更新一下，顺便把版面整理得更整齐，然后导出为 report_updated.pptx。
```

## 更多资料

- [完整指南与技术参考（英文）](docs/GUIDE.md) — 效果示例、命令行直接用法、功能清单、当前限制、手动安装与测试说明
- [主题配色方案](theme.md) — 可直接按名称指定的内置配色风格
- [技能执行指引](skills/open-pptd/SKILL.md) — AI 助手自动读取的制图与校验规则

## 许可证

本项目采用 [GNU Affero General Public License v3.0 或更高版本（`AGPL-3.0-or-later`）](LICENSE)。上游 MIT 版权与许可声明（`shiquda/open-pptd-skill`、`open-kimi-ppt-skill`、`slideforge`）及 Font Awesome Free 6.7.2（`CC BY 4.0`）署名保留于 [LICENSE-MIT](LICENSE-MIT) 与 [NOTICE](NOTICE)。
