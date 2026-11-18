//! `appendix-a` and `check-appendix-a`: appendix A of neoteletext-common's
//! `docs/page-rules.md`, the readable patch fields of each synthesizer
//! family, written from the families' `PARAMETERS` tables (page-rules.md,
//! RULE-29), and compared with the document so that the two cannot drift.
//! A page's rules name a field by its number, so a table that moved would
//! silently read another field.

use std::fmt::Write as _;
use std::path::PathBuf;

use neoteletext_audio_instruments::midi::{PatchField, family};
use neoteletext_audio_instruments::watch::fields;

/// The heading the appendix starts at, to the document's end.
const HEADING: &str = "## Appendix A. The readable patch fields of each family";
/// The document, within neoteletext-common.
const DOCUMENT: &str = "docs/page-rules.md";
/// synth7's operators and the fields of each (appendix A.2: "the six
/// operators' fields, 21 each").
const SYNTH7_OPERATORS: usize = 6;
const SYNTH7_OPERATOR_FIELDS: usize = 21;

/// How a section's table is laid out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// A number and a name: every value is a register's byte.
    Names,
    /// A number, a name and a range.
    Ranges,
    /// synth7's: one operator's fields, then the voice's own.
    Synth7,
}

/// One section of the appendix: its heading, the family and variant whose
/// table it prints, the sentence before the table if it has one, and its
/// layout.
struct Section {
    heading: &'static str,
    code: u8,
    variant: u8,
    note: Option<&'static str>,
    layout: Layout,
}

const SECTIONS: [Section; 8] = [
    Section {
        heading: "A.1 The synth6581 family (synth6581, synth8580): family code 12",
        code: family::SYNTH6581,
        variant: 0,
        note: Some(
            "The index is the ASID register identifier (synth6581-synth8580.md S6581-11); every value is the register's last written byte, 0 to 255.",
        ),
        layout: Layout::Names,
    },
    Section {
        heading: "A.2 synth7: family code 13",
        code: family::SYNTH7,
        variant: 0,
        note: Some(
            "Numbers 0 to 125 are the six operators' fields, 21 each: operator `k` (1 to 6) has its field `j` of the table below at number `(k − 1) × 21 + j`, named `op<k>_` and the name.",
        ),
        layout: Layout::Synth7,
    },
    Section {
        heading: "A.3 synth106: family code 14, variant 0",
        code: family::SYNTH106,
        variant: 0,
        note: None,
        layout: Layout::Ranges,
    },
    Section {
        heading: "A.4 synth60: family code 14, variant 1",
        code: family::SYNTH106,
        variant: 1,
        note: None,
        layout: Layout::Ranges,
    },
    Section {
        heading: "A.5 synthD: family code 15",
        code: family::SYNTHD,
        variant: 0,
        note: None,
        layout: Layout::Ranges,
    },
    Section {
        heading: "A.6 The synth53 family (synth53, synth52, synth51): family code 16, variants 0, 1 and 2",
        code: family::SYNTH53,
        variant: 0,
        note: None,
        layout: Layout::Ranges,
    },
    Section {
        heading: "A.7 synth8: family code 17",
        code: family::SYNTH8,
        variant: 0,
        note: Some("On synth8 part 1 is the upper patch and part 2 the lower (synth8.md S8-18)."),
        layout: Layout::Ranges,
    },
    Section {
        heading: "A.8 synth80: family code 18",
        code: family::SYNTH80,
        variant: 0,
        note: None,
        layout: Layout::Ranges,
    },
];

/// A field's range as the appendix writes it: `least to greatest`, or an
/// enumeration's values in order.
fn range(field: &PatchField) -> String {
    if field.values.is_empty() {
        return format!("{} to {}", field.least, field.greatest);
    }
    field.values.iter().map(|value| format!("`{value}`")).collect::<Vec<_>>().join(", ")
}

/// The rows of a table of numbers, names and ranges, the first numbered
/// `first`.
fn rows(out: &mut String, first: usize, fields: &[PatchField], name: impl Fn(&PatchField) -> &str) {
    for (number, field) in fields.iter().enumerate() {
        let _ = writeln!(out, "| {} | `{}` | {} |", first.saturating_add(number), name(field), range(field));
    }
}

