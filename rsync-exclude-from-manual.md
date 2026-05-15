# JustFolders Exclude-Patterns File Reference

This document describes the syntax of the exclude-patterns file used by JustFolders. The file is plain text containing one pattern per line; any source path that matches a pattern is skipped during backup.

## Table of Contents

1. [What Is the Exclude File?](#1-what-is-the-exclude-file)
2. [Basic File Format](#2-basic-file-format)
3. [Comments and Blank Lines](#3-comments-and-blank-lines)
4. [Wildcard Characters](#4-wildcard-characters)
5. [Pattern Rules: Anchoring](#5-pattern-rules-anchoring)
6. [Excluding Files vs. Directories](#6-excluding-files-vs-directories)
7. [Depth and Path-Based Exclusions](#7-depth-and-path-based-exclusions)
8. [Include Rules Inside an Exclude File](#8-include-rules-inside-an-exclude-file)
9. [Rule Ordering and First-Match Logic](#9-rule-ordering-and-first-match-logic)
10. [Advanced: Filter Rules in the Exclude File](#10-advanced-filter-rules-in-the-exclude-file)
11. [Testing Your Exclude File](#11-testing-your-exclude-file)
12. [Common Pitfalls](#12-common-pitfalls)
13. [Real-World Example Files](#13-real-world-example-files)
14. [Quick Reference Cheat Sheet](#14-quick-reference-cheat-sheet)

---

## 1. What Is the Exclude File?

JustFolders runs `rsync` under the hood and forwards your exclude file directly to it via `--exclude-from`. To attach one to a backup job:

- At config-creation time, pass `--exclude-from <FILE>` to `justfolders init` — its path is written into the `[default]` section as `exclude_from = …`.
- Or edit an existing config and add the line yourself:

```ini
[default]
exclude_from = /path/to/excludes.txt
```

Every subsequent `justfolders backup` then loads the file and skips any source path matching one of its patterns.

The file can live anywhere on the filesystem — there is no required name or extension. Common conventions are `.rsync-exclude`, `exclude-list.txt`, or `rsync-filter.txt`. The path is always on the **local (sending) machine**, even when the backup destination is remote.

---

## 2. Basic File Format

Each line in the file is one pattern. rsync reads them from top to bottom.

```
pattern-one
pattern-two
pattern-three
```

Rules:

- One pattern per line.
- Leading and trailing whitespace is **not** stripped — be exact.
- Empty lines and lines beginning with `#` are ignored (see next section).
- Patterns are matched against the **relative path** of each file as seen from the source root, not against the full absolute path on disk.

---

## 3. Comments and Blank Lines

Lines that start with `#` are treated as comments and are completely ignored by rsync. Blank lines are also ignored. Use both freely to organize and document your exclude file.

```
# -----------------------------------------------
# rsync exclude list for /home/alice/project
# Last updated: 2025-05-01
# -----------------------------------------------

# === Build artifacts ===
build/
dist/

# === Temporary files ===
*.tmp
*.swp

# === Logs (keep the directory, skip the files) ===
logs/*.log
```

> **Important:** A `#` anywhere other than the very start of a line is treated as part of the pattern, not as a comment. There are no inline comments.

---

## 4. Wildcard Characters

rsync supports three wildcard metacharacters. It does **not** use full regular expressions.

### `*` — Match anything within one path component

Matches any sequence of characters **except** a forward slash (`/`). It will not cross directory boundaries.

```
*.log          # matches error.log, access.log — but NOT logs/error.log
cache-*        # matches cache-v1, cache-old, cache-2025
*-backup       # matches data-backup, images-backup
```

### `?` — Match exactly one character (no slash)

```
file?.txt      # matches file1.txt, fileA.txt — NOT file10.txt
?.conf         # matches a.conf, b.conf — NOT db.conf
```

### `[...]` — Match one character from a set or range

Works like shell character classes.

```
file[123].txt   # matches file1.txt, file2.txt, file3.txt
[A-Z]*.sh       # matches any .sh file starting with an uppercase letter
log[0-9][0-9]   # matches log01, log99, etc.
```

### `**` — Match anything across path boundaries

Unlike `*`, the double asterisk crosses directory separators. It matches zero or more path components.

```
**/tmp          # matches tmp/ at any depth: tmp/, src/tmp/, a/b/tmp/
**.pyc          # matches all .pyc files at any depth
```

### `***` — Match a directory AND all its contents (rsync-specific shorthand)

A trailing `***` combines matching the directory itself (like `dir/`) with matching everything inside it (like `dir/**`). This is the most convenient way to fully exclude a directory tree.

```
node_modules/***    # excludes the directory AND everything inside it
.git/***            # excludes the entire .git directory tree
```

---

## 5. Pattern Rules: Anchoring

**Anchoring** controls whether a pattern matches only at the top level of the source, or anywhere in the file tree.

### Unanchored patterns (no leading `/`)

Without a leading slash, the pattern is matched against **any part of the path**. rsync checks whether the pattern matches the filename alone, or any trailing portion of the path.

```
*.log       # matches error.log, logs/error.log, a/b/c/error.log
tmp         # matches tmp/ AND src/tmp/ AND a/b/tmp/
cache       # matches any file or directory named "cache" anywhere
```

This is the default and the most "greedy" behavior.

### Anchored patterns (leading `/`)

A leading `/` pins the match to the **root of the transfer** (the source directory). The pattern will only match at the top level.

```
/tmp        # matches ONLY source/tmp — NOT source/a/tmp
/logs/      # matches ONLY source/logs/ — NOT source/app/logs/
/*.conf     # matches ONLY .conf files directly in the source root
```

Use anchoring when a common name (like `tmp` or `cache`) appears in many places but you only want to exclude the top-level instance.

---

## 6. Excluding Files vs. Directories

### Excluding a directory (and everything inside it)

Add a trailing `/` to make sure the pattern only matches directories, not files of the same name.

```
build/          # excludes the directory named "build" and all its contents
.git/           # excludes the .git directory
node_modules/   # excludes the node_modules directory
```

Without the trailing slash, a pattern like `build` would match both a file named `build` and a directory named `build`.

### Excluding only files inside a directory (keeping the directory itself)

Use `dir/*` to exclude the contents while preserving the empty directory structure at the destination.

```
logs/*          # excludes files inside logs/ but keeps the logs/ directory
cache/*         # excludes the cache contents, leaves the directory
```

### Excluding a directory at any depth vs. only at the top level

```
# Excludes ANY directory named "tmp" at any depth:
tmp/

# Excludes ONLY the top-level "tmp" directory:
/tmp/
```

### Fully excluding a directory including itself — use `***`

```
# Excludes node_modules/ itself AND everything within it:
node_modules/***
```

---

## 7. Depth and Path-Based Exclusions

You can write patterns that include a relative sub-path to target specific locations within the tree.

```
# Exclude a specific nested file:
src/generated/parser.js

# Exclude all .log files inside the logs directory only:
logs/*.log

# Exclude a deeply nested directory:
app/cache/tmp/

# Exclude config files only inside a particular subdirectory:
config/local/*.conf
```

> Patterns containing a `/` anywhere (other than a trailing one) are **automatically anchored** to the transfer root, just as if they had a leading `/`. So `logs/error.log` already only matches at the top level — you don't need to write `/logs/error.log`.

---

## 8. Include Rules Inside an Exclude File

The `--exclude-from` file is actually a simplified version of rsync's filter-rule system. You **can** embed `+ include` and `- exclude` directives in it, which makes it much more powerful.

### Syntax for include/exclude directives

```
+ pattern      # include this
- pattern      # exclude this
```

A bare line without `+` or `-` is treated as an exclude pattern (equivalent to `- pattern`).

So these two files are equivalent:

```
# File A (bare patterns):
*.log
build/
```

```
# File B (explicit directives):
- *.log
- build/
```

### Why include rules matter

rsync uses **first-match-wins** logic (see section 9). You can use `+` rules to carve exceptions out of broader `−` rules, as long as the `+` rules come **first**.

```
# Keep important.log, exclude all other .log files:
+ important.log
- *.log

# Keep the config/ directory itself, exclude its contents:
+ config/
- config/*

# Include all directories so rsync can descend into them,
# then exclude all non-.jpg files:
+ */
+ *.jpg
- *
```

---

## 9. Rule Ordering and First-Match Logic

**The first rule that matches a file wins.** rsync stops evaluating further rules the moment one matches. Rules lower in the file are only reached if no earlier rule matched.

This has critical implications:

### Exclude rules before include rules: include is ignored

```
# WRONG — the exclude fires first; the include is never reached:
- *.log
+ important.log     # ← never evaluated for .log files
```

### Include rules before exclude rules: correct

```
# CORRECT — the include fires first for important.log:
+ important.log
- *.log
```

### Broader rules before narrower rules: narrow rule is shadowed

```
# WRONG — "- *" excludes everything; the specific include below is never reached:
- *
+ *.jpg
```

### Narrower rules before broader rules: correct

```
# CORRECT:
+ *.jpg
- *
```

### Practical rule: always order from most specific to most general.

---

## 10. Advanced: Filter Rules in the Exclude File

Each line in the exclude file is processed by rsync as a simplified filter rule. Beyond the bare-pattern and `+`/`-` forms covered in section 8, rsync also recognizes a handful of additional rule prefixes inside the file:

| Prefix | Meaning |
|--------|---------|
| `-`    | Exclude (same as a bare pattern) |
| `+`    | Include |
| `H`    | Hide (exclude from sender's file list) |
| `P`    | Protect (prevent deletion on destination) |
| `R`    | Risk (allow deletion even if otherwise protected) |
| `!`    | Clear all rules accumulated so far |

For the vast majority of JustFolders backups, only `-` and `+` are needed.

---

## 11. Testing Your Exclude File

**Always validate your patterns before pointing a JustFolders config at them.** The simplest way is to invoke `rsync` directly with `--dry-run` (`-n`) and `-v`, or with `--list-only`, against the same source. You see exactly which paths would be kept or skipped without writing anything to disk.

```bash
# Dry run with verbose output:
rsync -av --dry-run --exclude-from='excludes.txt' /source/ /tmp/test-dest/

# List only the paths that WOULD be transferred:
rsync -a --list-only --exclude-from='excludes.txt' /source/
```

Look for files you expected to be excluded that still appear in the list, and files you expected to be included that don't. Once the output matches your intent, wire the file into your JustFolders config via `exclude_from = …` (or `--exclude-from` on `justfolders init`).

---

## 12. Common Pitfalls

### Pitfall 1: Absolute paths don't work

Patterns are matched against the **relative** path from the source root. An absolute path like `/home/alice/logs/` will never match anything.

```
# WRONG:
/home/alice/logs/

# CORRECT (relative to source root):
logs/
# or anchored:
/logs/
```

### Pitfall 2: A pattern without a trailing `/` matches files AND directories

```
tmp         # matches a FILE named "tmp" AND a DIRECTORY named "tmp"
tmp/        # matches ONLY a directory named "tmp"
```

### Pitfall 3: Forgetting that `*` doesn't cross slashes

```
logs/*.log   # matches logs/error.log — but NOT logs/archive/error.log
logs/**.log  # matches logs/error.log AND logs/archive/error.log
```

### Pitfall 4: Include rule placed after exclude rule

As explained in section 9, if a `- *.log` rule appears before `+ important.log`, the include is never reached.

### Pitfall 5: Excluding a directory itself kills all child include rules

When a directory is **itself** excluded (via `dir/` or `dir`), rsync stops descending into it entirely. Any include rule for something inside that directory is never evaluated — it is dead.

```
# BROKEN — rsync matches "- /something" and stops. Never sees /something/a:
+ /something/a
- /something
```

The fix depends on what you actually want:

**Option A — exclude the directory's *contents*, not the directory itself.**
Use `dir/*` instead of `dir/`. rsync will still walk into the directory, so child include rules are evaluated:

```
# WORKS — rsync enters /something/, finds "a", include rule fires:
+ /something/a
- /something/*
```

**Option B — keep the directory explicitly included, then exclude what you don't want.**
Necessary when you also need to block deeper nesting:

```
+ /something/
+ /something/a
- /something/*
```

**The key distinction to remember:**

| Pattern | Excludes the directory itself? | rsync descends? |
|---|---|---|
| `- /something/` | Yes | No — child rules are dead |
| `- /something/*` | No | Yes — child rules work |

### Pitfall 6: Inline comments

`#` only works as a comment at the **start** of a line:

```
*.log   # exclude all logs    ← WRONG: rsync reads this as a pattern
                              containing the "#" character

# exclude all logs            ← CORRECT comment
*.log
```

### Pitfall 7: Whitespace in patterns

rsync does not strip leading or trailing spaces. `  *.log` (with leading spaces) will not match `error.log`. Make sure there is no accidental whitespace.

### Pitfall 8: Confusing `--exclude-from` with `--files-from`

`--exclude-from` lists what to **skip**. `--files-from` lists what to **include** (only those files are synced). They are opposite in meaning.

---

## 13. Real-World Example Files

### General development project backup

```
# === Version control metadata ===
.git/***
.svn/***
.hg/***

# === Dependency directories ===
node_modules/***
vendor/***
__pycache__/***
.venv/***
venv/***

# === Build output ===
build/
dist/
out/
target/
*.class
*.pyc
*.o
*.a

# === Temporary and cache files ===
*.tmp
*.swp
*.swo
*~
.cache/
.DS_Store
Thumbs.db

# === Secrets and local config ===
.env
.env.local
.env.*.local
secrets.json
*.pem
*.key
```

### Home directory backup (excluding large media and system noise)

```
# === System and hidden noise ===
.Trash/***
.cache/***
.thumbnails/***
.local/share/Trash/***

# === Large media (backed up separately) ===
/Videos/***
/Music/***
/Downloads/***

# === Browser caches ===
.mozilla/firefox/*/cache*/***
.config/google-chrome/*/Cache/***

# === Virtual machine images ===
*.vmdk
*.vdi
*.iso
*.img

# === Log files ===
*.log
/var/log/***
```

### Web server document root sync (deployment)

```
# === Never deploy these to production ===
.git/***
.gitignore
.env
.env.*
*.env

# === Development tools ===
node_modules/***
/tests/***
/docs/***

# === Build artifacts not needed on server ===
*.map
*.ts
Makefile
Dockerfile
docker-compose*.yml

# === Local developer overrides ===
/config/local.php
/config/local.yml
wp-config-local.php
```

---

## 14. Quick Reference Cheat Sheet

| Pattern | Effect |
|---------|--------|
| `*.log` | Exclude all `.log` files anywhere in the tree |
| `/*.log` | Exclude `.log` files only in the source root |
| `tmp/` | Exclude any directory named `tmp` (at any depth) |
| `/tmp/` | Exclude only the top-level `tmp` directory |
| `dir/*` | Exclude contents of `dir/`, keep empty directory |
| `dir/***` | Exclude `dir/` and everything inside it |
| `**.log` | Exclude `.log` files at any depth (crosses `/`) |
| `file?.txt` | Exclude `file1.txt`, `fileA.txt`, etc. |
| `file[123].txt` | Exclude `file1.txt`, `file2.txt`, `file3.txt` |
| `+ *.jpg` | Include `.jpg` files (must come before a conflicting `-` rule) |
| `- *` | Exclude everything (catch-all, put last) |
| `# comment` | Ignored line — use for documentation |
| *(blank line)* | Ignored |

---

*Manual written for rsync 3.x. Test all patterns with `--dry-run` before running live transfers.*
