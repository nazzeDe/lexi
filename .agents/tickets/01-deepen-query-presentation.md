# Ticket 01：深化查询输出

## 状态

Ready

## 目的

把查询输出的 writer、显示策略、raw/full/terminal 策略、每词来源标注和跨记录分隔状态收进 `output` 模块。`query.rs` 每个查询批次只构造一个 `QueryOutput`，随后只提交一个 term 的最终匹配元组或 miss，不再拥有 `first_record` 协议，不再计算 provenance，也不再重复附加 stdout/stderr 写错误上下文。

行为保持基线是 `252e55d` 的当前输出字节，不是重构前的扁平输出。

## Seam

seam 位于 `output` 模块与 `query.rs` 之间：

- `QueryOutput::new` 一次配置 stdout、stderr 和完整 `Options`。
- `QueryOutput::records` 接收一个查询词的最终匹配 `(dictionary_name, headword, definition)` 元组迭代器；内部可收集借用元组以判断是否存在多个词典名，再逐条渲染。
- 私有 `record` helper 渲染一条记录：标题、definition 渲染、末尾换行、记录间空行、状态更新和 stdout 错误上下文。有效 `show_dictionary = options.show_dictionary || (!options.raw && multiple)`。
- `QueryOutput::miss` 完整处理诊断字节和 stderr 错误上下文。
- HTML 检测、解析、entity 解码、空转换回退、文本整理，以及既有私有 `output/structured.rs` 的 Oxford 分组、双语义项、省略脚注、链接、回退和复杂度保护全部保持输出模块私有 implementation。
- `&mut dyn Write` 是既有本地 adapter；不增加 writer trait，不公开 parser 或渲染 seam。

## 依赖

- 前置：[Ticket 02](02-concentrate-exact-lookup.md)，必须已完成并验证；此时 `query.rs` 已使用每批一个 `Lookup`，但仍使用当前 `Options`/provenance/`first_record`。
- 固定执行序列中的位置：第四项，也是最后一项。
- 本 ticket 不与 Ticket 02 并行，因为两者都修改 `src/query.rs`。
- 设计依据：[架构深化总规范](../specs/architecture-deepening.md)。

## 范围

修改：

- `src/output.rs`
- `src/query.rs`
- 两个文件内的单元测试

必要时只为编译适配 `tests/query.rs`，但不得改变该文件的行为断言。不改 `src/output/structured.rs` 的 parser。其他生产模块、集成测试、README 和 Cargo 文件不变。

## 非目标

- 不改变 `252e55d` 的输出文本、换行、空行、stdout/stderr 分流、退出码、来源标注、TTY 宽度或 `NO_COLOR` 字节。
- 不改变 HTML 到终端文本的任何转换规则，不公开或重写 `structured` parser。
- 不公开 `render_definition`、`html_to_text`、`structured::parse` 或其私有 helper。
- 不让 `output` 模块导入或依赖 `StoredEntry`、`Storage`、`Lookup` 或 `QueryTerm`。
- 不引入模板系统、格式化 trait、事件 enum、缓存字符串、异步 writer，或公开的 begin-term 状态机。
- 不把 `record` 做成公开 interface，也不恢复 `write_record`/`write_miss`。
- 不改变 Ticket 02 已建立的 lookup interface 和匹配策略。
- 不修改 `main.rs` 的 query 调用 interface。

## 最终 Interface

`src/output.rs` 新增以下 crate 内 interface；字段严格保持私有：

```rust
use std::io::Write;

pub(crate) struct QueryOutput<'a> {
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
    options: Options,
    wrote_record: bool,
}

impl<'a> QueryOutput<'a> {
    pub(crate) fn new(
        stdout: &'a mut dyn Write,
        stderr: &'a mut dyn Write,
        options: Options,
    ) -> Self;

    pub(crate) fn records<'record>(
        &mut self,
        records: impl IntoIterator<Item = (&'record str, &'record str, &'record str)>,
    ) -> anyhow::Result<()>;

    pub(crate) fn miss(
        &mut self,
        term: &str,
    ) -> anyhow::Result<()>;
}
```

`Options` 保持当前字段：`show_dictionary`、`raw`、`full`、`terminal`。私有 `record` helper 可接受，用于渲染单条记录；不是公开 interface。

interface 的完整语义：

