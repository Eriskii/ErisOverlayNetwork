//! A server that asks for a passphrase: devices bring it apps' notes along with the passphrase,
//! and it lets in only what checks out.

mod support;

use std::sync::{Arc, Mutex};

use erisoverlaynetwork::devices::{DeviceId, Op};
use erisoverlaynetwork::enroll::{self, EnrollService, Gate, Outcome};
use erisoverlaynetwork::mesh::{self, Config, Local, MemoryStore, Node};
use erisoverlaynetwork::net::{self, Network};
use erisoverlaynetwork::{Error, Note, Serves};
use iroh::SecretKey;
use iroh::protocol::Router;
use n0_future::boxed::BoxFuture;
use support::{Device, device, eventually, join, uses};

/// A gate with one passphrase, keeping whom it let in.
#[derive(Default)]
struct Book {
    passphrase: Mutex<String>,
    enrolled: Mutex<Vec<(DeviceId, Note)>>,
}

impl Gate for Book {
    fn enroll(
        &self,
        device: DeviceId,
        note: Note,
        passphrase: String,
    ) -> BoxFuture<erisoverlaynetwork::Result<Outcome>> {
        let right = *self.passphrase.lock().unwrap() == passphrase;
        if right {
            self.enrolled.lock().unwrap().push((device, note));
        }
        Box::pin(async move { Ok(if right { Outcome::Done } else { Outcome::Wrong { retry_after: None } }) })
    }

    fn change(&self, _: DeviceId, current: String, new: String) -> BoxFuture<erisoverlaynetwork::Result<Outcome>> {
        let mut passphrase = self.passphrase.lock().unwrap();
        let right = *passphrase == current;
        if right {
            *passphrase = new;
        }
        Box::pin(async move { Ok(if right { Outcome::Done } else { Outcome::Wrong { retry_after: None } }) })
    }
}

struct Server {
    node: Node,
    note: Note,
    book: Arc<Book>,
    _router: Router,
}

/// A database approved by `on`, single and taking the passphrase `secret`.
async fn database(on: &Device, secret: &str) -> Server {
    let key = SecretKey::generate();
    let serves = vec![Serves { capability: "database/1".parse().unwrap(), single: true, passphrase: true }];
    let note = on.node.issue(key.public(), "ErisDB", Default::default(), serves, None).unwrap();
    let endpoint = net::endpoint(key, &Network::Loopback).await.unwrap();
    let node = Node::start(Config {
        endpoint: endpoint.clone(),
        network: Network::Loopback,
        local: Local::App(note.clone()),
        store: Arc::new(MemoryStore::default()),
    })
    .await
    .unwrap();
    node.adopt(on.node.snapshot().await.unwrap()).await.unwrap();
    let book = Arc::new(Book { passphrase: Mutex::new(secret.into()), ..Book::default() });
    let router = Router::builder(endpoint)
        .accept(mesh::ALPN, node.protocol())
        .accept(enroll::ALPN, EnrollService::new(node.clone(), book.clone()))
        .spawn();
    Server { node, note, book, _router: router }
}

#[tokio::test]
async fn a_device_brings_an_app_and_the_passphrase_and_the_server_lets_it_in() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let db = database(&laptop, "correct horse").await;
    eventually("the laptop finds its database", || laptop.node.server("database/1").is_some()).await;
    assert!(laptop.node.server("database/1").unwrap().serving("database/1").unwrap().passphrase);

    let tasks = SecretKey::generate().public();
    let note = laptop.node.issue(tasks, "Tasks", uses("database/1", &["tasks:*"]), vec![], None).unwrap();
    let wrong = enroll::enroll(&laptop.node, db.node.id(), &note, "wrong horse").await.unwrap();
    assert_eq!(wrong, Outcome::Wrong { retry_after: None });
    assert!(db.book.enrolled.lock().unwrap().is_empty());
    let right = enroll::enroll(&laptop.node, db.node.id(), &note, "correct horse").await.unwrap();
    assert_eq!(right, Outcome::Done);
    assert_eq!(db.book.enrolled.lock().unwrap().as_slice(), &[(laptop.node.id(), note)]);

    assert_eq!(
        enroll::change(&laptop.node, db.node.id(), "correct horse", "battery staple").await.unwrap(),
        Outcome::Done
    );
    assert_eq!(*db.book.passphrase.lock().unwrap(), "battery staple");
}

#[tokio::test]
async fn only_a_device_with_a_note_of_its_own_for_what_the_server_serves_gets_through() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let phone = device("phone").await;
    join(&phone, &laptop).await;
    let db = database(&laptop, "correct horse").await;
    eventually("the phone finds the database", || phone.node.server("database/1").is_some()).await;

    // The laptop's note, brought by the phone.
    let theirs =
        laptop.node.issue(SecretKey::generate().public(), "Tasks", uses("database/1", &[]), vec![], None).unwrap();
    let refused = enroll::enroll(&phone.node, db.node.id(), &theirs, "correct horse").await;
    assert!(matches!(refused, Err(Error::Forbidden { .. })), "{refused:?}");
    // A note for something else.
    let files = phone.node.issue(SecretKey::generate().public(), "Photos", uses("files/1", &[]), vec![], None).unwrap();
    let refused = enroll::enroll(&phone.node, db.node.id(), &files, "correct horse").await;
    assert!(matches!(refused, Err(Error::BadRequest(_))), "{refused:?}");
    assert!(db.book.enrolled.lock().unwrap().is_empty());

    // Once removed, the phone gets nothing through, its own notes included.
    let own = phone.node.issue(SecretKey::generate().public(), "Tasks", uses("database/1", &[]), vec![], None).unwrap();
    laptop.node.append(Op::Remove { devices: vec![phone.node.id()] }).await.unwrap();
    eventually("the database drops the phone", || !db.node.list().unwrap().is_member(&phone.node.id())).await;
    let refused = enroll::enroll(&phone.node, db.node.id(), &own, "correct horse").await;
    assert!(refused.is_err());
    assert!(db.book.enrolled.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_server_whose_approval_was_revoked_lets_no_one_in() {
    let laptop = device("laptop").await;
    laptop.node.create("laptop").await.unwrap();
    let db = database(&laptop, "correct horse").await;
    eventually("the laptop finds its database", || laptop.node.server("database/1").is_some()).await;
    laptop.node.append(Op::Revoke { note: db.note.id }).await.unwrap();
    eventually("the database hears of it", || db.node.list().unwrap().is_revoked(&db.note.id)).await;
    let note =
        laptop.node.issue(SecretKey::generate().public(), "Tasks", uses("database/1", &[]), vec![], None).unwrap();
    let refused = enroll::enroll(&laptop.node, db.node.id(), &note, "correct horse").await;
    assert!(matches!(refused, Err(Error::Forbidden { .. })), "{refused:?}");
}
