# ErisAuth

Sign apps in to Eris services from a device you already trust, with one approval. Crate
`erisauth`.

ErisAuth builds on [ErisLogin](https://github.com/Eriskii/ErisLogin), the pairing every Eris service
speaks on its own: a ticket, a fingerprint on both screens, and a person approving all, some or none
of what an app asks for. Every app keeps working with ErisLogin alone; ErisAuth approves on the
person's behalf, and nothing an app does afterwards passes through it.

## On a device

A device joins ErisDB once, as an installation of its own over Iroh:

```sh
erisauth join --name laptop --ceiling 'notes:*' --ceiling 'tasks:*' 'erisdb://pair/…'
```

Cut the ticket with `erisdb pair`, or show one on a device that has joined (below), compare the
fingerprint, and approve. A device without a terminal joins from its approval page instead: a daemon
that has not joined shows a form for the ticket, a name and the ceiling, then the fingerprint, and
becomes a working device once someone approves. A link to the page carrying `&join=<ticket>` in its
fragment fills the ticket in. The device asks for the
right to open and approve pairings, to ask [home](#home), and to announce itself to and invite the
other devices, plus its ceiling: the grants it may
approve for an app by itself. Without `--ceiling` the ceiling is everything (`*`). Its key and
standing live in `$XDG_DATA_HOME/erisauth/`.

`erisauth serve` then listens on `$XDG_RUNTIME_DIR/erisauth/agent.sock`, where apps look for it with
ErisLogin's `agent` module. An ErisDB app in Rust does all of it in one call,
`erisdb_client::pair_via_erisauth`, which returns `None` when no ErisAuth is listening:

1. The app says which service it wants, its name, the grants and the key it will redeem with.
2. The device cuts a pairing at ErisDB and hands the app the ticket. The app redeems it exactly as it
   would a scanned one.
3. The device checks that the redeemer is the app that asked: the same key, name and grants.
   Anything else is denied before the person sees it.
4. The person is asked, with the program's path as the kernel reports it, not as the app claims.
   They approve it as asked, approve part of it, or say no.
5. What fits inside the device's grants, the device approves itself. Anything more goes to home, and
   the person types the passphrase.
6. The app hears the answer and collects its own credential from ErisDB. From then on it talks to
   ErisDB directly.

An app that hangs up withdraws its request, and ErisDB denies the pairing. With no ErisAuth listening,
the app pairs by ticket as it always can.

The person answers on the approval page, or at a terminal with `erisauth approvals`, which prompts
for each request as it arrives. The daemon serves the page on loopback and shows a desktop
notification for each request, unless it runs with `--no-notify`; choosing the notification, or
running `erisauth open`, opens the page. The page's API takes a token the daemon makes when it
starts, handed over in the link it writes to `$XDG_RUNTIME_DIR/erisauth/page` (mode 0600) in the URL
fragment, which the browser never sends. Any process of the same user can
reach the socket, as it can read the device's key. What the device approves by itself is within its
ceiling either way, and the passphrase stands between such a process and anything beyond it.

### Adding a device from one you have

A device that has joined adds the next one; nothing needs running on the ErisDB machine. On the page,
**Add a device** shows a QR code and the command to run on the new device; at a terminal,
`erisauth add` prints both and waits:

```sh
erisauth join --name phone --ceiling 'notes:*' 'erisdb://pair/…'   # on the new device
```

The ticket crosses from one screen to another, so the person compares the fingerprint both show before
approving, as with any ErisLogin pairing. What makes the newcomer an ErisAuth device is granted or
refused whole; its ceiling can be narrowed. A ceiling beyond the approving device's own grants takes
the passphrase, through home.

### ErisFiles among your devices

Devices in ErisAuth become [ErisFiles](https://github.com/Eriskii/ErisFiles) peers of one another with
no ceremony of their own: approving a device into ErisAuth is the approval. Each device finds the
ErisFiles running beside it (`$ERISFILES_HOME`, else `$XDG_DATA_HOME/erisfiles`) and announces itself
in ErisDB's `erisauth.devices` facet: its installation, its name and its ErisFiles device id. Of two
announced devices that are not yet peers, the one with the lower ErisFiles id opens a pairing on its
ErisFiles and posts the ticket to the other in `erisauth.mail`.

ErisFiles grants apply both ways, so joining a stranger's ticket would open this device's files to
them. A device therefore joins an invitation only when ErisDB says its sender wrote it, the sender has
announced itself, and the ticket is for the ErisFiles the sender announced; the inviter approves only
the ErisFiles key the invitee announced. Anything else is dropped. Holding `erisauth.devices:create` is
what lets an installation announce a device the others will share files with, and only ErisAuth devices
ask for it.

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

## Building and testing

The approval page is a SvelteKit app in `web/`, built by `build.rs` and embedded in the binary, so a
build needs Node 22+ and npm.

```sh
cargo test
(cd web && npm run check && npm test)
```

The suite runs a real Postgres (Docker), a real ErisDB core serving the shipped manifest over HTTP
and Iroh, the real `erisauth` daemon and plugin executables, and apps that pair over Iroh with keys
of their own.
