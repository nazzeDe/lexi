# Ticket 03：恢复存储测试 seam

## 状态

Completed

## 目的

删除 CLI 集成测试对存储 SQL implementation 的复制，让查询计划、索引使用和行读取 mechanics 只在 `src/storage.rs` 内验证。CLI 查询测试继续通过真实二进制观察成功、筛选、顺序、重复项、精确优先和当前输出行为，从测试侧恢复存储模块的 depth 与 locality。

本项范围不因 `252e55d` 输出重构而扩大：只改 `tests/query.rs`，删除重复查询计划测试和因此不再使用的 `rusqlite` 导入。

## Seam

seam 位于 CLI 合同测试与存储模块之间：

- `tests/query.rs` 只通过 `lexi` 可执行文件及其 stdout、stderr、退出码观察查询行为。
- `src/storage.rs` 的模块内测试可访问私有 SQL implementation，并独占 `EXPLAIN QUERY PLAN` 证明。
- 本 ticket 不创建 adapter；CLI 测试继续使用真实进程与真实 SQLite，存储单元测试继续使用临时真实 SQLite。

## 依赖

- 前置 ticket：无。
- 固定执行序列中的位置：第一项。
- 后继：[Ticket 04](04-deepen-cli-contract-fixture.md)。本 ticket 验证完成后再开始 04。
- 设计依据：[架构深化总规范](../specs/architecture-deepening.md)。

## 范围

只修改 `tests/query.rs`：

1. 删除测试 `lookup_query_plan_uses_the_folded_headword_index` 的完整函数。
2. 删除该测试成为唯一用途后不再需要的 `use rusqlite::Connection;`。删除前确认文件中没有新的 `rusqlite` 用法。
3. 保留该文件其余查询行为测试原样。

显式确认 `src/storage.rs` 已有并继续保留：

- `lookup_sql(None)` 的无筛选 SQL 形态。
- `lookup_sql(Some(2))` 的带词典筛选 SQL 形态。
- `lookup_query_plan_uses_the_folded_headword_index`。
- `assert_lookup_uses_index` 对 `entries_lookup` 的证明。
- 对 `scan e` / `scan entries` 且未使用索引的拒绝。

本阶段不改 README、Cargo 或生产代码，也不实施 Ticket 04/02/01。

## 非目标

- 不修改任何生产代码。
- 不修改 `src/storage.rs` 中 SQL、索引、helper 或测试。
- 不替换、重写或合并现有 CLI 行为测试。
- 不改变查询输出、匹配策略、词典筛选、数据库 schema 或性能行为。
- 不增加测试 support；共享夹具由后继 Ticket 04 处理。

## 最终 Interface

interface 不变。

```rust
// 生产 interface、测试调用方式和用户可见 CLI interface 均不变。
// 本 ticket 只删除跨过存储 seam 的重复 implementation 测试。
```

## Implementation 步骤

1. 打开 `tests/query.rs`，确认 `Connection` 仅由末尾查询计划测试使用，且没有其它新的 `rusqlite` 用法。
2. 删除 `use rusqlite::Connection;`，保留其他 `std`、`tempfile` 导入及其顺序交给 `cargo fmt` 规范化。
3. 删除完整的 `lookup_query_plan_uses_the_folded_headword_index` 测试，包括：两次临时 JSONL 导入、直接打开数据库、两段完整 SQL 文本、动态参数构造、`EXPLAIN QUERY PLAN` 执行和索引断言。
4. 不触碰 `tests/query.rs` 中其余 CLI 合同测试，包括空库/未知词典、精确优先、来源标注、`--raw`/`--full`、结构化输出、closed stdout/stderr。
5. 检查 `src/storage.rs` 的查询计划测试仍覆盖无筛选和筛选两种 SQL，并同时断言 `entries_lookup` 与非全表扫描。
6. 运行本 ticket 的 Validation。全部通过后才进入 Ticket 04。

## 必须保持的 Invariants 和错误

- 查询仍通过 folded headword 索引取候选；本 ticket 只删除重复证明，不削弱 `storage.rs` 中的证明。
- 无筛选和带词典筛选的两种 SQL 形态都继续受查询计划测试保护。
- `ORDER BY e.id`、重复项、词典筛选、全局原词头精确匹配优先，以及 `252e55d` 输出合同的 CLI 行为测试继续存在。
- 空数据库、未知词典、普通 miss、closed stdout 和 closed stderr 的错误与退出码测试继续存在。
- `tests/query.rs` 不再知道表连接、参数位置、索引名或查询计划细节。

## 测试变更

### 删除

