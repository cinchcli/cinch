use client_core::sync::lockfile::{LockKind, Lockfile};
use tempfile::tempdir;

#[test]
fn one_writer_at_a_time() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("sync.lock");

    let a = Lockfile::try_acquire(&p, LockKind::Desktop).unwrap();
    assert!(a.is_some(), "first acquire must succeed");

    let b = Lockfile::try_acquire(&p, LockKind::Cli).unwrap();
    assert!(b.is_none(), "second acquire must fail while first held");

    drop(a);

    let c = Lockfile::try_acquire(&p, LockKind::Cli).unwrap();
    assert!(c.is_some(), "after drop the lock is free");
}
