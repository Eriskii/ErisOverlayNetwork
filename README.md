# ErisOverlayNetwork

Your devices, as one mesh: one signed list of them, approvals for the apps on them, and the
connections between them, over [Iroh](https://iroh.computer). You add a device once, from any device
already in, and from then on every device and every app on it knows every other, wherever they are.

This repository holds:

- **`eon`** ([`eon/`](eon)): what each computer runs. It holds the device's key and its copy of the
  list, lets new devices in, and approves the apps that ask. The command is `eon`; it serves a page
  on loopback for doing the same in a browser.
- **The Android app** ([`android/`](android)): the same, on a phone, with screens of its own.
- **The library**, crate `erisoverlaynetwork` (this directory): what `eon` is built on, and what
  apps and servers use to take part.

```toml
erisoverlaynetwork = { git = "https://github.com/Eriskii/ErisOverlayNetwork", tag = "v0.3.0" }
```

## On a computer

`eon serve` runs it: the device's key and node, the socket apps ask on, the page, and desktop
notifications (`--no-notify` turns those off). It reaches the other devices over the internet,
through n0's relays and on the local network without them; `--network` or `ERIS_NETWORK` says
otherwise: `loopback`, or a relay URL to use that relay and nothing else. The device's key, the mesh
and the approvals it issued live in `$XDG_DATA_HOME/eon/`; the socket and the page's link in
`$XDG_RUNTIME_DIR/eon/` (on macOS, `$TMPDIR/eon/`, and notifications go to Notification Center).
Every other command asks the running `eon serve`.

`eon init --name laptop` starts a mesh with this device as its first. `eon open` opens the page.

## Adding a device

From any device already in, any of three ways:

1. **Scan the device that is in.** On it, **Add a device** (or `eon add`) shows a QR code. Scan it
   with the new device, a phone's camera most likely. Both screens show the same short fingerprint;
   approve on the device that showed the code.
2. **Type its code.** The same screen shows a code, `K7QX-M2PA`. Type it on the new device:
   `eon join --name mac K7QX-M2PA`, or the box on the new device's page. Then compare the
   fingerprints and approve, as above.
3. **Scan the new device.** A device in no mesh shows a QR code of its own: on its page, or with
   `eon join --name mac` at its terminal. Scan it with your phone, already in, and say yes there. The
   new device's screen then names the device offering and every device it would join; accept there.
   No fingerprint is needed: the phone read the new device's key off its own screen.

The new device then holds the list and learns of every other device, wherever they are. A code
finds the device showing it through n0's DNS, on the local network by mDNS, or through the one relay
a relay-only mesh has; it stops working once its ticket is used or ends.

| Command                                  | What it does                                                         |
| ---------------------------------------- | -------------------------------------------------------------------- |
| `eon add`                                | Show a QR code and a code for a new device, and approve it here      |
| `eon add 'eris://join/…' [--yes]`        | Add the new device that shows this ticket of its own                 |
| `eon join --name NAME CODE`              | Join with the code (or the `eris://pair/…` ticket) another device shows |
| `eon join --name NAME`                   | Show a QR code for a device already in to scan, and accept its offer |
| `eon devices`                            | List the devices: who added each, when, and whether it answers       |
| `eon rename DEVICE NAME`                 | Call a device something new                                          |
| `eon remove DEVICE [--keep D] [--yes]`   | Remove a device for good, with the devices it added                  |
| `eon approvals [--once]`                 | Answer what apps and new devices ask, at this terminal               |
| `eon add-app`                            | Show a ticket for an app with no eon beside it, such as a browser app |
| `eon apps`                               | List the apps this device approved                                   |
| `eon revoke APP`                         | Revoke an app's approval, everywhere                                 |
| `eon passphrase [CAPABILITY]`            | Change a server's passphrase, such as your database's               |
| `eon open`                               | Open the page                                                        |

A device is named by its name or the start of its id. Removing a device offers the devices it added
as removed with it; each goes unless you keep it. A removed device never counts again: to come back
it joins anew, with a new key. Every device tells its person what the others change: "laptop added
phone", "phone added this device", "server approved ErisDB as your database".

## Apps, and what they ask for

Apps ask for **capabilities**, named for what they are, not for the programs serving them: a
database, `database/1`; files, `files/1`. The number names the format: everything serving
`database/1` takes the same requests, so an app that asks for a database works with whatever serves
one. ErisDB serves `database/1`; ErisFiles uses and serves `files/1`.

An approval names the app's key, what it uses with the grants it holds in each, and what it serves,
signed by the device. An app cannot make itself a server: it serves something only once a person
approves it to. Serving says two more things:

- **single**: a mesh has one server of it, the most recently approved. Approving another replaces
  it, and the approval says so. A database is single; ErisFiles, one on each device, is not.
- **passphrase**: apps using it also take the server's passphrase, which the server checks itself
  and counts wrong guesses for, per device. A database takes one.

An app on the same computer asks over `$XDG_RUNTIME_DIR/eon/agent.sock`. The person sees the
program as the kernel names it, never as the app claims, and approves as asked, approves part of
the grants, or denies. An app that uses something taking a passphrase is approved only once the
person has typed it and the server has accepted. An app with no eon beside it, such as one in a
browser, asks by ticket instead: **Add a browser app** on the page, or `eon add-app`.

`eon apps` lists every approval this device issued; `eon revoke` revokes one, and it stops counting
on every device and at every server at once.

## The page

`eon serve` serves the page on loopback: what waits for you, the devices, adding devices and browser
apps, what your apps serve, the apps this device approved, and what changed lately. Its files hold
no data; its API takes a token handed over in the link `eon` writes to `$XDG_RUNTIME_DIR/eon/page`,
in the URL fragment, which the browser never sends. The page, the terminal and the Android app all
send the same commands; [`eon/src/device/command.rs`](eon/src/device/command.rs) lists them and the
state they read.

## On Android

The app `dev.eris.eon` runs the same daemon in its own process. A ticket scanned with the phone's
camera opens in it: a device's, `eris://pair/…`, to join; a new device's own, `eris://join/…`, to add
it. It shows **Add a device** with a QR code and a code, the devices, what waits for you, the apps it
approved and the passphrase, and posts a notification for each request and each change.

Android apps cannot reach one another's sockets, so apps bind the app's service, action
`dev.eris.eon.SIGN_IN`, with the client in `android/client`: two files to copy as they are.

```kotlin
ErisOverlayNetwork.connect(context) { eon ->
    if (eon == null) return@connect showTicketFlow()        // no eon on this phone
    eon.approve("Tasks", mapOf("database/1" to listOf("tasks:read")), app = myKeyHex) { answer ->
        when (answer) {
            is ErisOverlayNetwork.Answer.Approved -> keep(answer.note, answer.snapshot)
            is ErisOverlayNetwork.Answer.Denied -> tell(answer.why)
            is ErisOverlayNetwork.Answer.Failed -> tell(answer.why)
        }
    }
}
```

Android says which app is calling: the prompt shows its package and the SHA-256 of its signing
certificate, and the approval records them.

## The library

### The device list

Membership is a list of signed entries that every participant keeps and checks for itself:
**genesis** (the mesh begins; its id is the mesh's), **add**, **remove**, **name**, and **revoke**
(an app's approval stops counting). Any member device writes entries. Each entry names the entries
its author had seen, so every copy orders them the same way, and these rules decide what they mean:

1. An entry counts only if its author was a member when it wrote it, as far as its parents show.
2. A removal counts whenever its author was a member. Two devices that remove each other at once
   are both removed.
3. Once a device is removed, the other things it wrote count only if its remover had seen them. A
   stolen phone cannot add a computer behind the back of the device that removes it.

### Approvals and capabilities

[`note`](src/note.rs) is an approval, [`capability`](src/capability.rs) what it names. A server lets
a caller in with `Node::authorize(caller, capability, required)`, which checks the caller's approval
against the list, and watches `Node::subscribe()` to drop whatever stops counting.
`Node::servers(capability)` finds the apps serving something; `Node::server(capability)` the one in
use of a single one. A server that takes a passphrase serves [`enroll`](src/enroll.rs) (`eris-enroll/0`)
with a `Gate` of its own: devices bring it an app's approval with the passphrase there.

### The node

Every device and every app that serves something runs a `mesh::Node` under `eris-mesh/0`. It syncs
with whoever it talks to, pushes what it learns to everyone it knows at once, and announces itself
when it starts, when it joins, when its address changes, and every five minutes.

```rust
use erisoverlaynetwork::mesh::{self, Config, FileStore, Local, Node};
use erisoverlaynetwork::net::{self, Network};

let endpoint = net::endpoint(key, &Network::Internet).await?;
let node = Node::start(Config {
    endpoint: endpoint.clone(),
    network: Network::Internet,
    local: Local::App(note),
    store: std::sync::Arc::new(FileStore::new(home.join("mesh.json"))),
})
.await?;
let router = iroh::protocol::Router::builder(endpoint)
    .accept(mesh::ALPN, node.protocol())
    .accept(&capability.alpn(), my_server)
    .spawn();
```

### Letting devices and apps in

- [`ceremony`](src/ceremony.rs) (`eris-pair/0`): a member shows a ticket, `eris://pair/…`; the
  newcomer, a device or an app, redeems it; both screens show a [fingerprint](src/fingerprint.rs);
  a person approves.
- [`code`](src/code.rs) (`eris-code/0`): a short code standing in for a ticket, answered by an
  endpoint whose key the code derives.
- [`join`](src/join.rs) (`eris-join/0`): a device in no mesh shows a ticket of its own,
  `eris://join/…`; a member that scans it offers the mesh, its person accepts, and the member adds it.
- [`agent`](src/agent.rs): an app beside eon asks it over the socket.

Also here: `sys` (the runtime directory, the machine's name, a process's program, opening links, on
Linux and macOS), `token` (HMAC-signed tokens), `permission` (grant patterns: `files:read`,
`tasks:*`, `*`), `key`, `qr`, and `prompt` (putting a request to a person at a terminal).

The `native` feature (default) holds the node, the agent socket, enrollment, joining, QR codes, the
terminal prompt and mDNS. Without it the crate holds what a browser needs: the list, approvals,
capabilities, tickets, codes, the directory, and the ceremony's newcomer side.

## Building and testing

The page is a SvelteKit app in `eon/web/`, built by `eon/build.rs` and embedded in the binary, so
building `eon` needs Node 22+ and npm.

```sh
cargo test --workspace
(cd eon/web && npm run check && npm test)
scripts/build-android.sh        # the APK: needs the Android SDK and NDK, and cargo-ndk
```

The library's tests run real nodes on loopback Iroh endpoints and, for codes, through a relay on
loopback. `eon`'s run the real daemon as separate devices, each with its own directories, and drive
them as people and apps do. Its ErisDB tests run ErisDB's real core on a real Postgres (Docker),
built from `ERISDB_SOURCE` (beside this checkout in the Eris monorepo by default) or taken from
`ERISDB_BIN`.
