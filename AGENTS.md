# AGENTS.md: open-pptd-skill Agent 指南

欢迎来到 `open-pptd-skill` 仓库。对于任何 PPT 创建、编辑与交付任务，请严格遵守以下操作守则与标准流程。

---

## ⚡ Agent Quickstart (极速闭环)

接到 PPT 任务后，直接运行以下标准化流程：

```bash
# 1. 确保编译器已就绪
python3 skills/open-pptd/scripts/install_binary.py

# 2. 严格语义与结构校验
pptd check <project_dir>/deck.pptd

# 3. 极速编译为原生 PPTX
pptd build <project_dir>/deck.pptd -o <output.pptx> [--force]
# 4. 成品质量与合规性验收 (无泄漏标签、页数与备注检查)
python3 skills/open-pptd/scripts/validate_pptx.py <output.pptx> --expected-pages <N> --check-notes
```

---

## 📚 建议阅读顺序 (Reading Order)

在编写 PPTD 源码前，请按序查阅：
1. `skills/open-pptd/reference/slides_categories.md` ➔ 确定 PPT 业务场景与受众
2. `skills/open-pptd/reference/slides_categories/<scenario>.md` ➔ 掌握版式结构与密度红线
3. `theme.md` (或 `theme_EN.md`) ➔ 选用预设 30+ 种专业主题配色方案
4. `skills/open-pptd/reference/fonts.md` ➔ 查阅跨平台字体映射与回退机制
5. `skills/open-pptd/SKILL.md` 与 `skills/open-pptd/reference/pptd.md` ➔ 语法规范（严禁使用暂不支持的元素）

---

## ⚠️ 严禁使用的非支持特性 (Feature Boundaries)

以下为 `pptd` 编译器的特性支持与禁止边界（非支持语法会在 `check` / `build` 与质检阶段**直接拦截报错**）：
- ❌ **严禁使用 HTML 列表标签**：`<ul>`, `<ol>`, `<li>`。
  - 列表写法：使用常规段落配合文字符号（如 `• 第一点` 或 `1. 第一点`）。
- ✅ **行内超链接与上下标标签**：支持 `<a href="https://…">文本</a>`（编译为 run 级 `<a:hlinkClick>` 外部关系）以及 `<sup>` / `<sub>`（编译为 run 级 `baseline` 偏移与字号缩放）。复杂数学表达式仍可使用原生 LaTeX 数学公式 `<latex>…</latex>` 或 `\( … \)` 编译为可编辑 OOXML 公式。
- ✅ **原生图表透传**：`elementType: chart` 支持**原生图表部件**（`chartPart` 指向工程内 `media/chartN.xml`，可选 `workbookPart`）。这类元素由 `pptd convert` 自动产出，图表 XML 按原字节回写，请勿手写或改写图表 XML。
  - 若需要新建图表，请使用原生表格 `elementType: table`，或用外部脚本渲染为图片后以 `elementType: image` 引入；声明式 `data`/`series` 图表语法尚未编译。
---

## 🛠️ 异常分支与恢复指引 (Failure Branches)

1. **下载或网络受限**：
   - `install_binary.py` 会自动回退至固定 tag 的 `cargo install --git … --tag v<version>`。
   - 若本地无 Rust 工具链，可从 Release 页面手动下载归档与 `SHA256SUMS.txt`，本地校验后解压至 `~/.local/bin/pptd`。
2. **`pptd check` 报错**：
   - 检查 `shapeName` 是否拼写错误（必须为 DrawingML 预设形状如 `roundRect`, `rect`, `ellipse`, `diamond` 等）。
   - 检查 `$color` 引用是否已在 `theme.colors` 中声明。
   - 检查图片路径是否存在（统一放置于 `assets/` 目录）。
3. **`pptd build` 退出码 2 (.sync.hash 覆盖保护)**：
   - 说明目标 `.pptx` 在上次编译后被外部程序编辑过。若确认以 PPTD 源码为准覆盖外部修改，在命令中追加 `--force` 参数重新 build：`pptd build <project_dir>/deck.pptd -o <output.pptx> --force`；或在确认放弃外部修改后删除原文件再重新 build。
4. **`validate_pptx.py` 报错**：
   - 若报 `Leaked HTML tags`，排查源码并移除 `<ul>`/`<ol>`/`<li>` 等未支持标签。
   - 若页数不符，排查 `deck.pptd` 中的 `pages` 列表。
