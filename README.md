# ErisOverlayNetwork

The Eris mesh: one signed list of your devices, approvals for the apps on them, and the connections
between them, over [Iroh](https://iroh.computer). Crate `erisoverlaynetwork`.

```toml
erisoverlaynetwork = { git = "https://github.com/Eriskii/ErisOverlayNetwork", tag = "v0.1.0" }
```

A person's devices form one mesh. Each device runs ErisAuth, which holds the device's key. You add a
device once, from any device already in the mesh: a ticket, a fingerprint on both screens, one
approval. From then on every device and every app on it knows every other, wherever they are.

## The device list

Membership is a list of signed entries that every participant keeps and checks for itself. Nothing
is the authority over it: any member device writes entries, and an entry counts only when a device
the list already held signed it.

| Entry    | Says                                                                           |
| -------- | ------------------------------------------------------------------------------ |
| genesis  | The mesh begins, with its author as the first device. Its id is the mesh's id. |
| add      | A device joins, under a name.                                                  |
| remove   | Devices leave, for good. A removed key never counts again.                     |
| name     | A device is called something new.                                              |
| revoke   | An app's note stops counting.                                                  |
| pin      | One app provides a service to the whole mesh: the mesh's ErisDB.               |

Each entry names the entries its author had seen, so the entries form a graph that every copy
orders the same way, and the same entries always mean the same members:

1. An entry counts only if its author was a member when it wrote it, as far as its parents show.
2. A removal counts whenever its author was a member. Two devices that remove each other at once
   are both removed.
3. Once a device is removed, the other things it wrote count only if its remover had seen them. A
   stolen phone cannot add a computer behind the back of the device that removes it.

## Apps and notes

Apps are not members. A device approves an app with a **note**: the app's key, the service it takes
part in (`files`, `erisdb`), the grants it holds there, and whether it serves that service, signed
by the device. A note counts while its device is a member and no device has revoked it. Grants are
permission patterns, segment by segment, with `*` matching one segment or, last, all the rest:
`files:read`, `tasks:*`, `*`.

An app on a device with ErisAuth asks it over the [agent](src/agent.rs) socket,
`$XDG_RUNTIME_DIR/erisauth/agent.sock`, and gets its note and a snapshot of the mesh back. An app
with no ErisAuth beside it, such as one in a browser, asks a device by ticket instead, through the
same ceremony a device joins by.

## The node

Every device and every app that serves something runs a `mesh::Node` and registers its protocol
under `eris-mesh/0`. A node:

- **syncs** with whoever it talks to: both sides say who they are and where, compare the list's
  heads, and send each other what the other lacks, along with everyone they know of;
- **pushes** what it learns to everyone it knows at once, so a removal reaches every reachable
  participant without anyone asking;
- **announces** itself when it starts, when it joins, when its address changes, and every five
  minutes.

A service lets a caller in with `Node::authorize(caller, service, required)`, which checks the
caller's note against the list, and watches `Node::subscribe()` to drop whatever stops counting.
`Node::providers(service)` finds the apps serving a service; `Node::pinned(service)` finds the one
pinned to it.

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
    .accept(MY_ALPN, my_service)
    .spawn();
```

`Network::Internet` finds participants anywhere through n0's relays and DNS, and on the same network
through mDNS with no internet at all. `Network::Relay(url)` uses one relay and nothing else, as a
browser must. `Network::Loopback` is for tests.

## The ceremony

A member device opens a session and shows its ticket, `eris://pair/<base64url JSON>`: its endpoint
id, addresses, relay, and a one-time code. The newcomer redeems the code over `eris-pair/0`, saying
what it asks to be (`Ask::Device` or `Ask::App`), and computes the request's fingerprint itself.
The device shows the same fingerprint; a person compares the two and approves. Only the key that
redeemed the code can collect the answer: the list with the device added, or the app's note, each
with a snapshot of the mesh.

## Also here

`token` seals and opens HMAC-signed tokens; `permission` matches, encloses and intersects grants;
`key` derives or stores an Iroh key; `qr` renders tickets; `prompt` puts a request to a person at a
terminal.

## Features

`native` (default) holds the node, the agent socket, QR codes, the terminal prompt and mDNS. Without
it the crate holds what a browser needs: the list, notes, tickets, the directory and the
ceremony's client side.

## Testing

```sh
cargo test
```

The mesh tests run real nodes on loopback Iroh endpoints: devices joining one another by ticket,
apps under notes, removals and revocations reaching everyone, and a stranger turned away.
