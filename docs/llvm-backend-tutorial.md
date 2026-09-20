# 一步步替 Sai 加上 LLVM 編譯功能

目前的 Sai 會先讀懂程式的結構，再由直譯器逐步執行。這份教學要帶你增加另一條路：把 Sai 程式交給 LLVM，轉成電腦可以直接執行的機器指令。

你可以先把工作分成兩半：**前端負責讀懂程式，後端負責產生機器指令。** LLVM 能幫我們完成後半段的許多工作，但我們仍要告訴它，Sai 程式到底要做什麼。

學完並完成實作後，你的 Sai 將能：

- 使用直譯器執行，也能用它檢查編譯後的結果是否正確。
- 把通過型別檢查的程式轉成 LLVM IR，也就是 LLVM 使用的中間格式。
- 輸出文字格式的 LLVM IR（`.ll`）、目的檔（`.o`），以及可以直接啟動的執行檔。
- 使用即時編譯（JIT）：程式啟動後才編譯，接著立刻執行。
- 在互動介面（REPL）中記住前幾次輸入的變數與函式。
- 處理數字、布林值、字串、條件判斷、函式與遞迴，也能讓函式記住外層的變數。

## 閱讀前先知道的事

你不需要先學過編譯器。本文會先解釋觀念，再說明如何實作。不過，若要親手完成所有步驟，仍需要基本的 Rust、函式、結構與列舉知識，也要會在終端機執行指令。遇到較長的程式碼，可以先看前後說明，理解它的輸入、輸出和目的。

先記住下面幾個詞，後面會反覆用到：

| 名詞 | 白話說明 |
|---|---|
| 型別 | 資料的種類，例如整數、字串。型別檢查會確認操作是否合理，例如不能把整數當成函式呼叫。 |
| 運算式 | 一段會被求值的程式，例如 `1`、`(+ 1 2)` 或函式呼叫。 |
| AST（抽象語法樹） | 用樹狀結構記錄程式的寫法。例如 `(+ 1 2)` 是一個加法呼叫，下面有兩個數字。 |
| HIR（高階中間表示） | 比 AST 多了檢查結果的資料結構。它會記住變數指的是誰、每個運算式的型別等資訊。 |
| LLVM IR（中間表示） | 交給 LLVM 的指令格式。它比 Sai 更接近機器指令，但還不限定某一款 CPU。 |
| 轉換（lowering） | 把程式從一種表示方式改寫成下一階段需要的格式，例如 HIR 轉成 LLVM IR。 |
| 執行環境（runtime） | 程式執行時提供協助的程式碼，例如管理記憶體、比較字串、回報錯誤。 |
| 指標（pointer） | 記錄資料或函式在記憶體中的位址。可以把它想成地址，而不是地址裡的資料本身；`null` 表示沒有指向有效對象。 |
| 全域與區域變數 | 最外層宣告的變數稱為全域變數；函式或區塊內宣告的變數稱為區域變數。它們能被使用的範圍與保存時間不同。 |
| REPL 工作階段（session） | 從開啟互動介面到關閉為止的這段期間。一次輸入可以包含數個運算式。 |
| 閉包（closure） | 函式連同它記住的外層變數。即使離開原本的區塊，函式仍可能需要這些變數。 |

本文以這個專案現有的程式碼為起點。除了 LLVM，也會先處理 Sai 的語法分析、型別檢查與互動介面需要調整的地方。

> 下文的新檔案、型別與指令介面是實作建議，不代表專案已經具備這些功能。程式碼片段用來說明各個重點，並不是一份貼上後就能完整編譯的成品。請依序完成各階段，並執行每節的驗收步驟。

建議閱讀路線：

| 階段 | 章節 | 完成後能做到什麼 |
|---|---|---|
| 打好前端基礎 | 2 至 5 | 明確定義語言規則，保存檢查結果，出錯時撤銷本次修改 |
| 做出最小可用的後端 | 6 至 11 | 把數字、變數與條件判斷轉成合法的 LLVM IR |
| 加入函式與執行環境 | 12 至 15 | 支援遞迴、字串、閉包，統一傳遞執行結果的方式 |
| 執行編譯後的程式 | 16 至 17 | 產生執行檔，或即時編譯並執行 |
| 檢查成果 | 18 至 22 | 提供清楚的錯誤訊息，比較不同執行方式的結果 |

## 1. 最終成果與範圍

完成後，程式會依照下面的流程前進。直譯器和 LLVM 後端共用前面的檢查步驟，到 HIR 才分開：

```text
原始碼
  -> 詞法分析：切成名稱、數字、括號等小單位
  -> 語法分析：把這些小單位組成 AST
  -> 檢查 AST 的結構
  -> 確認每個名稱指的是誰，並檢查型別
  -> 帶有型別資訊的 HIR
       -> 交給直譯器執行
       或
       -> 轉成 LLVM IR
            -> 檢查 IR 是否合法
            -> 最佳化：讓程式更快或更小，執行結果相同
            -> 即時編譯並執行（JIT）
            或
            -> 目的檔 -> 連結器 -> 執行檔（AOT）
```

本文使用以下版本作為基準：

| 元件 | 基準 |
|---|---|
| 作業系統 | Ubuntu 26.04 x86-64 |
| Rust | 1.88 以上；目前開發環境為 1.95 |
| LLVM | 21.1 |
| Inkwell | 0.9.0 |
| Inkwell 的 LLVM 功能選項 | `llvm21-1-prefer-dynamic` |
| JIT 介面 | Inkwell `ExecutionEngine`，內部使用 LLVM 的 MCJIT |
| 第一版支援的平台 | 本機的 x86-64 Linux |

Inkwell 是讓 Rust 程式操作 LLVM 的函式庫。本文固定使用 LLVM 21.1，因為 Inkwell 0.9 有對應的功能選項。指定版本也能避免安裝來源的 `stable` 等標籤更新後，大家裝到不同版本。

Inkwell 0.9 也支援 LLVM 22。若要升級，需要一起修改 Inkwell 的功能選項、`llvm-sys` 的安裝路徑設定、工具指令，以及自動測試使用的環境映像；只更新系統套件並不夠。

本文的範圍是目前 AST 能表達的功能，尚不包含模組、迴圈、集合、例外處理、呼叫其他語言的介面（FFI）或除錯器。第一版會把閉包需要的記憶體保留到工作階段結束，避免過早釋放；代價是長時間執行時，記憶體用量可能持續增加。

## 2. 先理解目前的 Sai

目前主要的執行步驟位於 `src/cli/mod.rs` 的 `run_source_in_session`。下面這段程式依序把原始碼切開、分析結構、檢查型別，最後執行：

```rust
let mut lexer = Lexer::new(file.to_string(), source);
let tokens = lexer.tokenize();
let mut parser = Parser::new(file, tokens);
let program = parser.parse()?;
type_checker.check_program(&program)?;
interpreter.eval_program(&program)
```

主要資料結構如下：

| 階段 | 檔案 | 目前輸出 |
|---|---|---|
| 詞法分析（Lexer） | `src/parser/lexer.rs` | `Vec<Token>`：切好的名稱、數字、括號等 |
| 語法分析（Parser） | `src/parser/mod.rs` | `ast::Program`：程式的樹狀結構 |
| 結構檢查 | `src/parser/validator.rs` | 檢查發現的問題與位置 |
| 型別檢查 | `src/types/type_checker.rs` | 只有 `Result<(), _>`，表示成功或失敗 |
| 直譯器 | `src/interpreter/mod.rs` | `interpreter::Value`：執行結果 |
| 變數與函式的儲存空間 | `src/interpreter/runtime_env.rs` | `Rc<RefCell<RuntimeEnv>>` |

目前 `ast::Expr` 記錄了程式的寫法，但還沒記下每個名稱對應的變數，也沒有附上型別：

```rust
pub enum Expr {
    Literal(Literal),
    Variable(Variable),
    Defun(Defun),
    Declaration(Declaration),
    Assignment(Assignment),
    Block(Block),
    Funcall(Funcall),
    Closure(Closure),
    IfExpr(IfExpr),
    Or(OrExpr),
    And(AndExpr),
    Not(NotExpr),
    Error,
}
```

要產生 LLVM 指令，我們還缺少這些資訊：

- 每個運算式檢查後得到的型別。
- 每個變數的固定編號（ID）。只記名稱不夠，因為不同區塊可能都有一個叫 `x` 的變數。
- `"+"`、`"="` 等內建操作的確切種類，例如整數加法或浮點數加法。
- 已經解析好的數值，例如整數 `42`，而不是文字 `"42"`。
- 閉包用到了哪些外層變數，以及要複製它們的值，還是共用同一份儲存空間。
- 變數在哪些區塊可見、同名變數是否遮住外層變數，以及變數能否修改。這些資訊目前只存在型別檢查器暫時使用的 `HashMap` 裡。

例如，看到 `(+ x 1)` 時，後端需要知道 `x` 指的是誰，以及它是不是整數。型別檢查器其實已經查過，但沒有把答案留下來。

若後端直接讀這棵 AST，就得再查一次。同一套規則寫在兩個地方，很容易只改到其中一邊，讓直譯器和 JIT 得到不同結果。因此，第 5 節會加入 HIR，把檢查結果保存下來。

## 3. 起步階段：先確定語言規則，修好前端

在產生 LLVM IR 前，先讓前端確實擋下不合法的程式。例如，把整數當成條件，應該得到清楚的錯誤訊息，而不是讓後端執行到 `unreachable!()` 後崩潰。後端收到的輸入應該已經通過這些檢查。

### 3.1 開始前要先修好的問題

`src/types/type_checker.rs` 至少要先修正：

1. `check_closure` 現在回傳的是函式內容的型別。但閉包本身是函式，應回傳 `Type::Function`。
2. 函式內容算出的結果，必須符合宣告的 `return_type`。
3. 閉包參數只能在函式內使用，不能被加進外層的變數表。
4. 先統一「參數能不能修改」的規則。目前型別檢查器允許修改，直譯器卻不允許。
5. `check_if` 必須確認每個條件都是 `Bool`，也就是真或假。
6. 沒有 `:else` 的 `if` 必須是 `Unit`，表示不提供有用的結果。否則條件為假時，就沒有值可回傳。
7. 空區塊應得到 `Unit`，不能讀取空陣列中不存在的元素。
8. 區塊內的每個運算式只檢查一次。目前最後一個會被重複檢查。
9. `len` 與 `nth` 尚未實作，應先從內建函式清單移除，避免呼叫時碰到 `unreachable!()`。
10. 一次輸入只要有任何檢查錯誤，就不能保存其中任何新名稱。

區塊（block）是一組一起執行的運算式。建議採用以下規則：

- 空區塊的型別和值都是 `unit`。
- 非空區塊以最後一個運算式的型別和值作為結果。
- 每個區塊都有自己的作用範圍（scope），也就是變數名稱可被使用的範圍。
- 在區塊內新宣告的變數，離開區塊後就不能用。
- 若區塊修改了外層原本就存在、而且允許修改的變數，修改結果會留下來。

`if` 的規則則是：

- 條件必須是 `bool`。
- 有 `:else` 時，每條分支的結果必須是同一種型別。例如可以都是整數，不能一邊是整數、一邊是字串。
- 沒有 `:else` 時，每條分支及整個 `if` 都必須是 `unit`。

如此一來，需要回傳值的 `if` 不論走哪條路，都會有同型別的結果。第 11 節會介紹 LLVM 如何用 PHI 指令選出實際走到的那條路的值。

### 3.2 先讓文件範例和語法分析器一致

目前詞法分析器只認得 `(` 和 `)` 這兩種括號。`[`、`]` 與 `;;` 都會被當成一般名稱的一部分。因此，README 裡的寫法：

```lisp
(let [int age] 18)
;; comment
```

其實不是目前語法分析器接受的格式。現有的變數宣告寫法應使用圓括號：

```lisp
(let (int age) 18)
```

在擴充詞法分析器前，測試用的 Sai 程式請先使用圓括號，也不要加入 `;;` 註解。若要支援 README 的寫法，需要補上：

- 用 `LBracket` 與 `RBracket` 記錄方括號。
- 在括號組成的樹中，記住原本是哪一種括號。
- 遇到 `;;` 時，略過同一行剩餘的註解文字。
- 左右括號不配對時，顯示錯誤。
- 讓 REPL 分辨「還沒輸入完」與「語法錯誤」，例如左括號後還在等待右括號。

具名函式的分析也需要修正。現在 `parse_defun` 會把 `def` 的參數、回傳型別和函式內容重新包成 `TokenTreeList`，交給 `parse_closure`。但後者以為自己收到完整的 `fn`，仍從 `items[1]` 讀取參數與回傳型別，因此讀錯位置。

比較清楚的做法是抽出共用函式，專門分析「函式簽章」和「函式內容」。簽章（signature）就是參數型別及回傳型別。讓 `parse_defun` 和 `parse_closure` 各自傳入正確欄位，就不必把一種語法假裝成另一種。

還有一項準備工作：雖然已有 `TypeExpr::Function`，`parse_type` 目前卻只接受單一型別名稱。因此，我們還不能宣告「用來保存函式的變數」，或「會回傳另一個函式的函式」。在實作閉包前，先加入函式型別的寫法：

```lisp
(fn-type (int int) int)
```

這表示「接受兩個整數，回傳一個整數」，也可以寫成 `(int, int) -> int`。下面則宣告一個叫 `add-one` 的變數，保存接受一個整數並回傳整數的函式：

```lisp
(let ((fn-type (int) int) add-one)
  (fn ((int value) int)
    (+ value 1)))
```

修改 `parse_type`，讓它遇到以 `fn-type` 開頭的串列時，建立 `TypeExpr::Function { params, ret }`。也要測試語法分析、把結構印回程式碼的功能，以及函式型別內還有函式型別的情況。這樣後面才能寫出「回傳閉包，離開原本函式後再呼叫」的範例。

這些調整不是 LLVM 本身要求的。但範例必須先能被 Sai 讀懂，後面的編譯與執行測試才有意義。

### 3.3 數字超出範圍或除以零時，該怎麼辦？

`i64` 只能保存固定範圍的整數。如果結果太大或太小，就會「溢位」。LLVM 和 Rust 對這件事的預設處理不一定相同：

- LLVM 的 `add`、`sub`、`mul` 若沒加上 `nsw` 旗標，會只保留結果的最低 64 個位元。超出的部分會被捨去。
- Rust 的除錯版本遇到帶正負號的整數溢位，通常會中止程式；正式版本通常會繞回可表示的範圍。
- LLVM 的帶正負號整數除法 `sdiv`，不能直接拿來計算除以零，也不能計算 `i64::MIN / -1`。後者的正數結果超出 `i64` 上限。

為了讓所有執行方式得到一致的結果，第一版建議明確採用以下規則：

- `+`、`-`、`*` 超出範圍時，保留最低 64 位元，按二補數解讀正負號。這稱為繞回運算（wrapping）；例如最大整數加一會變成最小整數。
- 整數除以零時，回報 Sai 執行錯誤。
- `i64::MIN / -1` 回報整數溢位的執行錯誤。
- 浮點數遵循 IEEE-754 標準，也就是常見的電腦小數運算規則。
- NaN 是浮點數中表示「不是有效數字」的特殊值。只要比較涉及 NaN，`=`、`<`、`<=`、`>`、`>=` 都回傳 `false`；LLVM 把這類比較稱為 ordered comparison。

接著修改直譯器，改用 `wrapping_add`、`wrapping_sub`、`wrapping_mul`，並在除法前檢查上述兩種錯誤。Sai 的計算規則不應隨 Cargo 的編譯模式改變。

### 3.4 用測試把規則記下來

目前專案還沒有自行撰寫的測試。加入 LLVM 前，先建立測試來記錄現有行為及修正後應遵守的規則：

```text
tests/
├── parser.rs
├── type_checker.rs
├── interpreter.rs
└── session.rs
```

