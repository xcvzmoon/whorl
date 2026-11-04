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
fn postgres_schema_and_reservation_races() {
    sql(persistence::APPLICATION_MIGRATION);
    sql(include_str!("fixtures/schema.sql"));
    let threads: Vec<_> = (1..=12).map(|id| std::thread::spawn(move || {
        sql(&format!("SELECT reserve_capacity('00000000-0000-0000-0000-{id:012}', 'race', 1, 10, clock_timestamp() + interval '1 hour');"))
    })).collect();
    let admitted = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|result| result.trim() == "t")
        .count();
    assert_eq!(admitted, 3);
    assert_eq!(
        sql("SELECT count(*) || ':' || sum(bytes) FROM capacity_reservations;").trim(),
        "3:30"
    );
    sql("DO $$ BEGIN
        IF reserve_capacity('50000000-0000-0000-0000-000000000001', 'other', 3, 1, clock_timestamp() + interval '1 hour') THEN RAISE EXCEPTION 'global race limit'; END IF;
        IF NOT reserve_capacity('50000000-0000-0000-0000-000000000001', 'other', 2, 20, clock_timestamp() + interval '1 hour') THEN RAISE EXCEPTION 'remaining global capacity'; END IF;
        UPDATE capacity_reservations SET lease_until = created_at + interval '1 microsecond';
        IF reserve_capacity('50000000-0000-0000-0000-000000000002', 'other', 1, 1, clock_timestamp() + interval '1 hour') THEN RAISE EXCEPTION 'expired storage released before cleanup'; END IF;
        DELETE FROM capacity_reservations WHERE lease_until < clock_timestamp();
        IF NOT reserve_capacity('50000000-0000-0000-0000-000000000002', 'other', 1, 1, clock_timestamp() + interval '1 hour') THEN RAISE EXCEPTION 'cleanup did not release capacity'; END IF;
    END $$;");
}
