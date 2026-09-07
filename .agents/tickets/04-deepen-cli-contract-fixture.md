# Ticket 04：深化 CLI 合同夹具

## 状态

Completed

## 目的

把六个集成测试文件重复的临时路径、子进程配置、文件写入和 UTF-8 解码集中到一个测试专用 module。通过小而固定的 `CliFixture` interface 提供真实 `lexi` 进程、临时 JSONL 和临时 `XDG_DATA_HOME`，同时让退出码、输出内容和 SQLite 数据断言继续留在各行为测试中。

## Seam

seam 位于六个集成测试文件与操作系统测试设施之间，interface 定义在 `tests/support/mod.rs`：

- `CliFixture` 是临时 XDG、临时文件目录和 `lexi` 命令的 adapter。
- `Command`、`Output`、`TempDir`、文件系统与 Unix closed stream 留在 support implementation。
- 各集成测试通过 fixture 获得路径和运行结果，仍直接承担领域行为断言。
- 各测试专属 SQLite tuple 查询保留在原测试文件，不进入 support seam。
- `tests/output_terminal.rs` 的 PTY 分配、窗口尺寸、raw 属性和读循环保持本地，不进入 `CliFixture`。

## 依赖

- 前置：[Ticket 03](03-restore-storage-test-seam.md)，必须已完成并验证。
- 固定执行序列中的位置：第二项。
- 后继：[Ticket 02](02-concentrate-exact-lookup.md)。本 ticket 验证完成后再开始 02。
- 设计依据：[架构深化总规范](../specs/architecture-deepening.md)。

## 范围

新增 `tests/support/mod.rs`，迁移以下六个文件：

- `tests/cli_contract.rs`
- `tests/import.rs`
- `tests/list_remove.rs`
- `tests/query.rs`
- `tests/release_readiness.rs`
- `tests/output_terminal.rs`

support 只负责：

- 创建和持有两个 `TempDir`。
- 计算 data home、files 和数据库路径。
- 创建预配 `XDG_DATA_HOME` 且移除 `HOME` 的 `Command`。
- 执行命令并返回原始 `Output`。
- 在临时 files 目录写指定文件名和字节。
- 把 CLI 输出按 UTF-8 解码。
- 为 Unix 写错误测试构造 closed output。

`CliFixture` 的拟议公开 interface 不变。不改实际测试断言，不增加依赖。

## 非目标

- 不修改生产代码、README 或 Cargo 文件。
- 不改变任何测试覆盖的用户行为或断言文本。
- support 不断言退出码、stdout、stderr 或数据库内容。
- support 不提供 `import_ok`、`import`、`import_named` 等带成功语义的 helper。
- support 不封装 `rusqlite::Connection`，也不接收或返回各测试不同的 SQLite tuple。
- 不把 `manifest_dir`、Cargo.lock 解析、Rust 源码遍历等 release 专属 helper 移入 support。
- 不把 PTY 打开、窗口尺寸、raw 终端属性或 PTY 读循环移入 support。
- 不引入 mock 进程、内存数据库或新的测试依赖。

## 最终 Interface

`tests/support/mod.rs` 必须呈现以下 interface；字段保持私有：

```rust
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

pub struct CliFixture {
    data_home: TempDir,
    files: TempDir,
}

impl CliFixture {
    pub fn new() -> Self;
    pub fn data_home(&self) -> &Path;
    pub fn files(&self) -> &Path;
    pub fn database_path(&self) -> PathBuf;
    pub fn command(&self) -> Command;
    pub fn run(&self, args: &[&str]) -> Output;
    pub fn write_jsonl(
        &self,
        name: &str,
        contents: impl AsRef<[u8]>,
    ) -> PathBuf;
}

pub fn text(bytes: &[u8]) -> String;

#[cfg(unix)]
pub fn closed_output() -> Stdio;
```

interface 约束：

- `new` 创建互相独立的 data home 和 files 临时目录。
- `database_path` 始终返回 `data_home()/lexi/lexi.db`。
- `command` 使用 `env!("CARGO_BIN_EXE_lexi")`，设置 `XDG_DATA_HOME` 为 fixture 的 data home，移除 `HOME`；同时将 stdin 设为 `Stdio::null()`，避免测试进程继承交互输入。
- `run` 只做 `command().args(args).output()` 并返回 `Output`；运行失败时使用固定诊断 `lexi should run`。
- `write_jsonl` 将 `name` 直接连接到 `files()`，用 `fs::write` 写入 `contents` 并返回路径；文件名由调用测试显式提供。
- `text` 使用 `String::from_utf8(bytes.to_vec())`，解码失败时使用固定诊断 `CLI output should be UTF-8`。
- `closed_output` 保持当前 `UnixStream::pair`、丢弃 reader、把 writer 的 `OwnedFd` 转成 `Stdio` 的实现。

