#![cfg(unix)]

use erisoverlaynetwork::sys;

#[test]
fn a_process_is_named_by_the_executable_it_runs() {
    let me = sys::executable(std::process::id() as i32).unwrap();
    assert_eq!(me.canonicalize().unwrap(), std::env::current_exe().unwrap().canonicalize().unwrap());
    assert!(sys::executable(-1).is_none());
}

#[test]
fn this_machine_has_a_name() {
    let name = sys::hostname().unwrap();
    assert!(!name.is_empty() && !name.ends_with(".local"));
}
