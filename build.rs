//! Embeds the vendored Lucide icons (`assets/lucide`, see
//! `scripts/update-lucide.sh`) as a table sorted by name, so the daemon needs
//! no files at run time. Fails the build when the vendored set is incomplete.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/lucide");
    println!("cargo:rerun-if-changed={}", root.display());

    let version = fs::read_to_string(root.join("VERSION")).expect("assets/lucide/VERSION");
    let tags_json = fs::read_to_string(root.join("tags.json")).expect("assets/lucide/tags.json");
    let tags: BTreeMap<String, Vec<String>> =
        serde_json::from_str(&tags_json).expect("assets/lucide/tags.json is name -> [tag]");

    let icons_dir = root.join("icons");
    for entry in fs::read_dir(&icons_dir).expect("assets/lucide/icons") {
        let path = entry.expect("an icon file").path();
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        assert!(
            tags.contains_key(name),
            "{} is not named in tags.json; run scripts/update-lucide.sh",
            path.display()
        );
    }

    let mut table = String::new();
    writeln!(table, "/// The vendored Lucide version.").unwrap();
    writeln!(table, "pub const VERSION: &str = {:?};", version.trim()).unwrap();
    writeln!(table, "/// Every icon, sorted by name.").unwrap();
    writeln!(table, "pub static ICONS: &[Icon] = &[").unwrap();
    for (name, tags) in &tags {
        assert!(
            tags.iter().all(|tag| !tag.contains('\n')),
            "a tag of '{name}' holds a newline, which separates tags"
        );
        let tags = tags.join("\n");
        let svg = icons_dir.join(format!("{name}.svg"));
        assert!(
            svg.is_file(),
            "tags.json names '{name}' but {} is missing",
            svg.display()
        );
        let svg = svg.to_str().expect("a UTF-8 path");
        writeln!(
            table,
            "    Icon {{ name: {name:?}, svg: include_bytes!({svg:?}), tags: {tags:?} }},"
        )
        .unwrap();
    }
    writeln!(table, "];").unwrap();

    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("lucide.rs");
    fs::write(out, table).expect("write lucide.rs");
}
