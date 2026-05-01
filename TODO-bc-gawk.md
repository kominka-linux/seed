# TODO: bc and awk for Linux kernel build

Both tools are needed to generate kernel headers. Currently worked around by
pre-generating on the host and vendoring into the package — but ideally seed
handles them natively.

## bc

### Invocation
```
echo 250 | bc -q kernel/time/timeconst.bc
```
- `-q` flag: suppress the welcome banner (GNU bc extension; POSIX has no such flag)
- positional argument: script file to execute
- stdin: provides the HZ value read by the script via `read()`

### Features used by `kernel/time/timeconst.bc`
The script uses GNU bc (not POSIX bc), specifically:

- `define name(params) { ... }` — function definitions with `auto` locals
- `return expr` — function return
- `while (cond) { ... }` — while loop
- `for (init; cond; step) { ... }` — for loop
- `if (cond) { ... } else { ... }` — conditionals
- Arrays: `a[i]` indexing (one-dimensional)
- `obase = 16` — change output base (hex output)
- `ibase = 10` — input base
- `scale = 0` — integer arithmetic (no fractional digits)
- `print expr, " ", expr, "\n"` — **GNU extension**; POSIX bc just prints the
  result of the last expression; the script uses `print` for formatted output
- `halt` — **GNU extension** to exit
- Arithmetic: `+`, `-`, `*`, `/`, `%`, `^`, unary `-`
- Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`
- Logical: `&&`, `||`, `!`
- Assignment: `=`, `+=`, `-=`, `*=`, `/=`, `%=`
- `length(expr)` — number of significant digits

### Minimum bc surface area needed
Implement GNU bc, not POSIX bc. The two blocking features are `print` and `halt`.
Everything else (`define`, `auto`, `while`, `for`, arrays, `obase`/`ibase`,
`scale`) is shared with POSIX bc but must be correct.

---

## awk

### Scripts
- `arch/arm64/tools/gen-cpucaps.awk` — reads `arch/arm64/tools/cpucaps`,
  emits `arch/arm64/include/generated/asm/cpucap-defs.h`
- `arch/arm64/tools/gen-sysreg.awk` — reads `arch/arm64/tools/sysreg` (~3 000
  lines), emits `arch/arm64/include/generated/asm/sysreg-defs.h` (~15 000 lines)

Both scripts are invoked as:
```
awk -f script.awk input_file
```

### Features used (POSIX awk, not gawk)

**Core language**
- `BEGIN { }` and `END { }` blocks
- Pattern/action rules: `/regex/ { }`, `condition { }`, `{ }` (default)
- `next` — skip to next record
- `exit [code]` — terminate
- `print` and `printf fmt, args...`
- String concatenation via adjacency: `a = b c`
- Ternary: `cond ? a : b`
- `in` operator: `if (key in arr)`
- `delete arr[key]`

**Built-in variables**
- `NR`, `NF`, `FS`, `RS`, `OFS`, `ORS`, `FILENAME`
- `$0`, `$1`…`$NF`

**String functions** (all POSIX)
- `length(s)` / `length(arr)`
- `substr(s, start)` / `substr(s, start, len)`
- `index(s, t)`
- `split(s, arr)` / `split(s, arr, sep)`
- `sub(re, repl)` / `sub(re, repl, target)`
- `gsub(re, repl)` / `gsub(re, repl, target)`
- `match(s, re)` — sets `RSTART`, `RLENGTH`
- `sprintf(fmt, ...)`
- `tolower(s)` / `toupper(s)`

**Arithmetic**
- `int(x)`, `sin`, `cos`, `log`, `exp`, `sqrt`, `atan2`, `rand`, `srand`
  (at minimum `int()` is used; others likely not needed but include for
  POSIX compliance)

**I/O**
- `getline` — read next record into `$0`
- `getline var` — read into var
- `print > "file"` / `print >> "file"` — output redirection
- `close("file")`

**gen-sysreg.awk specifics**
This is the more demanding script. Known to use:
- Multi-line records assembled by `getline` loops
- Complex `printf` with `%s`, `%d`, `%x`, `%-Ns` (left-justified width)
- Regex in conditions: `$0 ~ /pattern/`, `$0 !~ /pattern/`
- Associative arrays with string keys
- `delete` on individual array elements
- `OFMT` / `CONVFMT` (may be implicit via numeric→string conversion)
- `split()` with single-char FS override

### Likely seed awk gaps
Based on what gen-sysreg.awk exercises:
1. `match()` with `RSTART`/`RLENGTH` side-effects
2. `getline` (bare and `getline var`)
3. `delete arr[key]`
4. `$0 ~ /re/` and `$0 !~ /re/` as conditions in rules
5. Width specifiers in `printf` (`%-40s`, `%08x`, etc.)
6. `tolower` / `toupper`

Verify by running both scripts against their input files under seed awk and
checking diff against the vendored output in `packages/linux/files/`.
