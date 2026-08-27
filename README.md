# Lexi

Linux 离线命令行查词工具。从已转换的 UTF-8 JSONL 导入词条到本地 SQLite，再精确查词。

第一版不读取 MDX/MDD。请先用外部软件转成 JSONL，再交给 `lexi --import`。导入成功后，数据库是唯一运行时数据源，源 JSONL 可以删除或移动。

命令见 `lexi --help`。

## 构建

需要：

- 较新的 stable Rust（本项目使用 2024 edition；请通过 [rustup](https://rustup.rs/) 安装，不要依赖过旧的发行版 `rustc`）
- 系统 `libsqlite3` 开发库
- `pkg-config`
- C 链接器（如 `gcc`）

程序链接系统 `libsqlite3`，不启用 `rusqlite` 的 `bundled` 功能。

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

```bash
cargo build --release
```

可执行文件在 `target/release/lexi`。

## JSONL

```json
{"headword":"hello","definition":"interjection\n1. 你好；您好"}
```

UTF-8 JSONL；允许 BOM；支持 `LF` / `CRLF`。空行忽略。
`headword` 非空，无首尾空白和控制字符。`definition` 非空白，解码后原样保存。
缺字段、类型错误或额外字段会使整次导入失败。同一词典允许重复词头。别名写成普通记录。

## 命令

```bash
lexi hello
lexi "take off" hello
lexi hello --dictionary oxford
lexi hello --show-dictionary
lexi hello --raw
lexi --import oxford.jsonl --name oxford
lexi --import oxford.jsonl --name oxford --force
lexi --list
lexi --remove oxford
lexi --help
lexi --version
```

`--import` 必须同时给 `--name`。同名词典默认报错；`--force` 原子替换，失败时保留原词典。
`--list` 每行 `名称<TAB>词条数`。`--remove` 不确认。不带参数与 `--help` 相同。
查询默认把 HTML 词条转成终端易读文本；`--raw` 输出库里保存的原文。

## 数据位置

```text
$XDG_DATA_HOME/lexi/lexi.db
```

未设置 `XDG_DATA_HOME` 时使用 `~/.local/share/lexi/lexi.db`。

## 退出码

| 退出码 | 含义 |
| --- | --- |
| 0 | 成功，或帮助/版本 |
| 1 | 一个或多个查询词没有结果 |
| 2 | 参数无效、缺少参数或模式冲突 |
| 3 | JSONL、文件 I/O、SQLite、未知词典等运行错误 |

## 开发

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```
