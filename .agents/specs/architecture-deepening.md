# Lexi 架构深化总规范

## 状态

Ready

## 背景和目标

Lexi 第一版已经具备严格 JSONL 导入、SQLite 持久化、精确查词、词典筛选和终端输出。HEAD `252e55d` 已完成 CLI 输出重构：默认可读文本、`--full`/`--raw`、Oxford 分组与双语义项、省略脚注、链接、回退与复杂度保护、`NO_COLOR` 与 TTY 宽度，以及非 raw 多词典来源标注。当前行为覆盖充分，但三个实现知识簇仍越过各自 seam：查询调用方管理输出分隔状态并计算每词来源标注；查询模块同时编排存储预检、词典范围和精确匹配策略；CLI 集成测试复制存储 SQL，并在六个测试文件中重复进程与临时路径配置。

本次工作通过四张可独立分配的 ticket 深化既有 module，不扩展产品能力。目标是缩小 interface，把策略和状态收回其 implementation，提高调用方 leverage，并让修改与验证具有更强 locality。

行为保持基线是 `252e55d` 的当前输出与全部既有测试字节，不是重构前的扁平输出。文档只校正过时约束与签名，不改变原目标、非目标和串行闸门。

## 设计术语

- **模块（module）**：拥有 interface 与 implementation 的代码单元；本规范涉及输出模块、精确查词模块、存储模块和测试支持模块。
- **接口（interface）**：调用方正确使用模块必须知道的全部事实，包括类型、方法、状态转移、顺序、错误和性能约束。
- **深度（depth）**：interface 提供的 leverage。小 interface 隐藏更多稳定行为时，模块更深。
- **接缝（seam）**：模块 interface 所在的位置；调用方通过这里使用行为，而不掌握内部 mechanics。
- **适配器（adapter）**：在 seam 处满足 interface 的具体对象。本次保留真实 `Write`、进程和 SQLite 对象作为既有 adapter，不引入仓储抽象。
- **杠杆（leverage）**：调用方学习更少 interface 即获得更多行为；例如一次创建查询输出对象便获得格式、分隔、HTML 转换、来源标注和写错误上下文。
- **局部性（locality）**：变更、缺陷、知识和验证集中在负责的 implementation；输出策略留在 `output` 模块（含既有私有 `output/structured.rs`），查词策略与 SQL 留在 `storage.rs`，CLI 合同夹具留在 `tests/support/mod.rs`。

## 当前结构与目标结构

### 当前结构

1. `query.rs` 调用 `storage.has_any_dictionary`、`storage.resolve_dictionaries` 和 `storage.lookup_folded`，并自行执行精确词头优先筛选。
2. `query.rs` 持有 `first_record`，按 term 计算多词典来源标注，再把 `output::Options` 和分隔状态传给 `output::write_record`；`output::write_miss` 是另一条浅接口。`Options` 现含 `show_dictionary`/`raw`/`full`/`terminal`。渲染（含既有私有 `output/structured.rs`）已在输出模块内。
3. `tests/query.rs` 复制两种完整查询 SQL 并直接执行 `EXPLAIN QUERY PLAN`，跨过存储 seam。
4. 六个集成测试文件分别实现临时 `XDG_DATA_HOME`、命令执行、UTF-8 解码和 JSONL 文件写入；`tests/output_terminal.rs` 另有本地 PTY 分配与读循环。

### 目标结构

1. `storage.rs` 提供 `Storage::lookup` 创建整批复用的 `Lookup`，后者用 `Lookup::find` 完成 folded 候选读取和全局原词头精确匹配优先。
2. `output` 模块提供每批一次创建的 `QueryOutput`，内部持有两个 writer、完整 `Options` 和 `wrote_record`；`query.rs` 对每个 term 提交最终匹配元组迭代器或 miss，不再计算来源标注或 `first_record`。
3. 查询计划证明只保留在 `storage.rs` 的单元测试；CLI 集成测试只观察真实可执行文件的行为。
4. `tests/support/mod.rs` 提供固定的 `CliFixture` interface，六个集成测试仍使用真实进程、临时 JSONL、临时 XDG 和真实 SQLite，但不再复制通用搭建代码。`output_terminal.rs` 的 PTY 分配、窗口尺寸、raw 属性和读循环保持本地。

目标调用关系为：

```text
main.rs
  -> query.rs
       -> Storage::lookup(...) -> Lookup::find(...)
       -> QueryOutput::new(...) -> records(...) / miss(...)

六个 CLI 集成测试
  -> tests/support/mod.rs
       -> 真实 lexi 进程 + 临时路径
  -> 各测试文件自己的行为断言和 SQLite tuple 查询
  -> tests/output_terminal.rs 本地 PTY 细节
```

