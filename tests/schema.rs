#[path = "../src/persistence/mod.rs"]
mod persistence;

use std::io::Write;
use std::process::{Command, Stdio};

fn sql(statement: &str) -> String {
    let database = std::env::var("WHORL_SCHEMA_TEST_URL")
        .expect("set WHORL_SCHEMA_TEST_URL to a disposable PostgreSQL 18 database");
    let mut child = Command::new("psql")
        .args(["-X", "-w", "-qAt", "-v", "ON_ERROR_STOP=1", &database])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("psql must be installed");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(statement.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn unsigned_hashes_preserve_every_bit() {
    for value in [0, 1, i32::MAX as u32, 0x8000_0000, u32::MAX] {
        assert_eq!(
            persistence::hash_from_sql(persistence::hash_to_sql(value)),
            value
        );
    }
    assert_eq!(persistence::hash_to_sql(u32::MAX), -1);
    assert!(persistence::APPLICATION_MIGRATION.starts_with("BEGIN;"));
}

#[test]
#[ignore = "requires a disposable PostgreSQL 18 database; see docs/persistence.md"]
fn postgres_schema() {
    sql(persistence::APPLICATION_MIGRATION);
    sql(include_str!("schema.sql"));
}
