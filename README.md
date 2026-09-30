# ErisAuth

Sign apps in to Eris services from a device you already trust, with one approval. Crate
`erisauth`.

ErisAuth builds on [ErisLogin](https://github.com/Eriskii/ErisLogin), the pairing every Eris service
speaks on its own: a ticket, a fingerprint on both screens, and a person approving all, some or none
of what an app asks for. Every app keeps working with ErisLogin alone; ErisAuth approves on the
person's behalf, and nothing an app does afterwards passes through it.

## Home

Home is ErisAuth's plugin in [ErisDB](https://github.com/Eriskii/ErisDB). It is an ErisDB
installation holding `*`, and it approves a pending pairing for a registered ErisAuth device that
brings the passphrase. A device approves what fits inside its own grants by itself; for anything more
it asks home, and the person types the passphrase. A stolen device without the passphrase gets only
its own grants, and the passphrase without an enrolled device gets nothing.

| Operation    | Permission            | Input                                          | Answer                 |
| ------------ | --------------------- | ---------------------------------------------- | ---------------------- |
| `approve`    | `erisauth:approve`    | `{pairing, granted, identity, passphrase}`     | `200 {approved}`       |
| `passphrase` | `erisauth:passphrase` | `{current, new}`, `new` at least 8 characters  | `200 {changed: true}`  |

`approve` approves `pairing` with `granted` only when the key that redeemed it is `identity`, the app
the device vouched for. Refusals are JSON `{error, message}`:

| Status | `error`            | Meaning                                                               |
| ------ | ------------------ | --------------------------------------------------------------------- |
| 403    | `not_a_device`     | The caller is not a registered installation (a manual token)          |
| 403    | `wrong_passphrase` | Counted against this device                                           |
| 429    | `locked`           | This device waits `retry_after` seconds before guessing again         |
| 404    | `no_such_pairing`  |                                                                       |
| 409    | `not_waiting`      | The pairing is not waiting for an answer                              |
| 409    | `not_the_app`      | A different key redeemed the pairing than the device vouched for      |

Home keeps the passphrase as an Argon2id hash in ErisDB's `erisauth.home` facet, which only home and
the operator can read, with each device's wrong guesses. A device has three free guesses; then it
waits a minute, then two, then four, doubling up to a day. A right guess clears its count, and one
device's guesses never lock out another.

### Install

On the ErisDB machine, as the user ErisDB runs as:

```sh
cargo install --locked --path . --bins
sudo install -m 0755 ~/.cargo/bin/erisdb-plugin-erisauth /usr/local/libexec/erisdb/
sudo install -m 0644 deploy/erisauth.json /etc/erisdb/plugins.d/
sudo install -d -o erisdb -g erisdb -m 0700 /var/lib/erisauth
```

Restart ErisDB so it loads the manifest. Cut a ticket with `erisdb pair` and hand it to setup:

```sh
erisauth home setup --erisdb http://127.0.0.1:7700 'erisdb://pair/…'
```

Setup shows a fingerprint to compare with the `erisdb pair` terminal. Approve the request as asked:
home asks for `*`. Setup then registers the facet and asks for the passphrase twice.

| Command                               | What it does                                              |
| ------------------------------------- | --------------------------------------------------------- |
| `erisauth home passphrase`            | Set a new passphrase without the old one                   |
| `erisauth home unlock [INSTALLATION]` | Let one device, or every device, guess again now           |

Each takes `--home DIR`, default `/var/lib/erisauth`, which must match the `--home` in the manifest.
Home's credential there is `erisdb.json`, mode 0600: back it up as a credential, or revoke the
installation and run setup again.

## Testing

```sh
cargo test
```

The suite runs a real Postgres (Docker), a real ErisDB core serving the shipped manifest, and the
real `erisauth` and plugin executables.
