use client_core::sync::lockfile::{LockKind, Lockfile};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = PathBuf::from(
        args.get(1)
            .cloned()
            .unwrap_or_else(|| "/tmp/cinch.lock".into()),
    );
    let kind = if args.get(2).map(|s| s.as_str()) == Some("first") {
        LockKind::Desktop
    } else {
        LockKind::Cli
    };
    match Lockfile::try_acquire(&path, kind).unwrap() {
        Some(_) => {
            println!("acquired");
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        None => {
            println!("busy");
        }
    }
}