- `new` 把 `wrote_record` 初始化为 `false`，不执行任何写入。
- `records` 接受**一个查询词**的最终匹配 `dictionary_name`/`headword`/`definition` 元组。为判断该词是否跨越多个词典名，实现可以先收集 `Vec` 借用元组；不得为此依赖 `StoredEntry`。
- 空迭代器不写任何字节，也不改变 `wrote_record`。
- 有效 `show_dictionary = options.show_dictionary || (!options.raw && multiple)`，其中 `multiple` 仅由这一次 `records` 调用中的词典名是否多于一个决定。
- 私有 `record` 在 `wrote_record == true` 时先向 stdout 写一个 `\n` 作为记录间空行；第一条成功记录前不写分隔空行。
- 有效 `show_dictionary == true` 时标题为 `[{dictionary_name}] {headword}\n`，否则为 `{headword}\n`。
- definition 走现有私有渲染路径（含 `render_definition`、structured 解析、TTY/颜色）；结果不以 `\n` 结尾时补一个换行，以换行结尾时不重复追加。
- 每条记录只有在分隔、标题、definition 和必要末尾换行全部成功后才把 `wrote_record` 设为 `true`。任一 stdout 写入失败时返回带 `failed to write query result to stdout` 上下文的错误，状态保持该条调用前的值。
- `miss` 始终向 stderr 写 `No entry found for: {term}\n`，失败时返回带 `failed to write query diagnostic to stderr` 上下文的错误。
- `miss` 成功或失败都不改变 `wrote_record`；miss 不能影响后续第一条 stdout 记录是否有前导空行。
- 已有记录成功后发生一次 record 写失败，`wrote_record` 保持 `true`，因为调用前已经有成功记录；状态不因失败反转。
- 没有公开 `record`，也没有公开的 begin-term 状态机。`query.rs` 只发送最终匹配迭代器，不再计算 provenance 或每条 `first_record`/`Options`。

删除旧 interface：

```rust
// 删除：output::write_record(..., options, first)
// 删除：output::write_miss(writer, term)
// 删除：query.rs 中的 first_record、multiple_dictionaries 与 per-record Options
```

## Implementation 步骤

1. 在 `src/output.rs` 导入 `anyhow::Context`，保留 `std::io::Write`；若 `io` 只服务旧返回类型，则删除不再使用的 `io` 导入（`Terminal::detect` 仍需要 `std::io`）。
2. 在文件顶部定义 `pub(crate) struct QueryOutput<'a>`，字段顺序严格为 `stdout`、`stderr`、`options`、`wrote_record`，全部私有。
3. 实现 `QueryOutput::new`，直接保存三个参数并将 `wrote_record` 设为 false。
4. 实现 `QueryOutput::records`：
   - 收集该词的借用元组，仅用于判断多个词典名。
   - 空集合立即成功返回。
   - 计算有效 `show_dictionary`，再对每条调用私有 `record`。
5. 把旧 `write_record` 的字节写入 implementation 移入私有 `record`：
   - 使用 `self.wrote_record` 判定分隔符，不再接收 `first`。
   - 使用保存的 `Options` 与本次有效 `show_dictionary`，不再由 query 逐条传入。
   - 保持现有标题、structured/HTML/raw、TTY、definition 字节和末尾换行分支。
   - 将整次写入错误统一附加既有 stdout 上下文。
   - 在该条全部写入成功后执行 `self.wrote_record = true`，不得提前更新。
6. 把旧 `write_miss` 移入 `QueryOutput::miss`，使用保存的 stderr，并在方法内附加既有 stderr 上下文；无论结果如何都不修改 `wrote_record`。
7. 删除 `pub fn write_record` 和 `pub fn write_miss` 旧 interface。HTML/`structured` helper 全部保持私有，函数体不做行为重写。
8. 修改已经完成 Ticket 02 的 `src/query.rs::run`：
   - 保持每批一次 `let lookup = storage.lookup(dictionaries)?;`。
   - 在 term 循环前增加一次 `let mut output = QueryOutput::new(stdout, stderr, options);`。
   - miss 分支改为 `output.miss(term.text())?`。
   - 命中改为一次 `output.records(matches.iter().map(|entry| (entry.dictionary_name(), entry.headword(), entry.definition())))?`。
   - 删除 `first_record`、每词 `multiple_dictionaries`、每条成功后更新状态的代码和 query 层两处 `.context(...)`。
9. 清理 `src/query.rs` 导入：使用 `crate::output::QueryOutput`，删除只为旧写错误上下文存在的 `anyhow::Context`，保留 `bail` 的需求以 Ticket 02 完成后的实际代码为准；不得重新引入已退出 query interface 的 storage helper。
10. 按“测试变更”把 stream/state/miss/错误覆盖迁到 `QueryOutput` interface。既有 parser/structured 私有测试可以保留为下层测试，不复制包装测试。
11. 运行 output/query 单元测试和 CLI query 集成测试，再运行格式与 clippy。
12. 用 `rg` 确认旧公开 interface 和 `first_record` 消失、`output` 模块没有 `StoredEntry`，并运行完整仓库开发门禁完成四项工作。

## 必须保持的 Invariants 和错误

