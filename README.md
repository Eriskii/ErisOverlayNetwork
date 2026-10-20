# ErisAuth

The core of each device in an Eris mesh: it holds the device's key, keeps the list of the person's
devices, and approves the apps that ask. Crate `erisauth`.

A person's devices form one mesh, built on
[ErisOverlayNetwork](https://github.com/Eriskii/ErisOverlayNetwork). Membership is a signed list of
the devices, which every device keeps and checks for itself; any device in the mesh adds the next one
or removes another. Apps are not members: a device approves an app with a signed note naming its key,
the service it takes part in and the grants it holds there. A mesh service such as ErisFiles takes one
tap. ErisDB, the enclave, takes a tap and the ErisDB passphrase, which ErisDB itself checks. A device
needs no ErisDB to start a mesh, join one, or approve apps for anything else.

## On a device

`erisauth serve` runs ErisAuth: the device's key and node, the agent socket apps ask on, the approval
page, and desktop notifications (`--no-notify` turns those off). It reaches the other devices over the
internet, through n0's relays and on the local network without them; `--network` or `ERIS_NETWORK`
says otherwise: `loopback`, or a relay URL to use that relay and nothing else. The device's key, the
mesh and the notes it issued live in `$XDG_DATA_HOME/erisauth/` (`key`, `mesh.json`, `notes.json`);
the socket and the page's link in `$XDG_RUNTIME_DIR/erisauth/` — on macOS, which has no
`XDG_RUNTIME_DIR`, in `$TMPDIR/erisauth/`, and notifications go to Notification Center, from which
the page opens with `erisauth open`. Every other command asks the running daemon.

Start a mesh on one device, then add each next one from any device already in it:

```sh
erisauth init --name laptop                       # the first device
erisauth add                                      # on any device in the mesh: shows a ticket
erisauth join --name phone 'eris://pair/…'        # on the new device
```

`erisauth add` shows the ticket as a QR code and as text. The new device redeems it and both screens
show the same fingerprint; the person compares them and approves on the device that showed the
ticket. The newcomer then holds the list and learns of every other device, wherever they are.

| Command                                   | What it does                                                    |
| ----------------------------------------- | --------------------------------------------------------------- |
| `erisauth devices`                        | List the devices: who added each, when, and whether it answers  |
| `erisauth rename DEVICE NAME`             | Call a device something new                                     |
| `erisauth remove DEVICE [--keep D] [--yes]` | Remove a device for good, with the devices it added           |
| `erisauth approvals [--once]`             | Answer what apps and new devices ask, at this terminal          |
| `erisauth add-app`                        | Show a ticket for an app with no ErisAuth beside it             |
| `erisauth apps`                           | List the apps this device approved                              |
| `erisauth revoke APP`                     | Revoke an app's note, everywhere                                |
| `erisauth passphrase`                     | Change ErisDB's passphrase, typing the current one              |
| `erisauth open`                           | Open the approval page                                          |

A device is named by its name or the start of its id. Removing a device offers the devices it added
as removed with it; each goes unless the person keeps it (`--keep` at the terminal, an unticked box on
the page), and one removal carries them all. A removed device never counts again: to come back it
joins anew, with a new key.

Every device tells its person what the others change: "laptop added phone", "laptop removed phone,
tablet", "server approved ErisDB as your ErisDB". A removed device hears it was removed. Each device
asks the others now and then whether they answer, so the list shows which are reachable.

## Apps

An app beside ErisAuth asks over `$XDG_RUNTIME_DIR/erisauth/agent.sock`, with the mesh library's
`agent` module: approve this key for this service, with these grants, serving it or not. The person
sees the program as the kernel names it, from the socket peer's `/proc/<pid>/exe`, never as the app
claims; it goes in the note's `program`. They approve as asked, approve part of the grants, or deny.
The app hears its note and a snapshot of the mesh, or the denial; hanging up withdraws the request.

- A mesh service, such as ErisFiles serving `files`, takes one tap.
- An app asking to serve `erisdb` asks to be the mesh's ErisDB; the prompt says so, and which ErisDB it
  replaces. A mesh has one: approving it issues its note and revokes every other approval to serve
  `erisdb` the device knows of. Every device and app finds ErisDB as the app approved to serve it,
  the most recently approved should two ever count at once, and talks to no other.
- An app for `erisdb` that does not serve it takes the ErisDB passphrase too. The device issues the
  app's note and enrolls it at the mesh's ErisDB, `POST /v1/enroll {note, passphrase}` over Iroh
  `erisdb/0` from the device's own key, and the app hears it is approved only once ErisDB has
  accepted. A wrong passphrase lets the person try again; once ErisDB locks the device out, the person
  hears how long to wait.

An app with no ErisAuth beside it, such as one in a browser, asks by ticket instead: **Add a browser
app** on the page, or `erisauth add-app`, shows one, the app redeems it with its own key, and the
request comes to the person like any other, with the fingerprint both screens show to compare. For
ErisDB the person types the passphrase there too.

`erisauth apps` lists every note this device issued and whether it still counts; `erisauth revoke`
revokes one, and it stops counting on every device and at every service at once.

## ErisDB's passphrase

The passphrase is ErisDB's: it keeps it, checks it and counts wrong guesses per device. From a device
in the mesh, `erisauth passphrase` (or the page) changes it with the current one, `POST /v1/passphrase`.
Setting the first one, resetting it and letting a locked-out device guess again happen where ErisDB
runs: `erisdb passphrase set`, `erisdb passphrase unlock`.

## The approval page

The daemon serves the page on loopback. It shows what waits for the person, the devices, the apps this
device approved, the mesh's ErisDB and what changed lately, and adds devices and browser apps. The
page's files hold no data; its API, `GET /api/state` and `POST /api/command`, takes a token the daemon
makes when it starts, handed over in the link it writes to `$XDG_RUNTIME_DIR/erisauth/page` (mode
0600) in the URL fragment, which the browser never sends. Its policy allows no framing and no script
but its own. Choosing a notification, or running `erisauth open`, opens it.

The page, the terminal and the Android app's screens all send the daemon the same commands, one JSON
object each; [`src/device/command.rs`](src/device/command.rs) lists them and the state they read.
Any process of this user can reach the socket, as it can read the device's key.

## On Android

The app `dev.eris.auth` (in `android/`) runs the same daemon in its own process, with Material screens
of its own: starting or joining a mesh (paste the ticket, or scan it with the camera, which opens the
`eris://pair/…` link in the app), the devices, adding a device or a browser app with a QR code, the
apps it approved, what waits for the person, and the passphrase. Screens with a ticket or a passphrase
on them are kept out of screenshots. It posts a notification for each request and each change.

Android apps cannot reach one another's sockets, so apps bind ErisAuth's service instead, with the
`dev.eris.auth.SIGN_IN` action. Android tells the service which app is calling: the prompt shows its
package and the SHA-256 of its signing certificate, and the note's `program` records them as
`dev.example.notes sha256:<hex>`. The conversation is the socket's: the request is the socket's
request line, the answer its reply line. An app that goes away withdraws its request, and only the app
that asked can withdraw it.

Apps use the client library in `android/client`, two files to vendor as they are:

```kotlin
ErisAuth.connect(context) { erisAuth ->
    if (erisAuth == null) return@connect showTicketFlow()        // no ErisAuth on this phone
    val request = erisAuth.approve("files", "ErisFiles", listOf("files:read"), serves = true, app = myKeyHex) { answer ->
        when (answer) {
            is ErisAuth.Answer.Approved -> keep(answer.note, answer.snapshot)
            is ErisAuth.Answer.Denied -> tell(answer.why)
            is ErisAuth.Answer.Failed -> tell(answer.why)
        }
    }
}
```

`request.withdraw()` takes a request back, `erisAuth.snapshot { json -> }` reads the mesh as the phone
has it, and `erisAuth.close()` lets ErisAuth go. `ErisAuth.kt` is the API; `Wire.kt` writes the Binder
transactions out rather than generating them from AIDL, so the client and the service share one
definition: `ASK(requestId, requestJson, callback)` and `WITHDRAW(requestId)` to the service,
`ANSWER(json)` to the callback, all one-way. The library's manifest declares the `<queries>` entry an
app needs to see ErisAuth.

Build it with `scripts/build-android.sh`, which needs the Android SDK and NDK, `cargo-ndk`, and the
`aarch64-linux-android` and `x86_64-linux-android` Rust targets. It builds the page and the JNI library
for both, runs the Kotlin unit tests, and assembles the debug APK.

## Building and testing

The approval page is a SvelteKit app in `web/`, built by `build.rs` and embedded in the binary, so a
build needs Node 22+ and npm.

```sh
cargo test
(cd web && npm run check && npm test)
./android/gradlew -p android testDebugUnitTest
```

The suite runs the real `erisauth` daemon as separate devices on loopback Iroh, each with its own data
and runtime directories, and drives them as the person and the apps do: meshes started, joined by
ticket through one another and changed, apps approved over the socket and by ticket. The ErisDB tests
run ErisDB's real core on a real Postgres (Docker); they build it from `ERISDB_SOURCE`, its `erisdb`
crate (beside this checkout in the Eris monorepo by default), or take it from `ERISDB_BIN`.
