# Ticket 02：集中精确查词策略

## 状态

Ready

## 目的

把空库预检、词典 scope 解析、folded 候选读取和全局原词头精确匹配优先收进 `storage.rs` 的一个深 module。`query.rs` 只表达“为整批查询建立 lookup，再按 term 取得最终结果”的意图，不再了解存储 mechanics 或匹配筛选规则。

本项不收回输出策略。完成时 `query.rs` 暂时继续使用当前 `output::Options`、每词 provenance 和 `first_record`，直到 Ticket 01。

## Seam

seam 位于 `query.rs` 与 `storage.rs` 之间，唯一新调用路径是：

```text
Storage::lookup(整批词典名) -> Lookup
Lookup::find(一个查询词) -> Vec<StoredEntry>
```

- `Storage::lookup` 是批次入口，隐藏安装状态检查和 `DictionaryScope` 解析。
- `Lookup::find` 是逐词入口，隐藏 folded key、SQL 候选和全局精确优先选择。
- SQLite `Connection` 是唯一 storage implementation；不增加 trait 或 adapter。
- `Lookup` 借用 `Storage`，保证同一批次复用同一连接和已解析 scope。

## 依赖

- 前置：[Ticket 04](04-deepen-cli-contract-fixture.md)，必须已完成并验证；其前置 Ticket 03 也必须已完成。
- 固定执行序列中的位置：第三项。
- 后继：[Ticket 01](01-deepen-query-presentation.md)。本 ticket 完成并验证后，才允许继续编辑 `query.rs`。
- 设计依据：[架构深化总规范](../specs/architecture-deepening.md)。

## 范围

修改：

- `src/storage.rs`
- `src/query.rs`
- 两个文件内的单元测试

保留并验证：

- `tests/query.rs` 的 CLI 查询行为测试；只做新 interface 引起的必要编译迁移，不改变断言。
- Ticket 03 已删除的跨 seam 查询计划测试不恢复。
- Ticket 04 已建立的 `tests/support/mod.rs` 和 fixture interface 不改变。

## 非目标

- 不改变 CLI 参数、输出、退出码或 README。
- 不改变 SQLite schema、索引、SQL 形态、`ORDER BY e.id` 或行构造。
- 不增加仓储 trait、第二种存储 implementation、mock 存储或查询 adapter。
- 不增加缓存、批量 SQL、并行查询、流式迭代器或异步代码。
- 不改变 `QueryTerm`/`DictionaryName` 的解析与 Unicode lowercase 规则。
- 不把 `StoredEntry` 传给输出模块；Ticket 01 仍使用字段值调用输出 interface。
- 不同时实施 Ticket 01 的 `QueryOutput` 改造；暂时保留当前 `output::write_record`/`write_miss`、`Options`、每词 provenance 和 `first_record`。

## 最终 Interface

`src/storage.rs` 新增并只向 crate 内暴露以下 interface；所有字段私有：

```rust
use crate::record::{DictionaryName, Entry, QueryTerm};

pub(crate) struct Lookup<'storage> {
    storage: &'storage Storage,
    scope: DictionaryScope,
}

impl Storage {
    pub(crate) fn lookup(
        &self,
        dictionaries: &[DictionaryName],
    ) -> anyhow::Result<Lookup<'_>>;
}

impl Lookup<'_> {
    pub(crate) fn find(
        &self,
        term: &QueryTerm,
    ) -> anyhow::Result<Vec<StoredEntry>>;
}
```

interface 的完整语义：

- `Storage::lookup` 首先检查是否安装任何词典；没有时返回 `no dictionaries are installed`。
- 预检成功后，`Storage::lookup` 遍历并解析调用方提供的整批 dictionary names；任一名称未知时返回既有 `unknown dictionary '{name}'` 错误，不返回 `Lookup`。
- 所有名称成功解析后才构造 `Lookup`，因此任何 `find` 和任何输出之前已经完成 scope 解析。
- 一个 `Lookup` 对同批所有 term 复用同一个 `DictionaryScope`，scope 不在 `find` 中重新解析。
- `Lookup::find` 使用 `term.folded()` 从 SQLite 读取 scope 内候选，保持 `ORDER BY e.id`、重复项和既有 SQL 错误上下文。
- 候选中只要存在 `entry.headword() == term.text()`，就只保留整个所选词典集合中的全部精确项；不存在精确项时返回全部 folded 候选。这是全局原词头精确匹配优先，不按词典分别 fallback。
- `DictionaryScope`、`has_any_dictionary`、`resolve_dictionaries`、`lookup_folded` 和 `select_matches` 都成为 storage 私有 implementation，不再出现在 query 可见 interface。
- `StoredEntry` 及其访问器保持当前行为与可见性，不在本 ticket 借机重命名或重构。