至少測試算術、型別限制、空區塊、條件判斷、閉包參數、變數修改，以及失敗輸入的復原。下面是建議的測試函式名稱；英文名稱可直接用在 Rust 測試中：

```text
integer_and_float_arithmetic
comparison_requires_matching_numeric_types
unit_equality_is_true
empty_block_is_unit
if_condition_must_be_bool
if_branches_must_match
if_without_else_is_unit
closure_has_function_type
closure_body_matches_declared_return_type
closure_parameter_does_not_escape
immutable_binding_rejects_assignment
mutable_binding_accepts_assignment
failed_submission_does_not_commit_bindings
short_circuit_does_not_evaluate_rhs
recursive_function_works
nested_recursive_function_reuses_its_environment
```

本節驗收指令：

```bash
cargo test --locked
cargo fmt --all -- --check
```

Clippy 是 Rust 的程式碼檢查工具，可能會找到專案原本就有的警告。必要的功能修正與單純的整理最好分開提交，讓每次修改容易檢查。

## 4. 讓前端可以被不同執行方式共用

CLI 是在終端機使用的命令列介面。目前專案只有 `src/main.rs` 這個執行檔入口，`run_source_in_session` 也直接需要 `TypeChecker` 和 `Interpreter`。之後的提前編譯（AOT）、即時編譯（JIT）與整合測試，都需要使用前端，因此應把可共用的功能放進函式庫。

先新增 `src/lib.rs`，把 CLI 也放在這個函式庫中：

```rust
pub mod diagnostics;
pub mod frontend;
pub mod hir;
pub mod interpreter;
pub mod cli;

mod ast;
mod parser;
mod types;
```

在 `src/cli/mod.rs` 實作 `pub fn run(...) -> ExitCode`。接著讓 `src/main.rs` 只負責啟動它：

```rust
use std::process::ExitCode;

fn main() -> ExitCode {
    sai::cli::run(std::env::args_os())
}
```

不要又在 `src/main.rs` 宣告 `mod cli;`。那會把同一份 CLI 程式碼當成執行檔自己的模組，而其中的 `crate::diagnostics`、`crate::interpreter` 等路徑就無法找到已搬到函式庫的模組。

前端先提供一個簡單介面：輸入檔名和原始碼，成功時回傳 HIR，失敗時回傳錯誤訊息清單。

```rust
pub struct CompileOutput {
    pub module: hir::Module,
}

pub fn compile_source(
    file: &str,
    source: &str,
) -> Result<CompileOutput, Vec<Diagnostic>>;
```

REPL 還需要「全部成功才保存」的機制。這稱為交易（transaction）：先在暫存版本上嘗試，成功才正式套用，失敗就捨棄。例如，一次輸入先宣告 `x`，後面卻除以零，這次新建的 `x` 就不應留下來。

下面的 `world` 保存已知的名稱和型別。`prepare` 準備暫存結果，`commit` 才正式保存：

```rust
pub struct FrontendSession {
    world: SemanticWorld,
}

pub struct PendingSubmission {
    module: hir::Module,
    next_world: SemanticWorld,
}

impl FrontendSession {
    pub fn prepare(
        &self,
        file: &str,
        source: &str,
    ) -> Result<PendingSubmission, Vec<Diagnostic>>;

    pub fn commit(&mut self, pending: PendingSubmission) -> hir::Module;
}
```

流程必須是：

```text
複製目前的名稱與型別資料，建立暫存版本
  -> 詞法與語法分析
  -> 結構檢查
  -> 確認名稱指向的對象，並檢查型別
  -> 檢查 HIR
  -> 成功：回傳 PendingSubmission，等待執行
  -> 失敗：丟棄暫存版本

呼叫端執行程式，或先用 JIT 編譯再執行
  -> 成功：一起保存前端資料和執行時的變數值
  -> 失敗：撤銷本次修改
```

這能避免目前的一種問題：型別檢查器已經記住新名稱，但同次輸入後面出錯，直譯器根本沒有執行。結果是前端以為變數存在，執行時卻找不到它的值。

直譯器也要能撤銷修改，才能和之後的 C 執行環境遵守相同規則。改成執行 HIR 後，建議用編號來管理資料，取代原本以名稱和 `Rc<RefCell<RuntimeEnv>>` 管理所有狀態的方式。

下面的 `CellId` 是共用儲存格的編號。多個閉包若共用同一個可修改變數，就保存相同的 `CellId`：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CellId(u32);

#[derive(Clone)]
struct RuntimeStore {
    globals: HashMap<hir::GlobalId, InterpretedValue>,
    cells: Vec<InterpretedValue>,
}

#[derive(Clone)]
struct InterpretedClosure {
    function: hir::FunctionId,
    captures: Vec<InterpretedCapture>,
}

#[derive(Clone)]
enum InterpretedCapture {
    Value(InterpretedValue),
    Cell(CellId),
}
```

每次輸入先複製一份 `RuntimeStore`，在副本上執行。成功才用副本取代正式資料，失敗就丟掉副本。

這裡用編號而不是直接複製每個閉包裡的變數值，有一個重要理由：假設兩個閉包原本共用儲存格 3，複製後它們仍會共用「副本裡的儲存格 3」，不會變成各自修改一份資料。

資料量變大後，可以改成記錄每次寫入前的舊值，出錯時再還原。無論採用哪一種做法，前端記住的名稱、型別和直譯器保存的值，都必須一起正式保存。如此才能公平比較直譯器與 JIT 的行為。

## 5. 建立帶有型別資訊的 HIR

### 5.1 HIR 的責任

HIR 的工作是把「已經查好的答案」保存下來。產生 HIR 後，後端不必再猜變數是誰、加法該用哪種指令。它應符合以下條件：

- 沒有語法分析失敗時暫時使用的 `Expr::Error`。
- 程式直接寫出的值已轉成 `i64`、`f64`、`bool` 或自行保存的 UTF-8 字串資料。
- 每個運算式都有確定的型別。
- 每個名稱都已換成對應的編號。
- 要修改的變數確實存在，而且允許修改。
- 函式呼叫的參數數量與型別都正確。
- 內建操作已換成明確種類，例如 `AddInt` 表示整數加法。
- `if` 的條件一定是 `bool`。
- 需要產生值的各條分支，結果型別完全相同。
- 函式內容的型別符合宣告的回傳型別。
- 閉包需要保存哪些外層變數，已按固定順序列出。

LLVM 後端和直譯器都只讀取 HIR。共用這份已檢查的資料，能減少兩種執行方式對語言規則有不同理解的機會。

### 5.2 建議的 ID 與型別

建立 `src/hir/ids.rs`：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CaptureId(pub u32);
```

每種編號都有自己的型別。例如 `LocalId(3)` 和 `FunctionId(3)` 雖然都是 3，代表的東西卻不同。若全部都用 `usize`，Rust 就無法幫你發現把變數編號誤當函式編號的錯誤。

