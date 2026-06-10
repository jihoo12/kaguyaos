# CC Syntax Reference

This document describes the syntax accepted by the tiny C JIT compiler (`src/cc/`).

All values are **`uint64_t`** (unsigned 64-bit integers). There are no other types.

---

## Program Structure

A program is one or more **function definitions**. Exactly one function must be named `main`; it is the entry point that gets executed.

```c
<type> <name>(<params>) {
    <statements>
}
```

### Example

```c
uint64_t helper(uint64_t x) {
    return x;
}

uint64_t main() {
    return helper(42);
}
```

---

## Functions

### Definition

```
<return_type> <function_name>(<param_list>) { <body> }
```

- **`<return_type>`** — any identifier (e.g. `uint64_t`). Parsed but not enforced; all values are u64.
- **`<function_name>`** — any identifier. Must be unique across the program.
- **`<param_list>`** — zero or more comma-separated parameters (max 6):
  - Empty: `()`
  - With params: `(uint64_t a, uint64_t b)`
- **`<body>`** — zero or more statements.

Parameters follow the **System V AMD64 ABI** and are passed via registers: `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`.

### Calling

```c
<function_name>(<arg_list>)
```

- **`<arg_list>`** — zero or more comma-separated expressions (max 6).
- A function call is an **expression** and can appear anywhere an expression is expected.

---

## Statements

Every statement ends with a semicolon (`;`).

### Variable Declaration

```c
<type> <name> = <expr>;
```

Declares a new local variable and initializes it.

```c
uint64_t x = 10;
uint64_t y = add(3, 4);
```

### Variable Assignment

```c
<name> = <expr>;
```

Assigns a new value to a previously declared variable (or parameter).

```c
x = 20;
x = foo();
```

### Return

```c
return <expr>;
```

Returns a value from the current function. The return value is placed in `rax`.

```c
return 42;
return x;
return add(1, 2);
```

### Inline Assembly

```c
asm("<assembly>");
__asm__("<assembly>");
```

Embeds raw x86-64 assembly into the output. The string is parsed by the built-in `tinyasm` assembler. Multiple instructions can be separated by `;` or newlines inside the string.

```c
asm("mov rax, 99; ret");
__asm__("nop");
```

---

## Expressions

An expression evaluates to a `uint64_t` value (placed in `rax`).

| Form | Description | Example |
|------|-------------|---------|
| `<number>` | Unsigned 64-bit integer literal (decimal) | `42` |
| `<name>` | Variable / parameter reference | `x` |
| `<name>(<args>)` | Function call with 0–6 arguments | `add(1, 2)` |

> **Note:** There are no arithmetic operators (`+`, `-`, `*`, `/`, etc.). Computation must be done via inline assembly or helper functions.

---

## Tokens

The lexer recognizes the following tokens:

| Token | Pattern |
|-------|---------|
| Identifier | `[a-zA-Z_][a-zA-Z0-9_]*` |
| Number | `[0-9]+` (parsed as `u64`) |
| String literal | `"..."` with escape sequences `\n \r \t \\ \"` |
| `(` | Left parenthesis |
| `)` | Right parenthesis |
| `{` | Left brace |
| `}` | Right brace |
| `;` | Semicolon |
| `,` | Comma |
| `=` | Equals (assignment) |
| `return` | Keyword (reserved identifier) |

Whitespace (spaces, tabs, newlines) is ignored between tokens.

---

## Limitations

- **Only `uint64_t` values.** No other types, no pointers, no structs, no arrays.
- **No arithmetic operators.** Use `asm(...)` for arithmetic.
- **No control flow.** No `if`, `while`, `for`, `goto`, etc.
- **Max 6 function parameters** (System V AMD64 ABI register limit).
- **No forward declarations.** All functions are resolved after parsing, so order doesn't matter.
- **No comments.** `//` and `/* */` are not supported.
- **Duplicate function names** are rejected.

---

## Full Example

```c
uint64_t add(uint64_t a, uint64_t b) {
    asm("mov rax, [rbp-8]; add rax, [rbp-16]");
    return a;
}

uint64_t main() {
    uint64_t x = 10;
    uint64_t y = 20;
    uint64_t result = add(x, y);
    return result;
}
```
