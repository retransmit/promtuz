# pzresolver

Promtuz resolver — a stateless relay discovery service. Relays register here so
clients can find them; it keeps only an in-memory map of which relays are online
and owns no message data. Part of [promtuz](../README.md).

Resolvers are run by the operator (you), not volunteers, so deployment is
deliberate and manual.

## Install (Debian / Ubuntu)

Add the Promtuz apt repo (the same one relays use), then install:

```sh
sudo install -d -m 0755 /etc/apt/keyrings
sudo curl -fsSL https://apt.promtuz.dev/promtuz-archive-keyring.asc \
     -o /etc/apt/keyrings/promtuz.asc

echo "deb [signed-by=/etc/apt/keyrings/promtuz.asc] https://apt.promtuz.dev edge main" \
  | sudo tee /etc/apt/sources.list.d/promtuz.list

sudo apt update && sudo apt install pzresolver
```

## Configure + run

```sh
# Place the resolver's cert/key (its node.key pubkey is the resolver IPK that
# relays seed in their configs):
sudo cp node.crt node.key /etc/pzresolver/

# Check the bind address:
sudoedit /etc/pzresolver/resolver.toml

sudo systemctl enable --now pzresolver
journalctl -u pzresolver -f
```

## Update

```sh
sudo apt update && sudo apt upgrade     # config preserved
```

## Paths

| What | Where |
|------|-------|
| binary | `/usr/bin/pzresolver` |
| config | `/etc/pzresolver/resolver.toml` (conffile) |
| certs + CA | `/etc/pzresolver/` (`node.crt`, `node.key`, `ca.pem`) |
| logs | `journalctl -u pzresolver` |
| version | `pzresolver --version` |

Stateless — no database, no `/var/lib`. Runs as the unprivileged `pzresolver`
user under a hardened systemd unit.

## Build from source

```sh
./scripts/build-deb.sh resolver
# → target/x86_64-unknown-linux-gnu/debian/pzresolver_<version>_amd64.deb
```