## 固定约束

### SQLite

- SQLite 继续是唯一运行时数据源。
- JSONL 只用于导入；成功导入后源文件可删除、移动或不再可用，查询、列出和删除仍只依赖 SQLite。
- 只保留当前 `Storage` 实现，不增加第二种存储 implementation，不增加仓储 trait，也不增加围绕 SQLite 的新 adapter seam。
- SQL、行构造、查询计划和索引证明保持在 `storage.rs`。

### 同步阻塞

- 第一版保持同步阻塞模型。
- 不引入异步运行时、后台任务、并发查询或流式输出协议。
- `Lookup::find` 返回完整的 `Vec<StoredEntry>`；`QueryOutput` 通过同步 `Write` 输出。

### 第一版范围

- 只深化已实现的精确查词、输出和测试结构。
- 不增加模糊查询、前缀查询、全文检索、词形变化、排序选项、缓存或新命令。
- HTML 到终端文本的转换、Oxford 结构化渲染和复杂度保护继续是 `output` 模块私有 implementation（含既有 `output/structured.rs`），不公开 parser，也不借架构任务重写或拆分新的渲染模块。

### 用户可见行为

以下行为以 `252e55d` 为基线逐字节保持：

- 查询默认输出可读文本；`--raw` 原样输出存储内容；`--full` 显示全部例句与语源且不写省略脚注；`--show-dictionary` 使用 `[词典名] 词头` 标题。
- 非 raw 时，若一个查询词的最终匹配跨越多个词典名，自动加上词典来源标注；`--raw` 从不自动加词典名；显式 `--show-dictionary` 在 raw 下仍显示词典名。
- Oxford 分组、双语义项、省略统计脚注、链接、无法结构化时的整段回退，以及过深/过复杂 HTML 的复杂度保护保持当前字节。
- `NO_COLOR` 与 TTY 宽度行为保持当前合同：非终端默认不着色、不按宽度折行；终端检测宽度；`NO_COLOR` 禁止颜色。
- 多条命中之间恰有一个空行；definition 已有末尾换行时不追加重复换行。
- HTML 转换结果为空白时回退到原始 definition。
- miss 写入 stderr，格式为 `No entry found for: {term}\n`；命中继续写 stdout；批次含 miss 时退出码为 1。
- 空数据库是运行错误；未知词典在任何查询结果或 miss 输出前失败。
- 词典筛选大小写不敏感，重复筛选名不复制结果，筛选参数顺序不改变主键顺序。
- folded 候选中存在与查询原词头完全相同的记录时，只返回所有全局精确项；不存在时返回所有 folded 候选。
- 查询保持 `ORDER BY e.id` 的主键顺序，并保留重复词条。
- stdout 与 stderr 写失败继续带既有上下文，并产生运行错误退出码 3。
- 导入、列表、删除、帮助、版本、数据路径和数据库 schema 行为不变。

## 四项最终设计摘要

### 03 恢复存储测试 seam

删除 `tests/query.rs` 中复制完整 SQL 的查询计划测试及其独占 `rusqlite` 导入。两种 SQL 形态、`entries_lookup` 使用和不发生 entries 全表扫描的证明继续由 `src/storage.rs` 测试负责。生产代码不变。

### 04 深化 CLI 合同夹具

新增 `tests/support/mod.rs`，由 `CliFixture` 统一临时数据目录、临时文件目录、数据库路径和进程配置，并提供文件写入、UTF-8 解码以及 Unix closed output。六个集成测试（含 `tests/output_terminal.rs`）继续拥有自己的行为断言和 SQLite tuple 查询；PTY 细节留在 `output_terminal.rs`。

### 02 集中精确查词策略

在 `query.rs` 与 `storage.rs` 之间建立 `Storage::lookup` / `Lookup::find` seam。词典存在性预检、整批 scope 解析、folded SQL 候选读取和全局原词头精确匹配优先都进入存储 implementation；`query.rs` 只消费最终 `StoredEntry`。本项暂时保留当前 `output::Options`、每词来源标注和 `first_record`，由 Ticket 01 收回。

### 01 深化查询输出

在 `output` 模块与 `query.rs` 之间以 `QueryOutput` 建立 seam。两个 writer、完整 `Options` 和是否已经成功写出记录的状态由输出 implementation 持有；`query.rs` 每批构造一次，对每个 term 调用 `records` 或 `miss`。自动来源标注、分隔状态和写错误上下文都留在输出模块。

## 依赖和执行顺序

固定执行顺序：

```text
03 -> 04 -> 02 -> 01
```

