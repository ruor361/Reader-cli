# Agent Terminal 阅读模式验证与运行时测试报告

本报告记录 Reader-cli Agent Terminal 阅读模式（Discreet Mode 改造）在真实 Windows ConPTY 仿真环境下的端到端自动化验收结果。

---

## 1. 验证方法与证据性质说明

- **测试后端**：基于 Windows ConPTY（通过 `pywinpty` + `pyte`），在生产级事件循环（`tui::run_reader_with_data`）中驱动真实按键并消费真实 VT 控制序列。
- **虚拟终端屏幕处理**：使用定制 `VtScreen` 响应 `CSI ? 1049 h` 序列，在进入备用屏幕缓冲（Alternate Screen Buffer）时重置独立单元缓冲，避免宿主残留。
- **栅格化说明**：测试产出的 `.png` 图像是真实 ConPTY VT 字符单元缓冲（12×28 单字栅格，等宽字体与色彩映射）的像素栅格化呈现，**不是** Windows Terminal 桌面 GUI 的屏幕截屏。两者在字符行列、色彩和布局语义上等价。
- **隔离环境**：所有测试运行在独立临时目录 `target/terminal-fixtures/` 下，拥有独立的 `library.json`、`configuration.json` 与 `journal/`，绝不访问用户个人书库或敏感配置。

---

## 2. 覆盖尺寸与状态矩阵

验收测试覆盖批准设计中定义的三种标准尺寸，每种尺寸均验证四种典型运行状态，共生成 12 张完整状态图像：

| 终端尺寸 | 状态 | VT 栅格图像 | 屏幕纯文本 | 说明 |
| :--- | :--- | :--- | :--- | :--- |
| **80×24** | Reading | [80x24-reading.png](runtime/80x24-reading.png) | [80x24-reading.txt](runtime/80x24-reading.txt) | 头部4行、正文16行、底部4行。Comfortable 无边框排版，72列完整利用，无冗余竖线 |
| **80×24** | Scrolled | [80x24-scrolled.png](runtime/80x24-scrolled.png) | [80x24-scrolled.txt](runtime/80x24-scrolled.txt) | `j` 精确平移 2 行，流式中文首行缩进与段落平滑位移 |
| **80×24** | Checkpoint | [80x24-checkpoint.png](runtime/80x24-checkpoint.png) | [80x24-checkpoint.txt](runtime/80x24-checkpoint.txt) | `b` 保存书签，异步落盘成功后显示 `Checkpoint saved` |
| **80×24** | Folded | [80x24-folded.png](runtime/80x24-folded.png) | [80x24-folded.txt](runtime/80x24-folded.txt) | `F12` 立即安全挂起会话，零正文、零路径、零书名泄露 |
| **120×30** | Reading | [120x30-reading.png](runtime/120x30-reading.png) | [120x30-reading.txt](runtime/120x30-reading.txt) | 头部4行、正文22行、底部4行。宽屏自适应与 CJK 语料检查工作流 |
| **120×30** | Scrolled | [120x30-scrolled.png](runtime/120x30-scrolled.png) | [120x30-scrolled.txt](runtime/120x30-scrolled.txt) | 2 行平移滚动，保持视觉重叠与正文锚点稳定 |
| **120×30** | Checkpoint | [120x30-checkpoint.png](runtime/120x30-checkpoint.png) | [120x30-checkpoint.txt](runtime/120x30-checkpoint.txt) | 书签耐久化与回执呈现 |
| **120×30** | Folded | [120x30-folded.png](runtime/120x30-folded.png) | [120x30-folded.txt](runtime/120x30-folded.txt) | 安全折叠状态，呈现中性语料处理挂起 |
| **60×18** | Reading | [60x18-reading.png](runtime/60x18-reading.png) | [60x18-reading.txt](runtime/60x18-reading.txt) | 紧凑布局：头部3行、正文11行、底部4行。窄窗 54 列排版优化 |
| **60×18** | Scrolled | [60x18-scrolled.png](runtime/60x18-scrolled.png) | [60x18-scrolled.txt](runtime/60x18-scrolled.txt) | 小尺寸下的 2 行精确平移滚动 |
| **60×18** | Checkpoint | [60x18-checkpoint.png](runtime/60x18-checkpoint.png) | [60x18-checkpoint.txt](runtime/60x18-checkpoint.txt) | 小尺寸下的书签持久化回执 |
| **60×18** | Folded | [60x18-folded.png](runtime/60x18-folded.png) | [60x18-folded.txt](runtime/60x18-folded.txt) | 小尺寸折叠，单行状态提示，零泄露 |

---

## 3. 核心机制验证项与断言结论

所有测试场景自动化执行并记录于 [results.json](runtime/results.json)：

1. **CJK 语料处理会话与双密度设计**：
   - 会话自然呈现 CJK 语料检查任务，使用中性文件路径 `corpus/zh_sample.txt`；
   - 彻底去除封闭式多层外框与冗余右竖线，Comfortable 默认模式无行号、流式排版；Inspect 模式带弱化行号；
   - 中文正文充分利用可用宽度（80 列终端达 72 列，60 列终端达 54 列），保留首行缩进与自然段落换行；
   - 彻底消除末尾大片空白与提前留白，输出高度自适应收缩；
   - 底栏严格移除阅读器按键提示，仅展示真实本地命令入口。

2. **精确平移与翻屏重叠**：
   - `j` 滚动：断言滚动前后字符单元严格满足 `before[2:] == after[:-2]`。
   - 字符锚点：正文重排和滚动时以 Unicode 字符锚点（`TextPosition`）为准，不受终端窗口改变或行列数量波动影响。

3. **异步持久化与 Checkpoint 耐久保证**：
   - 由独立后台线程串行写入，主线程不阻塞；
   - 仅在本地持久化成功后，才回执递增 Checkpoint 计数并显示成功；
   - 验证队列拥塞时退出仍然能够通过 `Finish` 刷新最新配置与进度；
   - 验证书签删除在队列拥塞拒绝入队时能正确回滚撤销乐观删除。

4. **F12 零泄露安全折叠**：
   - 折叠状态下使用独立安全渲染器，完全不传递 `ReaderState` 或书本内容；
   - 严格断言折叠屏幕不含任何正文字符、真实书名、章节标记、本地路径或通知文本；
   - 折叠状态下连续进行窗口缩放（`80×24` ↔ `120×30` ↔ `60×18`），屏幕始终保持严格安全折叠，无任何闪烁或内容回显；
   - 展开后恢复原始视口并保持完全一致的阅读字符锚点。

5. **输入与搜索草稿防护**：
   - 输入未完成本地命令（如 `:private_query`）后按 F12 折叠并恢复，草稿被彻底清空；
   - 普通模式下处于搜索框输入草稿（`/SECRET_QUERY`）时按 F12 折叠，搜索草稿被丢弃，不会在恢复时复现泄露。

6. **空闲帧稳定性**：
   - 终端在无键盘输入时空闲 0.7 秒以上，屏幕单元缓冲哈希严格一致，无任何 Thinking 动画、旋转符号或伪造的耗时跳动。

---

## 4. 自动化测试复现方法

开发者可通过以下命令在本地 Windows 环境下一键重跑全套 ConPTY 仿真：

```powershell
# 1. 编译测试二进制
$env:CARGO_INCREMENTAL='0'
$env:TEMP='G:/Document/reader/test-tmp'
$env:TMP=$env:TEMP
cargo +stable-x86_64-pc-windows-gnu test --lib --no-run

# 2. 运行 ConPTY 测试
& "C:\Users\30689\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe" tests/capture_agent_terminal.py
```
