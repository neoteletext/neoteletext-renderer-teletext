//! `cargo xtask check-noto` (REQ-FNT-001, REQ-FNT-012) and
//! `cargo xtask check-misc-fixed` (REQ-2223): every file of a third-party
//! font folder is accounted for. Each BDF file is the file its provenance
//! record names, by SHA-256; every font the record names is there; each
//! font line of the folder's `fonts.txt` gives the recorded checksum; and
//! any other file is one the folder is known to hold.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// A third-party font folder and the files in it that are not fonts.
pub struct FontFolder {
    /// The folder, from the workspace root.
    pub folder: &'static str,
    /// Every file in it other than its BDF files.
    pub others: &'static [&'static str],
}

/// The provenance record of every folder.
const PROVENANCE: &str = "PROVENANCE.md";
/// The library index of every folder (REQ-FNT-004).
const INDEX: &str = "fonts.txt";
/// A checksum line of the record: "- `NAME.bdf`: `SHA-256`".
const LINE_PREFIX: &str = "- `";
/// A font line of the index: `f`, the file, the name and the SHA-256, separated by tabs (REQ-FNT-004).
const INDEX_FONT_PREFIX: &str = "f\t";

/// Noto Teletext, the compiled font (DEC-1761).
pub const NOTO: FontFolder = FontFolder {
    folder: "third_party/noto",
    others: &[PROVENANCE, INDEX, "OFL.txt", "convert.py", "settings.txt", "sources.txt"],
};

/// The X11 misc-fixed 6x10 font and its 12 by 20 conversion (REQ-2223).
pub const MISC_FIXED: FontFolder = FontFolder { folder: "third_party/misc-fixed", others: &[PROVENANCE, INDEX] };

/// The checksums the record gives, by file name.
fn recorded(record: &str, provenance: &str) -> Result<BTreeMap<String, String>, String> {
    let mut found = BTreeMap::new();
    for line in record.lines() {
        let Some(rest) = line.strip_prefix(LINE_PREFIX) else { continue };
        let Some((name, after)) = rest.split_once("`: `") else { continue };
        if Path::new(name).extension().is_none_or(|extension| extension != "bdf") {
            continue;
        }
        let digest = after.trim_end_matches('`');
        if found.insert(name.to_string(), digest.to_string()).is_some() {
            return Err(format!("{provenance} records {name} twice"));
        }
    }
    Ok(found)
}

/// Each font line of the index agrees with the record.
fn check_index(index: &str, path: &str, expected: &BTreeMap<String, String>) -> Result<(), String> {
    for line in index.lines() {
        let Some(rest) = line.strip_prefix(INDEX_FONT_PREFIX) else { continue };
        let fields: Vec<&str> = rest.split('\t').collect();
        let [file, _, digest] = fields.as_slice() else {
            return Err(format!("{path}: {line:?} is not a font line of three fields"));
        };
        match expected.get(*file) {
            Some(recorded) if recorded == digest => {}
            Some(recorded) => {
                return Err(format!("{path} gives {file} SHA-256 {digest}, but the record gives {recorded}"));
            }
            None => return Err(format!("{path} names {file}, which its provenance record does not")),
        }
    }
    Ok(())
}

/// Checks one folder against its record.
pub fn check(root: &Path, font_folder: &FontFolder) -> Result<(), String> {
    let folder = font_folder.folder;
    let provenance = format!("{folder}/{PROVENANCE}");
    let record = fs::read_to_string(root.join(&provenance)).map_err(|e| format!("{provenance}: {e}"))?;
    let mut expected = recorded(&record, &provenance)?;
    let index = format!("{folder}/{INDEX}");
    check_index(&fs::read_to_string(root.join(&index)).map_err(|e| format!("{index}: {e}"))?, &index, &expected)?;
    let entries = fs::read_dir(root.join(folder)).map_err(|e| format!("{folder}: {e}"))?;
    let mut checked = 0usize;
    for entry in entries {
        let path = entry.map_err(|e| format!("{folder}: {e}"))?.path();
        let name = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
        if path.extension().is_none_or(|extension| extension != "bdf") {
            if path.is_file() && font_folder.others.contains(&name.as_str()) {
                continue;
            }
            return Err(format!("{folder}/{name} is neither a recorded font nor a file the folder is known to hold"));
        }
        let digest = expected.remove(&name).ok_or(format!("{folder}/{name} is not in {provenance}"))?;
        let bytes = fs::read(&path).map_err(|e| format!("{folder}/{name}: {e}"))?;
        let actual = neoteletext_web::sha256::to_hex(&neoteletext_web::sha256::digest(&bytes));
        if digest.as_bytes() != actual {
            return Err(format!(
                "{folder}/{name} has SHA-256 {}, but {provenance} records {digest}",
                String::from_utf8_lossy(&actual)
            ));
        }
        checked += 1;
    }
    if let Some(name) = expected.keys().next() {
        return Err(format!("{provenance} records {name}, which is not in {folder}"));
    }
    println!("{folder}: {checked} fonts match {provenance}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font line of the index must give the recorded checksum.
    #[test]
    fn the_index_must_agree_with_the_record() {
        let expected = BTreeMap::from([("a.bdf".to_string(), "11".to_string())]);
        assert!(check_index("# c\nf\ta.bdf\tA\t11\nc\ta.bdf\tG0-Latin\n", "fonts.txt", &expected).is_ok());
        let differs = check_index("f\ta.bdf\tA\t12\n", "fonts.txt", &expected).unwrap_err();
        assert!(differs.contains("gives a.bdf SHA-256 12"), "{differs}");
        let unknown = check_index("f\tb.bdf\tB\t11\n", "fonts.txt", &expected).unwrap_err();
        assert!(unknown.contains("names b.bdf"), "{unknown}");
    }

    /// Both folders as committed pass.
    #[test]
    fn the_committed_folders_match_their_records() {
        let root = crate::workspace_root();
        check(&root, &NOTO).unwrap();
        check(&root, &MISC_FIXED).unwrap();
    }
}
