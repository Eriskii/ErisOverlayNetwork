//! The app's side of the agent socket, against a stand-in agent that answers as ErisAuth does.

use erisoverlaynetwork::agent::{self, Answer, Ask, Reply, Request};
use erisoverlaynetwork::devices::DeviceList;
use erisoverlaynetwork::directory::Snapshot;
use erisoverlaynetwork::note::{Note, NoteBody};
use iroh::SecretKey;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// An agent that answers one request with whatever `answer` makes of it.
async fn agent(dir: &std::path::Path, answer: impl Fn(Request) -> Reply + Send + 'static) -> std::path::PathBuf {
    let path = agent::socket_in(dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut line = String::new();
        BufReader::new(read).read_line(&mut line).await.unwrap();
        let reply = answer(serde_json::from_str(&line).unwrap());
        let mut out = serde_json::to_vec(&reply).unwrap();
        out.push(b'\n');
        write.write_all(&out).await.unwrap();
    });
    path
}

fn mesh() -> (SecretKey, Snapshot) {
    let device = SecretKey::generate();
    let (list, _) = DeviceList::create(&device, "laptop").unwrap();
    (device, Snapshot { mesh: list.mesh(), entries: list.entries(), directory: vec![] })
}

fn note(device: &SecretKey, snapshot: &Snapshot, app: iroh::EndpointId, service: &str) -> Note {
    Note::issue(
        device,
        NoteBody {
            mesh: snapshot.mesh,
            device: device.public(),
            app,
            name: "ErisFiles".into(),
            service: service.into(),
            grants: vec!["files:read".into()],
            serves: true,
            program: Some("/usr/bin/erisfiles".into()),
            at: 0,
        },
    )
    .unwrap()
}

fn ask(app: iroh::EndpointId) -> Ask {
    Ask { service: "files".into(), name: "ErisFiles".into(), grants: vec!["files:read".into()], serves: true, app }
}

#[tokio::test]
async fn an_app_gets_its_note_and_the_mesh() {
    let dir = tempfile::tempdir().unwrap();
    let (device, snapshot) = mesh();
    let me = SecretKey::generate().public();
    let given = note(&device, &snapshot, me, "files");
    let (n, s) = (given.clone(), snapshot.clone());
    let path = agent(dir.path(), move |request| {
        assert!(matches!(request, Request::Approve { v: 2, ref ask } if ask.app == me && ask.serves));
        Reply::Approved { note: n.clone(), snapshot: s.clone() }
    })
    .await;
    let mut agent = agent::connect_to(&path).await.unwrap().unwrap();
    assert_eq!(agent.approve(&ask(me)).await.unwrap(), Answer::Approved { note: given, snapshot });
}

#[tokio::test]
async fn a_note_for_someone_else_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (device, snapshot) = mesh();
    let me = SecretKey::generate().public();
    let wrong = note(&device, &snapshot, SecretKey::generate().public(), "files");
    let path = agent(dir.path(), move |_| Reply::Approved { note: wrong.clone(), snapshot: snapshot.clone() }).await;
    let mut agent = agent::connect_to(&path).await.unwrap().unwrap();
    assert!(agent.approve(&ask(me)).await.is_err());
}

#[tokio::test]
async fn a_denial_is_an_answer() {
    let dir = tempfile::tempdir().unwrap();
    let path = agent(dir.path(), |_| Reply::Denied("no".into())).await;
    let mut agent = agent::connect_to(&path).await.unwrap().unwrap();
    let me = SecretKey::generate().public();
    assert_eq!(agent.approve(&ask(me)).await.unwrap(), Answer::Denied("no".into()));
}

#[tokio::test]
async fn no_agent_listening_is_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(agent::connect_to(&agent::socket_in(dir.path())).await.unwrap().is_none());
}
