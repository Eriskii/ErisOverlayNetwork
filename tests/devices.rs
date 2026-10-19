use erisoverlaynetwork::devices::{DeviceList, Op};
use erisoverlaynetwork::signed::Signed;
use iroh::SecretKey;

fn key() -> SecretKey {
    SecretKey::generate()
}

fn add(list: &mut DeviceList, by: &SecretKey, device: &SecretKey, name: &str) -> Signed {
    list.append(by, Op::Add { device: device.public(), name: name.into() }).unwrap()
}

/// A copy of `list` as another participant would hold it, from its entries.
fn copy(list: &DeviceList) -> DeviceList {
    DeviceList::from_entries(list.entries()).unwrap()
}

#[test]
fn a_mesh_begins_with_its_first_device_and_is_named_by_its_genesis() {
    let laptop = key();
    let (list, genesis) = DeviceList::create(&laptop, "laptop").unwrap();
    assert!(list.is_member(&laptop.public()));
    assert_eq!(list.member(&laptop.public()).unwrap().name, "laptop");
    assert_eq!(list.member(&laptop.public()).unwrap().added_by, None);
    assert_eq!(list.founder(), laptop.public());
    let again = DeviceList::from_entries([genesis]).unwrap();
    assert_eq!(again.mesh(), list.mesh());
    assert_eq!(again.state(), list.state());
}

#[test]
fn any_member_adds_a_device_and_every_copy_agrees() {
    let (laptop, phone, desktop) = (key(), key(), key());
    let (mut list, _) = DeviceList::create(&laptop, "laptop").unwrap();
    add(&mut list, &laptop, &phone, "phone");
    // The phone, once in, adds the next one by itself.
    add(&mut list, &phone, &desktop, "desktop");
    assert!(list.is_member(&desktop.public()));
    assert_eq!(list.member(&desktop.public()).unwrap().added_by, Some(phone.public()));
    assert_eq!(list.added_by(&phone.public()), vec![desktop.public()]);
    assert_eq!(copy(&list).state(), list.state());
}

#[test]
fn a_key_outside_the_mesh_writes_nothing_that_counts() {
    let (laptop, stranger, friend) = (key(), key(), key());
    let (mut list, _) = DeviceList::create(&laptop, "laptop").unwrap();
    assert!(list.append(&stranger, Op::Add { device: friend.public(), name: "friend".into() }).is_err());

    // Forging the entry by hand, on top of the real list, gets it dropped by every copy.
    let mut forger = copy(&list);
    let forged = {
        // The forger signs an addition as itself, naming real parents.
        #[derive(serde::Serialize)]
        struct Body<'a> {
            mesh: erisoverlaynetwork::MeshId,
            parents: &'a [erisoverlaynetwork::EntryId],
            at: i64,
            op: &'static str,
            device: iroh::PublicKey,
            name: &'static str,
        }
        let body = Body {
            mesh: forger.mesh(),
            parents: forger.heads(),
            at: 0,
            op: "add",
            device: friend.public(),
            name: "friend",
        };
        Signed::sign(&stranger, erisoverlaynetwork::devices::ENTRY_DOMAIN, &body).unwrap()
    };
    assert!(forger.merge([forged.clone()]).is_empty());
    assert!(list.merge([forged]).is_empty());
    assert!(!list.is_member(&friend.public()));
}

#[test]
fn a_tampered_entry_does_not_verify() {
    let (laptop, phone) = (key(), key());
    let (mut list, genesis) = DeviceList::create(&laptop, "laptop").unwrap();
    let mut entry = add(&mut list, &laptop, &phone, "phone");
    use base64::Engine;
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let body = String::from_utf8(b64.decode(&entry.body).unwrap()).unwrap().replace("phone", "evilx");
    entry.body = b64.encode(body);
    let mut other = DeviceList::from_entries([genesis]).unwrap();
    assert!(other.merge([entry]).is_empty());
    assert!(!other.is_member(&phone.public()));
}

#[test]
fn another_meshs_entries_are_not_taken() {
    let (laptop, other_laptop, phone) = (key(), key(), key());
    let (mut mine, _) = DeviceList::create(&laptop, "laptop").unwrap();
    let (mut theirs, _) = DeviceList::create(&other_laptop, "laptop").unwrap();
    let entry = add(&mut theirs, &other_laptop, &phone, "phone");
    assert!(mine.merge(theirs.entries().into_iter().chain([entry])).is_empty());
    assert!(!mine.is_member(&phone.public()));
}

#[test]
fn removal_is_for_good() {
    let (laptop, phone) = (key(), key());
    let (mut list, _) = DeviceList::create(&laptop, "laptop").unwrap();
    add(&mut list, &laptop, &phone, "phone");
    list.append(&laptop, Op::Remove { devices: vec![phone.public()] }).unwrap();
    assert!(!list.is_member(&phone.public()));
    assert!(list.state().removed.contains(&phone.public()));
    assert!(list.append(&laptop, Op::Add { device: phone.public(), name: "phone".into() }).is_err());
    // A removed device writes nothing more.
    assert!(list.append(&phone, Op::Name { device: laptop.public(), name: "mine".into() }).is_err());
}