- `tests/query.rs::lookup_query_plan_uses_the_folded_headword_index`。
- `tests/query.rs` 顶层 `rusqlite::Connection` 导入。

### 保留

- `src/storage.rs::tests::lookup_query_plan_uses_the_folded_headword_index`，继续覆盖两种 SQL 形态。
- `src/storage.rs::tests::schema_preserves_duplicate_headwords_and_indexes_folded_lookup`，继续验证 `entries_lookup` 列顺序。
- `tests/query.rs` 中查询成功、词典筛选大小写与重复名、筛选顺序、主键顺序、重复项、全局精确优先、folded fallback、批次 miss、输出字节、HTML/raw/full、来源标注和写失败的全部行为测试。

### 新增

不新增测试；已有 `storage.rs` 测试完整承担被删除测试的存储 implementation 证明。

## Acceptance

- [x] `git diff -- tests/query.rs` 只显示删除一个查询计划测试和一个不再使用的导入。证据：`git diff -- src/storage.rs tests/query.rs` 仅 `tests/query.rs`，去掉 `use rusqlite::Connection;` 与 `lookup_query_plan_uses_the_folded_headword_index`。
- [x] `tests/query.rs` 中不存在 `EXPLAIN QUERY PLAN`。证据：`rg` 退出码 1。
- [x] `tests/query.rs` 中不存在完整的 entries/dictionaries lookup SQL。证据：同一 `rg` 无匹配。
- [x] `tests/query.rs` 不再导入 `rusqlite`。证据：删除前仅该测试使用 `Connection`；删除后 `rg rusqlite` 无匹配。
- [x] `src/storage.rs` 仍有无筛选与筛选两种查询计划输入。证据：`lookup_sql(None)` 与 `lookup_sql(Some(2))` 仍命中。
- [x] `src/storage.rs` 仍断言查询计划包含 `USING INDEX entries_lookup`。证据：`src/storage.rs:666`。
- [x] `src/storage.rs` 仍拒绝未使用索引的 entries 全表扫描。证据：`scan e` / `scan entries` 检查仍在。
- [x] `cargo test --test query` 与存储查询计划单元测试通过。证据：storage 查询计划 1 passed；`cargo test --test query` 22 passed。
- [x] 没有生产文件变更。证据：`git diff -- src/storage.rs tests/query.rs` 无 `src/storage.rs`；工作区仅 `M tests/query.rs` 与未跟踪 `.agents/`。

## Validation

按顺序运行：

```bash
cargo fmt --check
cargo test storage::tests::lookup_query_plan_uses_the_folded_headword_index
cargo test --test query
rg -n "EXPLAIN QUERY PLAN|SELECT d\.name, e\.headword, e\.definition|rusqlite" tests/query.rs
rg -n "lookup_sql\(None\)|lookup_sql\(Some\(2\)\)|USING INDEX entries_lookup|scan entries|scan e" src/storage.rs
git diff --check
git diff -- src/storage.rs tests/query.rs
```

预期结果：

- 前三个命令成功。
- 对 `tests/query.rs` 的 `rg` 无匹配并以状态 1 结束；这是该检查的预期结果。
- 对 `src/storage.rs` 的 `rg` 同时命中两种 SQL 形态、索引断言和扫描检查。
- `git diff --check` 成功。
- 最后一条 diff 只显示 `tests/query.rs` 的指定删除，`src/storage.rs` 无变更。

## 风险和回滚

- 风险：误删 CLI 可观察行为测试，导致行为覆盖下降。控制方式是逐函数删除并用 diff 确认只有目标测试消失。
- 风险：错误移除仍被其他测试使用的导入。控制方式是删除前确认无新 `rusqlite` 用法，再用 `cargo test --test query` 捕获。
- 回滚：恢复 `tests/query.rs` 的本 ticket diff 即可；生产代码和数据库无迁移，不存在数据回滚。

## 完成条件

Acceptance 全部勾选，本 ticket 的 Validation 符合预期，变更中没有生产文件，且执行者已记录删除的测试名和保留的 storage 查询计划证明。完成后将工作交给 [Ticket 04](04-deepen-cli-contract-fixture.md)。

## 文档链接

- [架构深化总规范](../specs/architecture-deepening.md)
- [Ticket 03：恢复存储测试 seam](03-restore-storage-test-seam.md)
- [Ticket 04：深化 CLI 合同夹具](04-deepen-cli-contract-fixture.md)
- [Ticket 02：集中精确查词策略](02-concentrate-exact-lookup.md)
- [Ticket 01：深化查询输出](01-deepen-query-presentation.md)
