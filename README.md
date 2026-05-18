# JustFolders Backup

JustFolders is a backup tool built on a simple idea: your data should never depend on special software to be safe.

Instead of hiding files inside proprietary formats or databases, justfolders stores everything as plain directories and files. Every backup is a real folder you can open, browse, copy, or restore using nothing more than standard system tools. No lock-in, no decoding step, no surprises, timeless.

Backups are organized as snapshots over time, using efficient hard links so unchanged files aren’t duplicated. This keeps storage usage low while preserving a complete, point-in-time view of your data. You get the convenience of modern snapshot backups, with the transparency of a normal filesystem.

The goal is durability. Years from now—on a different machine, or even a different operating system—your backups will still be readable. If you can navigate folders, you can recover your data.

No agents.
No proprietary formats.
Just folders.

## Status

Early release. The CLI and config format may change before 1.0. Always verify your backups.

## Requirements

| Dependency | Where required | Notes |
|---|---|---|
| `rsync` | local machine (the machine running justfolders) | Used for all transfers. Any modern version works. |
| `ssh` | local machine | Required for remote sources or destinations. |
| `sha256sum` | **remote server** (only when destination is remote) | Used to generate and verify snapshot manifests. Part of GNU coreutils — present on all standard Linux distributions. Not needed for local-only backups. |

All three are standard tools available by default on any Linux system. macOS ships `ssh` and `rsync` but not `sha256sum` — use `brew install coreutils` if running justfolders on macOS against a remote destination.

## Command-line reference

JustFolders is invoked as `justfolders <COMMAND> [OPTIONS]`. Use `-h` / `--help` on any level for more detail:

```bash
justfolders --help              # list commands and global options
justfolders backup --help       # backup-specific options
justfolders init --help         # init-specific options
```

### Top level

When you run `justfolders` with no arguments, you see something like:

```text
JustFolders is a backup tool for creating folder snapshots without duplicating unchanged files.

Usage: justfolders <COMMAND>

Commands:
  backup       Run a backup job using the config file
  snapshots    List all available snapshots at the destination
  init         Generate a config file from provided arguments
  verify       Verify the integrity of a snapshot …
  test-notify  Send a test email using the [notify] SMTP settings
  help         Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help (see more with '--help')
  -V, --version  Print version
```

| Option | Meaning |
|--------|---------|
| `-h`, `--help` | Show the help text. With some clap versions, `--help` may show the long description from the binary; `-h` is often the compact view. |
| `-V`, `--version` | Print the JustFolders version and exit. |

### `backup` — run a backup

Reads `[default]` and `[policy]` from the config file, acquires the destination lock,
runs rsync into `tmp/`, promotes the snapshot, generates `SHA256SUMS`, applies the
retention policy, and prints a grouped summary of remaining snapshots.

```text
Usage: justfolders backup --config <FILE>

Options:
      --config <FILE>  Path to the config file
  -h, --help           Print help
```

| Option | Meaning |
|--------|---------|
| `--config <FILE>` | **Required.** Path to the `.ini` config file (see `init`). Relative paths are resolved from the current working directory. |
| `-h`, `--help` | Show help for this subcommand. |

### `snapshots` — list snapshots

Lists timestamped snapshot directories at the destination. If a `[policy]` section
is present in the config, output is grouped (Recent / Hourly / Daily / …) with a
one-line policy summary; otherwise names are printed flat, oldest first.

```text
Usage: justfolders snapshots --config <FILE>

Options:
      --config <FILE>  Path to the config file
  -h, --help           Print help
```

| Option | Meaning |
|--------|---------|
| `--config <FILE>` | **Required.** Same config as `backup` — only `src` / `dest` / `policy` matter for listing; the command uses the destination from `[default]`. |
| `-h`, `--help` | Show help for this subcommand. |

**Example output** (grouped mode, when `[policy]` is set). A `Policy:` line listing active
`keep_*` values is printed first, then each category. Names use `YYYY-MM-DD-HHMMSS`; within
each block they are **newest first**. Optional sections such as **Recent** (from
`keep_within_days`) or **Hourly** appear above **Daily** when those rules are enabled.
The program does not print a `Total:` line; the last line below is for readers only.

```text
Policy: daily=5  weekly=2  monthly=2  yearly=2

Daily
──────────────────────
2026-04-19-231542
2026-04-18-014403
2026-04-17-093015
2026-04-16-204811
2026-04-15-062247

Weekly
──────────────────────
2026-04-12-060515
2026-04-05-203911

Monthly
──────────────────────
2026-03-29-112847
2026-02-22-095302

Yearly
──────────────────────
2025-12-28-044128
2024-12-29-173656

Total: 11 snapshots
```

### `verify` — checksum a snapshot against its manifest

Re-reads every file listed in `SHA256SUMS` inside the snapshot and compares hashes.
Exits `0` if clean, `1` if any mismatch or missing file — suitable for cron and
monitoring.

```text
Usage: justfolders verify [OPTIONS] --config <FILE>

Options:
      --config <FILE>    Path to the config file
      --snapshot <NAME>  Snapshot to verify [default: latest]
  -h, --help             Print help
```