#[test]
fn a_stolen_phone_cannot_add_a_computer_behind_the_removers_back() {
    let (laptop, phone, tablet, thief) = (key(), key(), key(), key());
    let (mut laptops, _) = DeviceList::create(&laptop, "laptop").unwrap();
    add(&mut laptops, &laptop, &phone, "phone");
    let mut phones = copy(&laptops);
    // Months ago, the phone added the tablet, and the laptop heard about it.
    add(&mut phones, &phone, &tablet, "tablet");
    laptops.merge(phones.entries());
    assert!(laptops.is_member(&tablet.public()));

    // The phone is stolen. The thief adds a computer; the laptop has not heard.
    let computer = add(&mut phones, &phone, &thief, "computer");
    assert!(phones.is_member(&thief.public()));
    // The person removes the phone from the laptop, keeping the tablet.
    let removal = laptops.append(&laptop, Op::Remove { devices: vec![phone.public()] }).unwrap();

    // Whatever order the entries meet in, the computer never counts and the tablet stays.
    laptops.merge([computer]);
    phones.merge([removal]);
    for list in [&laptops, &phones, &copy(&laptops)] {
        assert!(!list.is_member(&phone.public()));
        assert!(!list.is_member(&thief.public()));
        assert!(list.is_member(&tablet.public()));
        assert!(list.is_member(&laptop.public()));
    }
    assert_eq!(laptops.state(), phones.state());
}

#[test]
fn a_removal_can_take_what_the_removed_device_added() {
    let (laptop, phone, tablet) = (key(), key(), key());
    let (mut list, _) = DeviceList::create(&laptop, "laptop").unwrap();
    add(&mut list, &laptop, &phone, "phone");
    add(&mut list, &phone, &tablet, "tablet");
    let also = list.added_by(&phone.public());
    list.append(&laptop, Op::Remove { devices: std::iter::once(phone.public()).chain(also).collect() }).unwrap();
    assert!(!list.is_member(&tablet.public()));
}

#[test]
fn two_devices_removing_each_other_at_once_are_both_removed() {
    let (laptop, phone, desktop) = (key(), key(), key());
    let (mut list, _) = DeviceList::create(&laptop, "laptop").unwrap();
    add(&mut list, &laptop, &phone, "phone");
    add(&mut list, &laptop, &desktop, "desktop");
    let mut phones = copy(&list);
    let a = list.append(&laptop, Op::Remove { devices: vec![phone.public()] }).unwrap();
    let b = phones.append(&phone, Op::Remove { devices: vec![laptop.public()] }).unwrap();
    list.merge([b]);
    phones.merge([a]);
    for l in [&list, &phones] {
        assert!(!l.is_member(&laptop.public()));
        assert!(!l.is_member(&phone.public()));
        assert!(l.is_member(&desktop.public()));
    }
    assert_eq!(list.state(), phones.state());
}

#[test]
fn a_device_can_be_named_anew_and_the_last_name_written_holds() {
    let (laptop, phone) = (key(), key());
    let (mut list, _) = DeviceList::create(&laptop, "laptop").unwrap();
    add(&mut list, &laptop, &phone, "phone");
    list.append(&phone, Op::Name { device: phone.public(), name: "pixel".into() }).unwrap();
    assert_eq!(list.member(&phone.public()).unwrap().name, "pixel");
    assert_eq!(copy(&list).member(&phone.public()).unwrap().name, "pixel");
}

#[test]
fn the_same_entries_in_any_order_mean_the_same_thing() {
    let keys: Vec<SecretKey> = (0..6).map(|_| key()).collect();
    let (mut list, genesis) = DeviceList::create(&keys[0], "zero").unwrap();
    for i in 1..keys.len() {
        add(&mut list, &keys[i - 1], &keys[i], &format!("d{i}"));
    }
    list.append(&keys[3], Op::Remove { devices: vec![keys[1].public()] }).unwrap();
    let mut entries = list.entries();
    entries.reverse();
    let mut shuffled = DeviceList::from_entries([genesis]).unwrap();
    // Entries whose parents have not arrived wait for them within one merge.
    shuffled.merge(entries);
    assert_eq!(shuffled.state(), list.state());
    assert_eq!(shuffled.heads(), list.heads());
}

#[test]
fn names_are_checked() {
    let (laptop, phone) = (key(), key());
    let (mut list, _) = DeviceList::create(&laptop, "laptop").unwrap();
    assert!(list.append(&laptop, Op::Add { device: phone.public(), name: "".into() }).is_err());
    assert!(list.append(&laptop, Op::Add { device: phone.public(), name: "a/b".into() }).is_err());
    assert!(list.append(&laptop, Op::Add { device: phone.public(), name: "x".repeat(65) }).is_err());
    assert!(DeviceList::create(&laptop, "").is_err());
}

#[test]
fn a_pin_names_the_app_providing_a_service_until_replaced_or_revoked() {
    let (laptop, phone) = (key(), key());
    let (mut list, _) = DeviceList::create(&laptop, "laptop").unwrap();
    add(&mut list, &laptop, &phone, "phone");
    let (db, db2) = (key().public(), key().public());
    let note = erisoverlaynetwork::NoteId([1; 32]);
    let note2 = erisoverlaynetwork::NoteId([2; 32]);
    list.append(&laptop, Op::Pin { service: "erisdb".into(), app: db, note }).unwrap();
    assert_eq!(list.pinned("erisdb").unwrap().app, db);
    assert_eq!(list.pinned("erisdb").unwrap().by, laptop.public());
    list.append(&phone, Op::Pin { service: "erisdb".into(), app: db2, note: note2 }).unwrap();
    assert_eq!(copy(&list).pinned("erisdb").unwrap().app, db2);
    list.append(&laptop, Op::Revoke { note: note2 }).unwrap();
    assert!(list.pinned("erisdb").is_none());
    assert!(list.append(&laptop, Op::Pin { service: "Not Ok".into(), app: db, note }).is_err());
}
