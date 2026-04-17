# JustFolders Backup

JustFolders is a backup tool built on a simple idea: your data should never depend on special software to be safe.

Instead of hiding files inside proprietary formats or databases, justfolders stores everything as plain directories and files. Every backup is a real folder you can open, browse, copy, or restore using nothing more than standard system tools. No lock-in, no decoding step, no surprises, timeless.

Backups are organized as snapshots over time, using efficient hard links so unchanged files aren’t duplicated. This keeps storage usage low while preserving a complete, point-in-time view of your data. You get the convenience of modern snapshot backups, with the transparency of a normal filesystem.

The goal is durability. Years from now—on a different machine, or even a different operating system—your backups will still be readable. If you can navigate folders, you can recover your data.

No agents.
No proprietary formats.
Just folders.

## Requirements

| Dependency | Where required | Notes |
|---|---|---|
| `rsync` | local machine (the machine running justfolders) | Used for all transfers. Any modern version works. |
| `ssh` | local machine | Required for remote sources or destinations. |
| `sha256sum` | **remote server** (only when destination is remote) | Used to generate and verify snapshot manifests. Part of GNU coreutils — present on all standard Linux distributions. Not needed for local-only backups. |

All three are standard tools available by default on any Linux system. macOS ships `ssh` and `rsync` but not `sha256sum` — use `brew install coreutils` if running justfolders on macOS against a remote destination.

## Usage

### 1. Generate a config file

```bash
justfolders init --config backup.ini \
  --src /home/user \
  --dest my-backup-server:/backups
```

Optional flags: `--port`, `--id-rsa`, `--log-dir`, `--exclude-from`, `--disable-logs`,
`--keep-daily N`, `--keep-weekly N`, `--keep-monthly N`, `--keep-yearly N`, and more.
Use `--dry-run` to print the generated config to stdout without saving it.

### 2. Run a backup

```bash
justfolders backup --config backup.ini
```

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
```

Supported TLS modes:

| Value | Port | Use case |
|-------|------|----------|
| `starttls` | 587 | Gmail, most providers (default) |
| `tls` | 465 | Implicit TLS / some corporate servers |
| `none` | 25 | Local relay or development only |

If the SMTP send fails (e.g. wrong credentials, unreachable server), a warning is
printed to stderr and the backup exit code is unaffected — a notification failure
never masks a backup error.

### 6. Restore

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
Running `justfolders verify` periodically — once implemented — will fill this gap by
re-checksumming snapshot files against a manifest recorded at backup time.

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
