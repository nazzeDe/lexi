# Lexi 第一版开发规格

## 1. 目标

Lexi 是一个使用 Rust 编写的 Linux 离线命令行查词工具。它从已经转换好的 JSON Lines 文件导入词条，将数据保存到本地 SQLite 数据库，并提供快速的精确查词能力。

第一版的核心流程是：

```text
MDX/MDD
  -> 外部转换软件
  -> UTF-8 JSONL
  -> lexi --import
  -> SQLite
  -> lexi <词头>
```

MDX/MDD 的解析、资源提取和文本转换属于外部软件。Lexi 不读取这些格式，也不依赖原始 MDX/MDD 文件。

## 2. 第一版范围

第一版提供以下能力：

- 导入一部或多部 JSONL 词典。
- 使用 SQLite 保存导入后的全部词条。
- 查询一个或多个精确词头。
- 按词典名称限定查询范围。
- 可选显示查询结果所属的词典。
- 列出数据库中的词典和词条数量。
- 原子替换同名词典。
- 删除词典。
- 提供稳定的退出码和适合管道处理的纯文本输出。

第一版不包含：

- MDX/MDD 解析或转换。
- MDD 中的图片、音频、样式或其他资源。
- 发音内容。
- 前缀查询、模糊查询、候选词或拼写纠正。
- 繁简转换、全角半角转换、假名转换或其他语言学归一化。
- 结构化解析词性、义项、例句或词源。
- 交互式 REPL、TUI、GUI 或后台服务。
- 自动分页、颜色输出或 ANSI 样式渲染。
- 配置文件、词典优先级或默认词典设置。
- 安装包、预编译发行包、shell 补全或 man page。

## 3. 命令行接口

程序名为 `lexi`。第一版不为业务选项提供短选项。

### 3.1 查询

```bash
lexi hello
lexi "take off" hello
lexi hello --dictionary oxford
lexi hello --dictionary oxford --dictionary longman
lexi hello --show-dictionary
```

规则：

- 每个位置参数表示一个独立词头。
- 包含空格的词头必须使用 shell 引号。
- 多个词头按照命令行输入顺序依次查询。
- `--dictionary <NAME>` 可重复，用于限定一部或多部词典。
- 未提供 `--dictionary` 时查询数据库中的全部词典。
- `--show-dictionary` 只影响查询输出。

### 3.2 导入

```bash
lexi --import oxford.jsonl --name oxford
lexi --import oxford.jsonl --name oxford --force
```

规则：

- `--import <PATH>` 和 `--name <NAME>` 必须同时提供。
- 同名词典已存在时，默认返回错误。
- `--force` 只适用于导入模式。
- `--force` 原子替换同名词典。
- 新数据导入失败时，原词典必须完整保留。
- 导入完成后不保存源文件路径，也不再依赖源 JSONL 文件。

成功输出：

```text
Imported oxford: 185432 entries
Replaced oxford: 185500 entries
```

### 3.3 列出词典

```bash
lexi --list
```

每行包含词典名称和词条数量，以制表符分隔，不输出表头：

```text
oxford\t185432
longman\t230119
```

这里的 `\t` 表示实际的制表符。词条数量是成功导入的 JSONL 记录数，重复词头也分别计数。数据库中没有词典时，`stdout` 为空并返回 `0`。

### 3.4 删除词典

```bash
lexi --remove oxford
```

删除不要求交互确认。成功输出：

```text
Removed oxford: 185432 entries
```

词典不存在时不修改数据库，并返回运行错误。

### 3.5 帮助和版本

```bash
lexi
lexi --help
lexi --version
```

不带参数的 `lexi` 与 `lexi --help` 行为相同，显示完整帮助并返回 `0`。

### 3.6 模式互斥

一次调用只能执行一种模式：

- 查询位置参数
- `--import`
- `--list`
- `--remove`
- `--help`
- `--version`