| Option | Meaning |
|--------|---------|
| `--config <FILE>` | **Required.** Config that points at the same destination as your backups. |
| `--snapshot <NAME>` | Which snapshot to check. Use the folder name `YYYY-MM-DD-HHMMSS`, or `latest` (default) to pick the lexicographically last timestamp in the listing. |
| `-h`, `--help` | Show help for this subcommand. |

### `test-notify` — send a test email

Loads the `[notify]` section from the config file and sends a single test
email using those SMTP settings. Useful for two things:

- Validating SMTP credentials right after you fill in `[notify]`, without
  having to wait for a real backup failure to find out something is wrong.
- Periodic health-checking from cron, so a silently broken SMTP setup (expired
  password, revoked app token, provider change) is noticed before the next
  backup actually needs to alert you.

Unlike the post-backup notification path — which deliberately swallows SMTP
errors so they cannot mask a real backup result — `test-notify` prints SMTP
errors directly and exits with a non-zero status on failure. That makes it
safe to use in monitoring pipelines.

```text
Usage: justfolders test-notify --config <FILE>

Options:
      --config <FILE>  Path to the config file
  -h, --help           Print help
```

| Option | Meaning |
|--------|---------|
| `--config <FILE>` | **Required.** Config file containing a `[notify]` section. If the section is missing the command fails with a clear error so you know notifications are not configured yet. |
| `-h`, `--help` | Show help for this subcommand. |

### `init` — generate a new config file

Creates a starter `.ini` from command-line arguments. **Required:** `--config`,
`--src`, and `--dest`. Everything else is optional and is written into the file
either as active lines or as commented placeholders (see generated file).

```text
Usage: justfolders init [OPTIONS] --config <FILE> --src <SRC> --dest <DEST>
```

| Option | Meaning |
|--------|---------|
| `--config <FILE>` | **Required.** Where to write the new config. If the path has no `.ini` extension, one is added automatically. Fails if the file already exists (delete or edit it by hand). |
| `--src <SRC>` | **Required.** Source to back up: local path, `user@host:/path`, or an SSH `Host` alias from `~/.ssh/config` (e.g. `myserver:/data`). A trailing `/` is normalized internally so rsync copies directory *contents*. |
| `--dest <DEST>` | **Required.** Backup root: local path or remote in the same forms as `--src`. Snapshot folders and `latest` are created inside this directory. |
| `-p`, `--port <PORT>` | SSH port for rsync’s `-e ssh -p …` when either side is remote. Written as `port = …` in `[default]`; defaults to 22 if omitted. |
| `-i`, `--id-rsa <FILE>` | Path to the private key for SSH. Written as `id_rsa = …`. Optional if your SSH config already sets `IdentityFile`. |
| `--rsync-flags <FLAGS>` | Full rsync argument string. Default is a safe, preserve-metadata set including `--delete` (see `--help` output for the exact default). Pass a quoted string if you need spaces. |
| `--disable-logs` | Sets `disable_logs = true` so rsync does not get `--log-file`. |
| `--log-dir <DIR>` | If set, rsync logs go here; written as `log_dir = …`. If omitted, the generated file uses a commented example and runtime default is `<dest>/.logs` when logging is enabled. |
| `--exclude-from <FILE>` | Path to an rsync exclude patterns file; written as `exclude_from = …`. |
| `--bwlimit <KB/s>` | Rsync bandwidth cap in kilobytes per second (e.g. `1000` ≈ 1 MiB/s). |
| `--keep-hourly <N>` | Seed `[policy] keep_hourly` in the generated file. |
| `--keep-daily <N>` | Seed `[policy] keep_daily`. |
| `--keep-weekly <N>` | Seed `[policy] keep_weekly`. |
| `--keep-monthly <N>` | Seed `[policy] keep_monthly`. |
| `--keep-yearly <N>` | Seed `[policy] keep_yearly`. |
| `--keep-within-days <N>` | Seed `[policy] keep_within_days` — keep **every** snapshot whose calendar date falls within the last *N* days from “today” when the policy runs (in addition to the `keep_*` counts). |
| `--dry-run` | Print the generated INI to stdout instead of writing the file. |
| `-h`, `--help` | Show help for this subcommand. |

Retention values from `init` only set the initial config; you can always edit
`[policy]` later. A value of `0` in the file means “disabled” for that rule.

## Usage

### 1. Generate a config file

```bash
justfolders init --config backup.ini \
  --src /home/user \
  --dest my-backup-server:/backups
```

Optional flags: `--port`, `--id-rsa`, `--log-dir`, `--exclude-from`, `--disable-logs`,
`--keep-within-days N`, `--keep-hourly N`, `--keep-daily N`, `--keep-weekly N`,
`--keep-monthly N`, `--keep-yearly N`, and more.
Use `--dry-run` to print the generated config to stdout without saving it.

### 2. Run a backup

```bash
justfolders backup --config backup.ini
```

