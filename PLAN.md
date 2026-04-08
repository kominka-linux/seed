# seed — Rust Busybox Replacement

Multi-call binary replacing the busybox applets used by `~/d/davinci/pm.ysh`
(a package manager written in ysh/Oils).

Implements the full flag set from busybox 1.37.0 for each included applet.
Flags marked **[pm]** are exercised by pm.ysh / Dockerfile.base — these
are the critical path. All other flags should also be implemented for
general-purpose use but are lower priority.

Busybox source is at `~/d/busybox` for reference. We should be writing
higher-quality code than busybox, not just translating C to Rust.

## Principles

**Shared foundation.** Build a common library of filesystem operations
(recursive traversal, copy/move with symlink handling, permission
manipulation, path canonicalization), I/O primitives (buffered
read/write, line iteration, stdin/stdout/file abstraction), and
argument parsing. Applets are thin wrappers over shared code. If two
applets do something similar, extract it — don't duplicate.

**Minimize allocations.** Prefer streaming and fixed-size buffers over
collecting into `Vec`s. Pipe data through `BufReader`/`BufWriter` in
reasonably sized chunks (8-64 KiB). Never read an entire file into
memory when line-by-line or block-by-block processing will do. For
sort, read lines into a Vec only because you must; everywhere else,
stream.

**Simplicity.** The right implementation is the shortest correct one.
No trait hierarchies for the sake of abstraction. No builder patterns
where a function with arguments works. If a match statement is clearer
than a HashMap, use the match. Busybox is simple because it has to be;
we should be simple because we choose to be.

**Strong types.** Use enums for modes and options, not raw strings or
booleans. File types, sort keys, compression formats, tar entry types,
find predicates — all should be types the compiler can check. Parse
arguments into a typed options struct per applet, then pass that struct
into the implementation. No stringly-typed flag checking deep in logic.

**Zero-copy I/O.** When piping data between stages (decompress →
tar extract, download → file write), pass byte slices through shared
buffers. Don't materialize intermediate representations unless the
algorithm demands it.

## Phases

Each phase ends with a testable milestone. Pass the busybox tests for
each applet before moving on. Tests are in `tests/busybox/`.

## Status

As of 2026-04-08:

- Phase 1 is complete.
- Phase 2 is complete for the implemented applets.
- Phase 3 has a working baseline implementation, but is not complete
  against the full planned busybox surface.

Implemented applets so far:

- `cat`
- `chmod`
- `cp`
- `diff`
- `grep`
- `mkdir`
- `mv`
- `od`
- `printf`
- `rm`
- `rmdir`
- `sort`
- `tee`
- `wc`

Current shared foundation:

- multi-call dispatch via `argv[0]` and `seed <applet>`
- `common::fs` for copy/move/remove primitives
- `common::io` for stream copying and stdin/stdout/file helpers
- `common::runtime` for signal handling and argv collection
- `common::applet` for applet result plumbing

Current verification floor:

- `cargo clippy --all-targets --all-features -- -D warnings`
- `make test`
- `make pc`

Important caveat:

- Some Phase 3 busybox tests are still intentionally skipped in the
  local runner because the corresponding option surface is not fully
  implemented yet. In particular, `sort`, `grep`, `diff`, and `od`
  still have skipped feature buckets.

### Phase 1 — Scaffold + cat

Set up the multi-call binary skeleton: `main()` dispatches based on
`argv[0]` (or `seed <applet>` form). Implement `cat` with all flags.
Pass `tests/busybox/cat.tests` and the old-style tests in `tests/busybox/cat/`.

Deliverable: `cargo build` produces a single binary that works as `cat`.

Status: complete.

### Phase 2 — File operations

Implement: `cp`, `mv`, `rm`, `rmdir`, `mkdir`, `chmod`.
Pass all busybox tests for each. This phase builds the shared
filesystem module that later applets depend on.

Status: implemented and currently passing the local runner for `cp`,
`mv`, `rm`, `rmdir`, and `mkdir`. `chmod` is implemented, but there is
no bundled busybox coverage here, so it has less validation than the
other Phase 2 applets.

### Phase 3 — Text processing

Implement: `grep`, `sort`, `diff`, `wc`, `tee`, `printf`, `od`.
Pass all busybox tests for each.

Status: baseline implementation exists for all listed applets and the
current local runner passes. This is not full completion against the
plan yet: some busybox `.tests` cases are still skipped in the runner
because the corresponding feature sets are not implemented.

### Phase 4 — Find + ls

Implement: `find` (with full `-exec` support via fork/exec), `ls`.
Pass all busybox tests for each.

### Phase 5 — Compression

Implement: `gzip`, `bzip2`, `xz`, `lzma`.
Pass all busybox tests for each (gzip has old-style tests).

These require implementing deflate, bzip2, and LZMA1/LZMA2 codecs from
scratch (no external crates). This is the most algorithmically dense
phase. Prioritize decompression — it's simpler and covers the critical
pm.ysh install path. Compression can follow.

### Phase 6 — Archives + networking

Implement: `tar`, `wget`.
Pass all busybox tests for each.