冲突的模式或不适用于当前模式的选项属于参数错误。例如：

```bash
lexi hello --list
lexi --list --force
lexi --remove oxford --show-dictionary
```

## 4. JSONL 数据契约

### 4.1 编码和行格式

- 文件必须使用 UTF-8。
- 文件开头允许一个 UTF-8 BOM。
- 支持 `LF` 和 `CRLF` 行结束符。
- 空行或只包含空白的行被忽略，不计入词条数量。
- 每个非空行必须是一个完整的 JSON 对象。
- 导入必须逐行流式处理，不允许将整个文件读入内存。

### 4.2 记录结构

每条记录只能包含两个必填字符串字段：

```json
{"headword":"hello","definition":"interjection\n1. 你好；您好"}
```

字段规则：

- `headword` 必须是非空字符串。
- `headword` 不得包含首尾空白。
- `headword` 不得包含换行、制表符或其他控制字符。
- `definition` 必须是非空白字符串。
- `definition` 的内容在 JSON 解码后原样保存和输出。
- `definition` 不做清理、裁剪、重排、ANSI 解释或控制字符过滤。
- 缺少字段、字段类型错误或额外字段均视为导入错误。

任意一条记录无效时，整次导入失败。错误必须包含源文件和从 1 开始计算的行号。

### 4.3 重复词头

同一部词典允许包含多个完全相同的 `headword`。这些记录不得覆盖或合并，查询时全部输出，顺序按该词典中的导入顺序保持稳定。

### 4.4 MDX 重定向和别名

Lexi 不保存重定向关系。外部转换软件负责解析类似 `@@@LINK=target` 的 MDX 重定向，并将别名展开为普通记录：

```json
{"headword":"color","definition":"颜色；色彩"}
{"headword":"colour","definition":"颜色；色彩"}
```

## 5. 词典名称

- 词典名称是非空 UTF-8 字符串。
- CLI 自动去除名称首尾空白。
- 名称可以包含中文、日文、空格和其他正常 Unicode 字符。
- 数据库保留原始写法用于显示。
- 名称查找和唯一性判断忽略 Unicode 大小写。
- `Oxford` 和 `oxford` 视为同一名称，不能同时存在。
- 名称包含空格时必须使用 shell 引号。

词典名称只用于管理和筛选。默认查询输出不显示词典名称。

## 6. 查询语义

### 6.1 查询规范化

对每个查询词头执行以下处理：

1. 去除查询字符串的首尾空白。
2. 空字符串属于参数错误。
3. 计算 Unicode 小写形式，用于大小写无关候选查询。

不执行 Unicode 兼容归一化，也不转换标点、重音、全角半角、繁简体或假名。

### 6.2 大小写筛选

查询所选词典中小写形式相同的全部候选，然后统一筛选：

1. 如果任意候选的原始 `headword` 与输入完全一致，只输出所有完全一致的记录。
2. 如果没有完全一致的候选，输出全部大小写无关候选。

该判断在全部选定词典上统一进行，而不是逐部词典回退。输出始终使用数据库中保存的原始词头。

第一版的“忽略大小写”使用 Rust 的 Unicode 小写转换，不引入与地区相关的排序或语言规则。

### 6.3 词典筛选

- `--dictionary` 可以重复。
- 名称匹配忽略大小写。
- 执行查询前必须验证所有指定词典均存在。
- 任意指定词典不存在时，不输出部分查询结果，返回运行错误。

### 6.4 多结果和顺序

- 不对跨词典或词典内的重复结果去重。
- 同一词典内按记录导入顺序输出。
- 跨词典不提供用户可配置的优先级。
- 实现应使用数据库主键顺序提供可复现结果，但调用方不应把它解释为词典质量或业务优先级。

## 7. 查询输出

### 7.1 默认格式

默认只显示实际匹配到的词头和释义：

```text
hello
interjection
1. 你好；您好
```

