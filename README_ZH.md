# Open PPTD

[English](README.md) | [日本語](README_JA.md) | [简体中文](README_ZH.md)

面向 AI Coding Agent 与开发者的离线声明式 PowerPoint（`.pptx`）编译器与 Agent Skill。用 YAML（`.pptd` + `.page`）编写文稿，经由原生 Rust 编译器 `pptd` 编译为可编辑 `.pptx`，亦可将已有 `.pptx` 反向转为 PPTD。

## 安装

复制下方指令，直接发给您的 AI Coding Agent（Codex、Claude Code、Cursor 等）：

```text
请帮我安装并配置 open-pptd 技能：
1. 执行：npx github:Sollomy/open-pptd-skill install -y
2. 执行：python3 ~/.agents/skills/open-pptd/scripts/install_binary.py
3. 用 `pptd --version` 验证（若提示未找到，请将 `$HOME/.local/bin` 加入 `PATH`）。
4. 完成后通知我。
```

初次安装需具备 Node.js、Python 3 与网络环境。本地 `pptd` 就绪后，构建与转换均可完全离线运行。

## 用法

### 让 AI Agent 制作文稿

直接说明主题、页数与风格要求，Agent 会同时交付可二次编辑的 PPTD 源码目录与编译好的 `.pptx` 文件。

```text
用 open-pptd 制作一份 8 页「分布式系统架构」演讲 PPT：
- 主题：deep-blue-atlas
- 包含封面、带箭头连线的架构图、核心指标对比表，以及每页演讲者备注
- 完成后运行 validate_pptx.py 校验通过
```

### 终端直接使用 CLI

```bash
pptd check my_deck/deck.pptd
pptd build my_deck/deck.pptd -o slides.pptx
pptd convert slides.pptx ./my_deck/
python3 ~/.agents/skills/open-pptd/scripts/validate_pptx.py slides.pptx --expected-pages 8 --check-notes
```

## 文档

- [完整指南与技术参考（英文）](docs/GUIDE.md) — 作品示例、安装原理、功能特性、能力边界、手动安装与测试指南
- [Agent 技能指引](skills/open-pptd/SKILL.md)
- [PPTD v2 语法规范](skills/open-pptd/reference/pptd.md)
- [主题配色方案](theme.md)
- [CLI 命令参考](skills/open-pptd/reference/cli.md)

## 许可证

本项目采用 [GNU Affero General Public License v3.0 或更高版本（`AGPL-3.0-or-later`）](LICENSE)。上游 MIT 版权与许可声明（`shiquda/open-pptd-skill`、`open-kimi-ppt-skill`、`slideforge`）及 Font Awesome Free 6.7.2（`CC BY 4.0`）署名保留于 [LICENSE-MIT](LICENSE-MIT) 与 [NOTICE](NOTICE)。