- 第一条成功 stdout 记录前无空行；后续每条成功记录前恰有一个空行。
- 中间出现任意数量 miss 都不改变 stdout 记录分隔；第一条命中晚于 miss 时仍无前导空行。
- definition 的前导/尾随空格与内部换行保持现有合同；只在缺少末尾换行时补 `\n`。
- `--show-dictionary` 标题、默认标题、`--raw` 字节、`--full` 字节完全不变。
- 非 raw 且一个 term 的最终匹配含多个词典名时自动显示词典标题；raw 从不自动加词典名；显式 `--show-dictionary` 在 raw 下仍显示。
- HTML 检测仅在看见类似标签时转换；script/style、标签 break、entity、标点间距、sense number 和 bullet 处理不变。
- Oxford 分组、双语义项、省略脚注、链接、无法结构化时的整段回退和复杂度保护保持 `252e55d` 字节。
- `NO_COLOR` 与 TTY 宽度行为保持当前 `Options.terminal` 合同。
- HTML 转换结果 `trim().is_empty()` 时回退到原始 definition。
- stdout 写失败错误链包含 `failed to write query result to stdout`；stderr 写失败错误链包含 `failed to write query diagnostic to stderr`。
- 部分写入无法撤销，但失败调用不得谎报成功状态：只有完整成功的单条 record 才能把 false 改成 true。
- miss 文本只写 stderr；record 字节只写 stdout。
- `output` 模块只认识字符串字段、`Options` 和 writer，不认识存储行类型。
- query 的 `Outcome::AllFound`/`SomeMissing`、空库/未知词典错误先后及 Ticket 02 lookup 复用不变。

## 测试变更

### `QueryOutput` interface 测试

stream/state/miss/写错误/来源标注覆盖迁到公开 `new`/`records`/`miss`，而不是恢复 `write_record`/`write_miss`，也不公开 `record`：

- 连续两次单条 `records` 仍分隔为 `a\none\n\nb\ntwo\n`。
- 显式 `show_dictionary` 仍使用括号标题并保持 verbatim definition。
- 默认/raw 的既有 HTML 可读性与 markup 字节可通过 `records` 或现有下层 helper 保持；不要为同一渲染路径再写一套包装测试。
- `miss` 只写 stderr，stdout 为空。
- 先 `miss` 后 `records`：stderr 有诊断，stdout 直接以 headword 开始且没有前导空行。
- 第一次写入返回 `BrokenPipe`、随后可写：第一次 `records` 错误链含既有 stdout 上下文，第二次成功且没有由失败调用产生的分隔空行。
- 第一条完整成功、第二条失败，恢复后第三次调用仍先写记录分隔；失败不会把已有 true 状态反转。
- 失败 stderr 调用 `miss`：错误链含既有 stderr 上下文且 stdout 为空。
- 空 `records` 不写字节、不改变分隔状态。
- 一次 `records` 含两个不同词典名且非 raw 时自动加词典标题；`raw: true` 时不自动加；显式 `show_dictionary` 在 raw 下仍显示。

测试 writer 只放在 `#[cfg(test)] mod tests`，实现最小 `Write` 行为，不成为生产 interface。

### 既有 parser/structured 测试

- 直接调用 `structured::parse`、`html_to_text`、`write_line`、`reference_targets` 等的下层测试可以保留。
- 现有通过内部 `rendered` helper 覆盖 Oxford 分组、省略脚注、链接、回退、复杂度保护和 TTY 样式的测试可以继续走私有渲染路径或单条 `records`；不要在 `QueryOutput` 上复制这些用例。
- 不以固定测试个数作为验收门槛。

### `src/query.rs` 单元测试

- 保留 Ticket 02 迁移后的 `empty_database_is_a_runtime_error_not_a_miss`，输出仍为空。
- 保留 `batch_writes_misses_to_stderr_and_keeps_hits`，期望 stdout 的跨 term 分隔和 stderr miss 字节完全不变。
- 只更新编译所需导入；不增加针对 `QueryOutput` 私有字段的 query 测试；query 不再断言 provenance 计算。

### CLI 行为测试

保留 `tests/query.rs` 的全部现存输出合同，包括但不限于：

- `output_bytes_match_the_text_contract`。
- `default_query_renders_html_and_raw_keeps_stored_text`。
- `raw_never_adds_automatic_dictionary_names_and_full_does`。
- `structured_modes_and_pipe_output_use_the_same_content_layers`。
- `batch_provenance_is_decided_per_terms_selected_matches`。
- `batch_query_continues_after_misses_and_exits_one`。
- `duplicate_query_headwords_are_looked_up_independently`。
- `closed_stderr_during_a_miss_is_a_runtime_error`。
- `closed_stdout_during_query_is_a_runtime_error`，stderr 仍以 `Error: failed to write query result to stdout` 开头。

`tests/output_terminal.rs` 的 TTY/`NO_COLOR`/`--full`/`--raw` 合同不因本 ticket 改断言。

## Acceptance