`wget` requires a from-scratch HTTP/1.1 client and TLS implementation
(or at minimum a TLS record layer using `rustls`-style approach with
raw sockets). This is the single hardest applet. Consider implementing
HTTP-only first, then layering TLS.

### Phase 7 — System utilities (macOS-testable)

Implement: `date`, `uname`, `sleep`, `env`.
Pass all busybox tests for each (date has old-style tests).

### Phase 7b — Linux-only system utilities

Implement: `losetup`, `mknod`.
These require Linux-specific syscalls/ioctls — test in Docker.

### Phase 8 — Integration

Test the full binary as a drop-in busybox replacement by running
`pm.ysh` against it via Dockerfile.base. Fix any behavioral gaps.

## Rust Requirements

**Dependencies:** `std` + `libc` only. No other crates. Implement
compression, HTTP, TLS, tar, regex, and everything else from scratch
or using `std`/`libc` primitives.

**Binary:** Single multi-call binary. `fn main()` inspects `argv[0]`
to select the applet. Must also support `seed <applet> [args...]` for
testing. Each applet is a module with a `pub fn main(args: &[String]) -> i32`
entry point (or similar).

**Targets:** `aarch64-unknown-linux-musl` and `x86_64-unknown-linux-musl`.
Fully static. No dynamic linking. No runtime dependencies.

**Development:** Can build and iterate on macOS (the non-Linux-specific
applets should work). Linux-specific applets (`losetup`, `mknod`) and
the final integration test require Linux (use Docker if needed).

**Suggested module layout:**
```
src/
  main.rs              # argv[0] dispatch table
  lib.rs               # re-exports
  common/
    args.rs            # argument parsing utilities
    error.rs           # AppletError type, die() helper, exit code constants
    io.rs              # BufReader/BufWriter wrappers, line iteration, copy_stream
    fs.rs              # recursive copy/remove, symlink handling, permission ops
    path.rs            # path manipulation, canonicalization
    pattern.rs         # glob matching (for find -name, tar --exclude)
    regex.rs           # BRE/ERE engine (for grep, find -regex)
  applets/
    cat.rs
    chmod.rs
    cp.rs              # thin wrapper over common::fs
    ...
  compression/
    deflate.rs         # inflate/deflate (gzip internals)
    bzip2.rs
    lzma.rs            # LZMA1 + LZMA2 (shared by lzma and xz applets)
    xz.rs              # XZ container format
  net/
    http.rs            # HTTP/1.1 client
    tls.rs             # TLS 1.2/1.3 record layer
```

Applets should be thin — a `parse_args` function that returns a typed
options struct, then a call into shared library code. When `cp` and
`mv` both need recursive copy, that lives in `common::fs`, not in
either applet.

**Compression aliases:** The multi-call dispatch table must also handle
these symlink names, mapping each to the appropriate applet + flags:
- `gunzip`, `zcat` → `gzip -d` / `gzip -dc`
- `bunzip2`, `bzcat` → `bzip2 -d` / `bzip2 -dc`
- `unlzma`, `lzcat` → `lzma -d` / `lzma -dc`
- `unxz`, `xzcat` → `xz -d` / `xz -dc`

**Style:**
- No `unsafe` unless strictly necessary (raw syscalls, ioctl). Document
  every `unsafe` block with a `// SAFETY:` comment.
- No `unwrap()`/`expect()` in applet code — handle errors, print a
  message to stderr, and return a nonzero exit code.
- Applet error messages: `<applet>: <message>` to stderr, matching
  busybox conventions.