## Implementation 步骤

1. 在 `src/storage.rs` 的 record 导入中加入 `QueryTerm`。
2. 将 `DictionaryScope` 改为 storage 私有 struct，保留私有 `ids: Option<Vec<i64>>` 及“空调用方列表表示所有词典”的语义。
3. 在 `Storage` 之后定义 `pub(crate) struct Lookup<'storage>`，字段严格为私有 `storage: &'storage Storage` 与 `scope: DictionaryScope`。
4. 在 `impl Storage` 中新增 `pub(crate) fn lookup`：
   - 先调用私有 `has_any_dictionary`；false 时立即 `bail!("no dictionaries are installed")`。
   - 再调用私有 `resolve_dictionaries(dictionaries)` 完成整批 scope 解析。
   - 两步成功后返回 `Lookup { storage: self, scope }`。
5. 把 `Storage::has_any_dictionary`、`Storage::resolve_dictionaries` 和 `Storage::lookup_folded` 从 query 可见方法改成 storage implementation 的私有方法；名称可以保留，以减少无关 diff。
6. 在 `impl Lookup<'_>` 中实现 `pub(crate) fn find`：
   - 调用 `self.storage.lookup_folded(term.folded(), &self.scope)` 取得按主键排序的候选。
   - 把原 `query.rs::select_matches` 的全局精确优先算法移入 storage 私有 implementation；可作为 `Lookup::find` 内部代码或 storage 私有函数，但不得向 query 暴露。
   - 如果有原词头精确项，保持候选原顺序并保留所有精确重复项；否则原样返回候选。
7. 保持 `lookup_sql`、`collect_lookup` 和 `StoredEntry` 行构造在 `src/storage.rs`；不改 SQL 文本、参数顺序或错误上下文。
8. 修改 `src/query.rs::run`：
   - 删除独立的空库检查和 scope 解析。
   - 在进入 term 循环前只调用一次 `let lookup = storage.lookup(dictionaries)?;`。
   - 每个 term 使用 `let matches = lookup.find(term)?;`。
   - 本 ticket 暂时保留现有 `output::Options`、每词 `multiple_dictionaries` provenance、`first_record`、`output::write_record` 与 `output::write_miss`，由 Ticket 01 后续替换。
9. 从 `src/query.rs` 删除 `StoredEntry` 导入、`select_matches` 函数及其 query 私有测试调用。
10. 按“测试变更”逐项迁移 `query.rs` 与 `storage.rs` 单元测试；先运行窄测试，再运行 CLI 查询测试。
11. 检查 `rg` 结果，确认退出 interface 的五个名称不再由 `query.rs` 引用，且没有新增 trait/adapter。
12. Validation 全部通过后才开始 Ticket 01。

## 必须保持的 Invariants 和错误

- 空数据库仍是运行错误而不是一组 miss，错误链包含 `no dictionaries are installed`；stdout 和 stderr 在此错误前都为空。
- 未知词典仍保留展示名称并报 `unknown dictionary '{name}'`，且在任何命中或 miss 输出前失败。
- 空词典筛选表示所有词典；重复且仅大小写不同的筛选名不复制结果。
- 筛选参数顺序不改变输出顺序，结果始终按 entries 主键顺序。
- 同一词典和跨词典的重复记录都保留。
- 精确项判断使用原始 `term.text()` 与存储 `headword` 的 Rust 字符串相等；只用 lowercase folded key 取候选，不增加 Unicode normalization、宽窄转换、音调折叠或标点处理。
- 精确项优先在全部已选词典候选上一次执行；任何词典存在精确项时，其他词典仅 folded 相等的大小写变体都被抑制。
- 没有精确项时，所有 folded 候选按原顺序返回。
- SQL prepare、query、行读取的既有错误上下文保持：`failed to prepare entry lookup`、`failed to look up entries`、`failed to read lookup results`。
- 安装检查与词典解析每批各执行一次，`find` 不重新查询词典 id。
- SQLite 保持唯一运行时数据源，同步阻塞模型不变。
- 当前输出合同（含来源标注、`Options` 和 `first_record` 分隔）在本 ticket 内字节不变。

