# JustFolders Backup
JustFolders is a backup tool built on a simple idea: your data should never depend on special software to be safe.

Instead of hiding files inside proprietary formats or databases, justfolders stores everything as plain directories and files. Every backup is a real folder you can open, browse, copy, or restore using nothing more than standard system tools. No lock-in, no decoding step, no surprises, timeless.

Backups are organized as snapshots over time, using efficient hard links so unchanged files aren’t duplicated. This keeps storage usage low while preserving a complete, point-in-time view of your data. You get the convenience of modern snapshot backups, with the transparency of a normal filesystem.

The goal is durability. Years from now—on a different machine, or even a different operating system—your backups will still be readable. If you can navigate folders, you can recover your data.

No agents. 
No proprietary formats.
Just folders.











## SSH Key Configuration

JustFolders delegates all remote connections to rsync and SSH.
Rather than accepting a private key path as a CLI argument, key configuration
is handled through your SSH config file — which is the standard and most flexible approach.

### Setting up per-host keys

Edit or create `~/.ssh/config` and add an entry for each backup server:
```
Host my-backup-server
HostName 192.168.1.10
User root
IdentityFile ~/.ssh/my_backup_key
Port 22
```

Then use the hostname alias directly as your destination:

```bash
justfolders backup /home/user my-backup-server:/backups
```

### Multiple servers with different keys
```
Host server-home
HostName 192.168.1.10
User root
IdentityFile ~/.ssh/key_home
Port 22
Host server-offsite
HostName backup.example.com
User backup
IdentityFile ~/.ssh/key_offsite
Port 2222
```

```bash
justfolders backup /home/user server-home:/backups
justfolders backup /home/user server-offsite:/backups
```

Each host uses its own key automatically.

### Generating a dedicated backup key

It is recommended to use a dedicated key for backups rather than your personal key:

```bash
ssh-keygen -t ed25519 -f ~/.ssh/my_backup_key -C "justfolders backup"
```

Then copy the public key to the server:

```bash
ssh-copy-id -i ~/.ssh/my_backup_key.pub root@192.168.1.10
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