多条结果之间增加一个空行：

```text
bank
n. 银行

bank
n. 河岸
```

程序写入词头和换行，然后原样写入 `definition`。如果 `definition` 末尾没有换行，程序补一个换行；记录之间再写入一个分隔空行。程序不得裁剪释义内容。

### 7.2 显示词典名称

使用 `--show-dictionary` 时，标题格式为：

```text
[oxford] hello
interjection
1. 你好；您好
```

### 7.3 未命中

单个词头未命中时，`stdout` 不写入该词的内容，`stderr` 输出：

```text
No entry found for: helo
```

批量查询继续处理后续词头。已找到的结果正常写入 `stdout`，每个未命中的词头分别写入 `stderr`。只要有任意词头未命中，最终退出码为 `1`。

数据库中没有任何词典时应报告运行错误，而不是把每个词头报告为普通未命中。

## 8. 退出码和错误流

```text
0  操作成功，或显示帮助/版本
1  一个或多个查询词没有结果
2  参数无效、缺少参数或操作模式冲突
3  JSONL、文件 I/O、SQLite、未知词典等运行错误
```

规则：

- 查询结果和成功的管理命令信息写入 `stdout`。
- 参数错误和运行错误写入 `stderr`。
- 运行错误优先于普通未命中退出码。
- 错误信息应保留操作、词典、文件和行号等诊断上下文。
- 错误信息和帮助文本第一版使用英文。

## 9. SQLite 存储设计

### 9.1 数据库位置

Lexi 只正式支持 Linux，并遵循 XDG 数据目录规范：

```text
$XDG_DATA_HOME/lexi/lexi.db
```

未设置 `XDG_DATA_HOME` 时使用：

```text
~/.local/share/lexi/lexi.db
```

父目录由 Lexi 在需要时创建。第一版不提供配置文件、`--database` 参数或其他数据库位置覆盖机制。测试通过临时设置 `XDG_DATA_HOME` 隔离数据。

### 9.2 SQLite 依赖

- 使用 `rusqlite`。
- 链接系统提供的 `libsqlite3`。
- 不启用 `rusqlite` 的 `bundled` 功能。
- README 必须说明编译时需要系统 SQLite 开发库。

### 9.3 建议模式

实际 SQL 可以在实现时调整，但必须表达以下信息和约束：

```sql
CREATE TABLE dictionaries (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL,
    normalized_name TEXT NOT NULL UNIQUE,
    entry_count     INTEGER NOT NULL
);

CREATE TABLE entries (
    id              INTEGER PRIMARY KEY,
    dictionary_id   INTEGER NOT NULL REFERENCES dictionaries(id) ON DELETE CASCADE,
    sequence        INTEGER NOT NULL,
    headword        TEXT NOT NULL,
    folded_headword TEXT NOT NULL,
    definition      TEXT NOT NULL,
    UNIQUE (dictionary_id, sequence)
);

CREATE INDEX entries_lookup
    ON entries(folded_headword, dictionary_id, sequence);
```

约束：

- 不对 `headword` 建立唯一约束。
- `folded_headword` 在 Rust 中计算，不依赖 SQLite 的 `LOWER()`。
- 外键约束必须启用。
- 使用 `PRAGMA user_version` 标记数据库模式版本。
- 第一版只需创建最新模式；遇到未知的未来模式版本时明确报错，不猜测迁移。
- `--force` 的删除旧记录、插入新记录和更新计数必须处于同一事务。

## 10. 规模与性能

实际 MDX 文件最大约 43 MB，但其转换后的 JSONL 可能更大。第一版目标为：

- 支持单个 JSONL 文件约 500 MB。
- 导入时内存占用基本不随文件大小增长。
- 导入使用一个事务和预编译语句，避免逐条提交。
- 词头查询必须使用索引，不扫描完整词条表。
- 正常热缓存精确查询目标低于约 100 ms。
- 不实现并行导入、后台索引、动态进度条或查询缓存。

