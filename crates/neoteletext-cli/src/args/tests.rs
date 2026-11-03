#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, `panic!` on outcomes it rules out, and indexes the Colour Map entries it checks"
)]
use super::*;
use neoteletext_app::exit;

fn parse(arguments: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(["neottx"].iter().chain(arguments))
}

fn level(arguments: &[&str]) -> Option<LevelChoice> {
    match parse(arguments).map(|cli| cli.command) {
        Ok(Command::List { common }) => common.level,
        other => panic!("not a list command: {other:?}"),
    }
}

/// REQ-CLI-007: each level is accepted, any other value is invalid
/// usage, and the former `--level1-only` option is gone.
#[test]
fn level_option_takes_the_four_levels() {
    assert_eq!(level(&["list", "in.t42"]), None);
    for (text, expected) in [
        ("1", LevelChoice::Level1),
        ("1.5", LevelChoice::Level15),
        ("2.5", LevelChoice::Level25),
        ("3.5", LevelChoice::Level35),
    ] {
        assert_eq!(level(&["list", "in.t42", "--level", text]), Some(expected), "{text}");
    }
    for invalid in ["2", "1.0", "4", "level1"] {
        let error = parse(&["list", "in.t42", "--level", invalid]).unwrap_err();
        assert!(error.use_stderr(), "{invalid} is invalid usage (exit status {})", exit::USAGE);
    }
    assert!(parse(&["list", "in.t42", "--level1-only"]).is_err());
}

/// TODO-1878: the help of `write-text` names every set the writer takes. The
/// list stays in the doc comment, because help is the same in every build
/// and a build without `write-text` has no table to generate it from.
#[cfg(feature = "write-text")]
#[test]
fn write_text_help_names_every_set() {
    use clap::CommandFactory as _;
    let mut command = Cli::command();
    let help = command.find_subcommand_mut("write-text").unwrap().render_long_help().to_string();
    let words: Vec<&str> = help
        .split(|character: char| !(character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'))
        .collect();
    for (set, ..) in neoteletext_app::write_text::SETS {
        assert!(words.contains(&set), "{set} is not in:\n{help}");
    }
}

/// REQ-377 (REQ-RND-005), DEC-2482: `--colours` replaces the Colour Map
/// from entry 0 with the colours given, keeps entry 8 transparent, and
/// refuses a colour that is not three hexadecimal digits or a list longer
/// than the map.
#[test]
fn the_colours_option_replaces_table_30_from_entry_0() {
    let colours = |arguments: &[&str]| match parse(arguments).map(|cli| cli.command) {
        Ok(Command::List { common }) => common.colours,
        other => panic!("not a list command: {other:?}"),
    };
    assert_eq!(colours(&["list", "in.t42"]), None);
    let map = colours(&["list", "in.t42", "--colours", "111,f80, 0F0"]).unwrap();
    assert_eq!(map[0], Rgb12::new(1, 1, 1));
    assert_eq!(map[1], Rgb12::new(15, 8, 0));
    assert_eq!(map[2], Rgb12::new(0, 15, 0));
    assert_eq!(map[3..], ColourMap::DEFAULT.entries[3..], "the rest stay Table 30's");
    let nine = "000,111,222,333,444,555,666,777,888";
    let map = colours(&["list", "in.t42", "--colours", nine]).unwrap();
    assert_eq!(map[usize::from(COLOUR_TRANSPARENT)], ColourMap::DEFAULT.entry(COLOUR_TRANSPARENT));
    for refused in ["12", "1234", "GGG", &["000"; 33].join(",")] {
        assert!(parse(&["list", "in.t42", "--colours", refused]).is_err(), "{refused}");
    }
}