建立 `src/hir/types.rs`：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    Unit,
    Function(Box<FunctionType>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionType {
    pub params: Vec<Type>,
    pub return_type: Box<Type>,
}
```

成功的 HIR 不應包含 `Type::Error`。這種錯誤型別只供型別檢查器暫時使用，讓它在發現一個問題後，仍能繼續找出其他問題。

### 5.3 記錄運算式、變數位置與內建操作

`Expr` 表示運算式，`Place` 表示要去哪裡讀取資料，`Intrinsic` 表示已確定型別的內建操作。`position` 則保留原始碼位置，方便出錯時指出是哪一行。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Place {
    Global(GlobalId),
    Local(LocalId),
    Capture(CaptureId),
    SelfClosure(FunctionId),
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
    pub position: Position,
}

#[derive(Debug, Clone)]
pub enum Constant {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intrinsic {
    AddInt,
    AddFloat,
    SubInt,
    SubFloat,
    MulInt,
    MulFloat,
    DivInt,
    DivFloat,
    EqInt,
    EqFloat,
    EqBool,
    EqString,
    EqUnit,
    LtInt,
    LtFloat,
    LeInt,
    LeFloat,
    GtInt,
    GtFloat,
    GeInt,
    GeFloat,
}
```

例如 `(+ 1 2)` 經過型別檢查後，應明確記成 `Intrinsic::AddInt`，而不是只留下字串 `"+"`。後端看到 `AddInt`，就知道要產生整數加法。

接著記錄函式要如何呼叫，以及各種運算式的內容。`Direct` 表示直接呼叫已知函式；`Indirect` 表示先算出一個函式值，再呼叫它。遞迴與閉包需要的細節會在後面說明。

```rust
#[derive(Debug, Clone)]
pub enum CallTarget {
    Direct(FunctionId),
    CurrentFunction(FunctionId),
    Indirect(Box<Expr>),
    Intrinsic(Intrinsic),
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Constant(Constant),
    Load(Place),
    FunctionRef(FunctionId),
    MakeClosure {
        function: FunctionId,
        captures: Vec<CaptureOperand>,
    },
    Call {
        target: CallTarget,
        args: Vec<Expr>,
    },
    If {
        condition: Box<Expr>,
        then_block: Block,
        else_block: Block,
    },
    LogicalAnd(Vec<Expr>),
    LogicalOr(Vec<Expr>),
    LogicalNot(Box<Expr>),
    Block(Block),
}
```

把 AST 轉成 HIR 時，可以順便把 `:elif` 改寫成「`else` 裡面再放一個 `if`」。這叫作正規化：把不同寫法整理成同一種形式。後端就只需處理有兩條分支的 `If`。

### 5.4 區塊裡的步驟與最後結果

每個 HIR 區塊分成兩部分：`statements` 保存依序執行的步驟，`tail` 保存決定整個區塊結果的最後一個運算式。

```rust
#[derive(Debug, Clone)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub tail: Box<Expr>,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub enum Statement {
    Let {
        local: LocalId,
        mutable: bool,
        init: Expr,
    },
    InitGlobal {
        global: GlobalId,
        init: Expr,
    },
    Assign {
        target: Place,
        value: Expr,
    },
    Expr(Expr),
}
```

若原始區塊是空的，轉換時就補上 `Constant::Unit` 作為 `tail`。如此一來，每個區塊都有最後結果，直譯器和後端都不用再處理「陣列裡沒有最後一項」的特殊情況。

### 5.5 函式，以及它需要記住的外層變數

`Module` 是一份編譯單位，記錄全域變數、函式和啟動時要執行的程式。`Function` 則記錄參數、區域變數、函式內容，以及捕捉清單 `captures`。「捕捉」就是讓函式記住它使用的外層變數。

```rust
#[derive(Debug, Clone)]
pub struct Module {
    pub globals: Vec<Global>,
    pub functions: Vec<Function>,
    pub init: Block,
}

#[derive(Debug, Clone)]
pub struct Global {
    pub id: GlobalId,
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub id: FunctionId,
    pub source_name: Option<String>,
    pub signature: FunctionType,
    pub params: Vec<Parameter>,
    pub locals: Vec<Local>,
    pub captures: Vec<Capture>,
    pub body: Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    ByValue,
    ByRef,
}

#[derive(Debug, Clone)]
pub struct Capture {
    pub id: CaptureId,
    pub source: Place,
    pub ty: Type,
    pub mode: CaptureMode,
}

#[derive(Debug, Clone)]
pub struct CaptureOperand {
    pub source: Place,
    pub mode: CaptureMode,
}
```

規則如下：

- 不能修改的變數可以複製值，記為 `ByValue`。
- 可以修改的變數必須共用儲存格，記為 `ByRef`。外層和所有閉包都讀寫同一份資料。
- 捕捉清單依名稱解析階段分配的編號排序，讓每次編譯的欄位順序一致。
- 若最內層函式用到更外層的變數，中間的函式也要能把這份資料轉交下去。

**函式呼叫分成三種情況：**

1. 已確認不需要捕捉變數的函式，可以用 `CallTarget::Direct`。額外傳入的環境指標設為 `null`，表示沒有捕捉資料。
2. 巢狀具名函式呼叫自己時，用 `CallTarget::CurrentFunction`。呼叫同一個 LLVM 函式，並把目前的環境指標繼續傳下去，才不會忘記捕捉的變數。
3. 其他函式值使用 `Indirect`，從閉包中取出函式位置與捕捉資料後再呼叫。

HIR 檢查器應拒絕把需要捕捉變數的函式標成普通 `Direct`。

**函式也可能把「自己」當成值。** `Place::SelfClosure(function_id)` 就是為此準備的。後端把目前函式的位址和 `current_environment` 組成 `%SaiClosure`；最外層或不需捕捉變數的函式，環境指標是 `null`。

這讓函式可以把自己當成參數、回傳自己，或讓內層閉包保存自己。內層閉包建立時，會把 `SelfClosure(parent_id)` 複製到自己的捕捉資料中，之後再用間接呼叫執行。HIR 檢查器必須確認：只有在對應函式的內容中，才能建立這種「自己」的值；若要傳到內層，必須透過捕捉清單。

**最外層的宣告需要長期保存。** 最外層的 `let`、`mut` 和 `def` 不能當成一般區域變數，因為後續的 REPL 輸入還要使用它們。`Module.globals` 記錄它們的編號與型別，`Module.init` 裡的 `InitGlobal` 則按原始碼順序計算初始值。

這些值放在執行環境管理的固定儲存位置（slot）。函式使用額外傳入的執行環境指標和 `GlobalId` 找到它們，不直接依賴某份可能被釋放的 JIT 模組。AOT 和 JIT 都採用同一套規則。

### 5.6 檢查 HIR 本身是否正確

建立：

```rust
pub fn validate(module: &Module) -> Result<(), HirInvariantError>;
```

至少檢查：

- 每個編號都能在對應的資料表中找到。
- `Expr.ty` 記錄的型別符合 `ExprKind` 的內容。
- 內建操作的輸入與結果型別正確。
- 直接和間接呼叫的參數、回傳型別正確。
- 被賦予新值的變數允許修改。
- `if` 的條件是 `Bool`。
- 分支型別符合整個運算式的型別。
- 函式內容符合回傳型別。
- 捕捉變數的編號、型別及保存方式正確。

在除錯版本中，直譯器和 LLVM 後端開始工作前都先呼叫這個檢查器。若它發現問題，代表編譯器產生了錯誤的 HIR，應回報編譯器本身的錯誤。

本節驗收：確認名稱已換成編號、整數加法已標成 `AddInt`、空區塊有 `unit` 結果、條件判斷已整理成兩條分支，而且捕捉方式正確。建議測試名稱如下：

```text
variable_reference_is_resolved_to_id
integer_add_becomes_add_int
empty_block_has_unit_tail
if_is_normalized_to_two_branches
recursive_call_uses_function_id
mutable_capture_is_by_ref
valid_hir_passes_validator
```

## 6. 安裝 LLVM 21 與 Inkwell

### 6.1 Ubuntu 26.04

使用 apt.llvm.org 提供的官方安裝腳本。下面先安裝下載與建置工具，再安裝 LLVM 21：

```bash
sudo apt update
sudo apt install -y \
  wget ca-certificates gnupg build-essential \
  lsb-release software-properties-common

wget https://apt.llvm.org/llvm.sh
chmod +x llvm.sh
sudo ./llvm.sh 21

sudo apt install -y \
  llvm-21 \
  llvm-21-dev \
  llvm-21-runtime \
  clang-21 \
  lld-21
```

Clang 是 C/C++ 編譯器，LLVM 則提供背後的編譯工具。這裡也需要 `llvm-21-dev`，因為它提供 `llvm-config-21`、C 介面的標頭檔，以及連結所需的資訊。只裝 Clang，Rust 的 `llvm-sys` 套件仍無法完成編譯。

確認版本與路徑：

```bash
llvm-config-21 --version
llvm-config-21 --prefix
llvm-config-21 --libdir
llvm-config-21 --shared-mode
clang-21 --version
ld.lld-21 --version
```

預期 LLVM 版本是 `21.1.x`。如果系統同時存在多個 LLVM：

```bash
export LLVM_SYS_211_PREFIX=/usr/lib/llvm-21
export PATH="/usr/lib/llvm-21/bin:$PATH"
```

環境變數中的 `211` 是 `llvm-sys` 對 LLVM 21.1 的編碼。

### 6.2 在 Cargo 加入 Inkwell

修改 `Cargo.toml`：

```toml
[dependencies]
rustyline = "18.0.0"
inkwell = {
    version = "=0.9.0",
    default-features = false,
    features = ["llvm21-1-prefer-dynamic", "target-x86"]
}
```

Cargo 的 feature 是選擇套件功能的開關。這裡的 `llvm21-1-prefer-dynamic` 選擇 LLVM 21.1，並優先使用動態函式庫；`target-x86` 則啟用 x86 平台支援。LLVM 版本只能選一個，不要同時啟用 `llvm20-1`、`llvm21-1` 與 `llvm22-1`。

若希望沒裝 LLVM 的使用者仍能編譯直譯器，可以改用下面的可選設定。只有加上 `--features llvm` 時，才會編譯 LLVM 後端：

```toml
[features]
default = []
llvm = ["dep:inkwell"]

[dependencies.inkwell]
version = "=0.9.0"
optional = true
default-features = false
features = ["llvm21-1-prefer-dynamic", "target-x86"]
```

對應的 Rust 模組也加上條件：

```rust
#[cfg(feature = "llvm")]
pub mod llvm;
```

檢查套件是否能編譯，以及實際啟用了哪些功能：

```bash
LLVM_SYS_211_PREFIX=/usr/lib/llvm-21 cargo check --features llvm
cargo tree -e features -i inkwell
cargo tree -i llvm-sys
```

常見錯誤：

- `No suitable version of LLVM was found`：確認 `LLVM_SYS_211_PREFIX` 和 `llvm-config-21`。
- 連結器找不到 `libLLVM.so`：用 `llvm-config-21 --libdir` 確認函式庫位置，並優先使用 `prefer-dynamic`。
- Cargo 同時出現兩個主要版本的 `llvm-sys`：檢查是否誤啟用了兩個 LLVM 版本選項。
- 使用 `no-llvm-linking` 後出現大量「找不到函式或符號」的錯誤：一般 Sai 建置不應使用這個選項，因為它會略過 LLVM 的連結。

## 7. 建立後端的基本架構

這一節先安排檔案與資料結構。`codegen` 是 code generation 的縮寫，意思是「產生程式碼」。型別轉換、執行環境呼叫、AOT 和 JIT 分開放，之後比較容易找到要修改的位置。

建議檔案：

```text
src/backend/
├── mod.rs
└── llvm/
    ├── mod.rs
    ├── codegen.rs
    ├── types.rs
    ├── runtime.rs
    ├── aot.rs
    └── jit.rs
```

後端的輸入是已經通過檢查的 HIR。它不應直接讀取 AST：

```rust
pub fn compile_module(
    context: &Context,
    hir: &hir::Module,
    options: &CodegenOptions,
) -> Result<CompiledModule, BackendError>;
```

`CodeGenerator` 保存產生 LLVM IR 時需要的工具和對照表：

- `context` 管理 LLVM 的型別與相關資料。
- `module` 裝著這次產生的函式與常數。
- `builder` 負責把新指令加入目前的位置。
- `functions`、`locals`、`captures` 等表格，把 HIR 編號對應到 LLVM 的值或儲存位置。

`Storage` 區分三種保存方式：已算好的值、函式執行期間的堆疊空間，以及可共用的儲存格。

```rust
use std::collections::HashMap;

use inkwell::{
    builder::Builder,
    context::Context,
    module::Module,
    values::{BasicValueEnum, FunctionValue, PointerValue},
};

pub struct CodeGenerator<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    functions: HashMap<hir::FunctionId, FunctionValue<'ctx>>,
    globals: HashMap<hir::GlobalId, GlobalLayout<'ctx>>,
    locals: HashMap<hir::LocalId, Storage<'ctx>>,
    captures: HashMap<hir::CaptureId, Storage<'ctx>>,
    current_function: Option<FunctionValue<'ctx>>,
    current_environment: Option<PointerValue<'ctx>>,
    next_string_id: u64,
    types: RuntimeTypes<'ctx>,
}

#[derive(Clone, Copy)]
struct GlobalLayout<'ctx> {
    ty: BasicTypeEnum<'ctx>,
    mutable: bool,
}

#[derive(Clone, Copy)]
enum Storage<'ctx> {
    Value(BasicValueEnum<'ctx>),
    Stack(PointerValue<'ctx>),
    Cell(PointerValue<'ctx>),
}

enum GeneratedValue<'ctx> {
    Value(BasicValueEnum<'ctx>),
    Unit,
}
```

`GeneratedValue::Unit` 用來表示「這個運算式沒有提供一般的資料值」，不能省略。LLVM 的 `void` 可以表示函式不回傳資料，卻不能像整數一樣放進 `BasicValueEnum`。

也不要用布林值 `i1 0` 假裝 `unit`，因為「假」和「沒有資料值」是不同的概念。後面處理函式回傳與分支結果時，必須一直維持這個區別。

下面的輔助函式用來確認：目前的操作是否真的拿到了一個可使用的值。

```rust
impl<'ctx> GeneratedValue<'ctx> {
    fn require_value(
        self,
        operation: &'static str,
    ) -> Result<BasicValueEnum<'ctx>, BackendError> {
        match self {
            Self::Value(value) => Ok(value),
            Self::Unit => Err(BackendError::ExpectedValue(operation)),
        }
    }
}
```

後端可能在不同階段失敗。`BackendError` 至少要區分 HIR 問題、建立指令失敗、IR 檢查失敗、目標平台設定、檔案操作及連結錯誤：

```rust
pub enum BackendError {
    InvalidHir(String),
    Builder(String),
    Verification(String),
    Target(String),
    Llvm(String),
    Io(std::io::Error),
    LinkerFailure {
        command: String,
        status: Option<i32>,
        stderr: String,
    },
    ExpectedValue(&'static str),
}
```

Inkwell 0.9 的 `builder.build_*` 方法幾乎都回傳 `Result`，因為建立指令也可能失敗。把 `BuilderError` 轉成 `BackendError` 後往上回報，比大量使用 `unwrap()` 直接中止程式更容易除錯。

## 8. 資料如何表示，以及函式如何傳遞資料

假設 Sai 的函式回傳字串，呼叫它的 Rust 或 C 程式必須知道：收到的是字串內容，還是內容的位址？長度放在哪裡？

這些共同遵守的規則叫作 ABI（應用程式二進位介面）。你可以把它想成資料交接的格式約定：雙方都用相同的欄位順序與大小，才讀得懂對方交來的資料。

### 8.1 把 Sai 型別換成 LLVM 型別

`i64` 是 64 位元整數，`i1` 只有一個位元，`double` 是 64 位元浮點數，`ptr` 則是記憶體位址，也叫指標。大括號表示把數個欄位放在同一個結構中。

| Sai HIR 型別 | LLVM 表示方式 |
|---|---|
| `Int` | `i64` |
| `Float` | `double` |
| `Bool` | LLVM 內部使用 `i1` |
| `String` | `%SaiString = { ptr, i64 }` |
| `Unit` | 函式回傳用 `void`，後端內部用 `GeneratedValue::Unit` |
| 函式值 | `%SaiClosure = { ptr, ptr }` |

LLVM 21 的指標只記為 `ptr`，本身不標示指向的資料型別，這稱為不透明指標（opaque pointer）。建立指標型別時使用：

```rust
context.ptr_type(AddressSpace::default())
```

舊版教學常用 `context.i8_type().ptr_type(...)` 建立指標型別；本文使用 LLVM 21，請改用上面的寫法。

### 8.2 建立共用的字串與閉包型別

所有函式應使用同一組 `SaiString` 和 `SaiClosure` 型別。先建立一次，再把它們保存下來供後續使用：

```rust
#[derive(Clone, Copy)]
struct RuntimeTypes<'ctx> {
    string: StructType<'ctx>,
    closure: StructType<'ctx>,
}

impl<'ctx> RuntimeTypes<'ctx> {
    fn new(context: &'ctx Context) -> Self {
        let string = context.opaque_struct_type("SaiString");
        string.set_body(
            &[
                context.ptr_type(AddressSpace::default()).into(),
                context.i64_type().into(),
            ],
            false,
        );

        let closure = context.opaque_struct_type("SaiClosure");
        let ptr = context.ptr_type(AddressSpace::default());
        closure.set_body(&[ptr.into(), ptr.into()], false);

        Self { string, closure }
    }
}
```

每個 LLVM `Context` 只呼叫一次 `RuntimeTypes::new`，再把這組型別交給同一個 `Context` 下的各個 `CodeGenerator`。

這是因為再次呼叫 `opaque_struct_type("SaiString")`，可能建立的是另一個型別，例如 `%SaiString.0`。即使欄位看起來一樣，LLVM 仍可能把它們視為不同型別，導致函式參數、字串常數與執行環境介面無法配合。

### 8.3 寫出型別轉換函式

下面的函式把每種 HIR 型別換成剛才選定的 LLVM 型別。`Unit` 要另外處理，因為它不是一般的 LLVM 資料值。

```rust
fn lower_basic_type<'ctx>(
    &self,
    ty: &hir::Type,
) -> Result<BasicTypeEnum<'ctx>, BackendError> {
    match ty {
        hir::Type::Int => Ok(self.context.i64_type().into()),
        hir::Type::Float => Ok(self.context.f64_type().into()),
        hir::Type::Bool => Ok(self.context.bool_type().into()),
        hir::Type::String => Ok(self.types.string.into()),
        hir::Type::Function(_) => Ok(self.types.closure.into()),
        hir::Type::Unit => Err(BackendError::InvalidHir(
            "unit does not have a BasicTypeEnum".into(),
        )),
    }
}
```

後面的片段有時簡寫成 `lower_basic_type(...)`。整合時請統一呼叫上面這個方法，使用已保存的 `RuntimeTypes`。

每個編譯後的 Sai 函式，都額外接受兩個由編譯器加入的參數。使用者不必在 Sai 原始碼中填寫它們：

- `runtime`：找到執行環境，例如全域變數和記憶體配置工具。
- `environment`：找到閉包保存的外層變數。

```text
(runtime: ptr, environment: ptr, user parameters...) -> Sai result
```

函式不需要捕捉變數時，仍保留 `environment` 這個參數，只是傳入 `null`。這樣具名函式和閉包就能使用相同的呼叫方式，也都能以 `%SaiClosure` 表示。

```rust
fn lower_function_type(
    &self,
    signature: &hir::FunctionType,
) -> Result<FunctionType<'ctx>, BackendError> {
    let ptr = self.context.ptr_type(AddressSpace::default());
    let mut params = vec![ptr.into(), ptr.into()];

    for param in &signature.params {
        params.push(self.lower_basic_type(param)?.into());
    }

    match signature.return_type.as_ref() {
        hir::Type::Unit => Ok(self.context.void_type().fn_type(&params, false)),
        ty => Ok(self.lower_basic_type(ty)?.fn_type(&params, false)),
    }
}
```

第一版可以先不允許 `unit` 型別的參數與變數。若之後要支援，前端仍可記錄它們，但轉成 LLVM 函式參數時要略過 `unit`，不能把 `void` 塞進參數列表。

## 9. 第一個編譯成果：數字與算術

先讓最簡單的 `(+ 40 2)` 成功。這一階段處理整數、浮點數、布林值與 `unit`；這類單一的基本值也稱為純量。

### 9.1 把常數交給 LLVM

常數是原始碼直接寫出的值，例如 `42` 或 `true`。HIR 已把它們解析好，下面只需建立對應的 LLVM 值：

```rust
fn codegen_constant<'ctx>(
    generator: &mut CodeGenerator<'ctx>,
    constant: &hir::Constant,
) -> Result<GeneratedValue<'ctx>, BackendError> {
    let value = match constant {
        hir::Constant::Int(value) => generator
            .context
            .i64_type()
            .const_int(*value as u64, true)
            .into(),
        hir::Constant::Float(value) => generator
            .context
            .f64_type()
            .const_float(*value)
            .into(),
        hir::Constant::Bool(value) => generator
            .context
            .bool_type()
            .const_int(u64::from(*value), false)
            .into(),
        hir::Constant::String(value) => {
            return generator.codegen_string_literal(value);
        }
        hir::Constant::Unit => return Ok(GeneratedValue::Unit),
    };

    Ok(GeneratedValue::Value(value))
}
```

HIR 已經把文字轉成 `i64` 或 `f64`，後端不必再次呼叫 `parse()`。

### 9.2 產生算術指令

算術操作可能有兩個以上的參數。先依原始碼順序求出各參數的值，再從左到右計算。例如 `(+ 1 2 3)` 先得到 `1 + 2`，再加上 `3`。

下面先特別處理 `unit` 的相等比較：仍要執行每個參數，保留它可能造成的變數修改；確認都是 `unit` 後，結果才是 `true`。其他操作則取出實際值，再產生對應指令。

```rust
fn codegen_intrinsic(
    &mut self,
    intrinsic: hir::Intrinsic,
    args: &[hir::Expr],
) -> Result<GeneratedValue<'ctx>, BackendError> {
    if intrinsic == hir::Intrinsic::EqUnit {
        for arg in args {
            let value = self.codegen_expr(arg)?;
            if !matches!(value, GeneratedValue::Unit) {
                return Err(BackendError::InvalidHir(
                    "EqUnit received a value-producing argument".into(),
                ));
            }
        }
        return Ok(GeneratedValue::Value(
            self.context.bool_type().const_int(1, false).into(),
        ));
    }

    let values = args
        .iter()
        .map(|arg| self.codegen_expr(arg)?.require_value("intrinsic"))
        .collect::<Result<Vec<_>, _>>()?;

    match intrinsic {
        hir::Intrinsic::AddInt => {
            let mut acc = values[0].into_int_value();
            for value in &values[1..] {
                acc = self.builder
                    .build_int_add(acc, value.into_int_value(), "add")?;
            }
            Ok(GeneratedValue::Value(acc.into()))
        }
        hir::Intrinsic::AddFloat => {
            let mut acc = values[0].into_float_value();
            for value in &values[1..] {
                acc = self.builder
                    .build_float_add(acc, value.into_float_value(), "fadd")?;
            }
            Ok(GeneratedValue::Value(acc.into()))
        }
        // 減法、乘法、除法與比較，可依相同結構實作。
        _ => Err(BackendError::InvalidHir(
            "intrinsic lowering is not implemented".into(),
        )),
    }
}
```

要使用上面的 `?` 傳遞錯誤，必須替 `BackendError` 實作 `From<inkwell::builder::BuilderError>`，或用 `map_err` 明確轉換錯誤型別。

整數除法不能直接產生 `sdiv`。先加入檢查，`lhs` 是被除數，`rhs` 是除數：

```text
rhs == 0                    -> 執行錯誤處理區塊
lhs == i64::MIN && rhs == -1 -> 執行錯誤處理區塊
其他情況                    -> 執行 sdiv
```

發現錯誤時，改走錯誤處理區塊，呼叫 `sai_rt_raise(runtime, error_code, span_id)`。這個函式會跳到外層的錯誤處理程序，不會回來執行下一行，所以後面可以放 LLVM 的 `unreachable`，表示執行不會繼續走到這裡。

第 13 與 15 節會說明如何用 C 的 `setjmp` 和 `longjmp` 把錯誤送回啟動 Sai 程式的呼叫端。此刻先記住：錯誤的除法不能進入 `sdiv`；會正常返回的函式後面，也不能隨意加上 `unreachable`，否則 LLVM 可能根據錯誤的假設改寫程式。

### 9.3 比較兩個值

比較指令還需要指定比較規則。下面的 `EQ` 是相等，`SLT` 是帶正負號整數的小於；`OEQ` 和 `OLT` 是浮點數的相等與小於，遇到 NaN 會得到 `false`。

```rust
use inkwell::{FloatPredicate, IntPredicate};

let result = match intrinsic {
    hir::Intrinsic::EqInt => self.builder.build_int_compare(
        IntPredicate::EQ,
        lhs.into_int_value(),
        rhs.into_int_value(),
        "eq",
    )?,
    hir::Intrinsic::LtInt => self.builder.build_int_compare(
        IntPredicate::SLT,
        lhs.into_int_value(),
        rhs.into_int_value(),
        "lt",
    )?,
    hir::Intrinsic::EqFloat => self.builder.build_float_compare(
        FloatPredicate::OEQ,
        lhs.into_float_value(),
        rhs.into_float_value(),
        "feq",
    )?,
    hir::Intrinsic::LtFloat => self.builder.build_float_compare(
        FloatPredicate::OLT,
        lhs.into_float_value(),
        rhs.into_float_value(),
        "flt",
    )?,
    _ => unreachable!("HIR validator restricts comparison intrinsics"),
};
```

這裡的 `unreachable!()` 只用於「HIR 檢查器已保證不會出現」的分支。如果真的走到這裡，代表編譯器有錯，而不是使用者寫錯型別。

### 9.4 驗收：產生第一份 LLVM IR

輸入：

```lisp
(+ 40 2)
```

產生的關鍵 IR 應接近下面的內容。`define` 定義函式，`entry` 是起始區塊，`add i64` 計算整數加法，`ret` 回傳結果。LLVM 也可能直接把常數加法算成 `42`。

```llvm
define i64 @__sai_scalar_entry(ptr %runtime, ptr %env) {
entry:
  %add = add i64 40, 2
  ret i64 %add
}
```

每次產生模組後，立刻請 LLVM 檢查指令是否合法：

```rust
generator
    .module
    .verify()
    .map_err(|error| BackendError::Verification(error.to_string()))?;
```

也把文字版 IR 寫入檔案，方便查看。執行前先準備好 `build` 資料夾：

```rust
generator.module.print_to_file("build/checkpoint.ll")?;
```

還可以用命令列工具再驗證一次。`llvm-as-21` 把文字 IR 轉成二進位的 `.bc` 格式，`opt-21` 則執行檢查：

```bash
llvm-as-21 build/checkpoint.ll -o build/checkpoint.bc
opt-21 -passes=verify build/checkpoint.bc -disable-output
```

## 10. 保存變數、執行區塊與修改變數

變數需要有地方放值。第一版先把沒有被閉包捕捉的區域變數放在堆疊（stack）上：這是函式執行期間使用的暫存記憶體，函式結束後就不能再依賴它。

LLVM 的 `alloca` 指令會保留這樣的空間。先把它們集中放在函式的起始區塊，之後 LLVM 比較容易把不必要的記憶體讀寫消掉，改用直接傳遞的值。這些值採用 SSA 形式，也就是每個結果名稱只被賦值一次。

### 10.1 在函式起點保留變數空間

下面另外建立一個 `alloca_builder`，專門把配置指令放在起始區塊前方，不影響原本產生其他指令的位置。

```rust
fn create_entry_alloca<'ctx>(
    &self,
    function: FunctionValue<'ctx>,
    ty: BasicTypeEnum<'ctx>,
    name: &str,
) -> Result<PointerValue<'ctx>, BackendError> {
    let alloca_builder = self.context.create_builder();
    let entry = function
        .get_first_basic_block()
        .ok_or_else(|| BackendError::InvalidHir("missing entry block".into()))?;

    if let Some(first) = entry.get_first_instruction() {
        alloca_builder.position_before(&first);
    } else {
        alloca_builder.position_at_end(entry);
    }

    Ok(alloca_builder.build_alloca(ty, name)?)
}
```

不要在產生程式碼的任意位置插入區域變數的 `alloca`。集中放在起始區塊，LLVM 的 SROA、mem2reg 等最佳化步驟才容易辨識並移除多餘的記憶體操作。

### 10.2 宣告、讀取與寫入變數

宣告變數分成三步：算出初始值、保留空間、把值寫入。LLVM 用 `store` 寫入記憶體，用 `load` 讀取。

```rust
fn codegen_let(
    &mut self,
    local: hir::LocalId,
    ty: &hir::Type,
    init: &hir::Expr,
) -> Result<GeneratedValue<'ctx>, BackendError> {
    let value = self.codegen_expr(init)?.require_value("initializer")?;
    let function = self.current_function.ok_or_else(|| {
        BackendError::InvalidHir("local outside function".into())
    })?;
    let ptr = self.create_entry_alloca(
        function,
        lower_basic_type(self.context, ty)?,
        "local",
    )?;

    self.builder.build_store(ptr, value)?;
    self.locals.insert(local, Storage::Stack(ptr));
    Ok(GeneratedValue::Unit)
}
```

LLVM 21 的 `ptr` 不包含所指資料的型別，所以 `build_load` 還需要你明確告訴它要讀取哪一種值：

```rust
let value = self.builder.build_load(
    lower_basic_type(self.context, ty)?,
    ptr,
    "load.local",
)?;
```

修改變數時，只需把新值 `store` 到原本的位置；能不能修改，HIR 已經檢查過。之後若變數被閉包共用，會改用 `Storage::Cell`，讀寫前先找到儲存格內保存值的位置。

### 10.3 把區塊轉成指令

HIR 已把最後結果獨立記在 `tail`，所以後端只要依序處理各個步驟，再計算 `tail`：

```rust
fn codegen_block(
    &mut self,
    block: &hir::Block,
) -> Result<GeneratedValue<'ctx>, BackendError> {
    for statement in &block.statements {
        self.codegen_statement(statement)?;
    }
    self.codegen_expr(&block.tail)
}
```

HIR 已把名稱換成 `LocalId`，後端不必再建立一層層以字串名稱查找變數的表格。只要每個 `LocalId` 在整個函式中都不重複，即使離開原始碼的區塊後還保留對照資料，也不會讓別的運算式誤用同名變數。

### 10.4 保存最外層的全域變數

在 REPL 輸入 `(mut (int x) 1)` 後，下次輸入仍要讀得到 `x`。因此，最外層宣告不能放在短期使用的 `alloca` 空間，而要由執行環境提供長期有效的固定儲存位置：

```llvm
declare ptr @sai_rt_global_slot(ptr %runtime, i64 %global_id,
                                i64 %size, i64 %align)
```

`GlobalId` 由 `FrontendSession` 分配，整個工作階段不能重複。若每次輸入都從零開始，新變數就可能誤用舊變數的位置。AOT 雖然只有一份模組，也使用相同介面，讓兩種執行方式遵守相同規則。

`InitGlobal` 的轉換步驟是：

```text
計算初始值
  -> 呼叫 sai_rt_global_slot(runtime, id, size, align) 取得位置
  -> 把初始值寫入該位置
  -> 這個宣告的結果是 Unit
```

`Load(Place::Global(id))` 和 `Assign` 都向執行環境取得同一個位置。也可以在函式起點先取得指標，後面重複使用。最外層 `def` 同樣使用全域儲存位置，只是裡面放的是 `%SaiClosure`；如此函式與一般變數就能共用登記和重新定義的機制。

每次使用同一個 `global_id` 時，要求的大小（size）和對齊方式（alignment）必須相同。對齊是指資料位址需要符合的倍數限制，第 13 節會再說明。若要求不一致，代表前端和 JIT 對資料格式的認知不同，應回報內部錯誤，不能偷偷另開一塊空間。

## 11. 條件判斷：程式接下來走哪條路？

加法只要一路往下算，`if` 卻需要依條件選擇不同的路。這種「下一步去哪裡執行」的安排稱為控制流程。

### 11.1 LLVM 的基本區塊

LLVM 把函式分成基本區塊（basic block）。每個區塊內的指令依序執行，最後用一個結束指令（terminator）決定接下來怎麼走：

- `ret`：結束函式，回傳結果。
- `br`：跳到另一個區塊，可以依條件選擇。
- `switch`：依數值選擇多條路中的一條。
- `unreachable`：標示執行不可能繼續到達這裡，例如前一個呼叫已跳去錯誤處理程序。

每個可到達的區塊都必須有結束指令，其後不能再放其他指令。另外，若區塊內還有 `if`，產生完指令後的位置可能已移到另一個區塊。要重新詢問 `builder`，不能以為還停在最初的 `then_block`。

### 11.2 `if` 的結果從哪裡來？

Sai：

```lisp
(if (< n 2)
  n
:else
  (+ n 1))
```

下面的 IR 示意兩條路如何會合。`%` 開頭的是值的名稱，`label` 後面是區塊名稱。

最重要的是最後一行的 `phi`：如果剛從 `if.then` 過來，結果就是 `%n`；若從 `if.else` 過來，結果就是 `%next`。它不會把兩邊都算一次，而是選取實際走過的那條路所提供的值。

```llvm
br i1 %condition, label %if.then, label %if.else

if.then:
  br label %if.merge

if.else:
  %next = add i64 %n, 1
  br label %if.merge

if.merge:
  %result = phi i64 [ %n, %if.then ], [ %next, %if.else ]
```

產生這段流程時，先建立「條件成立」「條件不成立」和「會合」三個區塊，再分別產生指令：

```rust
let function = self.current_function.ok_or_else(|| {
    BackendError::InvalidHir("if outside function".into())
})?;
let condition = self
    .codegen_expr(condition)?
    .require_value("if condition")?
    .into_int_value();

let then_block = self.context.append_basic_block(function, "if.then");
let else_block = self.context.append_basic_block(function, "if.else");
let merge_block = self.context.append_basic_block(function, "if.merge");

self.builder.build_conditional_branch(
    condition,
    then_block,
    else_block,
)?;

self.builder.position_at_end(then_block);
let then_value = self.codegen_block(then_body)?;
let then_end = self.builder.get_insert_block().ok_or_else(|| {
    BackendError::InvalidHir("missing then insertion block".into())
})?;
let then_reaches_merge = then_end.get_terminator().is_none();
if then_reaches_merge {
    self.builder.build_unconditional_branch(merge_block)?;
}

self.builder.position_at_end(else_block);
let else_value = self.codegen_block(else_body)?;
let else_end = self.builder.get_insert_block().ok_or_else(|| {
    BackendError::InvalidHir("missing else insertion block".into())
})?;
let else_reaches_merge = else_end.get_terminator().is_none();
if else_reaches_merge {
    self.builder.build_unconditional_branch(merge_block)?;
}

self.builder.position_at_end(merge_block);
```

若整個 `if` 是 `Unit`，就沒有資料值需要選擇，直接回傳 `GeneratedValue::Unit`。其他型別才建立 PHI，並加入各條路提供的值：

```rust
let phi = self.builder.build_phi(
    lower_basic_type(self.context, result_type)?,
    "if.result",
)?;

let then_value = then_value.require_value("then branch")?;
let else_value = else_value.require_value("else branch")?;
if then_reaches_merge {
    phi.add_incoming(&[(&then_value, then_end)]);
}
if else_reaches_merge {
    phi.add_incoming(&[(&else_value, else_end)]);
}
Ok(GeneratedValue::Value(phi.as_basic_value()))
```

這裡容易犯的錯是：把最初的 `then_block` 當成最後抵達會合點的區塊。若這條分支裡還有另一個 `if`，真正接到會合點的會是內層 `if` 的結尾。這個「直接連到目前區塊的上一個區塊」稱為前驅（predecessor），PHI 必須記錄它，而不是更早的起點。

之後若 Sai 加入 `return`，已經回傳的分支就不會走到會合點，也不能加入 PHI。若兩邊都不會抵達，會合區塊應以 `unreachable` 結束，並讓轉換結果回傳獨立的 `Diverged` 狀態，表示這個運算式不會正常完成。不要建立沒有任何來源值的 PHI。

### 11.3 `and` 與 `or`

Sai 的 `and`、`or` 需要短路求值：只要前面的結果已經足以決定答案，就不再執行後面的運算式。例如：

```lisp
(or true (= (/ 1 0) 0))
```

第一個參數已是 `true`，整個 `or` 就一定是 `true`，所以右邊的除以零不應執行。LLVM 的 `and`、`or` 指令本身不會幫忙略過產生右側值的指令，不能直接用它們取代 Sai 的短路行為。

最簡單的做法是在整理 HIR 時改寫成 `if`：

```text
(or a b)  -> if a then true else b
(and a b) -> if a then b else false
```

有三個以上的參數時，從最右邊開始包成一層層的 `If`，讓最左邊的條件仍最先執行。後端就能沿用剛才的分支與 PHI 處理方式。

`not` 只有一個輸入，直接把布林值反轉即可：

```rust
let value = self
    .codegen_expr(condition)?
    .require_value("not")?
    .into_int_value();
let result = self.builder.build_not(value, "not")?;
```

本節驗收：一般值使用 PHI，`unit` 不使用；巢狀 `if` 記錄正確的來源區塊；`or` 遇到 `true`、`and` 遇到 `false` 後不再執行右側。也要確認每個區塊有結束指令，整份模組通過檢查。

```text
if_with_value_builds_phi
unit_if_does_not_build_phi
nested_if_uses_actual_predecessor
or_does_not_evaluate_rhs_after_true
and_does_not_evaluate_rhs_after_false
every_block_has_a_terminator
module_passes_verifier
```

## 12. 加入具名函式與遞迴

遞迴就是函式呼叫自己。要產生這種呼叫，LLVM 必須先知道這個函式存在，以及它接受和回傳什麼型別。

### 12.1 先登記所有函式，再填入內容

分成兩輪處理，會比看到 `def` 就立刻產生完整函式更容易：

1. 走過 `hir::Module.functions`，替每個 `FunctionId` 建立函式宣告（prototype），先登記名稱、參數與回傳型別。
2. 再走一遍，填入每個函式的指令。

```rust
fn declare_functions(&mut self, hir: &hir::Module) -> Result<(), BackendError> {
    for function in &hir.functions {
        let function_type = lower_function_type(
            self.context,
            &function.signature,
        )?;
        let name = mangle_function_name(function);
        let value = self.module.add_function(&name, function_type, None);
        self.functions.insert(function.id, value);
    }
    Ok(())
}
```

`mangle_function_name` 用來產生 LLVM 使用的唯一名稱，不能只回傳 Sai 原始碼中的名字。不同區塊或模組可能都有同名函式，匿名函式更沒有使用者給的名字。第一版可把模組編號、函式編號與整理後的原名組合起來：

```text
__sai_fn_<module_id>_<function_id>_<sanitized_source_name>
```

例如：

```text
__sai_fn_0_3_fib
__sai_fn_0_7_lambda
```

第二輪再填入函式內容，並逐一檢查：

```rust
fn emit_functions(&mut self, hir: &hir::Module) -> Result<(), BackendError> {
    for function in &hir.functions {
        let llvm_function = self.functions[&function.id];
        self.emit_function(function, llvm_function)?;

        if !llvm_function.verify(true) {
            return Err(BackendError::Verification(format!(
                "function {:?} failed verification",
                function.id,
            )));
        }
    }
    Ok(())
}
```

這樣就能讓函式呼叫自己。不過，「A 呼叫 B、B 又呼叫 A」這種互相遞迴，還需要前端配合：前端必須先收集同一最外層範圍中的所有函式簽章，才認得後面出現的名字。

若要維持 Sai 目前按順序定義、只能使用當下已知名稱的規則，就只替前端已確認合法的直接呼叫建立宣告。後端先登記函式，不代表語言自動允許提前使用尚未定義的名字。

### 12.2 產生函式內容

每個函式都使用相同的參數順序。前兩個由編譯器加入，之後才是使用者寫的參數：

```text
0：執行環境指標
1：閉包捕捉資料的指標
2 起：使用者宣告的參數
```

```rust
fn emit_function(
    &mut self,
    hir_function: &hir::Function,
    function: FunctionValue<'ctx>,
) -> Result<(), BackendError> {
    let entry = self.context.append_basic_block(function, "entry");
    self.builder.position_at_end(entry);
    self.current_function = Some(function);
    self.locals.clear();
    self.captures.clear();

    let runtime = function.get_nth_param(0).ok_or_else(|| {
        BackendError::InvalidHir("missing runtime parameter".into())
    })?.into_pointer_value();
    let environment = function.get_nth_param(1).ok_or_else(|| {
        BackendError::InvalidHir("missing environment parameter".into())
    })?.into_pointer_value();
    self.current_environment = Some(environment);

    self.bind_captures(hir_function, environment)?;

    for (index, parameter) in hir_function.params.iter().enumerate() {
        let value = function
            .get_nth_param(index as u32 + 2)
            .ok_or_else(|| BackendError::InvalidHir(
                "LLVM parameter count does not match HIR".into(),
            ))?;
        value.set_name(&parameter.name);
        self.locals.insert(parameter.local, Storage::Value(value));
    }

    let result = self.codegen_block(&hir_function.body)?;
    let block = self.builder.get_insert_block().ok_or_else(|| {
        BackendError::InvalidHir("missing return block".into())
    })?;

    if block.get_terminator().is_none() {
        match hir_function.signature.return_type.as_ref() {
            hir::Type::Unit => {
                self.builder.build_return(None)?;
            }
            _ => {
                let value = result.require_value("function return")?;
                self.builder.build_return(Some(&value))?;
            }
        }
    }

    self.current_function = None;
    self.current_environment = None;
    Ok(())
}
```

上例先取得 `runtime`，整合時還要把它存到 `CodeGenerator` 目前函式的資料中。後續除法錯誤檢查、記憶體配置和字串操作，才都能取得執行環境。

### 12.3 直接呼叫已知的函式

直接呼叫也要帶上前兩個額外參數。只有 HIR 已確認這個函式不需要捕捉變數時，才能把環境指標設為 `null`：

```rust
fn codegen_direct_call(
    &mut self,
    target: hir::FunctionId,
    args: &[hir::Expr],
    return_type: &hir::Type,
) -> Result<GeneratedValue<'ctx>, BackendError> {
    let function = *self.functions.get(&target).ok_or_else(|| {
        BackendError::InvalidHir(format!("missing function {target:?}"))
    })?;

    let runtime = self.current_runtime()?;
    let null_env = self.context
        .ptr_type(AddressSpace::default())
        .const_null();
    let mut values = vec![runtime.into(), null_env.into()];

    for arg in args {
        values.push(
            self.codegen_expr(arg)?
                .require_value("function argument")?
                .into(),
        );
    }

    let call = self.builder.build_call(function, &values, "call")?;
    if matches!(return_type, hir::Type::Unit) {
        Ok(GeneratedValue::Unit)
    } else {
        Ok(GeneratedValue::Value(
            call.try_as_basic_value().unwrap_basic(),
        ))
    }
}
```

Inkwell 0.9 要用 `try_as_basic_value().unwrap_basic()` 取得呼叫結果，舊教學中的 `.left()` 不適用。呼叫回傳 `void` 時沒有一般的資料值，所以必須先分開處理 `Unit`。

需要捕捉變數的具名函式，也要以 `%SaiClosure` 呼叫，才能帶上它保存的資料。不能因為函式有名字，就把環境指標換成 `null`。之後可以再最佳化：若已知要呼叫哪個函式，而且它沒有捕捉資料，就改成直接呼叫。

巢狀具名函式呼叫自己時，使用 `CurrentFunction`，把目前的環境指標繼續傳下去：

```rust
let environment = self.current_environment.ok_or_else(|| {
    BackendError::InvalidHir("missing current environment".into())
})?;

// 其餘參數與直接呼叫相同，但環境指標沿用 environment，
// 不能改成 null_env。
let mut values = vec![
    self.current_runtime()?.into(),
    environment.into(),
];
```

`emit_function` 讀取第 1 號參數後，便設定 `current_environment`，處理完函式再清除。如此遞迴呼叫就能沿用同一組捕捉資料。

Sai 目前依順序定義函式，尚不支援互相遞迴。若未來要讓數個帶捕捉資料的閉包互相呼叫，需要先替它們保留一組閉包儲存位置，再填入各自的函式位址和環境指標。只用環境為 `null` 的直接呼叫是不夠的。

### 12.4 驗收：計算費氏數列

修正語法分析器後，用下列費氏數列函式測試遞迴。當 `n` 小於 2 時直接回傳 `n`，其餘情況則把前兩項相加：

```lisp
(def fib ((int n) int)
  (if (< n 2)
    n
  :else
    (+ (fib (- n 2))
       (fib (- n 1)))))

(fib 10)
```

驗收條件：

- 在產生 `fib` 的內容前，已先建立函式宣告。
- 遞迴呼叫使用同一個 LLVM `FunctionType`。
- 函式與整份模組都通過 LLVM 檢查。
- 直譯器與 JIT 都得到 `55`。

## 13. 加入字串與執行環境

數字可以直接放進一個 LLVM 值裡，字串卻可能很長。因此我們分開保存字串內容，以及用來找到內容的位址和長度。

### 13.1 用位址和長度表示字串

Sai 的 Rust `String` 使用 UTF-8 編碼，以一串位元組保存文字。它不像傳統 C 字串那樣，靠最後的零位元組表示結尾。因此固定使用這種結構：

```llvm
%SaiString = type { ptr, i64 }
```

欄位是：

| 欄位編號 | 欄位 | 意義 |
|---|---|---|
| 0 | `data` | 字串 UTF-8 資料的起始位址 |
| 1 | `len` | 資料佔幾個位元組，不包含結尾標記 |

一個位元組不一定是一個字。例如一個中文字通常需要數個位元組，而使用者看見的一個字形，也可能由數個 Unicode 編碼單位組成。

因此，未來實作 `len` 和 `nth` 前，要先決定它們是在數位元組、Unicode 碼位（更精確地說是 Unicode scalar value），還是使用者看見的字形組合（grapheme cluster）。這裡保存的位元組長度，不能直接當成畫面上的字數。

### 13.2 保存原始碼裡的字串

原始碼直接寫出的字串稱為字串常值（string literal）。把每個常值放進只供目前 LLVM 模組使用、不可修改的全域資料，再用 `%SaiString` 記錄位址和長度：

```rust
fn codegen_string_literal(
    &mut self,
    value: &str,
) -> Result<GeneratedValue<'ctx>, BackendError> {
    let bytes = self.context.const_string(value.as_bytes(), false);
    let name = format!(".sai.str.{}", self.next_string_id);
    self.next_string_id += 1;

    let global = self.module.add_global(bytes.get_type(), None, &name);
    global.set_initializer(&bytes);
    global.set_constant(true);
    global.set_linkage(inkwell::module::Linkage::Private);
    global.set_unnamed_addr(true);

    let string = self.types.string.const_named_struct(&[
        global.as_pointer_value().into(),
        self.context
            .i64_type()
            .const_int(value.len() as u64, false)
            .into(),
    ]);

    Ok(GeneratedValue::Value(string.into()))
}
```

`value.len()` 計算的是 UTF-8 位元組數。例如 `"台灣"` 佔 6 個位元組，因此這裡的長度是 6，不是 2。

比較字串時要比較內容，不能只比較指標。兩份都寫著 `"台灣"` 的資料，可能放在不同位址。

### 13.3 定義執行環境的介面

執行環境負責配置記憶體、管理全域變數、比較字串與回報錯誤。建議用固定的 C ABI 讓編譯後的程式呼叫這些功能。

下面的 `SaiValue` 用來把各種 Sai 值交回呼叫端，`SaiRuntimeError` 保存錯誤種類和原始碼位置編號。C 標頭檔、LLVM 型別，以及 Rust 加上 `#[repr(C)]` 的對應結構，都必須使用相同格式。第 15 節會說明如何裝入各種值。

```c
#include <stdint.h>
#include <stddef.h>

typedef struct SaiRuntime SaiRuntime;

typedef struct {
    uint32_t tag;
    uint32_t reserved;
    uint64_t payload[2];
} SaiValue;

typedef struct {
    uint32_t code;
    uint32_t span_id;
} SaiRuntimeError;

typedef int32_t (*SaiEntry)(SaiRuntime *runtime, SaiValue *out_value);

SaiRuntime *sai_rt_create(void);
void sai_rt_destroy(SaiRuntime *runtime);
void sai_rt_print_value(SaiRuntime *runtime, const SaiValue *value);
void sai_rt_last_error(
    const SaiRuntime *runtime,
    SaiRuntimeError *out_error
);

int32_t sai_rt_begin_transaction(SaiRuntime *runtime);
void sai_rt_commit_transaction(SaiRuntime *runtime);
void sai_rt_rollback_transaction(SaiRuntime *runtime);

void *sai_rt_alloc(
    SaiRuntime *runtime,
    uint64_t size,
    uint64_t align
);

void *sai_rt_alloc_cell(
    SaiRuntime *runtime,
    uint64_t size,
    uint64_t align
);

void *sai_rt_global_slot(
    SaiRuntime *runtime,
    uint64_t global_id,
    uint64_t size,
    uint64_t align
);

uint8_t sai_rt_str_eq(
    const uint8_t *lhs,
    uint64_t lhs_len,
    const uint8_t *rhs,
    uint64_t rhs_len
);

int32_t sai_rt_invoke(
    SaiRuntime *runtime,
    SaiEntry entry,
    SaiValue *out_value
);

_Noreturn void sai_rt_raise(
    SaiRuntime *runtime,
    uint32_t error_code,
    uint32_t span_id
);
```

LLVM 端也要宣告相同的函式，讓產生的指令能呼叫它們：

```llvm
declare ptr @sai_rt_alloc(ptr, i64, i64)
declare ptr @sai_rt_alloc_cell(ptr, i64, i64)
declare ptr @sai_rt_global_slot(ptr, i64, i64, i64)
declare i8 @sai_rt_str_eq(ptr, i64, ptr, i64)
declare void @sai_rt_raise(ptr, i32, i32) noreturn
```

`sai_rt_alloc`、`sai_rt_alloc_cell` 和 `sai_rt_global_slot` 若無法配置記憶體，或發現資料大小、對齊不一致，必須呼叫 `sai_rt_raise`。不能只回傳 `null`，否則接下來的欄位定位與寫入指令仍會執行，使用無效的位址。

可修改的捕捉變數必須透過 `sai_rt_alloc_cell` 建立儲存格，讓執行環境知道哪些共用儲存格仍然存在。這對後面的錯誤復原很重要。

### 13.4 執行出錯時，回到外層處理

假設 A 呼叫 B，B 又呼叫 C，而 C 發生除以零。C 原本應回傳整數，不能突然把「錯誤代碼」當成那個整數交給 B，讓 B 繼續計算。

第一版採用 C 的 `setjmp` 和 `longjmp`。可以把它理解成：執行前先設好一個錯誤返回點；深處的函式出錯時，直接跳回這個點。這個返回點所在的位置稱為錯誤處理邊界。

```text
Rust 呼叫端或 C main
  -> sai_rt_invoke
       -> 用 setjmp 設定錯誤返回點
       -> 呼叫編譯產生的 __sai_entry
            -> 執行 Sai 函式
                 -> sai_rt_raise
                      -> 保存錯誤，再用 longjmp 跳回返回點
       -> 把錯誤代碼交給呼叫端
```

`sai_rt_raise` 在 C 宣告為 `_Noreturn`，LLVM 端也標記 `noreturn`，意思是它不會正常返回呼叫處。因此呼叫它之後可以接 `unreachable`。`sai_rt_invoke` 收到跳轉後，會還原先前的錯誤處理設定，並回傳錯誤代碼。

呼叫端若收到非零值，就用 `sai_rt_last_error` 取得詳細資料，再把 `span_id` 查成原始碼位置，顯示錯誤訊息。C 執行環境只保存編號，不要保存指向短期 Rust `String` 的指標，避免文字已被釋放後還去讀取。

下面是核心 C 程式。管理整批記憶體的區域（arena）和全域變數表先省略，聚焦在錯誤返回流程：

```c
#include <setjmp.h>
#include <stdlib.h>

struct SaiRuntime {
    jmp_buf *active_boundary;
    SaiRuntimeError error;
    /* 此處省略記憶體管理區、全域變數表與交易快照。 */
};

int32_t sai_rt_invoke(
    SaiRuntime *runtime,
    SaiEntry entry,
    SaiValue *out_value
) {
    jmp_buf boundary;
    jmp_buf *previous = runtime->active_boundary;
    runtime->active_boundary = &boundary;

    int32_t jumped = setjmp(boundary);
    int32_t status = jumped == 0
        ? entry(runtime, out_value)
        : (int32_t)runtime->error.code;

    runtime->active_boundary = previous;
    return status;
}

_Noreturn void sai_rt_raise(
    SaiRuntime *runtime,
    uint32_t error_code,
    uint32_t span_id
) {
    runtime->error.code = error_code;
    runtime->error.span_id = span_id;
    if (runtime->active_boundary == NULL) {
        abort();
    }
    longjmp(*runtime->active_boundary, 1);
}
```

**回報錯誤之外，還要撤銷本次修改。** REPL 執行時使用第 4 節介紹的交易機制：

1. `begin`：備份現有全域變數和所有共用可修改儲存格的內容，並記下目前配置記憶體的位置，作為分界。
2. `rollback`：若出錯，還原舊內容、移除這次新增的變數與儲存格，再釋放分界之後配置的記憶體。
3. `commit`：若成功，保留修改，丟棄備份。

只備份全域變數並不夠。本次輸入可能呼叫舊閉包，進而修改舊閉包保存的共用儲存格，那些修改也要能撤銷。另一種做法是在每次寫入前記錄舊值；無論選哪種方式，都不能漏掉可修改資料的寫入。

Sai 目前沒有檔案或網路等外部輸入輸出功能，所以還原這些記憶體就能恢復使用者可觀察到的狀態。未來加入外部 I/O 後，已寫出的檔案或送出的資料不能靠記憶體備份撤銷，需要另外訂出錯誤時的規則。

**`longjmp` 有使用限制。** 它直接跳回返回點，不會沿路執行 Rust 物件的清理程序（destructor）。因此，從 `sai_rt_invoke` 到 `sai_rt_raise` 之間，只能經過不需這種清理的 LLVM 指令和 C 輔助函式，不能插入持有待清理 Rust 物件的回呼函式。

若未來要支援這類回呼、例外處理，或需要在離開時清理的堆疊物件，就應改成每層明確傳回成功或失敗，或採用能沿途清理的正式堆疊展開機制（unwinding）。

跨 C 介面傳遞布林值時，使用一個位元組的 `i8`，回到 LLVM 內部後再轉成 `i1`：

```llvm
%raw = call i8 @sai_rt_str_eq(ptr %a, i64 %a_len, ptr %b, i64 %b_len)
%equal = icmp ne i8 %raw, 0
```

不要直接把 LLVM 內部的 `i1` 當成這套 C 或 Rust 介面的布林參數與回傳值。

### 13.5 編譯執行環境

建立：

```text
runtime/
├── sai_runtime.c
└── sai_runtime.h
```

下面會產生兩種函式庫：`.a` 是靜態函式庫，連結時把所需程式碼放入執行檔；`.so` 是共用函式庫，可在程式執行時載入。

```bash
mkdir -p build/runtime
clang-21 -std=c11 -O2 -fPIC -c runtime/sai_runtime.c \
  -o build/runtime/sai_runtime.o
ar rcs build/runtime/libsai_runtime.a \
  build/runtime/sai_runtime.o
clang-21 -shared build/runtime/sai_runtime.o \
  -o build/runtime/libsai_runtime.so
```

AOT 連結 `libsai_runtime.a`，JIT 則在啟動時載入同一份程式碼編譯出的共用函式庫：

```rust
use std::path::Path;
use inkwell::support::load_library_permanently;

load_library_permanently(Path::new(
    "build/runtime/libsai_runtime.so",
))?;
```

`load_library_permanently` 載入後不支援卸載。本教學的 JIT 也會保留已成功執行的模組到工作階段結束，兩者的做法一致。

另一種方式是用 `ExecutionEngine::add_global_mapping`，直接告訴 JIT：某個執行環境函式對應到本機的哪個位址。若採用這種方式，AOT 和 JIT 仍要通過相同的行為測試，避免 Rust 端輔助函式和 C 版本做出不同結果。

### 13.6 讓 LLVM 計算資料大小與排列方式

結構裡的欄位不一定緊密相連。為了符合 CPU 的讀寫規則，欄位起點可能必須是某個數字的倍數，這叫作對齊（alignment）；中間補出的空間叫作填補位元組（padding）。

因此，閉包環境和儲存格要佔多少空間，應由 LLVM 目標平台資料計算，不能自己把欄位大小相加。不同平台的指標大小也可能不同，不能把它固定寫成 8 位元組後，就當作能支援所有平台。

```rust
let target_data = target_machine.get_target_data();
let size = target_data.get_store_size(&environment_type);
let align = target_data.get_abi_alignment(&environment_type);
```

整合時可能需要依 Inkwell 0.9 的參數型別補上列舉轉換。重點是交給 LLVM 計算資料排列方式，而不是由前端自行猜測。

本節驗收：比較空字串、相同和不同的英文字串，以及 Unicode 字串；字串傳入函式再回傳後，內容和長度不變。JIT 與 AOT 也要使用同一份比較函式。

```text
empty_strings_compare_equal
equal_ascii_strings_compare_equal
different_strings_compare_false
equal_unicode_strings_compare_equal
string_parameter_and_return_preserve_data_and_length
JIT_and_AOT_use_the_same_runtime_helper
```

## 14. 讓閉包記住外層變數

假設函式裡使用了外層的 `x`。之後呼叫這個函式時，除了知道要執行哪段程式，還必須找得到 `x`。閉包就是把這兩件事包在一起。

目前直譯器的 `FunctionValue` 已保存函式參數、內容與外層環境：

```rust
pub struct FunctionValue {
    pub params: Vec<String>,
    pub body: Block,
    pub env: EnvRef,
}
```

其中 `env` 讓函式能存取建立它時所在的外層環境。LLVM 後端也要提供相同效果，但不能直接把 Rust 的 `Rc<RefCell<_>>` 當成編譯後程式的資料格式。我們需要明確列出並保存它用到的變數。

### 14.1 閉包保存兩個位址

所有函式值統一使用以下結構：

```llvm
%SaiClosure = type { ptr, ptr }
```

| 欄位編號 | 欄位 | 意義 |
|---|---|---|
| 0 | `code` | 要執行的 LLVM 函式位址 |
| 1 | `env` | 捕捉資料的位址；沒有捕捉變數時可為 `null` |

即使 Sai 把函式寫在另一個函式裡，LLVM 端仍把它建立成模組中的獨立函式。原本需要的外層變數，改由 `environment` 參數帶進來：

```text
(runtime, environment, original parameters...) -> result
```

例如，下面的匿名函式接受 `y`，但還用到了外層的 `x`。呼叫時 `y` 是 2，`x` 是 40，所以結果是 42：

```lisp
(block
  (let (int x) 40)
  ((fn ((int y) int)
     (+ x y))
   2))
```

在 LLVM 中，先用 `%Env.lambda0` 保存 `x`。函式拿到 `%env` 後，找到欄位位置、讀出 `x`，再與 `y` 相加。`getelementptr`（常縮寫為 GEP）只計算欄位位址，真正讀取資料的是 `load`。

```llvm
%Env.lambda0 = type { i64 }

define i64 @__sai_lambda_0(ptr %runtime, ptr %env, i64 %y) {
entry:
  %x.ptr = getelementptr %Env.lambda0, ptr %env, i32 0, i32 0
  %x = load i64, ptr %x.ptr
  %sum = add i64 %x, %y
  ret i64 %sum
}
```

### 14.2 找出函式用到了哪些外層變數

在函式內使用、卻不是這個函式自己宣告的變數，稱為自由變數（free variable）。分析前先把每個名稱換成變數編號，才能正確區分不同區塊裡的同名變數。只比較字串 `"x"`，可能捕捉到錯的那個 `x`。

依序套用以下規則：

1. 參數和函式內宣告的變數，屬於目前函式。
2. 用到外層的區域變數或捕捉資料時，加入目前函式的捕捉清單。
3. `set!` 也是使用變數，不能只檢查讀取操作。
4. 全域變數有固定的儲存位置，不必重複放進閉包環境。
5. 最內層閉包用到更外層的變數時，中間每層都要能轉交那個值或共用儲存格。
6. 清單順序要固定，才能每次建立相同的資料格式。

把分析結果記在每個函式自己的 `CapturePlan`：

```rust
struct CapturePlan {
    captures: Vec<Capture>,
    environment_type: StructType<'ctx>,
}
```

每個 `FunctionId` 都有對應的環境型別，例如 `%SaiEnv.7`。因為不同函式需要記住的變數可能不同，它們的欄位數量和型別也不一定相同。

### 14.3 不能修改的變數：複製值即可

若外層變數不允許修改，閉包可以直接保存一份值。例如下面的環境保存一個整數和一個字串結構：

```llvm
%SaiEnv.7 = type { i64, %SaiString }
```

建立閉包的步驟：

1. 呼叫 `sai_rt_alloc`，配置保存捕捉資料的空間。
2. 用 GEP 找到各個欄位的位址。
3. 把捕捉的值寫入欄位。
4. 組成 `{ 函式位址, 環境位址 }`。

LLVM 21 的指標本身不記型別，因此 GEP 仍需要知道正在處理哪一種結構，才能算出欄位位址：

```rust
let field = self.builder.build_struct_gep(
    environment_type,
    environment_pointer,
    index,
    "capture.field",
)?;
```

部分一般用途的 GEP 介面標成 `unsafe`，因為給錯型別或欄位編號，就可能算出無效的位址。建議把這些操作集中在少數輔助函式中，統一使用 HIR 檢查過的資料和事先算好的欄位排列。

### 14.4 可以修改的變數：共用同一個儲存格

如果閉包要修改外層變數，複製一份值就不夠了。看下面的 `x`：

```lisp
(block
  (mut (int x) 1)
  ((fn (int)
     (block
       (set! x (+ x 1))
       x)))
  x)
```

閉包把 `x` 加一後，外層再讀取 `x`，結果必須是 `2`。若閉包只修改自己的副本，外層卻仍讀到 `1`，就違反 Sai 的規則。

解法是只建立一個共用儲存格，讓外層和所有閉包都指向它。這個儲存格放在堆積區（heap）：由執行環境管理，存活時間不必跟著原本的函式結束。

```llvm
%Cell.i64 = type { i64 }
%SaiEnv.mutable = type { ptr }
```

在變數宣告處建立一次儲存格，之後大家都保存相同的指標。讀寫方式如下：

```text
讀取 x：
  儲存格指標 -> GEP 找到第 0 個欄位 -> load i64

修改 x：
  儲存格指標 -> GEP 找到第 0 個欄位 -> store i64
```

第一版也可以把所有 `mut` 都放進堆積區的儲存格，先不分析哪些變數會在函式結束後繼續被使用。之後再把確定沒有被閉包捕捉的區域變數改回起始區塊的 `alloca`，減少記憶體配置。

### 14.5 組合閉包的函式與環境

下面把前兩節串在一起：配置環境、填入各個捕捉欄位，再組成 `%SaiClosure`。`ByValue` 存值，`ByRef` 則存共用儲存格的指標。

```rust
fn codegen_make_closure(
    &mut self,
    function_id: hir::FunctionId,
    operands: &[hir::CaptureOperand],
) -> Result<GeneratedValue<'ctx>, BackendError> {
    let function = self.functions[&function_id];
    let plan = &self.capture_plans[&function_id];
    let environment = self.allocate_environment(plan)?;

    for (index, capture) in operands.iter().enumerate() {
        let field = self.environment_field(plan, environment, index as u32)?;
        let value = match capture.mode {
            hir::CaptureMode::ByValue => self.load_place(capture.source)?,
            hir::CaptureMode::ByRef => self.cell_pointer(capture.source)?.into(),
        };
        self.builder.build_store(field, value)?;
    }

    let closure = self.types.closure.get_undef();
    let closure = self.builder
        .build_insert_value(
            closure,
            function.as_global_value().as_pointer_value(),
            0,
            "closure.code",
        )?
        .into_struct_value();
    let closure = self.builder
        .build_insert_value(
            closure,
            environment,
            1,
            "closure.env",
        )?
        .into_struct_value();

    Ok(GeneratedValue::Value(closure.into()))
}
```

整合時，函式位址轉成指標的寫法可能需要配合 Inkwell 的功能選項調整。關鍵是：`%SaiClosure.code` 只保存指標，而函式真正的參數與回傳型別仍由 HIR 記錄。

### 14.6 從閉包取出函式，再呼叫它

間接呼叫分成三步：先求出閉包值，再取出 `code` 與 `env`，最後依 HIR 記錄的簽章組合參數並呼叫。

```rust
let closure = self
    .codegen_expr(callee)?
    .require_value("callee")?
    .into_struct_value();
let code = self.builder
    .build_extract_value(closure, 0, "callee.code")?
    .into_pointer_value();
let environment = self.builder
    .build_extract_value(closure, 1, "callee.env")?
    .into_pointer_value();

let function_type = lower_function_type(self.context, signature)?;
let mut llvm_args = vec![
    self.current_runtime()?.into(),
    environment.into(),
];
for arg in args {
    llvm_args.push(
        self.codegen_expr(arg)?
            .require_value("closure argument")?
            .into(),
    );
}

let call = self.builder.build_indirect_call(
    function_type,
    code,
    &llvm_args,
    "closure.call",
)?;
```

指標本身不包含函式簽章。如果 `build_indirect_call` 使用錯誤的 `FunctionType`，雙方就可能用不同方式解讀參數，造成未定義行為（undefined behavior）：程式不再有可依賴的結果。

因此，HIR 檢查器必須確認閉包的函式型別和呼叫處要求的型別完全相同。

### 14.7 記憶體生命週期

閉包可能在建立它的函式結束後才被呼叫。因此，它的捕捉資料不能放在那個函式的短期堆疊空間裡，否則指標會指向已經失效的記憶體，稱為懸空指標（dangling pointer）。

教學版採用簡單的規則：成功保存的資料保留到工作階段結束。失敗輸入新增的資料，仍依第 13 節的規則撤銷並釋放。

- 閉包環境和可修改儲存格，保留到程式或 JIT 工作階段結束。
- 成功執行的 JIT 模組不卸載，讓閉包的函式位址與字串常值位址持續有效。
- AOT 程式結束時，由執行環境或作業系統回收資源。

這個做法容易避免過早釋放，但即使資料已經用不到，也可能繼續保留。因此，長時間開著 REPL，記憶體用量會持續增加。正式版本還需要更完整的回收策略：

| 策略 | 優點 | 主要問題 |
|---|---|---|
| 自動參考計數（ARC）：記錄有幾處還在使用資料 | 最後一個使用者放手時就能回收，時機明確 | 資料若互相引用成一圈，計數可能永遠不會歸零 |
| 追蹤式垃圾回收（Tracing GC）：從仍可使用的值出發，找出所有可到達的資料 | 能回收互相引用、但已沒人能使用的一整組資料 | 需要記錄物件格式、從哪些值開始追蹤，以及何時能暫停程式進行回收 |
| 區域配置（Region/Arena）：一批資料一起配置、一起釋放 | 整批回收的成本低 | 閉包可能比原本區域活得更久，REPL 也可能長期不結束 |

對閉包和共用可修改資料較多的語言，追蹤式垃圾回收是值得考慮的完整方案。不過，應先完成正確的 LLVM 後端，再另外規劃記憶體回收工作。

本節驗收：從不捕捉變數的閉包開始，再測試整數、字串與可修改變數。特別確認兩個閉包共用同一個儲存格、內層能取得更外層的變數，以及離開原函式後閉包仍可使用。最後測試間接呼叫、帶捕捉資料的遞迴，以及內層閉包保存外層函式自己的情況。

```text
closure_without_capture
immutable_integer_capture
immutable_string_capture
mutable_capture_updates_outer_binding
two_closures_share_one_mutable_cell
nested_closure_forwards_ancestor_capture
escaping_closure_does_not_reference_stack_memory
indirect_call_uses_the_HIR_signature
nested_recursive_function_keeps_current_environment
child_closure_can_capture_parent_self_closure
```

## 15. 用固定格式把執行結果交回來

REPL 這次可能算出整數，下次可能算出字串。如果每次 JIT 入口的回傳型別都不同，Rust 呼叫端就得不斷更換呼叫方式，很容易出錯。

因此，用固定的 `%SaiValue` 包裝所有結果：`tag` 說明資料種類，`payload` 保存實際資料，`reserved` 則是保留欄位。

```llvm
%SaiValue = type {
  i32,       ; tag
  i32,       ; reserved/padding
  [2 x i64]  ; payload
}
```

各種 `tag` 的意義如下。浮點數保存原本的位元內容，不是轉成整數後的小數截斷結果；指標則保存位址的位元內容。

| 標記值 | 型別 | 資料欄位內容 |
|---:|---|---|
| 0 | Unit | 未使用 |
| 1 | Int | 一個 `i64` |
| 2 | Float | `double` 的原始位元內容 |
| 3 | Bool | `0` 或 `1` |
| 4 | String | 字串資料位址與位元組長度 |
| 5 | Function | 函式位址與捕捉資料位址 |

每份可執行模組都提供相同形式的入口。它用 `out_value` 指向的空間交回結果，回傳的 `i32` 則表示執行狀態：

```llvm
define i32 @__sai_entry(ptr %runtime, ptr %out_value)
```

規則：

- `__sai_entry` 正常完成時回傳 `0`。
- 執行中出錯時呼叫 `sai_rt_raise`，跳到外層錯誤返回點。
- 呼叫端必須透過 `sai_rt_invoke(runtime, entry, out)` 進入程式。它會接住錯誤，並回傳非零錯誤代碼。
- 成功時，把最後一個運算式的結果裝進 `%SaiValue`。
- 如何把值印在畫面上，統一交給呼叫端或執行環境處理。

Rust 使用相同的欄位排列，並加上 `#[repr(C)]`，讓結構採用 C 相容的格式：

```rust
#[repr(C)]
pub struct SaiValue {
    tag: u32,
    reserved: u32,
    payload: [u64; 2],
}

type Entry = unsafe extern "C" fn(
    *mut SaiRuntime,
    *mut SaiValue,
) -> i32;
```

這份設計把指標裝進 64 位元欄位，因此第一版先限定 64 位元平台。若要支援 32 位元平台，需要依平台重新定義格式，或由執行環境提供「寫入整數」「寫入字串」等函式，負責正確包裝資料。

## 16. 提前編譯：產生可以直接執行的檔案

AOT（ahead-of-time compilation）就是先編譯出執行檔，之後再啟動它。LLVM 會先產生目的檔，裡面已有機器指令，但可能還缺執行環境函式。連結器會把需要的部分接起來，形成完整執行檔。

### 16.1 告訴 LLVM 要在哪個平台執行

`TargetMachine` 記錄 CPU、作業系統與產生指令的選項。下面取得本機的平台組合（target triple），建立目標機器，並把資料排列規則（data layout）寫進 LLVM 模組。

```rust
use inkwell::{
    OptimizationLevel,
    targets::{
        CodeModel, InitializationConfig, RelocMode,
        Target, TargetMachine,
    },
};

fn create_host_target_machine(
    module: &Module<'_>,
    optimization: OptimizationLevel,
) -> Result<TargetMachine, BackendError> {
    Target::initialize_x86(&InitializationConfig::default());

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple)
        .map_err(|error| BackendError::Target(error.to_string()))?;
    let machine = target.create_target_machine(
        &triple,
        "generic",
        "",
        optimization,
        RelocMode::PIC,
        CodeModel::Default,
    ).ok_or_else(|| BackendError::Target(
        "failed to create target machine".into(),
    ))?;

    module.set_triple(&triple);
    module.set_data_layout(
        &machine.get_target_data().get_data_layout(),
    );
    Ok(machine)
}
```

若要支援 x86 以外的平台，可以改用 `Target::initialize_native`，並在 Cargo 啟用對應平台的功能，或使用 `target-all`。本文只啟用 `target-x86`，減少編譯進來的內容。

`"generic"` 表示先產生較通用的 x86-64 指令。若確定程式只在本機執行，可以改用 `TargetMachine::get_host_cpu_name()` 和 `get_host_cpu_features()`，利用本機 CPU 的額外功能。

### 16.2 最佳化前後都要檢查

順序固定為：

```text
產生 LLVM IR
  -> module.verify()
  -> 執行最佳化步驟
  -> module.verify()
  -> 輸出目的檔
```

LLVM 會分多個步驟整理指令，每個步驟稱為 pass。Inkwell 0.9 使用新版的步驟管理介面（new pass manager）。下面的 `set_verify_each(true)` 要求每個步驟後都檢查 IR：

```rust
use inkwell::passes::PassBuilderOptions;

fn optimize(
    module: &Module<'_>,
    machine: &TargetMachine,
    pipeline: &str,
) -> Result<(), BackendError> {
    let options = PassBuilderOptions::create();
    options.set_verify_each(true);
    module
        .run_passes(pipeline, machine, options)
        .map_err(|error| BackendError::Verification(error.to_string()))
}
```

命令列的最佳化選項可以對應到以下流程。`-O0` 適合先除錯，較高等級著重效能，`-Os` 和 `-Oz` 著重縮小程式：

| Sai 命令列選項 | LLVM 最佳化流程 |
|---|---|
| `-O0` | `default<O0>` |
| `-O1` | `default<O1>` |
| `-O2` | `default<O2>` |
| `-O3` | `default<O3>` |
| `-Os` | `default<Os>` |
| `-Oz` | `default<Oz>` |

先不要啟用 `fast-math`。它可能讓 LLVM 為了速度改變浮點數的處理假設；在決定 NaN、正負零與無限大的規則前，不應犧牲這些結果的一致性。

### 16.3 寫出 LLVM IR 與目的檔

```rust
module
    .print_to_file(&emit_llvm_path)
    .map_err(|error| BackendError::Llvm(error.to_string()))?;

machine
    .write_to_file(
        module,
        inkwell::targets::FileType::Object,
        &object_path,
    )
    .map_err(|error| BackendError::Target(error.to_string()))?;
```

LLVM 介面常用自己管理的字串回報錯誤，並不是 Rust 的 `std::io::Error`。因此用 `BackendError::Llvm` 保存這些錯誤文字。

### 16.4 產生 C `main`

AOT 執行檔還需要作業系統啟動時使用的 `main` 入口。它負責準備執行環境、執行 Sai 程式，再整理結果：

```text
main
  -> 建立執行環境
  -> 準備 SaiValue 結果空間
  -> call sai_rt_invoke(runtime, __sai_entry, result)
  -> 成功時呼叫 sai_rt_print_value 顯示結果
  -> 釋放執行環境
  -> 回傳執行狀態
```

`main` 可以由 LLVM IR 產生，也可以寫成 C 程式，放在執行環境函式庫中。採用 C 版本時，產生的目的檔要提供可供外部呼叫的 `__sai_entry`。這樣啟動程序和顯示格式都留在 C 端，通常較容易測試：

```c
extern int32_t __sai_entry(
    SaiRuntime *runtime,
    SaiValue *out_value
);

int main(void) {
    SaiRuntime *runtime = sai_rt_create();
    if (runtime == NULL) {
        return SAI_ERROR_OUT_OF_MEMORY;
    }
    SaiValue value = {0};
    int32_t status = sai_rt_invoke(runtime, __sai_entry, &value);
    if (status == 0 && value.tag != SAI_UNIT) {
        sai_rt_print_value(runtime, &value);
    }
    sai_rt_destroy(runtime);
    return status;
}
```

這段程式也遵循目前 REPL 的規則：結果是 `unit` 時不顯示，其他值才印出來。

### 16.5 把目的檔連結成執行檔

Rust CLI 可以用 `std::process::Command` 啟動 Clang，請它安排連結工作：

```bash
clang-21 \
  build/program.o \
  build/runtime/libsai_runtime.a \
  -o build/program
```

需要 LLD 時：

```bash
clang-21 -fuse-ld=lld \
  build/program.o \
  build/runtime/libsai_runtime.a \
  -o build/program
```

使用 Clang 統籌連結，比直接呼叫 `ld.lld-21` 簡單。Clang 會替你處理啟動所需的目的檔、動態載入器、C 標準函式庫（libc）及平台選項。

建議替 Sai 加上以下命令列介面：

```text
sai build program.sai
sai build program.sai -o program
sai build program.sai --emit llvm-ir
sai build program.sai --emit object
sai run program.sai
sai check program.sai
```

記錄連結器的結束狀態和標準錯誤輸出（stderr），失敗時轉成 `BackendError::LinkerFailure`。成功後才保留最終執行檔；中途的目的檔可放在暫存資料夾，完成後清除。

本節驗收指令：

```bash
sai build tests/fixtures/arithmetic.sai --emit llvm-ir
llvm-as-21 arithmetic.ll -o arithmetic.bc
opt-21 -passes=verify arithmetic.bc -disable-output

sai build tests/fixtures/arithmetic.sai -o build/arithmetic
./build/arithmetic
```

## 17. 即時編譯：編譯後立刻在目前程式中執行

JIT 不必先寫出完整執行檔，而是在程式執行期間把 LLVM IR 轉成機器指令，再呼叫它。這很適合 REPL：使用者輸入一段程式，就編譯並執行一段。

### 17.1 了解 Inkwell 的 JIT 工具

Inkwell 0.9 的 `ExecutionEngine` 包裝了 LLVM 的 ExecutionEngine/MCJIT，足夠用來完成第一版。LLVM 也有較新的 ORCv2 架構，但這裡用的不是那套介面。

使用時要注意以下限制：

- `ExecutionEngine`、`Module`、`Context` 通常不具備 Rust 的 `Send` 或 `Sync`，不能隨意搬到其他執行緒或跨執行緒共用。
- 取得並呼叫 JIT 函式需要 `unsafe`，因為 Rust 無法自行確認機器碼的參數與回傳型別。
- 加入新模組後，先前取得的 `JitFunction` 可能不再有效。
- 一份模組不能同時交給兩個執行引擎管理。
- 同名函式如何重新定義、舊模組何時釋放，都需要明確規則。

### 17.2 透過固定入口執行

下面的 `execute` 先檢查模組、建立執行引擎，再取得 `__sai_entry` 的位址。接著啟動交易，透過 C 的錯誤處理入口執行。成功才保存結果與引擎，失敗則還原資料。

```rust
use inkwell::{
    OptimizationLevel,
    execution_engine::ExecutionEngine,
    targets::{InitializationConfig, Target},
};

type Entry = unsafe extern "C" fn(
    *mut SaiRuntime,
    *mut SaiValue,
) -> i32;

unsafe extern "C" {
    fn sai_rt_begin_transaction(runtime: *mut SaiRuntime) -> i32;
    fn sai_rt_commit_transaction(runtime: *mut SaiRuntime);
    fn sai_rt_rollback_transaction(runtime: *mut SaiRuntime);
    fn sai_rt_invoke(
        runtime: *mut SaiRuntime,
        entry: Entry,
        out_value: *mut SaiValue,
    ) -> i32;
}

pub struct JitSession<'ctx> {
    runtime: *mut SaiRuntime,
    engines: Vec<ExecutionEngine<'ctx>>,
}

impl<'ctx> JitSession<'ctx> {
    fn execute(
        &mut self,
        module: &Module<'ctx>,
    ) -> Result<OwnedValue, BackendError> {
        module.verify().map_err(|error| {
            BackendError::Verification(error.to_string())
        })?;

        let engine = module
            .create_jit_execution_engine(OptimizationLevel::Default)
            .map_err(|error| BackendError::Target(error.to_string()))?;
        let address = engine
            .get_function_address("__sai_entry")
            .map_err(|error| BackendError::Target(error.to_string()))?;
        let entry: Entry = unsafe { std::mem::transmute(address) };

        let mut result = SaiValue {
            tag: 0,
            reserved: 0,
            payload: [0; 2],
        };
        let begin_status = unsafe {
            sai_rt_begin_transaction(self.runtime)
        };
        if begin_status != 0 {
            return Err(decode_runtime_error(
                self.runtime,
                begin_status,
            ));
        }
        let status = unsafe {
            sai_rt_invoke(self.runtime, entry, &mut result)
        };
        if status != 0 {
            unsafe { sai_rt_rollback_transaction(self.runtime) };
            return Err(decode_runtime_error(self.runtime, status));
        }

        // 回傳前先把字串複製到 Rust 自己管理的記憶體。
        let owned = match unsafe { decode_and_copy_value(&result) } {
            Ok(value) => value,
            Err(error) => {
                unsafe { sai_rt_rollback_transaction(self.runtime) };
                return Err(error);
            }
        };
        unsafe { sai_rt_commit_transaction(self.runtime) };

        // 全域變數可能已保存指向這個引擎內部資料或程式碼的指標。
        self.engines.push(engine);
        Ok(owned)
    }
}
```

建立第一個工作階段前，先呼叫一次 `Target::initialize_native(&InitializationConfig::default())` 初始化本機平台支援。

上例使用 `get_function_address`，因為還要把入口位址交給 C 的 `sai_rt_invoke`。將位址轉成函式指標，以及呼叫 C 介面的地方，都應集中在小範圍的 `unsafe` 區塊，逐一確認型別正確。

`module.verify()` 只檢查 LLVM 模組，不會檢查 Rust 宣告的 `Entry` 是否和機器碼一致。因此只能透過第 15 節固定的入口 ABI 呼叫，不能隨意轉成其他函式型別。

**成功執行後，不能立刻丟掉 `ExecutionEngine`。** 回傳的字串可能指向模組常數，閉包可能指向 JIT 機器碼，全域變數也可能保存這些指標。若引擎已被釋放，資料卻還在使用，就會出錯。

因此，把成功的引擎放進 `JitSession.engines`，等整個工作階段結束再釋放。即使只執行一次，也要趁引擎還存在時，把字串複製成 Rust 自己管理的資料。函式結果只能先顯示成 `<function>`；若要留著之後呼叫，就必須連同所依賴的引擎或工作階段一起保存。

### 17.3 讓 JIT 找到執行環境函式

機器碼要呼叫 `sai_rt_str_eq` 等函式，JIT 必須先知道這些名稱對應的位址。這種可供連結或查找的名稱稱為符號（symbol）。建立或執行引擎前，先載入共用執行環境：

```rust
load_library_permanently(Path::new(
    "build/runtime/libsai_runtime.so",
))?;
```

若仍找不到符號，可以明確登記名稱和位址的對應關係：

```rust
let declaration = module
    .get_function("sai_rt_str_eq")
    .ok_or_else(|| BackendError::InvalidHir(
        "missing runtime declaration".into(),
    ))?;
engine.add_global_mapping(
    &declaration,
    sai_rt_str_eq as usize,
);
```

每個被登記的本機函式都必須使用 `extern "C"`，而且參數與回傳型別要完全符合 LLVM 的宣告。

### 17.4 第一版 REPL：重新編譯所有已知函式

若每次只編譯新增的函式，就得處理新舊模組如何互相呼叫、同名函式如何更新等問題。第一版先採用較容易做對的方法：

1. 前端保存所有成功輸入的 HIR，以及已知名稱和型別。
2. 收到新輸入時，在暫存副本上檢查型別。
3. 把目前已知的所有函式重新產生為一份新 LLVM 模組。
4. 入口只執行本次輸入的最外層程式，不重跑舊宣告的初始值計算。
5. 檢查模組並建立新的執行引擎。
6. 執行本次輸入的入口。
7. 成功才一起保存前端資料和執行結果，並保留引擎。

這種方式每次要編譯較多內容，但新模組能找到所有已知函式，比較容易做對。重新編譯時，`FunctionId`、`GlobalId` 和全域儲存位置都必須保持不變。

尤其要區分「重新編譯函式」和「重新執行舊程式」。例如 `x` 原本是 1，後來被改成 5，下次輸入不能又重跑宣告，把它重設成 1。舊引擎也要繼續保留，因為現有變數中可能還有指向舊機器碼或字串常數的值。

### 17.5 進階 REPL：只編譯新增的部分

基本版本穩定後，再考慮增量編譯，也就是每次只編譯新增部分。需要安排以下規則：

- 每次輸入使用不重複的模組名稱。
- 入口名稱使用 `__sai_repl_<submission_id>_entry`。
- 函式的 LLVM 名稱包含版本，避免新舊版本互相覆蓋。
- 最外層可修改變數放在執行環境管理的固定儲存格。
- 函式名稱對應到固定的閉包儲存位置。
- 舊模組保留到工作階段結束。
- 把每次原始碼存成 `<repl:0001>`、`<repl:0002>` 等來源，出錯時才能指出是哪次輸入。

教學版仍不卸載舊模組，因為閉包的函式位址、字串資料位址，以及執行環境保存的值，都可能指向它。新編譯的函式也可能直接呼叫舊版本。

第一版可以禁止在同一作用範圍重複定義函式。若要允許 `def` 重新定義，較簡單的規則是要求參數與回傳型別完全相同，再更新固定位置裡的閉包值。

若連型別都允許改變，就要替每次定義編版本，並決定舊閉包會看到哪一版。這項規則必須先想清楚，否則舊機器碼可能用錯誤的型別呼叫新函式。

### 17.6 一次輸入成功後，才正式保存

最後把前端、編譯與執行串成同一個交易：

```text
準備前端暫存資料
  -> 產生 LLVM 模組
  -> 檢查模組
  -> 找到執行環境函式的位址
  -> 執行入口，讓本次修改仍可撤銷
  -> 成功：一起保存名稱、型別與執行結果
  -> 失敗：捨棄暫存資料，還原本次修改
```

如果不採用前面的備份或寫入紀錄方案，也可以另外規定「執行出錯後，重建整個 JIT 工作階段」。但這會改變使用者看到的錯誤復原行為，必須一併調整測試。無論採用哪種做法，都不能讓前端記住某個名稱，執行環境卻沒有對應的值。

本節驗收：成功輸入的名稱、可修改全域值、閉包與字串都能在下次輸入繼續使用；失敗輸入不留下新名稱，也能還原對舊閉包共用儲存格的修改。

```text
successful_submission_is_visible_later
failed_submission_does_not_define_names
mutable_global_survives_submissions
closure_survives_submissions
old_string_literal_remains_valid
runtime_error_does_not_corrupt_session
failed_submission_rolls_back_existing_mutable_capture
```

## 18. 出錯時，告訴使用者哪裡出了問題

錯誤訊息（diagnostic）不應只說「失敗」。它應指出問題發生在哪裡，以及是原始碼有誤、編譯器有錯，還是環境設定不完整。先依階段分類：

| 階段 | 範例 | 分類 |
|---|---|---|
| 詞法分析 | 字串少了結尾引號 | 使用者的原始碼錯誤 |
| 語法分析 | 程式結構不符合語法 | 使用者的原始碼錯誤 |
| 型別檢查 | 需要整數的位置卻放了字串 | 使用者的原始碼錯誤 |
| 執行時檢查 | 整數除以零 | Sai 程式的執行錯誤 |
| HIR 檢查 | 變數編號不存在 | 編譯器內部錯誤 |
| LLVM 檢查 | 區塊缺少結束指令 | 編譯器內部錯誤 |
| 目標機器設定 | 不支援選定的平台 | 後端或環境設定錯誤 |
| 連結 | 找不到執行環境函式 | 後端或環境設定錯誤 |

LLVM 檢查失敗，代表產生的 IR 有問題，不應假裝是使用者的型別錯誤。訊息至少包含：

- `internal compiler error`，清楚標示為編譯器內部錯誤。
- 出問題的 LLVM 函式或模組名稱。
- 對應的原始碼位置或範圍。
- LLVM 檢查器提供的錯誤文字。
- 若有保存 `.ll`，附上檔案路徑，方便重現問題。

執行時的檢查則保存簡短的位置編號 `span_id`，呼叫端再查表還原完整位置：

```text
span_id -> 原始碼檔名、起始行與欄、結束行與欄
```

呼叫：

```llvm
call void @sai_rt_raise(
  ptr %runtime,
  i32 %division_by_zero,
  i32 %span_id
)
unreachable
```

配合第 13 節的錯誤處理設計，`sai_rt_raise` 必須標成 `noreturn`，而 JIT/AOT 入口要透過 `sai_rt_invoke` 執行，先設好錯誤返回點。這樣出錯時才會跳回呼叫端，而不會繼續執行 `unreachable`。

JIT 工作階段也要保存舊輸入的原始碼。否則舊閉包出錯時，雖然知道行號，卻無法顯示那一行寫了什麼。

## 19. 比較直譯、JIT 與 AOT 的結果

這種把同一個案例交給不同實作，再比較結果的方法，稱為差異測試（differential testing）。它比只檢查 `.ll` 裡有沒有某段文字，更能回答重要的問題：編譯後的程式真的做對了嗎？

每個測試程式只經過一次前端，得到同一份 HIR，再分別交給三種執行方式：

```text
fixture.sai
  -> 前端檢查一次，產生 HIR
       -> HIR 直譯器
       -> LLVM JIT
       -> LLVM AOT，另啟動一個程序執行
```

為了方便比較，把結果整理成共同格式：成功的值、編譯錯誤，或執行錯誤。

```rust
enum Observable {
    Value(NormalizedValue),
    CompileError(Vec<NormalizedDiagnostic>),
    RuntimeError(NormalizedRuntimeError),
}
```

比較規則：

- 整數、布林值和字串，直接比較值或內容。
- 浮點數優先比較 IEEE-754 的原始位元，才能分辨正零、負零等細節。若未來規格要求把不同 NaN 統一成同一表示，再調整比較規則。
- 函式值先統一表示為 `<function>`，不比較函式或環境位址，因為每種執行方式的位址本來就可能不同。函式行為則透過實際呼叫的案例測試。
- AOT 另啟動程序執行，比較標準輸出（stdout）、標準錯誤輸出（stderr）和結束狀態。
- 執行錯誤要由 `sai_rt_invoke` 帶回錯誤資料，不能讓一個除以零案例直接終止整個 Rust 測試程序。

建議準備以下測試用程式檔（fixtures），每個檔案專注於一項行為：

```text
tests/fixtures/
├── integer_arithmetic.sai
├── float_arithmetic.sai
├── comparisons.sai
├── block_scope.sai
├── mutable_local.sai
├── if_value.sai
├── short_circuit.sai
├── recursion.sai
├── string_equality.sai
├── closure_capture.sai
├── mutable_capture.sai
├── nested_closure.sai
└── division_by_zero.sai
```

至少要確認下列案例在各種執行方式下都符合預期：

| 類別 | 輸入 | 預期 |
|---|---|---|
| 整數 | `(+ 1 2 3)` | `6` |
| 浮點數 | `(/ 9.0 2.0)` | `4.5` |
| 布林值 | `(not false)` | `true` |
| 字串 | `(= "台灣" "台灣")` | `true` |
| 空字串 | `(= "" "")` | `true` |
| 條件判斷 | `(if true 1 :else 2)` | `1` |
| 短路求值 | `(or true (= (/ 1 0) 0))` | `true`，不得出錯 |
| 遞迴 | `(fib 10)` | `55` |
| 捕捉可修改變數 | 閉包更新 `x` | 外層讀到新值 |
| 執行錯誤 | `(/ 1 0)` | 回傳可辨識種類和位置的除法錯誤 |

### 19.1 檢查 LLVM IR 的關鍵結構

LLVM 小版本更新也可能改變指令順序和名稱，所以不要要求整份最佳化後的 IR 文字完全相同。針對重要規則檢查即可；分支與 PHI 等結構適合在最佳化前檢查，因為最佳化可能合法地改寫它們：

- 模組通過 LLVM 檢查。
- 會回傳資料值的 `if` 有正確的 PHI。
- 短路求值使用條件分支。
- 直接遞迴呼叫指向正確函式。
- 字串相等比較使用執行環境的比較函式。
- 閉包函式有 `runtime` 和 `environment` 這兩個額外參數。
- 可修改的捕捉資料保存共用儲存格指標。

另外保留少量 `-O0` 下的參考 IR 檔案，供教學與除錯對照即可。

### 19.2 建議命令

```bash
cargo test --locked
cargo test --locked --features llvm
cargo fmt --all -- --check
cargo clippy --locked --all-targets --features llvm -- -D warnings
```

檢查產生的 IR，並嘗試轉成目的檔：

```bash
llvm-as-21 build/test.ll -o build/test.bc
opt-21 -passes=verify build/test.bc -disable-output
llc-21 -filetype=obj build/test.bc -o build/test.o
```

連結並執行 AOT 程式：

```bash
clang-21 build/test.o build/runtime/libsai_runtime.a -o build/test
./build/test
```

## 20. 建議實作順序

下面把前面的工作整理成可勾選的清單。先完成前端和基本指令，再加入函式、執行環境與閉包，最後串接 AOT/JIT。各節列出的英文測試名稱可直接作為 Rust 測試函式的名稱。

### 第一步：讓前端確實擋下錯誤

- [ ] 加入語法分析、型別檢查與直譯器測試。
- [ ] 修正閉包型別，並確認函式內容符合宣告的回傳型別。
- [ ] 修正 `if` 條件、分支型別，以及沒有 `:else` 時的規則。
- [ ] 修正空區塊和最後一個運算式重複檢查的問題。
- [ ] 修正作用範圍，讓參數不會意外出現在函式外。
- [ ] 移除或完成 `len`、`nth`。
- [ ] 決定整數溢位和除法錯誤的處理規則。
- [ ] 讓失敗輸入不會留下部分新名稱。
- [ ] 決定正式的函式簽章和註解語法，讓 README 與語法分析器一致。
- [ ] 實作並測試 `fn-type` 型別標註。

### 第二步：保存檢查完成的 HIR

- [ ] 為變數、全域變數和函式建立各自的編號。
- [ ] 每個運算式都保存確定的型別。
- [ ] 把內建操作換成已知型別的操作種類，例如 `AddInt`。
- [ ] 把 `elif`、`and`、`or` 整理成統一的條件判斷形式。
- [ ] 分析閉包需要捕捉的變數。
- [ ] 建立 HIR 檢查器。
- [ ] 讓直譯器改為執行 HIR。
- [ ] 讓直譯器的儲存資料可撤銷修改，且複製後仍保持變數共用關係。

### 第三步：產生基本 LLVM 指令

- [ ] 加入 LLVM 21 與 Inkwell 0.9。
- [ ] 建立後端模組和錯誤型別。
- [ ] 轉換 `Int`、`Float`、`Bool`、`Unit`。
- [ ] 產生算術與比較指令。
- [ ] 處理區域變數、區塊和賦值。
- [ ] 處理 `if`、`not` 與短路求值。
- [ ] 每個模組都通過 LLVM 檢查。

### 第四步：加入函式

- [ ] 先宣告所有合法函式，再產生函式內容。
- [ ] 支援函式呼叫自己。
- [ ] 統一執行環境、捕捉資料與使用者參數的順序。

### 第五步：加入字串與執行環境

- [ ] 固定 `%SaiString` 的欄位排列。
- [ ] 把字串常值放進模組常數。
- [ ] 定義共用的執行環境 ABI。
- [ ] 實作 `sai_rt_raise`、`sai_rt_invoke`，並把錯誤代碼轉成錯誤資料。
- [ ] 由執行環境管理全域儲存位置，並檢查大小與對齊。
- [ ] 讓 AOT 和 JIT 使用同一套執行環境規則。
- [ ] 實作字串相等比較與值的顯示。

### 第六步：加入閉包

- [ ] 固定 `%SaiClosure` 的欄位排列。
- [ ] 把閉包內容建立成獨立的 LLVM 函式。
- [ ] 不能修改的捕捉變數，複製值。
- [ ] 可以修改的捕捉變數，使用共用儲存格。
- [ ] 巢狀閉包，以及離開原函式後仍會使用的閉包，都有足夠長期的堆積區環境。
- [ ] 間接呼叫使用 HIR 記錄的函式簽章。

### 第七步：完成 AOT 與單次 JIT 執行

- [ ] 設定目標平台和資料排列方式。
- [ ] 輸出 `.ll` 與 `.o`。
- [ ] 用 Clang 安排連結，產生執行檔。
- [ ] 用固定入口 ABI 執行 JIT。

### 第八步：讓 JIT REPL 記住前面的輸入

- [ ] 每次輸入有不重複的來源、模組和入口編號。
- [ ] 前端和執行環境一起保存成功結果，或一起撤銷修改。
- [ ] 撤銷時還原全域值與既有共用儲存格，並回收本次新增配置。
- [ ] 全域變數和可修改儲存格都有固定的儲存位置。
- [ ] 舊模組和原始碼保留到工作階段結束。
- [ ] `JitSession` 保存所有仍可能被使用的執行引擎。
- [ ] 決定函式能否重新定義，以及如何處理舊版本。
- [ ] 用多次連續輸入，比較直譯器和 JIT 的行為。

## 21. 常見問題

| 症狀 | 原因 | 修正 |
|---|---|---|
| `No suitable LLVM version` | `llvm-sys` 找不到 LLVM 21 | 設定 `LLVM_SYS_211_PREFIX=/usr/lib/llvm-21` |
| 大量 `undefined references`，找不到 LLVM 函式 | 錯用 `no-llvm-linking`，略過了 LLVM 連結 | 改用 `llvm21-1-prefer-dynamic` |
| Builder 回傳錯誤 | 還沒設定插入位置，或區塊已經結束 | 先定位 builder，並檢查區塊是否已有結束指令 |
| `block has no terminator` | 某條路沒有 `br`、`ret` 等結束指令 | 檢查每條分支的最後一個區塊 |
| `PHI predecessor mismatch` | PHI 記的是分支起點，而不是實際連到會合點的區塊 | 產生完分支後，重新取得目前區塊 |
| `call parameter mismatch` | 函式兩端的參數規則不一致 | 統一 `runtime`、`env` 和使用者參數的順序與型別 |
| 對 `void` 呼叫取值時失敗 | `Unit` 沒有一般資料值，卻呼叫了 `unwrap_basic()` | 回傳 `GeneratedValue::Unit` |
| 最佳化後才出錯 | 非法除法、溢位假設或未初始化資料，讓結果不再有保證 | 明確檢查非法運算，確認初始值，並在最佳化前後檢查 IR |
| 內容相同的字串卻不相等 | 比較了位址 | 改為比較長度與每個位元組 |
| 中文字串長度看似錯誤 | 內部格式記的是位元組數 | 先決定 `len` 要數的是位元組、碼位還是字形 |
| 閉包偶爾讓程式崩潰 | 捕捉資料指向已結束函式的堆疊空間 | 可能在函式外繼續使用的環境，放在堆積區 |
| 閉包修改後，外層值沒變 | 閉包改的是複製出的值 | 宣告時建立共用儲存格，閉包保存其指標 |
| 兩個閉包看到不同的計數值 | 各自建立了儲存格 | 同一個變數只在宣告處建立一次儲存格 |
| JIT 找不到執行環境函式 | 未載入共用函式庫，或函式未開放給外部查找 | 使用 `load_library_permanently` 或 `add_global_mapping` |
| JIT 第二次輸入找不到舊名稱 | 沒有保存已知函式及全域資料 | 先重新編譯所有已知函式，再考慮增量編譯 |
| 新輸入呼叫舊閉包時崩潰 | 舊模組已被釋放 | 保留可能仍被使用的模組到工作階段結束 |
| 輸入失敗後，新名稱卻留下來 | 前端太早正式保存 | 使用先 `prepare`、成功才 `commit` 的交易流程 |
| REPL 開久後記憶體一直增加 | 教學版把資料保留到工作階段結束 | 正式版本需加入參考計數、垃圾回收或資源限制 |

## 22. 怎樣才算完成第一版？

不只讓一個加法範例跑出答案，而是同時做到以下各點，才算完成第一版 LLVM 後端：

- 前端產生的 HIR 已記錄型別、解析名稱，並通過檢查。
- 直譯器與 LLVM 後端使用同一份 HIR。
- 後端不再用字串名稱重新查找變數與函式。
- 每份 LLVM 模組在最佳化前後都通過檢查。
- AOT 能輸出目的檔，再由 Clang 連結成執行檔。
- JIT 的 `unsafe` 集中在少數可逐一檢查的介面操作中，並遵守固定 ABI。
- 整數、浮點數、布林值、`unit`、字串、函式與閉包都有明確的資料格式和傳遞規則。
- `if` 選對分支，`and`、`or` 能在答案確定後停止求值。
- 可修改的捕捉變數使用共用儲存格。
- 非法整數除法會回報錯誤，不會進入 LLVM 的未定義行為。
- REPL 輸入失敗後，不會留下影響後續輸入的部分修改。
- 直譯器、JIT 和 AOT 的差異測試通過。
- 每個實作階段都有可重現的指令和測試程式。

完成後再逐步處理正式使用需要的功能，例如讓除錯器能對應原始碼、支援更多平台、改用 ORC 做增量 JIT、加入語言模組系統，以及自動回收記憶體。執行時錯誤偵測工具、連結時最佳化（LTO）和在本機編譯其他平台程式的交叉編譯，也可以留待後續專案。

## 23. 參考資料

- [Inkwell 0.9.0 API](https://docs.rs/inkwell/0.9.0/inkwell/)
- [Inkwell 0.9.0 features](https://docs.rs/crate/inkwell/0.9.0/features)
- [apt.llvm.org](https://apt.llvm.org/)
- [LLVM Language Reference](https://llvm.org/docs/LangRef.html)
- [LLVM Kaleidoscope Tutorial](https://llvm.org/docs/tutorial/MyFirstLanguageFrontend/index.html)
- [LLVM New Pass Manager](https://llvm.org/docs/NewPassManager.html)
- [LLVM ORCv2 Design](https://llvm.org/docs/ORCv2.html)

第一次查資料時，可以先讀 Kaleidoscope 教學理解整體流程，再用 LLVM Language Reference 查指令規則，用 Inkwell API 文件查 Rust 方法的實際參數與回傳型別。

Kaleidoscope 使用 C++，LLVM 版本也可能與本文不同，而且沒有完整處理 Sai 這裡的帶型別閉包、共用可修改變數與跨次輸入的 REPL 資料。適合借用的是中間表示、控制流程圖（CFG）、PHI、JIT 和目的檔輸出的觀念；名稱管理與 AST 轉換架構仍要配合 Sai 的需求設計。
