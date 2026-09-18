//! The drift gate: what is committed in `harness/protocol` is exactly what the
//! generator produces today.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // harness/xtask -> harness -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

mod xtask {
    #[test]
    fn typegen_is_committed() {
        let root = super::repo_root();
        match xtask::typegen::check(&root) {
            Err(e) => panic!("typegen could not run: {e}"),
            Ok(drift) if drift.is_empty() => {}
            Ok(drift) => {
                let lines: Vec<String> = drift.iter().map(ToString::to_string).collect();
                panic!(
                    "harness/protocol has drifted from orrery-proto:\n  {}",
                    lines.join("\n  ")
                );
            }
        }
    }

    #[test]
    fn the_schema_is_deterministic() {
        assert_eq!(
            xtask::typegen::schema(),
            xtask::typegen::schema(),
            "two runs must produce the same bytes, or the drift gate flaps"
        );
    }
}