## Implementation 步骤

1. 新建 `tests/support/mod.rs`，实现上述完整 interface。implementation 中可以使用 `expect`/`unwrap` 表达测试设施无法建立，但不得包含领域断言。
2. 在六个集成测试文件首行加入 `mod support;`，并按实际使用导入 `CliFixture`、`text`、`closed_output`。
3. 迁移 `tests/cli_contract.rs`：
   - 用每个测试自己的 `CliFixture::new()` 替代普通 data-home `TempDir`。
   - 用 `fixture.run(args)` 替代通用 `run`。
   - 用 `fixture.database_path()` 替代重复的 `lexi/lexi.db` 拼接。
   - closed stdout/stderr 场景用 `fixture.command()` 继续定制 `.stdout(closed_output())`、`.stderr(closed_output())` 或 `.status()`。
   - HOME fallback 与相对 XDG 两个特殊环境测试继续直接构造 `Command` 和它们自己的临时目录，因为它们刻意覆盖 fixture 的默认环境；文件仍声明并使用 `mod support`，其他测试使用 fixture。
   - SQLite schema 检查和所有行为断言留在本文件。
4. 迁移 `tests/import.rs`：
   - 删除本地通用 `run`、`text`、`write_jsonl`。
   - 每个测试创建 fixture，并通过 `fixture.write_jsonl(明确文件名, contents)` 与 `fixture.run(args)` 操作。
   - `dictionary_rows`、`entry_rows` 继续留在本文件；把参数收窄为 `&CliFixture` 或数据库 `&Path`，查询 tuple 和 SQL 原样保留。
   - `import_named`、`seed_oxford_and_keep`、`assert_original_dictionary_still_usable` 继续作为本文件的领域 helper，但改为调用 fixture；成功断言仍由这些本地 helper 负责。
5. 迁移 `tests/list_remove.rs`：
   - 删除本地通用 `run`、`text`。
   - 保留本地 `import` 领域 helper 和成功断言，内部使用 `fixture.write_jsonl` 与 `fixture.run`。
   - `dictionary_rows`、`entry_rows` 及其不同 tuple 继续留在本文件并使用 fixture 数据库路径。
6. 迁移 `tests/query.rs`：
   - 删除本地通用 `run`、`text`、`closed_output`、`write_jsonl`。
   - 保留本地 `import` 领域 helper及其导入成功断言，内部使用 fixture。
   - 所有查询、筛选、顺序、重复项、精确优先、folded fallback、输出字节、来源标注和错误断言保持原值。
   - closed stdout/stderr 场景使用 `fixture.command()` 进行单项定制。
7. 迁移 `tests/release_readiness.rs`：
   - 删除本地通用 `run`、`text`，临时端到端测试改用 fixture。
   - `manifest_dir`、`lock_package_dependencies`、`rust_sources_under` 保留在本文件。
   - `ldd` 测试继续直接构造自己的 `Command`，因为目标不是 fixture 配置的 `lexi` CLI 调用。
8. 迁移 `tests/output_terminal.rs`：
   - 声明 `mod support;`，用 `CliFixture` 取代通用临时 data home / files。
   - 用 `fixture.write_jsonl` 写导入输入，用 `fixture.command()` 做导入和普通（非 PTY）进程配置。
   - PTY 打开、`grantpt`/`unlockpt`、窗口尺寸、raw 属性、slave stdout 和读循环继续留在本文件的本地 helper。
   - 不改断言，不增加依赖。
9. 运行 `cargo fmt --check`、六个集成测试和 `cargo clippy --all-targets -- -D warnings`。
10. dead-code 固定决策规则：初始 `tests/support/mod.rs` 不加 allow；若且仅若第 9 步的 clippy 实际报告该 support 模块 public item 的 `dead_code`，就在 support 模块顶层加入带原因的 `#![allow(dead_code, reason = "共享 support 在各集成测试 crate 中只使用其 interface 子集")]`；若 clippy 未报告则保持无 allow。加属性后必须重新运行同一 clippy 命令并通过。
11. 检查 diff，确认 support 中没有任何行为断言、导入成功 helper 或 SQLite tuple 查询，然后进入 Ticket 02。