## 测试变更

### `src/query.rs` 单元测试

逐项处理：

- 保留 `empty_database_is_a_runtime_error_not_a_miss`，继续通过 `run` 证明 lookup 构造错误在任何输出前传播。
- 删除 `global_exact_match_suppresses_case_variants_from_other_dictionaries`；同一策略由 storage 的 `Lookup::find` 单元测试和 CLI 测试承担。
- 删除 `case_fallback_keeps_every_folded_candidate`；同一策略由 storage 的 fallback 单元测试和 CLI 测试承担。
- 保留 `batch_writes_misses_to_stderr_and_keeps_hits`，它验证 query 批次编排、命中与 miss 并存及 `Outcome::SomeMissing`。
- 删除 `select_matches_is_global_not_per_dictionary`，因为 `select_matches` 退出 query implementation；在 storage 中以 `Lookup::find` interface 重建等价测试。
- 更新测试导入，移除 `select_matches` 及不再直接需要的类型/helper。

### `src/storage.rs` 单元测试

逐项处理：

- 把 `folded_lookup` 测试 helper 改为通过 `storage.lookup(&[])` 和 `lookup.find(&QueryTerm::parse(raw).unwrap())` 取得结果；helper 名称改成表达最终查词结果的 `find_entries`。
- 把 `lookup_returns_duplicates_across_dictionaries_in_primary_key_order` 改为使用一个没有原词头精确项的 query，例如候选为 `Hello`/`hello`/`HELLO` 时查询 `HeLLo`，从而通过 `Lookup::find` 同时证明 folded fallback、跨词典重复项和主键顺序。
- 把 `lookup_filter_is_case_insensitive_and_does_not_multiply_repeated_names` 改为先用重复大小写名称构造一次 `Lookup`，再调用 `find`，断言只返回 Oxford 一份结果。
- 把 `resolve_dictionaries_rejects_unknown_names` 重命名为 `lookup_rejects_unknown_names_before_find`，直接断言 `storage.lookup(&names)` 的既有错误。
- 保留 `lookup_query_plan_uses_the_folded_headword_index`，仍直接测试私有 `lookup_sql` 的无筛选与筛选两种形态。
- 新增 `lookup_rejects_an_empty_database`，直接验证 storage interface 的预检错误。
- 新增 `find_prefers_all_global_original_headword_matches`：跨两个词典建立 `Hello`、两个 `hello` 和其他大小写候选，查询 `hello`，断言只返回两个精确重复项且保持主键顺序。
- 新增 `one_lookup_reuses_scope_for_multiple_terms`：构造带词典筛选的单个 `Lookup`，连续查两个 term，断言两次结果都只来自已解析词典；测试代码不得调用私有 scope 解析函数。
- 更新 replace/remove 测试中调用旧 `folded_lookup` helper 的位置，使其通过最终 lookup interface 验证持久结果，不直接恢复旧 methods 的测试依赖。

### CLI 行为测试

保留 `tests/query.rs` 中以下可观察合同，不改测试名和断言：

- 空数据库运行错误。
- 未知词典在任何结果或 miss 前失败。
- 全词典主键顺序和重复项。
- 重复筛选名大小写不敏感且不复制。
- 筛选参数顺序不覆盖主键顺序。
- 全局精确筛选不按词典 fallback。
- 无精确项时返回每个 folded 候选。
- 不做额外 normalization。
- 批次 miss、重复查询词和筛选 miss。
- 当前来源标注、`--raw`/`--full` 与结构化输出字节。
- Ticket 03 已删除的查询计划测试保持删除状态。

其他五个集成测试文件（`cli_contract`、`import`、`list_remove`、`release_readiness`、`output_terminal`）只需继续编译通过，不因本 ticket 改变断言。

## Acceptance

