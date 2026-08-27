# Lexi

Lexi 是一个 Linux 离线命令行查词工具。它从已经转换好的 UTF-8 JSON Lines 文件导入词条，保存到本地 SQLite 数据库，并提供精确查词。

第一版不读取 MDX/MDD，也不依赖原始词典文件。请先用外部软件转换成 JSONL，再交给 `lexi --import`。导入成功后，数据库是唯一运行时数据源，源 JSONL 可以删除或移动。

## 构建

需要：

- 较新的 stable Rust（本项目使用 2024 edition；请通过 [rustup](https://rustup.rs/) 安装，不要依赖过旧的发行版 `rustc`）
- 系统 `libsqlite3` 开发库
- `pkg-config`
- C 链接器（如 `gcc`）

程序链接系统提供的 `libsqlite3`，不启用 `rusqlite` 的 `bundled` 功能，并保持同步阻塞模型。不要把 SQLite 源码打进二进制。

Debian / Ubuntu：

```bash
sudo apt install build-essential pkg-config libsqlite3-dev
```

Fedora：

```bash
sudo dnf install gcc pkgconf sqlite-devel
```

Arch Linux：

```bash
sudo pacman -S base-devel sqlite
```

安装 Rust 后，在项目根目录构建 release 二进制：

```bash
cargo build --release
```

可执行文件位于 `target/release/lexi`。可将该路径加入 `PATH`，或直接用绝对路径调用。

## JSONL 边界

Lexi 只接受 UTF-8 JSONL，不解析 MDX/MDD、不提取媒体资源、不展开 `@@@LINK=` 之类的重定向。外部转换软件需要把别名写成普通记录。

文件规则：

- 编码必须是 UTF-8；文件开头允许一个 UTF-8 BOM。
- 支持 `LF` 和 `CRLF`。空行或只包含空白的行会被忽略。
- 导入逐行流式处理，不会把整个文件读入内存。
- 每个非空行必须是只含两个必填字符串字段的 JSON 对象：

```json
{"headword":"hello","definition":"interjection\n1. 你好；您好"}
```

- `headword` 必须非空，不能有首尾空白，也不能包含换行、制表符或其他控制字符。
- `definition` 必须是非空白字符串；JSON 解码后原样保存和输出。
- 缺少字段、类型错误或额外字段都会使整次导入失败。错误信息包含源文件路径和从 1 开始的行号。
- 同一部词典允许重复词头；这些记录不会覆盖或合并。

## 命令

程序名为 `lexi`。业务选项只提供长选项。一次调用只能执行一种模式。

查询：

```bash
lexi hello
lexi "take off" hello
lexi hello --dictionary oxford
lexi hello --dictionary oxford --dictionary longman
lexi hello --show-dictionary
```

导入：

```bash
lexi --import oxford.jsonl --name oxford
lexi --import oxford.jsonl --name oxford --force
```

`--import` 必须同时提供 `--name`。同名词典默认报错；`--force` 会原子替换，失败时保留原词典。

列出和删除：

```bash
lexi --list
lexi --remove oxford
```

`--list` 每行输出词典名称、制表符和词条数量，没有表头。`--remove` 不要求确认。

帮助和版本：

```bash
lexi
lexi --help
lexi --version
```

不带参数与 `--help` 相同，显示完整帮助并返回 `0`。

## 数据位置

数据库遵循 XDG 数据目录规范：

```text
$XDG_DATA_HOME/lexi/lexi.db
```

未设置 `XDG_DATA_HOME` 时使用：

```text
~/.local/share/lexi/lexi.db
```

第一版不提供配置文件或 `--database` 参数。测试应设置临时 `XDG_DATA_HOME`。

## 退出码

| 退出码 | 含义 |
| --- | --- |
| 0 | 操作成功，或显示帮助/版本 |
| 1 | 一个或多个查询词没有结果 |
| 2 | 参数无效、缺少参数或操作模式冲突 |
| 3 | JSONL、文件 I/O、SQLite、未知词典等运行错误 |

查询结果和成功的管理信息写入 `stdout`；参数错误和运行错误写入 `stderr`。

## 第一版范围

第一版提供导入、精确查词、词典筛选、列表和删除。它不做前缀/模糊查询、语言学归一化、TUI、配置文件、安装包或 MDX/MDD 解析。完整契约见 [`DEVELOPMENT.md`](DEVELOPMENT.md)。

## 开发

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

测试使用临时 JSONL 和临时 `XDG_DATA_HOME`，不读取仓库中的 MDX/MDD 或已转换词典文件。