- Handle SIGPIPE gracefully (exit silently, don't panic).

## Test Suite

Busybox tests are in `tests/busybox/`. Two formats:

**New-style** (`.tests` files): Shell scripts using `testing.sh` harness.
The `testing` function takes 5 args:
```
testing "description" "command" "expected_stdout" "file_input" "stdin"
```
It writes `file_input` to a file named `input`, pipes `stdin` to the
command, and compares stdout against `expected_stdout`.

**Old-style** (directories): Each file is a standalone shell script.
Uses `busybox <applet>` invocations. Exit 0 = pass, nonzero = fail.

To run tests against our binary:
1. Build: `cargo build`
2. Create a symlink directory: `mkdir -p /tmp/seed-links && for applet in cat cp chmod ...;  do ln -sf /path/to/seed /tmp/seed-links/$applet; done`
3. For new-style tests: `cd tests/busybox && PATH=/tmp/seed-links:$PATH sh cat.tests`
4. For old-style tests: replace `busybox` invocations with the seed
   binary path, or symlink `seed` as `busybox` in the links dir (seed
   should recognize `busybox <applet>` as an invocation form, same as
   `seed <applet>`).

Available tests (19 of 28 applets covered):
- `.tests` files: cat, cp, diff, find, grep, ls, od, printf, sort, tar
- Old-style dirs: cat, cp, date, find, gzip, ls, mkdir, mv, rm, rmdir,
  tar, tee, wc, wget
- No tests: chmod, env, bzip2, lzma, sleep, uname, xz, losetup, mknod

## Applets

28 applets total. Each section lists the full busybox 1.37.0 interface.

---

### cat

```
cat [-nbvteA] [FILE]...
```

Print FILEs (or stdin) to stdout.

| Flag | Description |
|------|-------------|
| `-n` | Number all output lines |
| `-b` | Number nonempty lines only (overrides `-n`) |
| `-v` | Show nonprinting characters as `^x` or `M-x` |
| `-t` | Like `-v`, but also show tabs as `^I` |
| `-e` | Like `-v`, but also show line endings as `$` |
| `-A` | Same as `-vte` |

**pm.ysh usage:** `cat` with no flags — stdin-to-stdout passthrough for
plain `.tar` files in the `decompress` function. **[pm]**

---

### chmod

```
chmod [-Rcvf] MODE[,MODE]... FILE...
```

Change file mode. MODE is an octal number (bit pattern `sstrwxrwxrwx`)
or symbolic form `[ugoa]{+|-|=}[rwxXst]` (comma-separated for multiple).

| Flag | Description |
|------|-------------|
| `-R` | Recurse into directories |
| `-c` | List changed files |
| `-v` | Verbose — list all files |
| `-f` | Hide errors |

Octal mode: 3-4 digit octal number (e.g., `755`, `4755`).

Symbolic mode: `[ugoa...][+-=][rwxXst...]` where:
- `u`/`g`/`o`/`a` = user/group/other/all
- `+`/`-`/`=` = add/remove/set exactly
- `r`/`w`/`x` = read/write/execute
- `X` = execute only if directory or already executable
- `s` = setuid/setgid, `t` = sticky bit

Multiple symbolic modes can be comma-separated: `u+x,g-w`.

**pm.ysh usage:** `chmod +x FILE...` **[pm]**

---

### cp

```
cp [-arPLHpfinlsTu] SOURCE DEST
cp [-arPLHpfinlsu] SOURCE... { -t DIRECTORY | DIRECTORY }
```

Copy SOURCEs to DEST.

| Flag | Description |
|------|-------------|
| `-a` | Same as `-dpR` (archive) |
| `-R`, `-r` | Recurse into directories **[pm]** |
| `-d`, `-P` | Preserve symlinks (don't dereference); default if `-R` **[pm]** |
| `-L` | Follow all symlinks (dereference) **[pm]** |
| `-H` | Follow symlinks on command line only |
| `-p` | Preserve file attributes (mode, ownership, timestamps) **[pm]** |
| `-f` | Force overwrite **[pm]** |
| `-i` | Prompt before overwrite |
| `-n` | Don't overwrite existing files |
| `-l` | Create hard links instead of copying |
| `-s` | Create symlinks instead of copying |
| `-T` | Refuse to copy if DEST is a directory |
| `-t DIR` | Copy all SOURCEs into DIR |
| `-u` | Copy only when SOURCE is newer than DEST |

**pm.ysh usage:**
- `cp -f src dest` **[pm]**
- `cp -fP src dest` **[pm]**
- `cp -LRf src dest` **[pm]**
- `cp -fRp src... dest` **[pm]**

---

### date

```
date [OPTIONS] [+FMT] [[-s] TIME]
```

Display time (using `+FMT`), or set time.

| Flag | Description |
|------|-------------|
| `-u` | Work in UTC |
| `-s TIME` | Set system time |
| `-d TIME` | Display TIME instead of now |
| `-D FMT` | strptime format for parsing `-s`/`-d` TIME |
| `-r FILE` | Display last modification time of FILE |
| `-R` | Output RFC-2822 date |
| `-I[SPEC]` | Output ISO-8601 date (`date`, `hours`, `minutes`, `seconds`, `ns`) |

Format string `+FMT` uses strftime conversion specs. Must support at least:
`%Y` `%m` `%d` `%H` `%M` `%S` `%a` `%b` `%c` `%p` `%Z` `%z` `%s`
`%n` `%t` `%T` `%e` `%%` and padding modifiers.

Recognized TIME formats for `-s`/`-d`:
- `@seconds_since_1970`
- `hh:mm[:ss]`
- `[YYYY.]MM.DD-hh:mm[:ss]`
- `YYYY-MM-DD hh:mm[:ss]`
- `[[[[[YY]YY]MM]DD]hh]mm[.ss]`

**pm.ysh usage:** `date +%Y-%m-%d-%H:%M` — local time. **[pm]**

---

### diff

```
diff [-abBdiNqrTstw] [-L LABEL] [-S FILE] [-U LINES] FILE1 FILE2
```

Compare files line by line. Busybox supports unified diffs only.

| Flag | Description |
|------|-------------|
| `-a` | Treat all files as text |
| `-b` | Ignore changes in amount of whitespace |
| `-B` | Ignore changes whose lines are all blank |
| `-d` | Try hard to find smaller set of changes |
| `-i` | Ignore case differences |
| `-L LABEL` | Use LABEL instead of filename in header |
| `-N` | Treat absent files as empty |
| `-q` | Output only whether files differ |
| `-r` | Recurse into directories |
| `-S FILE` | Start with FILE when comparing directories |
| `-T` | Make tabs line up by prefixing a tab |
| `-s` | Report when two files are the same |
| `-t` | Expand tabs to spaces in output |
| `-U LINES` | Output LINES of context (default 3) **[pm]** |
| `-w` | Ignore all whitespace |

Exit codes: 0 = identical, 1 = differ, 2 = error.

**pm.ysh usage:** `diff -U 3 file1 file2 2>/dev/null || true` **[pm]**

---

### env

```
env [-i0] [-u NAME]... [-] [NAME=VALUE]... [PROG ARGS]
```

Print current environment or run PROG after modifying environment.

| Flag | Description |
|------|-------------|
| `-`, `-i` | Start with empty environment |
| `-0` | NUL-terminated output (for printing env) |
| `-u NAME` | Remove NAME from environment |

With no PROG: print environment variables to stdout.
With PROG: exec PROG with modified environment.

**pm.ysh usage:** `env VAR=val... COMMAND ARGS` **[pm]**

---

### find

```
find [-HL] [PATH]... [OPTIONS] [ACTIONS]
```

Search for files and perform actions. Default PATH is `.`, default
action is `-print`.

**Top-level options:**

| Flag | Description |
|------|-------------|
| `-L`, `-follow` | Follow symlinks |
| `-H` | Follow symlinks on command line only |
| `-xdev` | Don't descend into other filesystems |
| `-maxdepth N` | Descend at most N levels |
| `-mindepth N` | Don't act on first N levels |
| `-depth` | Act on directory after traversing it |

**Tests:**

| Predicate | Description |
|-----------|-------------|
| `-name PATTERN` | Glob match on basename **[pm]** |
| `-iname PATTERN` | Case-insensitive `-name` |
| `-path PATTERN` | Glob match on full path **[pm]** |
| `-ipath PATTERN` | Case-insensitive `-path` |
| `-regex PATTERN` | Regex match on full path |
| `-type X` | File type: `f`,`d`,`l`,`b`,`c`,`s`,`p` **[pm]** |
| `-executable` | File is executable |
| `-perm MASK` | Permission test: `+MASK` (any), `-MASK` (all), or exact |
| `-mtime DAYS` | mtime `+N`/`-N`/`N` days ago |
| `-atime DAYS` | atime `+N`/`-N`/`N` days ago |
| `-ctime DAYS` | ctime `+N`/`-N`/`N` days ago |
| `-mmin MINS` | mtime `+N`/`-N`/`N` minutes ago |
| `-amin MINS` | atime `+N`/`-N`/`N` minutes ago |
| `-cmin MINS` | ctime `+N`/`-N`/`N` minutes ago |
| `-newer FILE` | mtime more recent than FILE |
| `-inum N` | Inode number is N |
| `-samefile FILE` | Same inode as FILE |
| `-user NAME/ID` | Owned by user |
| `-group NAME/ID` | Owned by group |
| `-size N[bck]` | File size (`c`=bytes, `k`=KiB, `b`=512B blocks) |
| `-links N` | Number of hard links |
| `-empty` | Empty file or directory |

**Operators:**

| Operator | Description |
|----------|-------------|
| `( ACTIONS )` | Group **[pm]** |
| `! ACT` | Negate **[pm]** |
| `ACT1 [-a] ACT2` | AND (implicit) **[pm]** |
| `ACT1 -o ACT2` | OR **[pm]** |

**Actions:**

| Action | Description |
|--------|-------------|
| `-print` | Print path, newline-terminated **[pm]** |
| `-print0` | Print path, NUL-terminated |
| `-exec CMD ARG ;` | Run CMD per file, `{}` replaced |
| `-exec CMD ARG +` | Run CMD with batched `{}` list **[pm]** |
| `-ok CMD ARG ;` | Prompt before running CMD |
| `-prune` | Don't descend into directory **[pm]** |
| `-delete` | Delete file/directory (turns on `-depth`) |
| `-quit` | Exit immediately |

**`-exec` implementation:** Must support arbitrary external commands via
`fork`/`exec`. The `;` form runs the command once per matched file
(replacing `{}` in each arg). The `+` form batches — appends all
matched paths as arguments and runs the command once (or in chunks if
`ARG_MAX` would be exceeded).

Note: pm.ysh passes `sh -c 'mv ...' {} +` through find. Since we don't
implement `sh`, this works because find `exec`s the real `/bin/sh` (or
whatever `sh` is on the system). find does NOT need to interpret shell
commands itself — it just calls `execvp("sh", ...)`.

**pm.ysh usage patterns:**
```
find DIR/. ! -name . -prune -exec sh -c 'mv -f "$0" "$@" .' {} +
find DIR/. ! -name . -prune -exec sh -c 'cp -fRp "$0" "$@" .' {} +
find BASE ! -path BASE -type d -exec printf '%s/\n' {} + \
    -o \( ! -type d -a ! -name '*.la' -a ! -name charset.alias \) -print
find /packages -name build -exec chmod +x {} +
```

---

### grep

```
grep [-HhnlLoqvsrRiwFExmABC] [-e PATTERN]... [-f FILE]... [FILE]...
```

Search for PATTERN in FILEs (or stdin).

| Flag | Description |
|------|-------------|
| `-H` | Add `filename:` prefix |
| `-h` | Suppress `filename:` prefix |
| `-n` | Add `line_no:` prefix |
| `-l` | Show only filenames that match **[pm]** |
| `-L` | Show only filenames that don't match |
| `-c` | Show only count of matching lines |
| `-o` | Show only the matching part of line |
| `-q` | Quiet — return 0 if match found **[pm]** |
| `-v` | Select non-matching lines **[pm]** |
| `-s` | Suppress open/read errors |
| `-r` | Recurse into directories |
| `-R` | Recurse and dereference symlinks |
| `-i` | Ignore case |
| `-w` | Match whole words only |
| `-x` | Match whole lines only **[pm]** |
| `-F` | PATTERN is a fixed string (literal) **[pm]** |
| `-E` | PATTERN is an extended regexp |
| `-m N` | Match up to N times per file |
| `-A N` | Print N lines of trailing context |
| `-B N` | Print N lines of leading context |
| `-C N` | Same as `-A N -B N` |
| `-e PTRN` | Specify pattern (can repeat) |
| `-f FILE` | Read patterns from file **[pm]** |
| `--` | End of options **[pm]** |

Default (no `-E`/`-F`): BRE (basic regular expressions).
ERE (`-E`): adds `+`, `?`, `|`, `()` without backslash.

Exit codes: 0 = match, 1 = no match, 2 = error.

**pm.ysh usage:**
- `echo $arg | grep -q '[][!* /]'` **[pm]**
- `grep -lxF STRING FILE...` **[pm]**
- `grep -Fxf PATFILE FILE...` **[pm]**
- `grep -vFxf PATFILE FILE` **[pm]**
- `grep -lFx -- STRING FILE...` **[pm]**

---

### gzip

```
gzip [-cfkdt123456789] [FILE]...
```

Compress FILEs (or stdin) with gzip/deflate.

| Flag | Description |
|------|-------------|
| `-d` | Decompress **[pm]** |
| `-c` | Write to stdout (keep input) |
| `-f` | Force |
| `-k` | Keep input files |
| `-t` | Test integrity |
| `-1`..`-9` | Compression level (default 6) |

Without `-c` and with FILE args: in-place compress/decompress.
With `-c` or no FILE: stdin to stdout.

**pm.ysh usage:** `gzip -d` (decompress), `gzip -6` (compress level 6). **[pm]**

---

### bzip2

```
bzip2 [-cfkdt123456789] [FILE]...
```

Compress FILEs (or stdin) with bzip2.

| Flag | Description |
|------|-------------|
| `-1`..`-9` | Compression level |
| `-d` | Decompress **[pm]** |
| `-c` | Write to stdout |
| `-f` | Force |
| `-k` | Keep input files |
| `-t` | Test integrity |

**pm.ysh usage:** `bzip2 -d` (decompress), `bzip2 -z` (compress). **[pm]**

Note: busybox accepts `-z` as explicit "compress" flag. Must be accepted.

---

### ls

```
ls [-1AaCxdLHRFplinshrSXvctu] [-w WIDTH] [FILE]...
```

List directory contents.

| Flag | Description |
|------|-------------|
| `-1` | One column output |
| `-a` | Include entries starting with `.` |
| `-A` | Like `-a`, but exclude `.` and `..` |
| `-x` | List by lines |
| `-d` | List directory names, not contents **[pm]** |
| `-L` | Follow symlinks |
| `-H` | Follow symlinks on command line |
| `-R` | Recurse |
| `-p` | Append `/` to directory names |
| `-F` | Append type indicator (`*/=@\|`) |
| `-l` | Long format **[pm]** |
| `-i` | Show inode numbers |
| `-n` | Numeric UIDs/GIDs |
| `-s` | Show allocated blocks |
| `-h` | Human-readable sizes |
| `-r` | Reverse sort order |
| `-S` | Sort by size |
| `-X` | Sort by extension |
| `-v` | Sort by version |
| `-t` | Sort by mtime |
| `-c` | With `-l`: show ctime; with `-t`: sort by ctime |
| `-u` | With `-l`: show atime; with `-t`: sort by atime |
| `-w N` | Format N columns wide |
| `--full-time` | Full date/time |
| `--group-directories-first` | Directories before files |
| `--color[={always,never,auto}]` | Colorize output |

Long format: `TYPE+PERMS  LINKS  OWNER  GROUP  SIZE  DATE  NAME`

Permission string: 10 characters. Type + 9 mode bits with `s`/`S`/`t`/`T`.

**pm.ysh usage:** `ls -ld PATH` — parsed for owner name and permission
string (octal conversion). **[pm]**

---

### lzma

```
lzma -d [-cfk] [FILE]...
```

Decompress FILEs (or stdin). LZMA1 format (`.lzma`), not `.xz`.

| Flag | Description |
|------|-------------|
| `-d` | Decompress **[pm]** |
| `-c` | Write to stdout **[pm]** |
| `-f` | Force |
| `-k` | Keep input files |
| `-t` | Test integrity |

Busybox lzma is decompression-only but pm.ysh also calls `lzma -z`
for compression — must support compression too. **[pm]**

---

### mkdir

```
mkdir [-m MODE] [-p] DIRECTORY...
```

| Flag | Description |
|------|-------------|
| `-m MODE` | Set permission mode **[pm]** |
| `-p` | Create parents, no error if exists **[pm]** |

---

### mknod

```
mknod [-m MODE] NAME TYPE [MAJOR MINOR]
```

Create a special file (block, character, or pipe).

| Flag | Description |
|------|-------------|
| `-m MODE` | Creation mode (default `a=rw`) |

TYPE:
- `b` — Block device
- `c` or `u` — Character device
- `p` — Named pipe (MAJOR MINOR must be omitted)

Linux-only (uses `mknod(2)` syscall).

---

### mv

```
mv [-finT] SOURCE DEST
mv [-fin] SOURCE... { -t DIRECTORY | DIRECTORY }
```

| Flag | Description |
|------|-------------|
| `-f` | Don't prompt before overwriting **[pm]** |
| `-i` | Prompt before overwrite |
| `-n` | Don't overwrite existing file |
| `-T` | Refuse to move if DEST is a directory |
| `-t DIR` | Move all SOURCEs into DIR |

Must handle cross-filesystem moves (copy + delete).

**pm.ysh usage:** `mv -f SRC DEST` **[pm]**

---

### losetup

```
losetup [-rP] [-o OFS] {-f|LOOPDEV} FILE
losetup -c LOOPDEV
losetup -d LOOPDEV
losetup -a
losetup -f
```

Associate loop devices with files.

| Flag | Description |
|------|-------------|
| `-o OFS` | Start OFS bytes into FILE |
| `-P` | Scan for partitions |
| `-r` | Read-only |
| `-f` | Show/use next free loop device |
| `-c` | Reread file size |
| `-d` | Disassociate loop device |
| `-a` | Show all loop device status |

Linux-only (uses `/dev/loop*` and `ioctl`).

---

### od

```
od [-abcdfhilovxs] [-t TYPE] [-A RADIX] [-N SIZE] [-j SKIP] [-S MINSTR] [-w WIDTH] [FILE]...
```

Print FILEs (or stdin) unambiguously. Default: octal bytes.

**Type specifiers (`-t`):** `a` (named), `c` (C-style) **[pm]**,
`d[SIZE]` (signed decimal), `f[SIZE]` (float), `o[SIZE]` (octal),
`u[SIZE]` (unsigned), `x[SIZE]` (hex).

**Traditional single-letter flags:** `-a`=`-t a`, `-b`=`-t o1`,
`-c`=`-t c`, `-d`=`-t u2`, `-f`=`-t fF`, `-h`/`-x`=`-t x2`,
`-i`=`-t dI`, `-l`=`-t dL`, `-o`=`-t o2`, `-s`=`-t d2`.

| Flag | Description |
|------|-------------|
| `-A RADIX` | Address radix: `o`/`d`/`x`/`n` **[pm]** |
| `-N SIZE` | Read only SIZE bytes **[pm]** |
| `-j SKIP` | Skip SKIP bytes |
| `-S MINSTR` | Output strings at least MINSTR chars |
| `-w WIDTH` | Bytes per line (default 16) |

**pm.ysh usage:** `od -A o -t c -N 18 FILE` — ELF/ar magic detection. **[pm]**

---

### printf

```
printf FORMAT [ARG]...
```

C-style printf. Must support: `%s`, `%d`, `%i`, `%u`, `%o`, `%x`, `%X`,
`%c`, `%f`, `%e`, `%g`, `%%`, `%b` (escaped string). Escape sequences:
`\n`, `\t`, `\\`, `\0NNN`, `\xHH`. Width/precision: `%10s`, `%-20s`,
`%05d`, `%.2f`, `%*d`. `\c` stops output.

When more ARGs than format specs: reuse format. When fewer: zero/empty.

**pm.ysh usage:** via `find -exec printf '%s/\n' {} +`. **[pm]**

---

### rm

```
rm [-irf] FILE...
```

| Flag | Description |
|------|-------------|
| `-i` | Prompt before each removal |
| `-f` | Never prompt, ignore nonexistent **[pm]** |
| `-R`, `-r` | Recurse into directories **[pm]** |

---

### rmdir

```
rmdir [-p] [--ignore-fail-on-non-empty] DIRECTORY...
```

| Flag | Description |
|------|-------------|
| `-p` | Remove parents too |
| `--ignore-fail-on-non-empty` | No error if not empty |

**pm.ysh usage:** `rmdir DIR 2>/dev/null` **[pm]**

---

### sleep

```
sleep [N]...
```

Pause for the total of all args. Optional suffixes: `s` (seconds,
default), `m` (minutes), `h` (hours), `d` (days). Multiple args summed.
Fractional values allowed.

**pm.ysh usage:** `sleep 0.3` **[pm]**

---

### sort

```
sort [-nrughMcszbdfiokt] [-o FILE] [-k N[,M]] [-t CHAR] [FILE]...
```

Sort lines of text.

| Flag | Description |
|------|-------------|
| `-o FILE` | Output to FILE |
| `-c` | Check whether input is sorted |
| `-b` | Ignore leading blanks in sort key |
| `-f` | Ignore case |
| `-i` | Ignore unprintable characters |
| `-d` | Dictionary order |
| `-n` | Numeric sort |
| `-g` | General numeric sort |
| `-h` | Sort human-readable numbers |
| `-M` | Sort by month name |
| `-V` | Version sort |
| `-t CHAR` | Field separator **[pm]** |
| `-k N[,M]` | Sort key **[pm]** |
| `-r` | Reverse **[pm]** |
| `-s` | Stable sort |
| `-u` | Deduplicate **[pm]** |
| `-z` | NUL-terminated I/O |

Key spec: `-k START[.OFS][OPTS][,END[.OFS][OPTS]]` with per-key flags.

**pm.ysh usage:** `sort`, `sort -r`, `sort -ur`, `sort -ut / -k1,1`,
`sort -uk1,1` **[pm]**

---

### tar

```
tar c|x|t [-zJjahmvokO] [-f TARFILE] [-C DIR] [-T FILE] [-X FILE] [LONGOPT]... [FILE]...
```

**Operations:** `c` (create) **[pm]**, `x` (extract) **[pm]**, `t` (list) **[pm]**

| Flag | Description |
|------|-------------|
| `-f FILE` | Archive file (`-` for stdin/stdout) **[pm]** |
| `-C DIR` | Change to DIR **[pm]** |
| `-v` | Verbose |
| `-O` | Extract to stdout |
| `-m` | Don't restore mtime |
| `-o` | Don't restore user:group |
| `-k` | Don't replace existing files |
| `-z` | Filter through gzip **[pm]** |
| `-J` | Filter through xz |
| `-j` | Filter through bzip2 |
| `--lzma` | Filter through lzma |
| `-a` | Auto-detect compression |
| `-h` | Follow symlinks |
| `-T FILE` | Include names from FILE |
| `-X FILE` | Exclude patterns from FILE |
| `--exclude PATTERN` | Exclude glob |
| `--overwrite` | Replace existing files |
| `--strip-components N` | Strip N leading path components |
| `--no-recursion` | Don't descend |
| `--numeric-owner` | Use numeric UID/GID |
| `--no-same-permissions` | Don't restore permissions |
| `--to-command CMD` | Pipe files to CMD |

Positional FILEs: on create = files to archive, on extract = paths to
extract (filter). Archive format: POSIX ustar + pax extended headers.

**pm.ysh usage:** `tar xf -`, `tar xf FILE`, `tar tf FILE`,
`tar cf - .`, `tar xzf - -C / ./usr/local/bin/` **[pm]**

---

### tee

```
tee [-ai] [FILE]...
```

| Flag | Description |
|------|-------------|
| `-a` | Append instead of overwrite |
| `-i` | Ignore SIGINT |

**pm.ysh usage:** `tee FILENAME` **[pm]**

---

### uname

```
uname [-amnrspvio]
```

| Flag | Description |
|------|-------------|
| `-a` | Print all fields |
| `-m` | Machine type **[pm]** |
| `-n` | Hostname |
| `-r` | Kernel release |
| `-s` | Kernel name (default) |
| `-p` | Processor type |
| `-v` | Kernel version |
| `-i` | Hardware platform |
| `-o` | OS name |

**pm.ysh usage:** `uname -m` **[pm]**

---

### wc

```
wc [-cmlwL] [FILE]...
```

| Flag | Description |
|------|-------------|
| `-c` | Count bytes **[pm]** |
| `-m` | Count characters |
| `-l` | Count newlines |
| `-w` | Count words |
| `-L` | Print longest line length |

Default (no flags): `-lwc`. Output: right-justified, space-separated.
With FILE args: print filename. Multiple files: `total` line at end.

**pm.ysh usage:** `wc -c < FILE` **[pm]**

---

### wget

```
wget [-cqS] [--spider] [-O FILE] [-o LOGFILE] [--header STR]
     [--post-data STR | --post-file FILE] [-Y on/off]
     [--no-check-certificate] [-P DIR] [-U AGENT] [-T SEC] URL...
```

| Flag | Description |
|------|-------------|
| `--spider` | Only check URL existence |
| `--header STR` | Add custom header |
| `--post-data STR` | POST string |
| `--post-file FILE` | POST file contents |
| `--no-check-certificate` | Skip TLS verification **[pm]** |
| `-c` | Continue interrupted download |
| `-q` | Quiet **[pm]** |
| `-P DIR` | Save to DIR |
| `-S` | Show server response headers |
| `-T SEC` | Network timeout |
| `-O FILE` | Save to FILE (`-` for stdout) **[pm]** |
| `-o LOGFILE` | Log to file |
| `-U STR` | User-Agent |
| `-Y on/off` | Use proxy |

Must support HTTP/1.1, HTTPS, redirects, chunked encoding.
Implement TLS from scratch (or minimal embedded implementation) since
no external crates are allowed.

**pm.ysh usage:** `wget --no-check-certificate -qO DEST URL`,
`wget --no-check-certificate -qO- URL` **[pm]**

---

### xz

```
xz -d [-cfk] [FILE]...
```

| Flag | Description |
|------|-------------|
| `-d` | Decompress **[pm]** |
| `-c` | Write to stdout **[pm]** |
| `-f` | Force |
| `-k` | Keep input files |
| `-t` | Test integrity |
| `-0`..`-9` | Compression level |

Busybox xz is decompression-only but pm.ysh calls `xz -z` for
compression — must support both. **[pm]**

---

## Implementation Notes

These are non-obvious details that will cause subtle test failures or
wasted time if missed.

**Argument parsing quirks.** Most applets use standard short-option
parsing where `-rf` is `-r -f`. But several have non-standard syntax:
- `tar`: first argument can be `xzf` without a leading dash (old-style).
  `tar xzf -` and `tar -x -z -f -` must both work.
- `find`: arguments are an expression tree, not flags. Implement as a
  recursive-descent parser with `-a` binding tighter than `-o`.
- `sort -k`: has its own sub-syntax: `START[.OFS][OPTS][,END[.OFS][OPTS]]`
  where OPTS are per-key modifier characters like `n`, `r`, `b`.
- `chmod`: MODE argument is positional, not a flag. Must parse both
  octal (`755`) and symbolic (`u+rwx,go-w`) forms.
- `od`: both traditional (`-c`, `-x`) and POSIX (`-t c`, `-t x2`)
  option forms must work simultaneously.

**Regex engine (grep, find -regex).** Implement a simple NFA-based
engine. Required features: `.`, `*`, `^`, `$`, `[...]` character
classes (including POSIX classes like `[:alpha:]`), `\(\)` grouping
and `\1`-`\9` backreferences (BRE), `+`, `?`, `|`, `()` (ERE).
Word boundary for `-w`: match at start/end of string or where adjacent
character is/isn't `[a-zA-Z0-9_]`.

**diff algorithm.** Use Myers' O(ND) diff algorithm (the standard).
Unified output only (busybox limitation we inherit).

**tar format details.**
- On create: detect hard links (nlink > 1) and emit hardlink entries
  pointing to the first occurrence. The tar tests explicitly check this.
- Pax extended headers: key-value format `<length> <key>=<value>\n`.
  Must handle at least `path` and `linkpath` (long filenames).
- On extract: strip `../` path components to prevent directory traversal
  attacks (busybox does this, tests verify it).

**mv cross-filesystem.** Try `rename(2)` first. If it returns `EXDEV`,
fall back to recursive copy + remove. The copy must preserve all
attributes (mode, ownership, timestamps, symlinks).

**ls output format.** Tests compare exact output. Column alignment in
long format matters. Use `getpwuid`/`getgrgid` from libc for
user/group name lookup.

**date and strftime.** Use libc's `strftime(3)`, `mktime(3)`,
`localtime_r(3)`, and `strptime(3)` via the `libc` crate rather than
reimplementing calendar math. These are available in musl. Same for
`getpwuid`/`getgrgid` — use libc, don't reimplement passwd parsing.

**uname.** Use libc's `uname(2)` — it fills a `utsname` struct with
all the fields. Trivial wrapper.

**Compression reference algorithms.**
- Deflate (gzip): RFC 1951. Inflate is ~500 lines, deflate is much
  larger. Study zlib or miniz for reference.
- Bzip2: Burrows-Wheeler transform + Huffman + run-length encoding.
  Study the bzip2 source or Julian Seward's original paper.
- LZMA/LZMA2 (xz): Range coder + LZ77. Study the LZMA SDK or
  `lzma-rs` for the decoder structure. LZMA2 is a framing layer
  over LZMA1 with reset capabilities.

**TLS (wget).** This is the single hardest component. Minimum viable:
TLS 1.2 with one cipher suite (e.g., `TLS_RSA_WITH_AES_128_GCM_SHA256`
or `TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256`). Requires implementing:
record layer framing, handshake state machine, RSA or ECDHE key
exchange, AES-GCM, SHA-256, X.509 certificate parsing (for non
`--no-check-certificate` mode). Consider studying `rustls` internals
or BearSSL (compact C TLS library) for architecture guidance.
Since pm.ysh always passes `--no-check-certificate`, cert verification
can be deferred — but the TLS handshake and encryption are still
required.

## Cross-Cutting Concerns

**SIGPIPE:** Install a SIGPIPE handler or check `BrokenPipe`. Exit
silently, don't panic.

**Exit codes:** POSIX conventions. 0 = success, 1 = operational failure,
2 = usage error.

**Symlinks:** `cp -P` preserves, `cp -L` dereferences. `find` doesn't
follow by default. `rm -rf` removes without following.

**Permissions:** `mkdir -m`, `chmod`, `cp -p`, `ls -l`, `mknod -m`.

**Error messages:** `<applet>: <message>` to stderr.

**Locale:** None needed. Byte-order sort. ASCII/UTF-8 only.

**`--help`:** Each applet responds to `--help` with a usage summary
printed to stderr, then exits 0. Match busybox's terse format.

**grep aliases:** The dispatch table should also map `egrep` → `grep -E`
and `fgrep` → `grep -F`.
