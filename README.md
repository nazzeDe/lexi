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
lexi hello --full
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

### 查询输出

查询输出普通文本后退出，不启动 TUI、分页器、模型或网络服务。

- 默认：识别受支持的 HTML 结构后，按源词条分组（如 `post 1/2/3`）、词性、主义项、子义项和短语的原顺序排版。英文释义和中文译文分行，保留全部释义，包括罕见义项、标签、屈折变化、派生词和用法说明。没有总长度限制。
- 每个主义项默认最多显示其直接附带的第一个例句及译文；不显示子义项的例句。短语义项使用相同规则。已识别的语源默认隐藏。确有省略时，每条记录末尾只提示一次实际省略的例句数量和是否省略语源，并提示 `--full`。
- `--full`：使用相同的新排版，显示全部例句和语源。
- `--raw`：保留库中 definition 原文（包括 HTML），沿用原来的词头标题、记录分隔和末尾换行规则，不加颜色或自动换行。与 `--full` 冲突；两个选项都只用于查询。

同一个查询词的最终命中跨多个词典时，可读输出自动在标题显示 `[词典名]`；只有一个词典时默认不显示。`--show-dictionary` 可强制显示。`--raw` 不自动增加词典名，但仍支持 `--show-dictionary`。匹配优先级、记录顺序和重复记录不变。

终端输出以粗体显示标题和已识别的词性、淡色显示屈折变化和例句，没有边框；子义项以 `-` 标记，例句组以 `>` 标记，译文与续行对齐，管道中也能区分层级。按终端宽度换行，计算中包含中文字符宽度。设置 `NO_COLOR` 可禁用颜色而保留换行。管道和重定向输出不含程序添加的 ANSI 样式，也不插入终端宽度相关的换行；内容和层级相同。极窄终端会缩减缩进；单个双宽字符无法塞入只有一列的终端。

### 格式支持与限制

结构化排版根据 `CK/DC/JS/CY/CX` 等 HTML 结构指纹识别，而不是根据导入时指定的词典名称。HTML5 容错解析会处理该格式中的自闭合锚点；可恢复的内联标签保留文字顺序。释义、词性或例句译文缺失时只保留现有字段，不猜词性、不做语言检测或补译。内联文字保留，装饰性方块和重复词头不重复显示。

纯文本 definition 保持原文。未知格式或结构不可靠的整条记录回退到原有通用 HTML 转文本，不做例句或语源省略；未知区段也保留为通用文本。通用转换不是浏览器，不能还原图片、CSS 布局或所有复杂 HTML 的语义。`entry://` 引用保留可见文字与目标（通用回退在后面列出 `Reference:`），不递归查找。`--full` 是完整可读文本，不是原始 HTML；需要原始标记时使用 `--raw`。

构建 HTML 树前使用 HTML5 token 流检查复杂度：超过 8192 个起始标签或保守估计嵌套深度超过 128 层时，整条记录使用通用回退，不省略内容；树构建后还会迭代检查实际深度。回退仍提取 `entry://` 目标，`--raw` 不受限制。省略闭合标签等不规则 HTML 可能提前触发保守回退；这不是通用 HTML 资源沙箱。

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
