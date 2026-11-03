//! Discovery's reader of `<link>` elements (REQ-2476, REQ-TST-040): any
//! bytes as a page's body; at most `MAX_ELEMENT_POINTERS` pointers come
//! out, each target that is not empty lies within the first `DISCOVERY_BODY_BYTES` of the
//! body, and each is checked and resolved against the answering address.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_web::discovery::{self, DISCOVERY_BODY_BYTES, MAX_ELEMENT_POINTERS};
use neoteletext_web::url::Url;

fuzz_target!(|data: &[u8]| {
    let Ok(answered) = Url::parse("https://example.org/news/") else { return };
    let read = data.as_ptr_range();
    let limit = data.as_ptr().wrapping_add(DISCOVERY_BODY_BYTES.min(data.len()));
    let mut count = 0_usize;
    discovery::read_elements(data, &mut |pointer| {
        count += 1;
        let target = pointer.target.as_ptr_range();
        // An empty target may be a constant empty slice, which lies nowhere.
        let inside = target.start >= read.start && target.end <= limit;
        assert!(pointer.target.is_empty() || inside, "a target outside the bytes read");
        let _ = core::hint::black_box(discovery::resolve(&pointer, &answered));
    });
    assert!(count <= MAX_ELEMENT_POINTERS, "{count} pointers from the elements");
});
