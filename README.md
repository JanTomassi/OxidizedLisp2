# OxidizedLisp2

A small Lisp runtime written in Rust with:

- a parser built on `nom`
- an evaluator with a mutable environment (`Env`) and a set of built-in functions
- support for first-class user lambdas and lexical capture
- a command-line REPL powered by `rustyline` (history + line editing)

The crate name is currently `lisp_runtime_rust`.

## Table of contents

- [What this project is](#what-this-project-is)
- [Project layout and architecture](#project-layout-and-architecture)
- [Language model and runtime semantics](#language-model-and-runtime-semantics)
- [Built-in functions](#built-in-functions)
- [How to build](#how-to-build)
- [How to run](#how-to-run)
- [How to test](#how-to-test)
- [Code coverage](#code-coverage)
- [REPL command reference](#repl-command-reference)
- [Example expressions to try](#example-expressions-to-try)
- [Known limitations and behavior notes](#known-limitations-and-behavior-notes)

---

## What this project is

This repository implements a minimal Lisp interpreter runtime from scratch in Rust.

At a high level:

1. Input is parsed into an AST-like `Atom` structure.
2. S-expressions are represented as linked cons cells (`SExpr { car, cdr }`) terminated by `Nil`.
3. `eval` recursively evaluates expressions in an `Env` that contains:
   - value bindings (`val`)
   - function table (`fun`)
4. A REPL loop allows interactive evaluation and file loading.

This project is especially useful if you want to study:

- how to model Lisp data in Rust enums
- how to build list structures with `Arc`
- how lexical closures can be implemented with environment capture
- how special forms differ from normal function calls

---

## Project layout and architecture

### `src/main.rs`

Entry point and REPL host.

Responsibilities:

- initializes `ReplState` (`loaded_file`, `loaded_text`, `env`)
- optionally preloads a file passed as first CLI argument
- starts a `rustyline::DefaultEditor` loop
- dispatches either:
  - REPL commands (`:help`, `:load`, etc.), or
  - Lisp expressions, parsed and then evaluated
- persists history into `.myrepl_history`

### `src/atom.rs`

Defines the core runtime values:

- `Atom::T`, `Atom::Nil`
- `Atom::Num(f64)`
- `Atom::Str(String)`
- `Atom::Sym(String)`
- `Atom::Cons(SExpr)`
- `Atom::Fun(Arc<Fun>)`

Also defines function representation:

- `Fun::Native` for built-ins
- `Fun::User` for lambdas

The `SAtom` alias is `Arc<Atom>`, so values are reference-counted and cheap to clone.

### `src/sexpr.rs`

Defines cons cells (`SExpr`) and iteration over list structures.

- `SExpr` stores `car` and `cdr` as `SAtom`
- custom `Debug` formatting prints Lisp-like list syntax, including dotted tails
- `FromIterator` implementations build proper linked lists from vectors/iterators
- `SExprIter` walks proper lists until `Nil`

### `src/lisp_parsing.rs`

Parser layer using `nom`.

Parsers implemented:

- numbers (`double`)
- strings (`"..."`)
- symbols (`alpha1` + alnum/underscore tail)
- s-expressions (`(...)` with nested atom parsing)

`parse(input)` is the public convenience function and returns an `Atom`.

### `src/lisp_eval.rs`

Core evaluator.

- `eval(v, env)` handles symbols, lists, and literals.
- For list calls, dispatches function invocation logic.
- Supports calling built-ins via symbol lookup and calling function objects directly.
- Uses `Args` enum (`S(&SExpr)` / `Nil`) for function argument passing.

Important semantics:

- normal function calls evaluate arguments before passing
- special forms (`lambda`, `quote`, `if`) suppress default eager argument evaluation

### `src/env.rs`

Defines the runtime environment and registers built-in functions.

`Env` fields:

- `val: HashMap<String, SAtom>`: variables and constants (`nil`, `t`)
- `fun: Arc<HashMap<String, Fun>>`: built-ins by name

Contains helpers for argument counting/extraction, numeric coercion, and all built-in implementations.

### `src/easy_cons.rs`

Utility macros to make test/runtime expression construction concise:

- `num!`, `str!`, `sym!`, `nil!`, `t!`
- `sexpr!` for list construction
- `cons!` for direct pair construction

---

## Language model and runtime semantics

### Data model

The interpreter treats everything as an `Atom`.

- Lists are chains of `Cons` cells.
- Empty list is `Nil`.
- Truthiness: `Nil` is false; anything else is true (as used by `if`).

### Function calls

For a list like `(f a b)`:

1. Evaluate/resolve `f`.
2. Determine whether args should be pre-evaluated:
   - **No pre-eval** for `lambda`, `quote`, `if`.
   - **Pre-eval** for other built-ins and common calls.
3. Convert list tail to `Args` and invoke `Fun::call`.

### Lambdas and lexical capture

`lambda` creates `Fun::User` with:

- captured environment snapshot (`captured_env`)
- parameter list
- body expression

On invocation, implementation temporarily swaps `call_state.val` to the captured lexical scope, binds params to received argument values, evaluates body, and restores prior caller bindings.

### Apply and funcall

- `apply`: takes a callable and argument expressions list tail and invokes it.
- `funcall`: similar dynamic call path (included as separate built-in).

---

## Built-in functions

### Arithmetic

- `(add a b ...)` - fold add, at least 2 args
- `(sub a b ...)` - fold subtraction
- `(mul a b ...)` - fold multiply
- `(div a b ...)` - fold divide

### List operations

- `(list x y ...)` - constructs a proper list
- `(cons a b)` - constructs a cons pair
- `(car list-or-symbol)` - first element
- `(cdr list-or-symbol)` - tail

### Control and comparison

- `(quote x)` - returns x without evaluating it
- `(if test then else)` - conditional
- `(eq x y)` - structural/value equality check (`T` or `Nil`)

### Functions

- `(lambda (params...) body)` - creates user function
- `(apply fun arg1 arg2 ...)` - invoke callable
- `(funcall fun arg1 arg2 ...)` - invoke callable

---

## How to build

### Prerequisites

- Rust toolchain (recommended stable)
- Cargo

Check versions:

```bash
rustc --version
cargo --version
```

### Debug build

```bash
cargo build
```

Binary path:

```text
target/debug/lisp_runtime_rust
```

### Release build

```bash
cargo build --release
```

Binary path:

```text
target/release/lisp_runtime_rust
```

---

## How to run

### Start REPL

```bash
cargo run
```

You should get a prompt:

```text
>
```

### Start REPL and preload a file

```bash
cargo run -- path/to/program.lisp
```

This loads and evaluates the file before interactive mode starts.

---

## How to test

Run all unit tests:

```bash
cargo test
```

The repository contains tests for parser and evaluator behavior, including:

- symbol/number/string parsing
- arithmetic and nested evaluation
- list operations (`car`, `cdr`, `cons`, `list`)
- quoting
- conditionals
- lambda calls and recursive patterns (via self-application)

Optional strict checks:

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

---


## Code coverage

This repository includes an automated coverage workflow at:

```text
.github/workflows/coverage.yml
```

It uses [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) to generate:

- a terminal summary (`--summary-only`)
- an `lcov.info` file (uploaded as a CI artifact)
- optional upload to Codecov (`fail_ci_if_error: false`)

### Run coverage locally

1. Install the cargo subcommand once:

```bash
cargo install cargo-llvm-cov
```

2. Generate a local summary + LCOV report:

```bash
cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info --summary-only
```

3. (Optional) Open HTML coverage report locally:

```bash
cargo llvm-cov --workspace --all-features --open
```

### Notes

- `cargo llvm-cov` automatically sets the required LLVM coverage flags for Rust builds.
- If you only want the numeric terminal summary, keep `--summary-only`.
- If you want machine-consumable output for external services, use the generated `lcov.info`.

---


## REPL command reference

The REPL supports colon-prefixed host commands:

- `:help` - prints command help
- `:q` or `:quit` - exits the REPL
- `:load <path>` - reads file contents and evaluates them in current env
- `:show` - displays currently loaded file text
- `:clear` - clears loaded file metadata from REPL state

Any line not starting with `:` is treated as a Lisp expression.

---

## Example expressions to try

You can paste these directly into the REPL.

### 1) Basic arithmetic

```lisp
(add 3 4 5)
(sub 10 1 2)
(mul 2 3 4)
(div 20 2 2)
```

### 2) Nested arithmetic

```lisp
(add (mul 3 4) 5)
(mul (add 3 4) (sub 9 1))
```

### 3) Quoting and list construction

```lisp
(quote a)
(quote (1 2 3))
(list 1 2 3)
(cons 1 2)
(cons (list 1 2) (list 3 4))
```

### 4) `car` / `cdr`

```lisp
(car (list 1 2 3))
(cdr (list 1 2 3))
(car (cdr (list 1 2 3)))
```

### 5) Conditionals and equality

```lisp
(eq 1 1)
(eq (list 1 2) (quote (1 2)))
(if (eq 1 1) "yes" "no")
(if (eq 1 2) "yes" "no")
```

### 6) Lambdas and apply

```lisp
(apply (lambda (a b) (add a b)) 1 2)
(apply (lambda () (car (list "good"))) )
(apply (lambda (fun) (apply fun 1)) (lambda (n) (add 1 n)))
```

### 7) Recursive style via self-application (Fibonacci)

```lisp
((lambda (n)
   ((lambda (FIB)
      (apply FIB FIB n))
    (lambda (FIB n)
      (if (eq n 0)
          0
          (if (eq n 1)
              1
              (add (apply FIB FIB (sub n 1))
                   (apply FIB FIB (sub n 2))))))))
 10)
```

Expected result is `55`.

---

## Known limitations and behavior notes

- Parser symbol rule currently requires the first character to be alphabetic; symbols starting with `-`, `+`, `?`, etc. are not accepted.
- String parser uses a simple quoted form and does not implement advanced escaping behavior.
- Several internal paths still contain `todo!()` / `expect(...)`, so invalid input may panic in edge cases.
- Evaluator currently prints debug trace output (`eval: ...`) for each call.
- Error messages are intentionally simple and are plain static strings.