- [ ] `src/storage.rs` 存在字段私有的 `pub(crate) struct Lookup<'storage>`，且字段与本 ticket 完全一致。
- [ ] `Storage::lookup` 与 `Lookup::find` 的可见性、参数和返回类型与最终 interface 一致。
- [ ] `Storage::lookup` 先做空库预检，再解析整个 dictionary name 批次，成功后才返回 `Lookup`。
- [ ] `query::run` 每批只构造一次 `Lookup`，term 循环只调用 `find`。
- [ ] `query.rs` 不引用 `DictionaryScope`、`has_any_dictionary`、`resolve_dictionaries`、`lookup_folded` 或 `select_matches`。
- [ ] `DictionaryScope` 及三个 storage helper 都是 storage 私有 implementation。
- [ ] `Lookup::find` 执行全局原词头精确匹配优先，并保留 fallback、主键顺序与重复项。
- [ ] SQL、参数、行构造和两种查询计划测试仍在 `storage.rs`。
- [ ] 没有新增 trait、第二存储 implementation 或 adapter。
- [ ] query/storage 单元测试按本 ticket逐项迁移，不保留针对旧浅 interface 的重复测试。
- [ ] CLI 查询行为测试全部通过，输出和退出码无变化。
- [ ] 当前输出 `Options`/provenance/`first_record` 仍由 query 持有，未提前实施 Ticket 01。
- [ ] `cargo fmt --check` 和 clippy 通过。

## Validation

按顺序运行：

```bash
cargo fmt --check
cargo test storage::tests
cargo test query::tests
cargo test --test query
cargo clippy --all-targets -- -D warnings
rg -n "DictionaryScope|has_any_dictionary|resolve_dictionaries|lookup_folded|select_matches" src/query.rs
rg -n "pub\(crate\) struct Lookup|pub\(crate\) fn lookup|pub\(crate\) fn find|fn has_any_dictionary|fn resolve_dictionaries|fn lookup_folded|ORDER BY e\.id" src/storage.rs
rg -n "trait .*Storage|dyn .*Storage|impl .*Storage for" src tests
git diff --check
git diff -- src/storage.rs src/query.rs tests/query.rs tests/support/mod.rs
```

预期结果：

- 格式、storage/query 单元测试、CLI query 测试和 clippy 成功。
- 对 `src/query.rs` 的第一个 `rg` 无匹配并以状态 1 结束。
- 对 `src/storage.rs` 的 `rg` 命中新 `Lookup` interface、三个私有 helper 和 `ORDER BY e.id`。
- trait/动态存储检查无匹配并以状态 1 结束。
- `tests/query.rs` 仅存在前置 ticket 的 fixture 迁移，不因本 ticket 改变行为断言；`tests/support/mod.rs` 不变。
- diff 仍包含当前 `write_record`/`first_record`/provenance，证明尚未开始 Ticket 01。

## 风险和回滚

- 风险：把精确优先误做成每词典分别筛选。控制方式是 storage 跨词典精确测试与既有 CLI `global_exact_filter_does_not_fall_back_per_dictionary`。
- 风险：在每次 `find` 里重新解析 scope，失去批次 leverage。控制方式是 `Lookup` 直接持有私有 scope，`find` 不接收 dictionary names。
- 风险：新入口改变空库与未知词典的错误先后。控制方式是 `Storage::lookup` 固定先预检、后解析，并保留 query/CLI 错误测试。
- 风险：测试迁移削弱索引证明。控制方式是查询计划测试完全留在 storage implementation，并继续覆盖两种 SQL。
- 风险：提前实施 `QueryOutput`。控制方式是本 ticket 明确保留当前输出 Options/provenance/`first_record`。
- 回滚：恢复 `query.rs` 原三步调用和 `select_matches`，删除 `Lookup` 并恢复 storage helper 可见性，同时恢复对应单元测试。schema 与用户数据不变，无需数据迁移。

## 完成条件

Acceptance 全部勾选，Validation 符合预期，CLI 字节与退出码无回归，`query.rs` 只看见新的两步 lookup interface，且没有 trait/adapter。完成后才能开始 [Ticket 01](01-deepen-query-presentation.md)。

## 文档链接

- [架构深化总规范](../specs/architecture-deepening.md)
- [Ticket 03：恢复存储测试 seam](03-restore-storage-test-seam.md)
- [Ticket 04：深化 CLI 合同夹具](04-deepen-cli-contract-fixture.md)
- [Ticket 02：集中精确查词策略](02-concentrate-exact-lookup.md)
- [Ticket 01：深化查询输出](01-deepen-query-presentation.md)
