# ErisLogin

Pairing for Eris apps and devices over [Iroh](https://iroh.computer): a host shows a QR code, a
newcomer scans it and asks for permissions, both screens show the same fingerprint, and a person
approves all, some, or none of the request. Crate `erislogin`.

```toml
erislogin = { git = "https://github.com/Eriskii/ErisLogin", tag = "v0.4.0" }
```

Features `iroh` (`key`, `Ticket::endpoint_addr`), `qr`, `prompt` and `agent` are on by default. A client
that only reads tickets and signs or checks tokens can take `default-features = false` and leave
Iroh, QR rendering and the terminal out of its build.

| Module       | What it holds                                                                                 |
| ------------ | --------------------------------------------------------------------------------------------- |
| `agent`      | Asking the device's ErisAuth for approval over its socket, instead of showing a ticket         |
| `ticket`     | `<app>://pair/<base64url(JSON)>`: an Iroh endpoint id and/or URL, direct addresses, a code     |
| `qr`         | A ticket as terminal blocks, SVG, a greyscale raster, or a PNG                                |
| `pairing`    | In-memory sessions: `pending → requested → approved → collected`, or `denied`                  |
| `prompt`     | The host's terminal ceremony: show the ticket, then `[a] approve [s] select [d] deny`          |
| `identity`   | Who is asking: an Iroh key proven by QUIC, or a browser's S256 commitment; request fingerprints |
| `permission` | `files:read`, `meta:*`, `*`: covering, enclosure, intersection, validation                     |
| `token`      | `<prefix>.<b64url(json)>.<b64url(hmac)>` signed payloads                                      |
| `key`        | An Iroh secret key derived from a secret and a tag, or kept in an owner-only file             |

## The ceremony

```rust
use erislogin::{identity::Identity, pairing::{Pairings, Status}, ticket::Ticket};

// Host
let pairings = Pairings::new("myapp-pair-v1");
let (session, code) = pairings.open(600);
let ticket = Ticket::new(code, Some(endpoint_id_hex), None, Some("laptop".into()))?;
println!("{}", ticket.encode("myapp")?); // myapp://pair/eyJ2Ijox…

// Newcomer, over an Iroh connection the host accepted from `peer`:
let identity = Identity::enrollment(Some(&format!("iroh:{peer}")), None)?;
let request = pairings.redeem(&code, "Phone", vec!["files:read".into()], identity.clone())?;
// Both screens show request.fingerprint, e.g. 12AB-34CD-56EF.

// Host, once the person agrees:
pairings.approve(session, None)?;

// Newcomer, polling:
if let Status::Approved { granted } = pairings.collect(&code, &identity)? { /* paired */ }
```

The code in a ticket grants nothing by itself: it lets one identity ask. A copied QR can race to
redeem first, which is why the person compares fingerprints rather than trusting a device name, and
once one identity has redeemed a code nobody else can collect its answer.

Fingerprints are six bytes of the SHA-256 of `(domain, session, identity, requested)` as JSON,
shown as `12AB-34CD-56EF`. The domain tag keeps one app's fingerprints from matching another's.

## Tickets

```json
{ "v": 1, "name": "laptop", "eid": "e718…6a06", "addrs": ["192.168.1.4:4919"], "token": "…" }
```

`v` and `token` are required, with at least one of `eid` (64 hex characters) and `url`. `name` is an
untrusted label. `addrs` lets a device on the same network dial before discovery answers. Parsers
refuse unknown versions, damaged encodings, and missing addresses rather than guessing.

## Testing

```sh
cargo test
```