## 必须保持的 Invariants 和错误

- 每个测试实例拥有隔离的临时 XDG 数据目录和临时文件目录，生命周期覆盖所有子进程和数据库检查。
- fixture 默认命令始终设置绝对 `XDG_DATA_HOME`、移除 `HOME` 并关闭 stdin；特殊 HOME fallback 测试继续显式覆盖环境。
- 所有 CLI 调用继续运行真实 `CARGO_BIN_EXE_lexi`，不绕过进程 interface。
- 所有导入输入继续来自临时 JSONL，所有持久性检查继续打开临时真实 SQLite。
- `run` 保留原始 `Output`，不把非零退出码转换为设施错误。
- `text` 的 UTF-8 错误诊断不变；`run` 的 spawn 错误诊断不变。
- closed stream helper 只在 Unix 测试路径可用，写失败测试仍观察运行错误退出码 3 和既有上下文。
- support 不吞掉 stderr、不改写参数、不自动导入、不自动断言成功。
- `output_terminal.rs` 继续自己完成 PTY 分配与读循环。

## 测试变更

### 迁移

- 六个集成测试文件都添加 `mod support;`。
- 通用临时目录、进程、UTF-8、文件写入和 closed output 代码迁至 `tests/support/mod.rs`。
- 所有测试名和行为断言保持不变；Ticket 03 已明确删除的查询计划测试不恢复。不以臆造测试个数作为门槛。

### 保留在各测试文件

- `cli_contract.rs`：schema 查询、HOME/XDG 特例和 CLI 合同断言。
- `import.rs`：词典/词条 tuple 查询、事务和导入领域 helper、全部导入断言。
- `list_remove.rs`：与该文件 tuple 对应的 SQLite 查询、导入领域 helper、列表删除断言。
- `query.rs`：导入领域 helper和全部查询行为断言。
- `release_readiness.rs`：manifest/lock/source 检查 helper、`ldd` 进程和发布合同断言。
- `output_terminal.rs`：PTY helper 以及 TTY 折行、颜色、`NO_COLOR`、`--full`、`--raw` 断言。

### dead-code 验证

先用没有 allow 的 support 运行 clippy。只有 clippy 实际给出 support public item 的 dead-code 诊断时，才加入步骤 10 指定且带原因的模块级 allow；诊断未出现时不加入该属性。此规则可执行且结果由命令输出唯一决定。

## Acceptance

- [x] `tests/support/mod.rs` 存在且 `CliFixture` 只有 `data_home`、`files` 两个私有字段。证据：`tests/support/mod.rs` 字段仅这两项且私有。
- [x] `CliFixture` 和共享函数的 interface 与本 ticket 代码块一致。证据：`new`/`data_home`/`files`/`database_path`/`command`/`run`/`write_jsonl`/`text`/`closed_output` 签名与约束一致。
- [x] 六个集成测试文件都声明 `mod support;` 并实际使用 support 项。证据：`rg -n "^mod support;$"` 六文件各命中一次。
- [x] 六个文件中不再定义重复的通用 `run` 或 `text`。证据：第二条 `rg` 退出码 1。
- [x] `closed_output` 只定义在 support，查询和 CLI 合同写失败测试仍通过。证据：`cli_contract` 9 passed、`query` 22 passed，含 closed stdout/stderr。
- [x] JSONL 通用文件写入只定义在 support，各调用方显式传文件名。证据：第二条 `rg` 无 `fn write_jsonl(`；调用方传入 `sample.jsonl`/`{name}.jsonl` 等。
- [x] support 中没有 `assert!`、`assert_eq!`、退出码判断、输出内容判断、SQLite 查询或带成功语义的导入 helper。证据：第三条 `rg` 退出码 1。
- [x] 各文件原有 SQLite tuple 查询仍留在原文件，没有被统一成共享 tuple。证据：`import.rs` 仍为四元组 entries，`list_remove.rs` 仍为三元组 entries。
- [x] PTY 分配、窗口尺寸、raw 属性和读循环仍在 `output_terminal.rs`。证据：`openpt`/`grantpt`/`unlockpt`/`tcsetwinsize`/`make_raw` 仅该文件；support 无匹配。
- [x] 测试继续使用真实二进制、临时 JSONL、临时 XDG 和真实 SQLite。证据：`command()` 使用 `CARGO_BIN_EXE_lexi` 并设置绝对 `XDG_DATA_HOME`、移除 `HOME`；`write_jsonl` 写临时 files；SQLite 仍由各测试打开 `database_path()`。
- [x] clippy dead-code 规则已按实际诊断执行，最终 clippy 无警告。证据：首次无 allow 的 `cargo clippy --all-targets -- -D warnings` 报告 support public item `dead_code`（`files`/`write_jsonl`/`database_path`/`run`/`closed_output`）；随后加入模块级 `#![allow(dead_code, reason = "共享 support 在各集成测试 crate 中只使用其 interface 子集")]` 并复跑同一 clippy 通过。
- [x] 六个集成测试目标全部通过。证据：cli_contract 9、import 13、list_remove 5、query 22、release_readiness 7、output_terminal 1；`cargo test` 112 passed。
- [x] 没有生产代码、README 或 Cargo 文件变更。证据：工作区仅 `M` 六个测试文件与未跟踪 `tests/support/`，外加本 ticket/spec 勾选。

