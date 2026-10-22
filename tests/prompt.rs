use erisoverlaynetwork::prompt::{Request, decide};

fn request() -> Request {
    Request {
        client: "Phone".into(),
        fingerprint: "12AB-34CD-56EF".into(),
        asked: vec!["files:read".into(), "files:write".into()],
        expires: i64::MAX / 2,
    }
}

async fn answer(lines: &[&str]) -> (anyhow_like::Result, String) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    for line in lines {
        tx.send(line.to_string()).unwrap();
    }
    drop(tx);
    let mut out = Vec::new();
    let result = decide(&mut rx, &mut out, &request()).await.map_err(|e| e.to_string());
    (result, String::from_utf8(out).unwrap())
}

mod anyhow_like {
    pub type Result = std::result::Result<Option<Vec<String>>, String>;
}

#[tokio::test]
async fn a_approves_as_asked() {
    let (result, out) = answer(&["a"]).await;
    assert_eq!(result.unwrap(), Some(vec!["files:read".into(), "files:write".into()]));
    assert!(out.contains("12AB-34CD-56EF"));
    assert!(out.contains("Phone wants:"));
    assert!(out.contains("  2. files:write"));
}

#[tokio::test]
async fn d_denies() {
    assert_eq!(answer(&["d"]).await.0.unwrap(), None);
}

#[tokio::test]
async fn blank_and_unknown_answers_ask_again() {
    let (result, out) = answer(&["", "x", "a"]).await;
    assert!(result.unwrap().is_some());
    assert_eq!(out.matches("Enter a, s, or d.").count(), 2);
}

#[tokio::test]
async fn s_selects_by_number_and_rejects_bad_numbers() {
    let (result, out) = answer(&["s", "3", "2"]).await;
    assert_eq!(result.unwrap(), Some(vec!["files:write".into()]));
    assert!(out.contains("Enter valid permission numbers from 1 to 2."));
}

#[tokio::test]
async fn an_empty_selection_denies() {
    assert_eq!(answer(&["s", ""]).await.0.unwrap(), None);
}

#[tokio::test]
async fn closed_input_cancels() {
    assert!(answer(&[]).await.0.is_err());
}

#[test]
fn a_ticket_is_shown_with_its_code_and_says_who_scans_it() {
    use erisoverlaynetwork::prompt::show;
    use erisoverlaynetwork::ticket::Ticket;
    let id = iroh::SecretKey::generate().public();
    let pair = Ticket::new("secret".into(), id, Some("laptop".into())).unwrap();
    let mut out = Vec::new();
    show(&mut out, &pair, &pair.encode().unwrap(), Some("K7QX-M2PA")).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("Scan this with the new device"), "{out}");
    assert!(out.contains("Or type this code on the new device:  K7QX-M2PA"), "{out}");
    assert!(!out.contains("secret"), "the one-time secret is shown only inside the ticket: {out}");

    let join = Ticket::join("secret".into(), id, "mac".into()).unwrap();
    let mut out = Vec::new();
    show(&mut out, &join, &join.encode().unwrap(), None).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("Scan this with your phone, or any device already among yours."), "{out}");
    assert!(!out.contains("type this code"), "{out}");
}