After a successful run, JustFolders applies the **`[policy]`** section of your config,
deletes snapshots that no longer match the rules, then prints a grouped summary of what
remains. See **[Retention policy](#retention-policy)** below for how each rule behaves.

### How a backup run works

Every backup follows the same three steps:

1. **Lock** — a `backup.lock/` directory is created at the destination root containing
   the PID of the running process. This prevents two instances from writing to the same
   destination at the same time.

2. **Transfer** — rsync writes into a `tmp/` directory at the destination.
   Unchanged files are hard-linked from the `latest` snapshot instead of being copied,
   so only changed data is transferred.

3. **Promote** — on success, `tmp/` is renamed to a timestamped snapshot folder
   (e.g. `2025-04-15-120000`) and the `latest` symlink is updated. The lock is released.

### Interrupted and retried backups

If a backup is interrupted for any reason (network drop, Ctrl+C, power loss) the
`tmp/` directory is left as-is at the destination. This is intentional.

The next run will:
- Detect the stale `backup.lock/` (the previous PID is no longer alive) and clear it automatically.
- Resume rsync into the same `tmp/` directory. The `--delete` flag removes anything that
  should not be there, and `--link-dest` hard-links unchanged files from `latest` as usual.
- Rename `tmp/` to a snapshot once complete.

There is no manual cleanup required after an interrupted backup. Simply run
`justfolders backup --config backup.ini` again.

### Concurrent run protection

If a second instance is started while a backup is already running, it reads the PID from
`backup.lock/pid`, confirms the process is alive, and exits immediately with an error.
The running backup and its `tmp/` directory are left untouched.

If a `backup.lock/` is left behind by a crash so severe that the process entry is gone
(e.g. a hard reboot), it is detected as stale on the next run and cleared automatically.
No manual intervention is needed.

## Retention policy

Snapshot names are always `YYYY-MM-DD-HHMMSS`. The retention engine only **deletes**
directories that match that pattern; anything you rename (see [Pinning a snapshot forever](#8-pinning-a-snapshot-forever)) is ignored forever.

Configuration lives in the **`[policy]`** section of your `.ini` file:

```ini
[policy]
keep_within_days = 7    # 0 = disabled
keep_hourly      = 4
keep_daily       = 7
keep_weekly      = 4
keep_monthly     = 12
keep_yearly      = 5
```

- A key that is **missing** or set to **`0`** disables that rule.
- You can enable any subset; the rest stay off.
- **“Keep” always wins:** a snapshot is removed only if *no* rule requires keeping it.

### How rules interact (cascading buckets)

Count-based rules run in order from **fine → coarse**: **hourly → daily → weekly → monthly → yearly**.

For each step, JustFolders looks at snapshots **not yet claimed** by a finer rule. It also
**skips whole time buckets** (days, ISO weeks, months, years) that are *already represented*
by a snapshot kept earlier. That way **weekly** does not waste its slots on the same week
**daily** already covered — you get real older weeks instead of duplicates (this is a
common pitfall in other tools’ retention logic).

**`keep_within_days`** is special: count-based rules always see the **full** snapshot list
so cascading works correctly. *After* that, any snapshot whose **calendar date** falls in
the last *N* days (from the machine’s local “today” when the policy runs) is **forced kept**:
either re-labelled under **Recent** in the printed summary, or **rescued** from deletion if
no count-based rule picked it up. So you can run many backups per day and still keep all of
them for the recent window, while **daily / weekly / …** extend coverage further back.

### Rule reference

| Key | What it does |
|-----|----------------|
| **`keep_hourly`** | Keeps the **N most recent** snapshots whose date is **today** (local calendar date when the backup runs). It is **not** “one snapshot per clock hour”: two runs at 23:19 and 23:20 both count toward *N*. If you only back up weekly, most days have no “today” snapshots and this rule simply claims nothing useful — pair it with **daily** or rely on **`keep_within_days`**. |
| **`keep_daily`** | Keeps up to **N distinct calendar days**, choosing the **newest** snapshot in each day. Days already covered by **hourly** are skipped so coarser rules can use their quota on older periods. |
| **`keep_weekly`** | Keeps up to **N distinct [ISO weeks](https://en.wikipedia.org/wiki/ISO_week_date)** (week-year + week number), again the **newest** snapshot per bucket. Weeks already represented by **hourly/daily** snapshots are skipped. |
| **`keep_monthly`** | Up to **N distinct calendar months** (newest snapshot per month), skipping months already covered by finer rules. |
| **`keep_yearly`** | Up to **N distinct calendar years**, same skipping idea. |
| **`keep_within_days`** | Keeps **every** snapshot from the last **N calendar days** (inclusive of “today”), **no cap** on how many per day. Shown as **Recent** in grouped output. Anchored to the **real current date**, not “the date of your newest snapshot” — so behaviour stays predictable if you list snapshots a day after the last backup. |

### Edge cases and expectations

**Irregular backup schedules** (e.g. only on Fridays)  
You might have fewer distinct days/weeks in the destination than your *N* values. The
engine keeps as many **distinct** buckets as exist; it does not invent missing weeks.
**Weekly/monthly/yearly** only fill in once there are snapshots in **older** buckets not
already covered by **daily**.

**Combining `keep_within_days` with counts**  
Recent days can hold **many** snapshots (all kept). **Daily / weekly / …** still claim
their quotas from the full timeline first, then **Recent** is applied for display and for
anything in the window that would otherwise be deleted. You get both deep history *and*
a safe “keep everything recent” window.

**Very aggressive numbers** (e.g. `keep_daily = 1` only)  
After the next backup you might drop to a **single** retained snapshot besides the one just
created — by design. You are **not** left with **zero** snapshots after a successful backup:
the snapshot created in that run is always protected.

**Time zones and “today”**  
All calendar boundaries use the **system local timezone** of the machine running JustFolders
(`chrono::Local`). Align expectations with where backups run (e.g. cron on the same host).

**Changing policy in the `.ini`**  
The next `backup` run applies the new rules immediately. Snapshots that no longer match
may be deleted. Tightening policy can remove a lot of history in one go — plan ahead, or
**pin** important folders before changing numbers.

**What retention never touches**  
`tmp/`, `latest`, `backup.lock/`, `.logs/`, `SHA256SUMS`, and any directory whose name is
**not** exactly `YYYY-MM-DD-HHMMSS` are not subject to this algorithm.

For the full design notes (including why `keep_within_days` is a post-process), see the
module documentation in `src/core/retention.rs` in the repository.

### 3. List snapshots

```bash
justfolders snapshots --config backup.ini
```

### 4. Verify a snapshot

Re-checksums every file in a snapshot against the manifest recorded at backup time.
Use this to confirm a snapshot is safe to restore from before you actually need it.

```bash
# Verify the most recent snapshot
justfolders verify --config backup.ini

# Verify a specific snapshot
justfolders verify --config backup.ini --snapshot 2025-04-15-120000
```

Exits with code 0 if the snapshot is clean, code 1 if any file fails — safe to use
in scripts and monitoring pipelines.

### 5. Email notifications

justfolders can send an email after every backup run — always on failure,
optionally on success. Credentials live in the config file only and are never
passed on the command line.

After running `init`, open the generated `.ini` file and fill in the `[notify]` section:

```ini
[notify]
smtp_host     = smtp.example.com
smtp_port     = 587
smtp_user     = alerts@example.com
smtp_password = your-password-here
from          = justfolders@example.com
to            = admin@example.com
tls           = starttls   # starttls (default) | tls | none
on_success    = false      # set to true to also notify on successful backups
on_warnings   = true       # set to false to skip emails when rsync skipped some files but the snapshot was still created
```

The notification flags control which outcomes produce an email:

| Flag | Default | Behavior |
|------|---------|----------|
| (always on) | — | `Backup FAILED` is always sent and cannot be suppressed. |
| `on_warnings` | `true` | Send `Backup WARNING` when some files were skipped (typically permission denied or vanished mid-transfer) but the snapshot was still created. Set to `false` if skipped files are expected on your system and the emails become noise. Ignored when `strict = true` — in that mode warnings are escalated to failures and always notified. |
| `on_success`  | `false` | Send `Backup OK` on a clean run. Useful as a heartbeat for daily cron jobs to confirm the backup actually ran. |

Supported TLS modes:

| Value | Port | Use case |
|-------|------|----------|
| `starttls` | 587 | Gmail, most providers (default) |
| `tls` | 465 | Implicit TLS / some corporate servers |
| `none` | 25 | Local relay or development only |

If the SMTP send fails (e.g. wrong credentials, unreachable server), a warning is
printed to stderr and the backup exit code is unaffected — a notification failure
never masks a backup error.

**Test your SMTP setup with `test-notify`.** After filling in `[notify]`, send a
one-off test email to verify everything works before relying on it:

```bash
justfolders test-notify --config backup.ini
```

The command exits `0` if the email was delivered to the SMTP server and non-zero
otherwise, with the underlying SMTP error printed to stderr. This is also a good
candidate for a weekly cron job — running `test-notify` periodically catches
silently broken notifications (expired credentials, revoked app tokens, provider
changes) *before* a real backup failure tries to use them.

### 6. Error handling and failure modes

Backups touch a lot of files, and a long-running transfer can hit edge cases that
are not really "the backup failed" — a single file you cannot read, a temp file
that vanished mid-copy, a permission you forgot to grant on one subdirectory.
JustFolders distinguishes these soft conditions from genuine failures so a
perfectly good 500 GB snapshot is not thrown away because one cache file was
unreadable.

A backup run can finish in one of three states. The state is decided by rsync's
exit code — rsync deliberately uses different codes to communicate severity, and
JustFolders surfaces that distinction explicitly:

| State | rsync exit code | Snapshot created? | `latest` updated? | Notification subject |
|-------|-----------------|-------------------|-------------------|----------------------|
| **Clean**    | `0`           | yes    | yes    | `Backup OK: <src>` |
| **Warnings** | `23` or `24`  | yes    | yes    | `Backup WARNING: <src>` |
| **Failed**   | anything else | **no** | **no** | `Backup FAILED: <src>` |

- **Clean** — every file in the source was transferred. The normal case.
- **Warnings** — some files were not transferred, but the rest of the run
  completed. By far the most common cause is *permission denied* on individual
  files (e.g. backing up `/` as a non-root user and hitting `/root/.ssh/…`),
  followed by files that vanished mid-transfer (rotating logs, browser caches,
  sockets). The snapshot is still created from the files that *were*
  transferred, and `latest` is updated. The rsync exit code is included in the
  email so you can tell which case it is — `23` ("partial transfer due to
  error") vs `24` ("vanished source files").
- **Failed** — anything else: SSH could not connect, the destination ran out of
  disk, rsync crashed, the lock could not be acquired, the manifest step
  failed, etc. The `tmp/` directory (where rsync stages the in-progress
  transfer) is left on disk for inspection but is **not** promoted to a real
  snapshot, and `latest` is **not** changed. The
  backup chain is unaffected — the previous `latest` snapshot is still your
  most recent good backup.

#### Why warnings are not failures by default

The mainstream convention across backup tools is to treat "some files could
not be read" as a soft condition rather than a hard failure. The reasoning is
straightforward: on any realistically sized filesystem, you will eventually
hit a file you cannot read (permission, lock, transient I/O issue), and the
alternative — discarding the entire snapshot because of one such file —
means large filesystems silently fail to back up forever.

JustFolders follows the same convention: warnings produce a usable snapshot,
and the user is told which files were skipped via the notification email so the
underlying issue can be investigated.

#### Email notifications are strongly recommended

Because warnings and failures only become visible through the notification
email (and the rsync log file, but cron jobs typically run unattended),
**setting up SMTP via the `[notify]` section is strongly recommended for any
production backup**. Without notifications, a silently degrading backup — for
example, a permission that was recently changed and now blocks 30% of your home
directory — will not be noticed until you actually need to restore.

The notification rules are simple:

- `Backup FAILED` is sent **always**, regardless of any flag — failure emails
  cannot be suppressed.
- `Backup WARNING` is sent when `on_warnings = true` (the default).
- `Backup OK` is sent when `on_success = true` (default off — useful for daily
  "yes, the backup ran" peace-of-mind emails).

See [Email notifications](#5-email-notifications) for how to fill in the
`[notify]` section.

#### Strict mode — failing on any skipped file

For environments where a partial backup is not acceptable (regulated,
compliance, anything covered by an SLA that says "every file"), set
`strict = true` in the `[default]` section:

```ini
[default]
src    = /home/me
dest   = /backups
strict = true
```

With `strict = true`, rsync exit codes 23 and 24 are escalated to a hard
failure: the `tmp/` directory is **not** promoted, `latest` stays pointing
at the previous snapshot, and a `Backup FAILED` email is sent. Use this only
when you would genuinely rather retry the backup than keep a partial one — for
most users the default (`strict = false`) is the right choice.

#### Use cases

A few worked examples of how the three-state model handles real situations:

**1. Permission denied on a single file.**  
You back up `/home/alice` as the `alice` user, and a colleague leaves a file
with `chmod 000` in `~/projects/`. rsync prints "Permission denied" for that
one file and exits 23. JustFolders creates the snapshot, promotes `latest`, and
sends a `Backup WARNING` email containing the rsync output. You see exactly
which file was skipped and can either fix the permission or add the file to
`exclude_from`.

**2. Files vanishing on a live system.**  
You back up a live web server while `/var/log/nginx` rotates mid-transfer. A
few log files disappear between rsync indexing the directory and copying them.
Exit code 24. Snapshot is created, email body lists the vanished files. If
this becomes routine noise, set `on_warnings = false` in the `[notify]` section
— the snapshot is still made, you just stop receiving the email.

**3. Destination is unreachable.**  
The backup server is rebooting when the backup starts. SSH fails to connect,
rsync exits with code 255. JustFolders sends a `Backup FAILED` email and exits
non-zero. The previous `latest` snapshot is untouched and the next scheduled
run picks up automatically.

**4. Destination disk full.**  
rsync exits with code 11 (file I/O error) midway through the transfer.
`Backup FAILED` email is sent. The partially-written `tmp/` directory is left
on disk so you can inspect it (or `rm -rf` it once you have made room).
`latest` still points at the previous snapshot, so older backups remain usable.

**5. Strict mode on a regulated dataset.**  
You run a `strict = true` backup of a dataset under SLA, and a colleague's
home directory has unreadable cache files. rsync exits 23. Because strict is
enabled this becomes a hard failure: the snapshot is discarded and a
`Backup FAILED` email is sent. You fix the permissions and retry — the next
run either succeeds cleanly or fails for a real reason. You never end up with
a half-complete snapshot you might mistake for a good one.

#### Where to look when something goes wrong

In order of usefulness:

1. **The notification email body** — contains the rsync exit code and the
   captured stderr (which lists the actual unreadable files for code 23, or
   vanished files for code 24).
2. **Terminal stderr** — rsync's stderr is also mirrored to the terminal as
   the backup runs, so anything in the email is also visible if you ran the
   backup interactively.
3. **`<dest>/.logs/<timestamp>.log`** — written by rsync's `--log-file`
   (unless `disable_logs = true`). Contains the per-file transfer log.
4. **The `tmp/` directory** under the destination — where rsync stages the
   in-progress transfer. Left on disk after a hard failure for inspection.
   Safe to delete (`rm -rf <dest>/tmp`) once you no longer need it. A
   successful next run will simply create it again.

### 7. Restore

There is no restore command — and there does not need to be one.

Every snapshot is a plain directory containing your files exactly as they were at that
point in time. To restore, just copy what you need using any standard tool:

```bash
# Restore a single file
cp /backups/2025-04-15-120000/home/user/document.txt /home/user/document.txt

# Restore an entire directory
cp -r /backups/2025-04-15-120000/home/user/projects /home/user/projects

# Use rsync for a full restore with progress
rsync -av /backups/latest/ /home/user/
```

For remote destinations, use the same approach over SSH or rsync:

```bash
# Restore a single file from a remote backup
scp my-backup-server:/backups/2025-04-15-120000/home/user/document.txt /home/user/document.txt

# Full restore from remote with rsync
rsync -av my-backup-server:/backups/latest/ /home/user/
```

Because snapshots are plain folders, you can also browse them in any file manager,
open files directly without any extraction step, or restore on a machine that has never
had JustFolders installed. No special software is needed — ever.

### 8. Pinning a snapshot forever

The retention engine only considers directories whose name matches the
`YYYY-MM-DD-HHMMSS` format. **Renaming a snapshot to anything else makes it
completely invisible to the retention algorithm** — it will never be touched,
regardless of how aggressive the policy is.

This is the simplest way to preserve a specific point-in-time permanently:
a release, a state before a migration, a known-good baseline, etc.

```bash
# Local — rename in place
mv /backups/2025-04-15-120000 /backups/before-migration

# Remote — rename over SSH
ssh my-backup-server "mv /backups/2025-04-15-120000 /backups/before-migration"
```

The renamed folder sits alongside the regular timestamped snapshots and is left
untouched by every future backup run. Restore from it the same way as any other
snapshot — it is still just a plain folder.

To unpin it later (put it back under retention control), rename it back to the
original timestamp:

```bash
mv /backups/before-migration /backups/2025-04-15-120000
```

### 9. Creating a self-contained archive

A snapshot is a directory of hard-linked files — the files are space-efficient on
disk but share underlying data with other snapshots. Running `tar` on a snapshot
breaks those hard links and writes each file's data independently into the archive.
The result is a **fully self-contained, independent copy** of that exact point in
time — no dependency on any other snapshot, safe to move to cold storage, an
external drive, or a cloud bucket.

```bash
# Local — create a compressed archive in an archives folder
mkdir -p /backups/archives
tar -czf /backups/archives/snapshot-2025-04-15.tar.gz \
    -C /backups 2025-04-15-120000

# Remote — create the archive directly on the server
ssh my-backup-server \
    "mkdir -p /backups/archives && \
     tar -czf /backups/archives/snapshot-2025-04-15.tar.gz \
         -C /backups 2025-04-15-120000"

# Remote — stream the archive to local storage
# (no temporary disk space needed on the server)
ssh my-backup-server \
    "tar -czf - -C /backups 2025-04-15-120000" \
    > /local/archives/snapshot-2025-04-15.tar.gz
```

Because the archive filename does not match `YYYY-MM-DD-HHMMSS`, it is invisible
to the retention engine and will never be automatically deleted — just like a
pinned folder.

To extract the archive when you need it:

```bash
tar -xzf snapshot-2025-04-15.tar.gz
```

> **Note on size:** Because `tar` stores each file independently, the archive
> will be larger than the snapshot's apparent on-disk usage (which benefits from
> hard-link deduplication). The archive size reflects the true, unshared size of
> the data at that point in time — which is exactly what makes it a reliable
> standalone copy.

**Data integrity:** The archive is not only convenient for transport; it is a
**second physical embodiment** of the same logical snapshot. Hard-linked
snapshots on one volume share blocks; a silent error in those blocks can surface
in every snapshot that references them. The `tar` stream writes out each file’s
bytes anew, so the archive’s payload is **not** tied to those original inodes.
Store the archive on **different media** (another disk, removable drive, or
offsite object storage) and you materially **improve resilience** to media
degradation and localized corruption for that frozen point in time—while
accepting that the archive medium must itself be monitored and verified over its
lifetime. This complements, rather than replaces, [`verify`](#4-verify-a-snapshot)
on live snapshots. For the underlying tradeoff, see
[The hard-link and bit-rot tradeoff](#the-hard-link-and-bit-rot-tradeoff).

## Storage efficiency and data safety

### How hard-link deduplication works

When a file has not changed since the previous snapshot, justfolders does not copy it
again — rsync creates a hard link pointing to the same inode on disk. The new snapshot
appears as a complete directory of files, but unchanged files share physical storage with
older snapshots.

```
snapshot_2025-04-01/home/user/photo.jpg  ──┐
snapshot_2025-04-02/home/user/photo.jpg  ──┤──▶  inode 48291  (one physical copy)
snapshot_2025-04-03/home/user/photo.jpg  ──┘
```

The result: you can keep dozens of point-in-time snapshots for roughly the cost of one
full copy plus the accumulated changes.

### The hard-link and bit-rot tradeoff

Hard-link deduplication is efficient, but it has an important implication: **if the
underlying inode develops silent corruption (bit-rot), every snapshot that hard-links to
it is affected at once.** A corrupt inode is a corrupt inode — the number of directory
entries pointing to it does not matter.

This means:

- Multiple snapshots on the **same disk** protect you against accidental deletion and
  logical errors, but not against disk-level corruption or hardware failure — if the
  disk dies, every snapshot dies with it, regardless of how many you kept.
- To be protected against hardware failure and bit-rot, you need an independent copy
  on a **different physical device**.

**Self-contained archives.** A practical way to obtain such a copy for a *specific*
point in time—without giving up space-efficient snapshots for routine retention—is
described in [Creating a self-contained archive](#9-creating-a-self-contained-archive).
Because archiving **materializes** each file’s data into a new artifact (hard links
are not preserved as shared storage inside the archive), corruption of blocks on the
primary backup volume does not automatically invalidate bytes already written into
an archive you stored **elsewhere**. Used together with offsite or secondary media,
that improves your odds of recovering a known-good state when the live snapshot
tree is no longer trustworthy. It is not a substitute for diverse backups and
periodic verification; it is an additional layer for milestones you care about most.

### The 3-2-1 rule

The industry standard for reliable backups is the **3-2-1 rule**:

| | What it means |
|---|---|
| **3** copies of your data | original + at least two backups |
| **2** different storage media | e.g. local disk and a remote server |
| **1** copy offsite | physically separate from the others |

JustFolders is well-suited as one layer of a 3-2-1 strategy. A practical setup:

```
Source machine  ──▶  local NAS (justfolders job 1, fast, for quick restores)
Source machine  ──▶  remote server or cloud (justfolders job 2, offsite, for disasters)
```

Each job has its own config file and runs independently. The two destinations are on
different physical hardware, so a corrupted inode on the NAS does not affect the remote
copy — and vice versa.

### A note on ZFS and btrfs

If your destination filesystem is **ZFS** or **btrfs** with checksumming enabled, it
performs block-level checksum verification on every read and can detect (and with
redundancy, repair) silent corruption automatically. This largely eliminates the bit-rot
risk for files stored on that filesystem.

On standard filesystems (ext4, xfs, NTFS, network shares) there is no such protection.
Running [`justfolders verify`](#4-verify-a-snapshot) periodically re-checksums files
against the manifest recorded at backup time and helps detect corruption early.

## Pull vs push backups

Backups can be initiated from either side of the connection:

- **Push** — the source machine connects to the backup destination and sends its data there.
- **Pull** — the backup server connects to the source machine and copies the data back.

JustFolders supports both. Because `src` and `dest` in the config can each be local or remote,
the same binary covers either topology — run it on the source for a push setup, or on the
backup server for a pull setup. Snapshotting, hard-linking, and retention work identically;
the only thing that changes is **who holds the credentials and who initiates the connection.**

That choice has real security implications and is worth making deliberately.

### Pull (backup server initiates)

The backup server holds an SSH key to each source it backs up. Sources do not need any
credentials to reach the backup destination.

**Advantages**

- **The source cannot reach the backup.** If a source machine is compromised — ransomware,
  intrusion, a runaway script — the attacker has no path to the backup server. They cannot
  delete, overwrite, or encrypt snapshots stored elsewhere. This is the strongest argument
  for pull, and the main reason it is preferred for high-value targets.
- **Centralized scheduling and inventory.** All cron entries, configs, and logs live on one
  machine. Adding a new source means adding one config file on the backup server.
- **Sources stay minimal.** Source machines need only an inbound SSH listener and `rsync` —
  no backup software, no outbound credentials, no scheduler.

**Downsides**

- **Inbound SSH on every source, usually with broad read access** (often `root`) so the
  puller can capture system files, mail spools, database dumps, and so on. That is a
  non-trivial credential and attack surface to grant, and it is awkward when sources sit
  behind NAT or strict firewalls.
- **The backup server becomes a high-value target.** It now holds keys that can read
  everything on every source. A compromise of the backup server is, effectively, a
  compromise of every source it pulls from. Harden it accordingly: dedicated host, no
  general-purpose use, restricted SSH, and forced rsync commands on the source side
  (`rrsync -ro <path>`) so each key can only read the path it is meant to read.
- **Source-side state changes are harder to coordinate.** Tasks like "dump the database,
  then back up", or "snapshot the LVM volume, back up the snapshot, release it" are awkward
  when the puller does not directly own those steps. They typically end up as scheduled
  jobs on the source plus a time window the puller has to hit — fragile compared to a
  single ordered script running on the source itself.

### Push (source initiates)

The source machine holds an SSH key to the backup destination and writes its own snapshots there.

**Advantages**

- **Friendlier networking.** Only outbound connections from the source — fine through NAT,
  dynamic IPs, laptops, and cloud instances behind tight ingress rules. The backup server
  just needs to accept incoming SSH from known keys.
- **Local control of timing.** Pre-backup work like database dumps, application quiescing,
  or filesystem snapshots happens in the same script on the same host, immediately before
  the push. No cross-machine choreography.
- **Less trust concentrated on the destination.** Each source only has credentials to its
  own destination path, not to other sources. Compromising the backup host still loses the
  archive, but it does not directly hand the attacker read access into every source.

**Downsides**

- **A compromised source can damage its own backups.** Whatever credentials the source
  uses to push are usable by anyone who takes that source over — including for deleting
  or overwriting the snapshots already there. This is the central weakness of naive push
  setups and the main thing ransomware exploits.
- **Mitigations need real effort.** Common defences include: forcing the SSH key into a
  write-restricted rsync wrapper (`rrsync -wo <path>`, though that conflicts with anything
  that needs to rename or delete on the destination — including JustFolders' `tmp/` →
  snapshot promotion and retention), giving the source a chrooted account whose tree it
  cannot escape, and — most robustly — having the **backup server itself** take periodic
  filesystem snapshots (ZFS, btrfs, LVM) of the destination on a schedule the source
  cannot influence. Without one of these, "push" and "secure against source compromise"
  do not coexist.
- **Distributed configuration.** Each source carries its own config, its own cron, its own
  logs. Auditing "did everything back up last night" means walking every source, not
  reading one log.

### Practical guidance

A robust real-world setup often combines both:

- Run JustFolders in **push** mode from each source for fast, frequent, locally-scheduled
  backups — convenient, and a good fit for NAT and firewall realities.
- On the destination, take periodic **filesystem-level snapshots** (ZFS / btrfs / LVM) that
  the pushing sources have no way to delete. These act as a tamper-resistant safety net
  behind the per-source snapshot tree.
- For the most sensitive systems, also run a **second JustFolders job in pull mode** from a
  hardened backup host — separate machine, separate keys, separate schedule — so that a
  full compromise of the source plus its primary destination still leaves an isolated copy
  somewhere else.

This is the same idea as the [3-2-1 rule](#the-3-2-1-rule) extended to trust boundaries:
different media, different locations, *and* different attack paths — so a single compromise
does not propagate to every copy.

## SSH Key Configuration

JustFolders delegates all remote connections to rsync and SSH.
You can supply a private key either via `--id-rsa` in the `init` command (written into
the config file as `id_rsa = ...`) or by configuring it in your SSH config file, which
is the most flexible approach for complex setups.

### Authorizing the key on the server

After generating your key, you need to allow it to connect on the remote server.
The simplest way is using `ssh-copy-id`:

```bash
ssh-copy-id -i ~/.ssh/my_backup_key.pub root@192.168.1.10
```

Or manually append the public key to the server's `~/.ssh/authorized_keys`:

```bash
cat ~/.ssh/my_backup_key.pub | ssh root@192.168.1.10 "mkdir -p ~/.ssh && cat >> ~/.ssh/authorized_keys"
```

Make sure the permissions are correct on the server, otherwise SSH will refuse the key:

```bash
chmod 700 ~/.ssh
chmod 600 ~/.ssh/authorized_keys
```

Incorrect permissions on `authorized_keys` is one of the most common reasons
SSH key authentication silently falls back to password authentication.

### Setting up per-host keys

Edit or create `~/.ssh/config` and add an entry for each backup server:

```
Host my-backup-server
HostName 192.168.1.10
User root
IdentityFile ~/.ssh/my_backup_key
Port 22
```

Then use the hostname alias directly in your config:

```bash
justfolders init --config backup.ini --src /home/user --dest my-backup-server:/backups
```

### Generating a dedicated backup key

It is recommended to use a dedicated key for backups rather than your personal key:

```bash
ssh-keygen -t ed25519 -f ~/.ssh/my_backup_key -C "justfolders backup"
```

Then copy the public key to the server:

```bash
ssh-copy-id -i ~/.ssh/my_backup_key.pub root@host
```

### Passwordless access for automated backups

If you run JustFolders as a cron job or scheduled task, make sure the key
has no passphrase, or use `ssh-agent` to unlock it once per session:

```bash
eval $(ssh-agent)
ssh-add ~/.ssh/my_backup_key
```

## Excluding files and directories

JustFolders can skip selected files and directories on every backup run by forwarding an rsync exclude-patterns file to rsync. Point at the file with `--exclude-from <FILE>` when generating a config (or add `exclude_from = /path/to/excludes.txt` to the `[default]` section of an existing `.ini`); the patterns then apply automatically on each `justfolders backup`.

For the full pattern syntax — wildcards, anchoring, include rules, common pitfalls, and ready-to-copy templates — see the [Exclude-patterns file reference](rsync-exclude-from-manual.md).

## Warning

Backups are critical. Bugs can and do happen.
Always keep multiple backup copies using different tools and storage locations.

## Contributions

By submitting a contribution (pull request), you agree that your code will be licensed under the same MIT License as this project.

## License

This project is licensed under the MIT License – see the LICENSE file for details.

## Disclaimer

This software is provided "as is", without any warranties or guarantees of any kind.

The author is not responsible for any data loss, corruption, or damage that may occur from using this software.

This tool is intended for experimental and personal use. It has not been thoroughly tested for all scenarios.

DO NOT rely on this software as your only backup solution.
Always verify your backups and test restore procedures regularly.

Use at your own risk.