## Validation

按顺序运行：

```bash
cargo fmt --check
cargo test --test cli_contract
cargo test --test import
cargo test --test list_remove
cargo test --test query
cargo test --test release_readiness
cargo test --test output_terminal
cargo clippy --all-targets -- -D warnings
rg -n "^mod support;$" tests/cli_contract.rs tests/import.rs tests/list_remove.rs tests/query.rs tests/release_readiness.rs tests/output_terminal.rs
rg -n "fn run\(|fn text\(|fn closed_output\(|fn write_jsonl\(" tests/cli_contract.rs tests/import.rs tests/list_remove.rs tests/query.rs tests/release_readiness.rs tests/output_terminal.rs
rg -n "assert!|assert_eq!|rusqlite|SELECT |status\.code|import_ok|fn import\(" tests/support/mod.rs
git diff --check
git diff -- tests/support/mod.rs tests/cli_contract.rs tests/import.rs tests/list_remove.rs tests/query.rs tests/release_readiness.rs tests/output_terminal.rs
```

预期结果：

- 格式、六个测试目标和 clippy 成功。
- 第一条 `rg` 恰好在六个集成测试文件各命中一次。
- 第二、三条 `rg` 无匹配并以状态 1 结束；这是检查通过的预期结果。
- diff 只涉及新增 support 与六个集成测试迁移，不含生产代码、README 或 Cargo 文件。

## 风险和回滚

- 风险：共享 helper 开始表达领域成功语义，形成浅层便利函数。控制方式是 Acceptance 中对断言、SQLite 和导入 helper 的负向检查。
- 风险：特殊 HOME/XDG 测试被默认 fixture 环境掩盖。控制方式是保留这两个测试的直接 `Command` 构造。
- 风险：把 PTY 细节塞进 fixture，扩大 interface。控制方式是 PTY 分配与读循环保持本地，只复用 `command()`/`write_jsonl`。
- 风险：每个集成测试 crate 只使用 support interface 子集而触发 clippy。控制方式是步骤 10 的单一诊断规则和带原因模块属性。
- 风险：迁移时改变文件名或环境导致测试意义变化。控制方式是逐文件迁移并保持原断言字节。
- 回滚：整体恢复六个测试文件并删除 `tests/support/mod.rs`；生产代码、数据库 schema 与用户数据均无需回滚。

## 完成条件

Acceptance 全部勾选，Validation 符合预期，dead-code 规则已有 clippy 输出作为证据，六个测试目标的行为断言无变化，且 diff 不含生产代码。完成后将工作交给 [Ticket 02](02-concentrate-exact-lookup.md)。

## 文档链接

- [架构深化总规范](../specs/architecture-deepening.md)
- [Ticket 03：恢复存储测试 seam](03-restore-storage-test-seam.md)
- [Ticket 04：深化 CLI 合同夹具](04-deepen-cli-contract-fixture.md)
- [Ticket 02：集中精确查词策略](02-concentrate-exact-lookup.md)
- [Ticket 01：深化查询输出](01-deepen-query-presentation.md)
