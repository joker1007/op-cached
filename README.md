# op-cached

A daemon + CLI that caches `op read` / `op inject` results from the 1Password CLI in memory,
encrypted with your default GPG key, to make repeated lookups fast.

Requires `op` (1Password CLI) and `gpg` to be installed. Nothing is persisted to disk.

## Installation

```sh
cargo install --git https://github.com/joker1007/op-cached
```

## Usage

```sh
op-cached read "op://vault/item/field"          # auto-starts the daemon if needed
op-cached inject -i template.env                # caches the `op inject` result per file
op-cached inject -i template.env -o .env
op-cached status                                # entry count / TTL / uptime
op-cached clear                                 # drop all cached entries
op-cached stop                                  # stop the daemon
op-cached daemon --ttl 12h                      # run the daemon explicitly in the foreground
```

- `read` caches values per `op://` URL.
- `inject` runs `op inject -i FILE` and caches the whole output per file (canonical path).
  The file's mtime is recorded; if it changes the entry is re-generated. Templates from stdin are not supported.
- Values are encrypted with `gpg --default-recipient-self --encrypt` when cached and decrypted via
  `gpg --decrypt` (through gpg-agent) on every lookup. The daemon never holds plaintext.

## Configuration

| Setting | CLI | Environment | Default |
|---|---|---|---|
| TTL | `daemon --ttl 7d` | `OP_CACHED_TTL` | `7d` |
| Socket | `--socket PATH` | `OP_CACHED_SOCKET` | `$XDG_RUNTIME_DIR/op-cached.sock` (falls back to `/tmp/op-cached-<uid>.sock`) |

TTL accepts humantime formats such as `7d`, `12h`, `30m`. Expired entries are dropped on lookup and
by a sweep that runs every 60 seconds.

## Running as a systemd user service

```ini
# ~/.config/systemd/user/op-cached.service
[Unit]
Description=op-cached daemon

[Service]
ExecStart=%h/.cargo/bin/op-cached daemon
Environment=OP_CACHED_TTL=7d
Restart=on-failure

[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now op-cached
```

## License

MIT