- [ ] `QueryOutput<'a>` 的可见性、字段、字段顺序和私有性与最终 interface 一致。
- [ ] `new`、`records`、`miss` 的参数和 `anyhow::Result` 返回类型与最终 interface 一致；`record` 若存在则不是公开 interface。
- [ ] `records` 只接收一个查询词的最终字符串元组，空迭代器不写字节。
- [ ] 有效 `show_dictionary` 由输出模块计算，`query.rs` 不再计算 provenance。
- [ ] 单条 record 只在全部 stdout 写入成功后把 `wrote_record` 设为 true。
- [ ] `miss` 的成功与失败路径都不修改 `wrote_record`。
- [ ] stdout/stderr 错误上下文已从 query 调用方收进 `QueryOutput`，字符串逐字不变。
- [ ] `query::run` 每批只构造一个 `QueryOutput`，`src/query.rs` 中没有独立词 `first_record`。
- [ ] 旧 `write_record`/`write_miss` 公开 interface 已删除，且 `src`、`tests` 中无调用方。
- [ ] `output` 模块不依赖 `StoredEntry` 或其他 storage 类型。
- [ ] HTML/`structured` 转换全部保持私有，现有转换代码没有无关重写。
- [ ] stream/state/错误覆盖在 `QueryOutput` interface 上，下层 parser 测试未重复包装。
- [ ] query 单元测试和 CLI query/TTY 行为测试全部通过，输出逐字节不变。
- [ ] 完整仓库开发门禁通过，四项架构深化均完成。

## Validation

按顺序运行：

```bash
cargo fmt --check
cargo test output::tests
cargo test query::tests
cargo test --test query
cargo clippy --all-targets -- -D warnings
rg -n "pub fn write_record|pub fn write_miss|output::write_record|output::write_miss" src tests
rg -n "\bfirst_record\b|multiple_dictionaries" src/query.rs
rg -n "StoredEntry|Storage|Lookup|QueryTerm" src/output.rs src/output
rg -n "pub\(crate\) struct QueryOutput|pub\(crate\) fn new|pub\(crate\) fn records|pub\(crate\) fn miss|wrote_record = true" src/output.rs
cargo test
cargo build --release
git diff --check
git diff -- src/output.rs src/output src/query.rs tests/query.rs
```

预期结果：

- 格式、窄测试、CLI query、clippy、完整测试和 release build 全部成功。
- `write_record`/`write_miss` 公开/调用方检查无匹配并以状态 1 结束。私有 `fn record` 可以存在。
- `\bfirst_record\b|multiple_dictionaries` 检查无匹配并以状态 1 结束。
- storage 类型检查无匹配并以状态 1 结束。
- 新 interface 检查命中 struct、`new`/`records`/`miss` 与成功后的状态更新。
- `tests/query.rs` 不因本 ticket 改变输出断言；diff 主要集中在 `src/output.rs` 与 `src/query.rs`。

## 风险和回滚

- 风险：在写完 definition 前更新状态，导致失败重试出现错误空行。控制方式是失败状态测试并检查赋值位于所有 fallible 写入之后。
- 风险：错误上下文在迁移时丢失或改字。控制方式是 output 单元测试与现有 closed stdout CLI 测试。
- 风险：miss 错误地参与记录分隔。控制方式是先 miss 后 records 的双 writer 字节测试和批次 CLI 测试。
- 风险：把来源标注做成跨 term 状态或依赖 `StoredEntry`。控制方式是 `records` 只看本次迭代器中的词典名，且负向检查 output 模块无存储类型。
- 风险：借深化 interface 重写 HTML/`structured` parser，引入不可见回归。控制方式是保持 helper 函数体不变并运行全部 output/CLI/TTY 测试。
- 回滚：恢复 `output::write_record`、`output::write_miss` 与 query 的 `first_record`/provenance 协议，同时恢复旧 output 测试调用。Ticket 02 的 lookup 设计可独立保留；数据库与用户数据无需回滚。

## 完成条件

Acceptance 全部勾选，Validation 全部符合预期，其中旧 `write_record`/`write_miss` 公开 interface 与 `\bfirst_record\b` 两项检查均无匹配并以状态 1 结束；四项开发门禁成功，`252e55d` 输出字节与错误上下文无变化，`QueryOutput` 成为 query 唯一输出 interface，且本 ticket 没有触碰存储设计、README 或 Cargo 文件。

## 文档链接

- [架构深化总规范](../specs/architecture-deepening.md)
- [Ticket 03：恢复存储测试 seam](03-restore-storage-test-seam.md)
- [Ticket 04：深化 CLI 合同夹具](04-deepen-cli-contract-fixture.md)
- [Ticket 02：集中精确查词策略](02-concentrate-exact-lookup.md)
- [Ticket 01：深化查询输出](01-deepen-query-presentation.md)
