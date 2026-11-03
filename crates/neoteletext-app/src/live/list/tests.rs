#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "a test"
)]
use super::*;

/// The page in the tests, and its list of 130 sub-pages: three parts.
const PAGE: u16 = 0x100;
const TOTAL: u16 = 130;

/// The sub-codes of the list: the first [`TOTAL`] a page may have, from 0001.
fn subcodes() -> Vec<u16> {
    (1u16..).filter(|&value| neoteletext_core::limits::is_sub_code(value)).take(usize::from(TOTAL)).collect()
}

/// The sub-code at `place` in the list, from 0.
fn at(place: usize) -> SubCode {
    SubCode::new(subcodes()[place])
}

/// The last place in the list.
const LAST: usize = TOTAL as usize - 1;

fn part(number: u8) -> ListPart {
    let all = subcodes();
    let first = usize::from(number) * MAX_LIST_ROWS;
    let rows = all[first..(first + MAX_LIST_ROWS).min(all.len())].to_vec();
    ListPart { page: PAGE, part: number, total: TOTAL, version: "v".into(), subcodes: rows }
}

/// Steps once, answering every part asked for at once, and returns the
/// sub-code reached and the parts asked for.
fn step_answered(list: &mut PageList, current: SubCode, forward: bool) -> (SubPageStep, Vec<u8>) {
    let mut asked_all = Vec::new();
    loop {
        let mut asked = Vec::new();
        let step = list.step(current, forward, &mut |number| asked.push(number));
        asked_all.extend(&asked);
        if step != SubPageStep::Waiting {
            return (step, asked_all);
        }
        assert!(!asked.is_empty(), "waiting with nothing asked for");
        for number in asked {
            list.take(&part(number));
        }
    }
}

// Verifies REQ-328 and REQ-1890: stepping forward from before the first
// reaches every sub-page of the list in ascending order, across both
// boundaries between parts, asks for each part once, and wraps to the first.
#[test]
fn stepping_forward_reaches_every_sub_page_in_order() {
    let mut list = PageList::new(PAGE);
    let mut current = SubCode::ANY;
    let mut reached = Vec::new();
    let mut asked = Vec::new();
    for _ in 0..TOTAL {
        let (step, parts) = step_answered(&mut list, current, true);
        asked.extend(parts);
        let SubPageStep::To(next) = step else { panic!("{step:?}") };
        reached.push(next.value());
        current = next;
    }
    assert_eq!(reached, subcodes());
    assert_eq!(asked, [0, 1, 2]);
    assert_eq!(step_answered(&mut list, current, true).0, SubPageStep::To(at(0)), "wraps");
}

// Verifies REQ-328: stepping back from the first wraps to the list's last.
#[test]
fn stepping_back_from_the_first_wraps_to_the_last() {
    let mut list = PageList::new(PAGE);
    let (step, asked) = step_answered(&mut list, at(0), false);
    assert_eq!(step, SubPageStep::To(at(LAST)));
    assert_eq!(asked, [0, 2]);
    let (step, _) = step_answered(&mut list, at(LAST - 1), true);
    assert_eq!(step, SubPageStep::To(at(LAST)));
}

// Verifies REQ-328: a sub-page opened far into the list is found by asking
// for the parts between, and its neighbour follows.
#[test]
fn a_sub_page_far_into_the_list_is_found() {
    let mut list = PageList::new(PAGE);
    let (step, asked) = step_answered(&mut list, at(LAST - 1), true);
    assert_eq!(step, SubPageStep::To(at(LAST)));
    assert_eq!(asked, [0, 1, 2]);
}

// Verifies DOC-2038: a page with no list (part 0 missing) steps through the
// store, and so does a list that broke its order.
#[test]
fn no_list_or_a_broken_one_steps_through_the_store() {
    let mut list = PageList::new(PAGE);
    assert_eq!(list.step(SubCode::ANY, true, &mut |_| {}), SubPageStep::Waiting);
    list.missing(0);
    assert_eq!(list.step(SubCode::ANY, true, &mut |_| {}), SubPageStep::NoList);
    let mut broken = PageList::new(PAGE);
    let mut descending = part(0);
    descending.subcodes.reverse();
    broken.take(&descending);
    assert_eq!(broken.step(SubCode::ANY, true, &mut |_| {}), SubPageStep::NoList);
}

// Verifies REQ-1893: no more than four parts are held at once.
#[test]
fn few_parts_are_held() {
    let mut list = PageList::new(PAGE);
    let large = |number: u8| ListPart { total: 8190, ..ListPart { part: number, ..part(0) } };
    for number in 0..10 {
        let mut part = large(number);
        part.subcodes = (0..MAX_LIST_ROWS).map(|row| u16::try_from(usize::from(number) * 100 + row).unwrap()).collect();
        list.take(&part);
    }
    assert_eq!(list.parts.len(), MAX_HELD_PARTS);
    assert!(list.parts.contains_key(&9));
}
