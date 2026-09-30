# 程式語言 Sai

## 安裝

```
git clone git@github.com:hakozhi/sai.git
cargo install --path .
```

## 使用


```bash
# 執行 repl 
sai
sai <程式檔案>
```

## 語法

### 變數宣告

```lisp
(let [string name] "Yubai") ;; This line creates a string constant named 'name'
;; Variables are immutable by default.
(set! name "Bill") ;; This will raise an exception!!

(mut [int age] 18) ;; Use 'mut' to create mutable variables
(set! age 19) ;; This is legal.
```

### 一般資料形態

```lisp
19 ;; int
114.514 ;; float
"Hello, Sai" ;; string
true ;; boolean
```

### 函式

```lisp
(def fib ([int n] int)
    (if (< n 2)
        n
    :else
        (+ (fib (- n 2))
           (fib (- n 1)))
    )
)

;; lambda functions
((fn ([int a] int) a) 2) ;; 2
```

### 條件

```lisp
(or (> 2 1) (< 2 1))
(and true false)

(if (> 2 1)
  2
:else
  1
) 

(let [string name] "Donald")
(if (= name "Jack")
  "Jack"
:elif (= name "William")
  "William"
:elif (= name "Truls")
  "Truls"
:else
  "Aaron"
)
```