/// synth7's section: operator 1's fields without their prefix, once the
/// other five are seen to be the same fields, then the voice's own.
fn synth7(out: &mut String, fields: &[PatchField]) -> Result<(), String> {
    let operators = SYNTH7_OPERATORS.saturating_mul(SYNTH7_OPERATOR_FIELDS);
    let (operator_fields, voice) =
        fields.split_at_checked(operators).ok_or("synth7's table is shorter than its operators' fields")?;
    let (first, _) = operator_fields.split_at(SYNTH7_OPERATOR_FIELDS);
    for (index, operator) in operator_fields.chunks(SYNTH7_OPERATOR_FIELDS).enumerate() {
        let prefix = format!("op{}_", index.saturating_add(1));
        for (field, pattern) in operator.iter().zip(first) {
            let same = field.name.strip_prefix(&prefix) == pattern.name.strip_prefix("op1_")
                && (field.least, field.greatest, field.values) == (pattern.least, pattern.greatest, pattern.values);
            if !same || !field.name.starts_with(&prefix) {
                return Err(format!(
                    "synth7's field {} is not operator 1's {} for its operator",
                    field.name, pattern.name
                ));
            }
        }
    }
    out.push_str("| `j` | Field (after `op<k>_`) | Range |\n|---|---|---|\n");
    rows(out, 0, first, |field| field.name.strip_prefix("op1_").unwrap_or(field.name));
    out.push_str("\nThe voice's own fields follow:\n\n| Number | Field | Range |\n|---|---|---|\n");
    rows(out, operators, voice, |field| field.name);
    Ok(())
}

/// The appendix as the families' tables give it, from its heading to the
/// document's end.
fn text() -> Result<String, String> {
    let mut out = format!(
        "{HEADING}\n\nThe parameter number is the field's place in its family's table, from 0 (RULE-29). The tables below are the program protocol's parameter types at schema version 13, in schema order; a range written as a list is an enumeration, read as the ordinal of its value from 0.\n"
    );
    for section in &SECTIONS {
        let fields =
            fields(section.code, section.variant).ok_or_else(|| format!("no table for {}", section.heading))?;
        let _ = write!(out, "\n### {}\n\n", section.heading);
        if let Some(note) = section.note {
            let _ = write!(out, "{note}\n\n");
        }
        match section.layout {
            Layout::Names => {
                out.push_str("| Number | Field |\n|---|---|\n");
                for (number, field) in fields.iter().enumerate() {
                    let _ = writeln!(out, "| {number} | `{}` |", field.name);
                }
            }
            Layout::Ranges => {
                out.push_str("| Number | Field | Range |\n|---|---|---|\n");
                rows(&mut out, 0, fields, |field| field.name);
            }
            Layout::Synth7 => synth7(&mut out, fields)?,
        }
    }
    Ok(out)
}

/// `appendix-a`: prints the appendix.
pub fn print() -> Result<(), String> {
    print!("{}", text()?);
    Ok(())
}

/// The folder of neoteletext-common: where `NEOTELETEXT_COMMON` says, or
/// beside the other repositories in the home folder's `devel`, as the gate
/// machinery finds it (`scripts/gate.sh`).
fn common() -> Result<PathBuf, String> {
    if let Some(folder) = std::env::var_os("NEOTELETEXT_COMMON") {
        return Ok(PathBuf::from(folder));
    }
    let home = std::env::var_os("HOME").ok_or("neither NEOTELETEXT_COMMON nor HOME is set")?;
    Ok(PathBuf::from(home).join("devel").join("neoteletext-common"))
}

/// `check-appendix-a`: compares the appendix with the document's, and says
/// where they first differ.
pub fn check() -> Result<(), String> {
    let path = common()?.join(DOCUMENT);
    let document = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let start = document.find(HEADING).ok_or_else(|| format!("{} has no \"{HEADING}\"", path.display()))?;
    let written = document.get(start..).unwrap_or_default();
    let expected = text()?;
    if written == expected {
        println!("ok: appendix A of {} is the families' PARAMETERS tables", path.display());
        return Ok(());
    }
    let line = written.lines().zip(expected.lines()).position(|(written, expected)| written != expected);
    let line = line.unwrap_or_else(|| written.lines().count().min(expected.lines().count()));
    Err(format!(
        "appendix A of {} is not what the families' PARAMETERS tables print (cargo xtask appendix-a); they first differ at its line {}:\n  the document: {}\n  the tables:   {}",
        path.display(),
        line.saturating_add(1),
        written.lines().nth(line).unwrap_or("(its end)"),
        expected.lines().nth(line).unwrap_or("(their end)"),
    ))
}