500 MB 是设计目标，不要求在持续集成测试中保存或生成同等大小的固定夹具。

## 11. 代码结构原则

建议按责任组织代码，不要求为了层次而创建空壳模块：

- CLI 层负责参数解析、模式互斥和退出码映射。
- 应用层负责导入、查询、列出和删除用例的编排。
- JSONL 模块负责流式解码和记录校验。
- SQLite 模块负责模式初始化、事务、索引查询和持久化不变量。
- 输出模块负责稳定的纯文本格式，不理解释义内部语义。
- 领域类型负责词典名称、词条和查询词的验证及大小写键计算。

SQLite 模块应隐藏 SQL 和事务细节。调用方不应拼接 SQL、管理表名或依赖 SQLite 行结构。只有一个 SQLite 实现时不创建抽象仓储 trait；测试可以直接使用临时的真实 SQLite 数据库。

程序保持同步阻塞模型。第一版不引入 Tokio、线程池或异步抽象。

## 12. 依赖原则

预期使用：

- `clap`：命令行解析和帮助文本。
- `rusqlite`：系统 SQLite 访问，不启用 bundled。
- `serde` 和 `serde_json`：严格 JSONL 解码。
- 一个轻量错误上下文方案，例如 `anyhow` 或明确的应用错误类型。
- `tempfile`、`assert_cmd` 等仅作为测试依赖，按实际需要选择。

不要为第一版加入网络、异步、日志框架、全文检索、终端 UI 或配置框架依赖。

项目使用 Rust 2024 edition 和较新的 stable Rust。第一版不承诺较低 MSRV。

## 13. 测试策略

测试通过公开 CLI 行为和稳定模块接口保护需求，使用临时 XDG 数据目录和真实 SQLite 数据库。测试不得读取 `dictfile/` 中的真实 MDX/MDD 文件。

必须覆盖：

- 合法 JSONL 导入和词条计数。
- UTF-8 BOM、LF、CRLF 和空行。
- JSON 错误、缺失字段、额外字段、空字段和非法词头。
- 错误行号和整次导入回滚。
- 同一词典中的重复词头。
- 多词典查询和重复 `--dictionary` 筛选。
- 大小写候选查询和全局精确结果筛选。
- 不进行全角半角、标点或其他额外归一化。
- 多词批量查询、部分未命中和退出码 `1`。
- 默认输出、`--show-dictionary` 和释义原样输出。
- 同名导入失败。
- `--force` 成功替换。
- `--force` 导入失败时保留旧词典。
- `--list` 的名称、计数和空数据库行为。
- `--remove` 成功、未知词典和无交互行为。
- 模式冲突、缺少参数、无参数帮助和所有退出码。
- 导入后删除或移动源 JSONL 文件仍可查询。

不测试 MDX/MDD 解析、转换结果的语言学正确性或媒体资源。

## 14. 验证命令

实现完成后至少运行：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

还应使用临时 JSONL 做一次人工端到端验证：

```bash
export XDG_DATA_HOME="$(mktemp -d)"
lexi --import sample.jsonl --name oxford
lexi --list
lexi hello
lexi hello --show-dictionary
lexi --remove oxford
```

## 15. 第一版完成标准

第一版完成时应满足：

1. 新用户可以按 README 安装 Rust 和系统 SQLite 开发库并完成 release 构建。
2. 所有已定义命令、输出、错误和退出码符合本规格。
3. 500 MB 以内 JSONL 可以流式导入，不需要将文件整体载入内存。
4. 导入成功后，源 JSONL 可以删除，查询仍正常工作。
5. 任何失败导入都不会留下部分词典或破坏被 `--force` 替换的旧词典。
6. 精确查询通过 SQLite 索引完成，并正确执行大小写候选筛选。
7. `cargo fmt --check`、Clippy、测试和 release 构建全部通过。
