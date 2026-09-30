//! An app asking the device's ErisAuth for approval, against a stand-in
//! agent that speaks the wire format line by line.

use std::path::PathBuf;

use erislogin::agent::{self, Answer, Ask};
use erislogin::identity::Identity;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

fn ask() -> Ask {
    Ask {
        service: "erisdb".into(),
        client: "Notes".into(),
        requested: vec!["notes:read".into()],
        identity: Identity::Iroh("ab".repeat(32)),
    }
}

/// A stand-in agent: reads the first line, hands it to the test, and sends `replies`.
fn agent(replies: Vec<Value>) -> (tempfile::TempDir, PathBuf, tokio::task::JoinHandle<Value>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut line = String::new();
        BufReader::new(read).read_line(&mut line).await.unwrap();
        for reply in replies {
            write.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
        }
        serde_json::from_str(&line).unwrap()
    });
    (dir, path, task)
}

#[tokio::test]
async fn an_app_asks_redeems_its_ticket_and_hears_the_answer() {
    let (_dir, path, agent) = agent(vec![json!({"ticket": "erisdb://pair/abc"}), json!({"approved": ["notes:read"]})]);
    let mut connection = agent::connect_to(&path).await.unwrap().expect("an agent is listening");
    assert_eq!(connection.ask(&ask()).await.unwrap(), "erisdb://pair/abc");
    assert_eq!(connection.answer().await.unwrap(), Answer::Approved(vec!["notes:read".into()]));
    assert_eq!(
        agent.await.unwrap(),
        json!({
            "v": 1, "op": "pair", "service": "erisdb", "client": "Notes", "requested": ["notes:read"],
            "identity": {"kind": "iroh", "key": "ab".repeat(32)}
        })
    );
}

#[tokio::test]
async fn a_denial_is_an_answer_and_an_agent_error_is_an_error() {
    let (_dir, path, _agent) = agent(vec![json!({"ticket": "t"}), json!({"denied": "the person said no"})]);
    let mut connection = agent::connect_to(&path).await.unwrap().unwrap();
    connection.ask(&ask()).await.unwrap();
    assert_eq!(connection.answer().await.unwrap(), Answer::Denied("the person said no".into()));

    let (_dir, path, _agent) = agent(vec![json!({"error": "this device has not joined ErisDB"})]);
    let mut connection = agent::connect_to(&path).await.unwrap().unwrap();
    let error = connection.ask(&ask()).await.unwrap_err().to_string();
    assert!(error.contains("this device has not joined ErisDB"), "{error}");
}

#[tokio::test]
async fn an_agent_that_hangs_up_is_an_error_not_an_answer() {
    let (_dir, path, _agent) = agent(vec![json!({"ticket": "t"})]);
    let mut connection = agent::connect_to(&path).await.unwrap().unwrap();
    connection.ask(&ask()).await.unwrap();
    assert!(connection.answer().await.is_err());
}

#[tokio::test]
async fn no_agent_means_none_so_the_app_pairs_by_ticket() {
    let dir = tempfile::tempdir().unwrap();
    assert!(agent::connect_to(&dir.path().join("agent.sock")).await.unwrap().is_none(), "no socket");

    // A socket file left behind by an agent that is gone.
    let stale = dir.path().join("stale.sock");
    drop(UnixListener::bind(&stale).unwrap());
    assert!(agent::connect_to(&stale).await.unwrap().is_none(), "nobody listening");
}

#[test]
fn the_socket_lives_in_the_runtime_directory() {
    assert_eq!(
        agent::socket_in(std::path::Path::new("/run/user/1000")),
        PathBuf::from("/run/user/1000/erisauth/agent.sock")
    );
}