- 03 先移除跨 seam 的重复 SQL 测试，为后续查询 interface 迁移清除错误测试依赖。
- 04 随后统一六个集成测试的基础设施，使 02 和 01 的 CLI 回归验证使用稳定夹具。
- 02 再修改 `storage.rs` 与 `query.rs`，先把查词策略集中到存储 implementation。
- 01 最后修改 `output` 模块与已经完成 02 迁移的 `query.rs`，避免两张 ticket 并行编辑 `query.rs`。
- 02 完成并验证前不得开始 01；四张 ticket 均按上述顺序串行落地。

## 共同测试策略

- replacement 原则：当旧测试直接验证已退出 interface 的 helper 或复制 implementation 时，删除或迁移为新 interface 测试，不叠加两套测试。
- interface 是测试面：`QueryOutput` 的状态、来源标注与字节合同在 `output` 模块测；`Lookup` 的预检、scope、顺序、重复项和匹配优先级在 `storage.rs` 测；`query.rs` 测批次编排与 `Outcome`；CLI 测试只观察进程输出、退出码和持久数据。既有 parser/structured 私有测试可以留在输出模块内部，不在 `QueryOutput` 上复制包装测试。
- 保留真实 adapter：集成测试继续运行真实 `lexi` 二进制、写临时 JSONL、设置临时 `XDG_DATA_HOME` 并打开真实 SQLite。
- 先运行每张 ticket 指定的窄验证，再运行仓库开发门禁：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`、`cargo build --release`。
- 03 的文档执行阶段只改测试；04 只改测试基础设施；02 和 01 每项都必须先通过对应模块测试与 CLI 行为测试，再进入下一项。
- 不以臆造的测试个数作为静态验收门槛；以既有测试字节和新 interface 覆盖为准。

## 非目标

- 不改变命令行参数、退出码或 README 合同。
- 不改变 JSONL 格式、校验、事务和导入替换语义。
- 不改变数据库 schema、索引定义、数据位置或系统 SQLite 链接方式。
- 不公开 HTML 转换 helper，不公开 `structured` parser，不拆分新的渲染模块，也不借架构任务重写渲染。
- 不引入 mock 存储、内存存储、trait、泛型仓储或动态分派仓储。
- 不把各测试专属的 SQLite 查询结果 tuple 和断言移入共享 support。
- 不把 PTY 分配与读循环移入 `CliFixture`。
- 不借机重命名无关类型、重排无关代码或修改 README。

## 整体验收

- [ ] 严格按 `03 -> 04 -> 02 -> 01` 完成，02 与 01 未并行修改 `query.rs`。
- [x] `tests/query.rs` 不再复制 lookup SQL 或执行查询计划，`storage.rs` 仍证明两种 SQL 使用 `entries_lookup` 且不全表扫描 entries。证据：Ticket 03 Validation；`rg` 对 `tests/query.rs` 退出码 1；storage 查询计划测试通过。
- [ ] 六个集成测试均声明 `mod support;` 并使用 `CliFixture`/共享函数；测试行为断言和专属 SQLite tuple 查询仍在各自文件；`output_terminal.rs` 的 PTY 细节仍本地。
- [ ] `Storage::lookup` 一次完成空库预检和整批词典 scope 解析；`Lookup::find` 隐藏 folded mechanics 并执行全局原词头精确匹配优先。
- [ ] `query.rs` 不再可见 `DictionaryScope`、`has_any_dictionary`、`resolve_dictionaries`、`lookup_folded` 或 `select_matches`。
- [ ] `QueryOutput` 精确持有两个 writer、一份 `Options` 和 `wrote_record`；公开 interface 为 `new`/`records`/`miss`；只有单条 record 完整成功后更新状态，`miss` 不更新状态。
- [ ] `output` 模块不依赖 `StoredEntry`；HTML/`structured` helper 保持私有。
- [ ] 所有既有 CLI 输出字节、错误上下文、退出码、顺序、重复项、筛选、匹配优先、来源标注、TTY 与 `NO_COLOR` 行为通过测试。
- [ ] SQLite 仍为唯一运行时数据源，删除或移动成功导入的 JSONL 后查询仍成功。
- [ ] `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 和 `cargo build --release` 全部通过。
- [ ] 实现提交只包含四张 ticket 明确列出的生产代码与测试文件变更。

## Tickets

- [总规范](../specs/architecture-deepening.md)
- [03 恢复存储测试 seam](../tickets/03-restore-storage-test-seam.md)
- [04 深化 CLI 合同夹具](../tickets/04-deepen-cli-contract-fixture.md)
- [02 集中精确查词策略](../tickets/02-concentrate-exact-lookup.md)
- [01 深化查询输出](../tickets/01-deepen-query-presentation.md)
