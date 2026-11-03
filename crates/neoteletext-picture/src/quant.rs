//! Three ways to reduce the picture to a few colours, each the start of a
//! palette the encoder then fits: median cut, maximum coverage and a fast
//! octree. They follow a widely used picture library's quantizers step for
//! step (`CREDITS.md`), hash table and heap included, since the iteration
//! order of those structures decides the result; the octree breaks ties
//! between equal counts in bucket order (DEC-2142 item 3).
//!
//! They work on the page's 6,000 sixels and palettes of at most 32
//! colours, never on the input's sizes, so each function that indexes
//! allows it with that reason.

use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Px {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Px {
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn channel(self, axis: usize) -> u8 {
        [self.r, self.g, self.b][axis]
    }
}

#[allow(
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn dist_sqr(a: Px, b: Px) -> u32 {
    let sq = |x: u8, y: u8| {
        let d = i32::from(x) - i32::from(y);
        d * d
    };
    (sq(a.r, b.r) + sq(a.g, b.g) + sq(a.b, b.b)) as u32
}

/// The pixel hash.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn pixel_hash(p: Px) -> u32 {
    u32::from(p.r).wrapping_mul(463)
        ^ (u32::from(p.g) << 8).wrapping_mul(10069)
        ^ (u32::from(p.b) << 16).wrapping_mul(64997)
}

#[derive(Clone, Copy)]
enum Compare {
    /// By the colours' hashes, so two colours with one hash are one key.
    ByHash,
    /// By red, then green, then blue.
    Unshifted,
}

/// A chained hash table: chains kept sorted by the comparison, resized
/// on insertion, iterated bucket by bucket. The iteration order is the output.
struct Table {
    compare: Compare,
    chains: Vec<Vec<(Px, u32)>>,
    count: u32,
}

const MIN_LENGTH: u32 = 11;
const RESIZE_FACTOR: u32 = 3;

/// The table's next length, as the original computes it: `!start % t` is always 0, so the
/// loop that looks like a primality test never rejects, and the result is
/// the first value whose low four bits pass the unit table.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn find_prime(mut start: u32, dir: i32) -> u32 {
    const UNIT: [bool; 16] =
        [false, true, false, true, false, false, false, true, false, true, false, true, false, true, false, false];
    while start > 1 {
        if UNIT[(start & 0x0f) as usize] {
            break;
        }
        start = start.wrapping_add_signed(dir);
    }
    start
}

impl Table {
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn new(compare: Compare) -> Self {
        Self { compare, chains: vec![Vec::new(); MIN_LENGTH as usize], count: 0 }
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn length(&self) -> u32 {
        self.chains.len() as u32
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn cmp(&self, a: Px, b: Px) -> i32 {
        match self.compare {
            Compare::ByHash => {
                let (x, y) = (pixel_hash(a), pixel_hash(b));
                match x.cmp(&y) {
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Greater => 1,
                }
            }
            Compare::Unshifted => {
                if a.r != b.r {
                    i32::from(a.r) - i32::from(b.r)
                } else if a.g != b.g {
                    i32::from(a.g) - i32::from(b.g)
                } else {
                    i32::from(a.b) - i32::from(b.b)
                }
            }
        }
    }

    /// Where `key` is in its chain, or where it goes: `Ok(found)`, `Err(insert at)`.
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn find(&self, bucket: usize, key: Px) -> Result<usize, usize> {
        for (index, node) in self.chains[bucket].iter().enumerate() {
            let order = self.cmp(node.0, key);
            if order == 0 {
                return Ok(index);
            }
            if order > 0 {
                return Err(index);
            }
        }
        Err(self.chains[bucket].len())
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn bucket(&self, key: Px) -> usize {
        (pixel_hash(key) % self.length()) as usize
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn resize(&mut self) {
        let old = self.length();
        let mut new = old;
        if self.count * RESIZE_FACTOR < old {
            new = find_prime(old / 2 - 1, -1);
        } else if old * RESIZE_FACTOR < self.count {
            new = find_prime(old * 2 + 1, 1);
        }
        if new < MIN_LENGTH {
            new = old;
        }
        if new != old {
            let chains = std::mem::replace(&mut self.chains, vec![Vec::new(); new as usize]);
            self.count = 0;
            for chain in chains {
                for (key, value) in chain {
                    let bucket = self.bucket(key);
                    match self.find(bucket, key) {
                        Ok(index) => self.chains[bucket][index] = (key, value),
                        Err(index) => {
                            self.chains[bucket].insert(index, (key, value));
                            self.count += 1;
                        }
                    }
                }
            }
        }
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn insert(&mut self, key: Px, value: u32) {
        let bucket = self.bucket(key);
        match self.find(bucket, key) {
            Ok(index) => self.chains[bucket][index].1 = value,
            Err(index) => {
                self.chains[bucket].insert(index, (key, value));
                self.count += 1;
                self.resize();
            }
        }
    }

    /// Counts one more of `key`.
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn count_pixel(&mut self, key: Px) {
        let bucket = self.bucket(key);
        match self.find(bucket, key) {
            Ok(index) => self.chains[bucket][index].1 += 1,
            Err(index) => {
                self.chains[bucket].insert(index, (key, 1));
                self.count += 1;
                self.resize();
            }
        }
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn lookup(&self, key: Px) -> Option<u32> {
        let bucket = self.bucket(key);
        self.find(bucket, key).ok().map(|index| self.chains[bucket][index].1)
    }
}

// ---------------------------------------------------------------------------
// Median cut
// ---------------------------------------------------------------------------

const NIL: usize = usize::MAX;

struct PixelList {
    next: [usize; 3],
    prev: [usize; 3],
    p: Px,
    flag: bool,
    count: u32,
}

struct BoxNode {
    l: usize,
    r: usize,
    head: [usize; 3],
    tail: [usize; 3],
    volume: i32,
    pixel_count: u32,
}

#[allow(
    clippy::if_not_else,
    clippy::many_single_char_names,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn mergesort_pixels(list: &mut [PixelList], head: usize, i: usize) -> usize {
    if head == NIL || list[head].next[i] == NIL {
        if head != NIL {
            list[head].next[i] = NIL;
            list[head].prev[i] = NIL;
        }
        return head;
    }
    let (mut c, mut t) = (head, head);
    while c != NIL && t != NIL {
        c = list[c].next[i];
        t = if list[t].next[i] != NIL { list[list[t].next[i]].next[i] } else { NIL };
    }
    if c != NIL {
        let before = list[c].prev[i];
        if before != NIL {
            list[before].next[i] = NIL;
        }
        list[c].prev[i] = NIL;
    }
    let mut a = mergesort_pixels(list, head, i);
    let mut b = mergesort_pixels(list, c, i);
    let mut head = NIL;
    let mut p = NIL;
    while a != NIL && b != NIL {
        if list[a].p.channel(i) > list[b].p.channel(i) {
            c = a;
            a = list[a].next[i];
        } else {
            c = b;
            b = list[b].next[i];
        }
        list[c].prev[i] = p;
        list[c].next[i] = NIL;
        if p != NIL {
            list[p].next[i] = c;
        }
        p = c;
        if head == NIL {
            head = c;
        }
    }
    if a != NIL {
        list[c].next[i] = a;
        list[a].prev[i] = c;
    } else if b != NIL {
        list[c].next[i] = b;
        list[b].prev[i] = c;
    }
    head
}

/// A binary max-heap of boxes by pixel count, from index 1.
struct Heap {
    heap: Vec<usize>,
}

#[allow(
    clippy::cast_possible_wrap,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn box_cmp(boxes: &[BoxNode], a: usize, b: usize) -> i32 {
    (boxes[a].pixel_count as i32).wrapping_sub(boxes[b].pixel_count as i32)
}

impl Heap {
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn add(&mut self, boxes: &[BoxNode], value: usize) {
        self.heap.push(0);
        let mut k = self.heap.len() - 1;
        while k != 1 {
            if box_cmp(boxes, value, self.heap[k / 2]) <= 0 {
                break;
            }
            self.heap[k] = self.heap[k / 2];
            k >>= 1;
        }
        self.heap[k] = value;
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn remove(&mut self, boxes: &[BoxNode]) -> Option<usize> {
        let count = self.heap.len() - 1;
        if count == 0 {
            return None;
        }
        let result = self.heap[1];
        let value = self.heap[count];
        let count = count - 1;
        let mut k = 1;
        while k * 2 <= count {
            let mut l = k * 2;
            if l < count && box_cmp(boxes, self.heap[l], self.heap[l + 1]) < 0 {
                l += 1;
            }
            if box_cmp(boxes, value, self.heap[l]) > 0 {
                break;
            }
            self.heap[k] = self.heap[l];
            k = l;
        }
        self.heap[k] = value;
        self.heap.truncate(count + 1);
        Some(result)
    }
}

#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn compute_box_volume(boxes: &mut [BoxNode], list: &[PixelList], node: usize) -> i32 {
    if boxes[node].volume >= 0 {
        return boxes[node].volume;
    }
    let (head, tail) = (boxes[node].head, boxes[node].tail);
    boxes[node].volume = if head[0] == NIL {
        0
    } else {
        let range =
            |axis: usize| i32::from(list[head[axis]].p.channel(axis)) - i32::from(list[tail[axis]].p.channel(axis)) + 1;
        range(0) * range(1) * range(2)
    };
    boxes[node].volume
}

#[allow(
    clippy::many_single_char_names,
    clippy::if_not_else,
    clippy::too_many_lines,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn split(boxes: &mut Vec<BoxNode>, list: &mut [PixelList], node: usize) {
    let (head, tail, pixel_count) = (boxes[node].head, boxes[node].tail, boxes[node].pixel_count);
    let range = |axis: usize| i32::from(list[head[axis]].p.channel(axis)) - i32::from(list[tail[axis]].p.channel(axis));
    let f = [range(0) * 77, range(1) * 150, range(2) * 29];
    let (mut best, mut axis) = (f[0], 0);
    for (i, &value) in f.iter().enumerate().skip(1) {
        if best < value {
            best = value;
            axis = i;
        }
    }

    let (mut left, mut count0, mut count1, mut right_items) = (0u32, 0u32, 0u32, 0u32);
    let mut c = head[axis];
    while c != NIL {
        left = left.wrapping_add(list[c].count);
        count0 = count0.wrapping_add(list[c].count);
        list[c].flag = false;
        c = list[c].next[axis];
        if left.wrapping_mul(2) > pixel_count {
            break;
        }
    }
    if c != NIL {
        let split_value = list[list[c].prev[axis]].p.channel(axis);
        while c != NIL {
            if split_value != list[c].p.channel(axis) {
                break;
            }
            list[c].flag = false;
            count0 = count0.wrapping_add(list[c].count);
            c = list[c].next[axis];
        }
    }
    while c != NIL {
        list[c].flag = true;
        right_items += 1;
        count1 = count1.wrapping_add(list[c].count);
        c = list[c].next[axis];
    }
    if right_items == 0 {
        c = tail[axis];
        let split_value = list[c].p.channel(axis);
        while c != NIL {
            if split_value != list[c].p.channel(axis) {
                break;
            }
            list[c].flag = true;
            count0 = count0.wrapping_sub(list[c].count);
            count1 = count1.wrapping_add(list[c].count);
            c = list[c].prev[axis];
        }
    }

    let mut new_head = [[NIL; 3]; 2];
    let mut new_tail = [[NIL; 3]; 2];
    for i in 0..3 {
        let (mut l, mut r) = (NIL, NIL);
        let mut c = head[i];
        while c != NIL {
            let n = list[c].next[i];
            if list[c].flag {
                if r != NIL {
                    list[r].next[i] = c;
                } else {
                    new_head[1][i] = c;
                }
                list[c].prev[i] = r;
                r = c;
            } else {
                if l != NIL {
                    list[l].next[i] = c;
                } else {
                    new_head[0][i] = c;
                }
                list[c].prev[i] = l;
                l = c;
            }
            c = n;
        }
        if l != NIL {
            list[l].next[i] = NIL;
        }
        if r != NIL {
            list[r].next[i] = NIL;
        }
        new_tail[0][i] = l;
        new_tail[1][i] = r;
    }

    let left_box = boxes.len();
    boxes.push(BoxNode { l: NIL, r: NIL, head: new_head[0], tail: new_tail[0], volume: -1, pixel_count: count0 });
    boxes.push(BoxNode { l: NIL, r: NIL, head: new_head[1], tail: new_tail[1], volume: -1, pixel_count: count1 });
    boxes[node].l = left_box;
    boxes[node].r = left_box + 1;
    boxes[node].head = [NIL; 3];
    boxes[node].tail = [NIL; 3];
    // A split node's lists are emptied; only leaves are walked.
}

#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn annotate(boxes: &[BoxNode], list: &[PixelList], node: usize, table: &mut Table, next_box: &mut u32) {
    let n = &boxes[node];
    if n.l != NIL && n.r != NIL {
        annotate(boxes, list, n.l, table, next_box);
        annotate(boxes, list, n.r, table, next_box);
        return;
    }
    let mut p = n.head[0];
    while p != NIL {
        table.insert(list[p].p, *next_box);
        p = list[p].next[0];
    }
    if n.head[0] != NIL {
        *next_box += 1;
    }
}

#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn mean_channel(total: u32, count: u32) -> u8 {
    (0.5 + f64::from(total) / f64::from(count)) as i32 as u8
}

/// The distances between palette entries, and for each entry the others
/// by distance, then index. The comparison is a total order, so any sort agrees.
struct Distances {
    n: usize,
    distance: Vec<u32>,
    order: Vec<u32>,
}

#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn build_distances(palette: &[Px]) -> Distances {
    let n = palette.len();
    let mut distance = vec![0u32; n * n];
    for i in 0..n {
        for j in 0..i {
            let d = dist_sqr(palette[i], palette[j]);
            distance[j * n + i] = d;
            distance[i * n + j] = d;
        }
    }
    let mut order = vec![0u32; n * n];
    for i in 0..n {
        let mut row: Vec<u32> = (0..n as u32).collect();
        row.sort_by_key(|&j| (distance[i * n + j as usize], j));
        order[i * n..(i + 1) * n].copy_from_slice(&row);
    }
    Distances { n, distance, order }
}

/// The palette entry nearest `pixel`, searched outwards from `start`.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn nearest(tables: &Distances, palette: &[Px], pixel: Px, start: u32) -> u32 {
    let mut best_match = start;
    let first = dist_sqr(palette[start as usize], pixel);
    let mut best = first;
    let limit = first << 2;
    let row = start as usize * tables.n;
    for j in 0..tables.n {
        let index = tables.order[row + j];
        if tables.distance[row + index as usize] <= limit {
            let d = dist_sqr(palette[index as usize], pixel);
            if d < best {
                best = d;
                best_match = index;
            }
        } else {
            break;
        }
    }
    best_match
}

#[allow(
    clippy::too_many_lines,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
pub fn median_cut(pixels: &[Px], colors: u32) -> Vec<Px> {
    let mut table = Table::new(Compare::ByHash);
    for &pixel in pixels {
        // The original rescales its hash above 65,536 entries, which a
        // page's 6,000 sixels never reach.
        table.count_pixel(pixel);
    }

    // Each new node goes to the head of all three lists.
    let mut list: Vec<PixelList> = Vec::new();
    let mut heads = [NIL; 3];
    for chain in &table.chains {
        for &(pixel, count) in chain {
            let index = list.len();
            list.push(PixelList { next: heads, prev: [NIL; 3], p: pixel, flag: false, count });
            for i in 0..3 {
                if heads[i] != NIL {
                    list[heads[i]].prev[i] = index;
                }
                heads[i] = index;
            }
        }
    }
    for (i, head) in heads.iter_mut().enumerate() {
        *head = mergesort_pixels(&mut list, *head, i);
    }

    let mut tails = [NIL; 3];
    for i in 0..3 {
        let mut t = heads[i];
        while t != NIL && list[t].next[i] != NIL {
            t = list[t].next[i];
        }
        tails[i] = t;
    }
    let mut boxes =
        vec![BoxNode { l: NIL, r: NIL, head: heads, tail: tails, volume: -1, pixel_count: pixels.len() as u32 }];
    let mut heap = Heap { heap: vec![0] };
    heap.add(&boxes, 0);
    let mut remaining = colors;
    'cut: loop {
        remaining -= 1;
        if remaining == 0 {
            break;
        }
        let node = loop {
            let Some(node) = heap.remove(&boxes) else { break 'cut };
            if compute_box_volume(&mut boxes, &list, node) != 1 {
                break node;
            }
        };
        split(&mut boxes, &mut list, node);
        let (l, r) = (boxes[node].l, boxes[node].r);
        heap.add(&boxes, l);
        heap.add(&boxes, r);
    }

    let mut entries = 0u32;
    annotate(&boxes, &list, 0, &mut table, &mut entries);

    let n = entries as usize;
    let mut total = vec![[0u32; 3]; n];
    let mut count = vec![0u32; n];
    for &pixel in pixels {
        let entry = table.lookup(pixel).unwrap_or(0) as usize;
        if entry >= n {
            continue;
        }
        total[entry][0] += u32::from(pixel.r);
        total[entry][1] += u32::from(pixel.g);
        total[entry][2] += u32::from(pixel.b);
        count[entry] += 1;
    }
    let palette: Vec<Px> = (0..n)
        .map(|i| Px {
            r: mean_channel(total[i][0], count[i]),
            g: mean_channel(total[i][1], count[i]),
            b: mean_channel(total[i][2], count[i]),
        })
        .collect();

    let tables = build_distances(&palette);
    let mut cache: HashMap<Px, u32> = HashMap::new();
    pixels
        .iter()
        .map(|&pixel| {
            let entry = *cache.entry(pixel).or_insert_with(|| {
                let start = table.lookup(pixel).unwrap_or(0);
                nearest(&tables, &palette, pixel, start)
            });
            palette[entry as usize]
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Maximum coverage
// ---------------------------------------------------------------------------

#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
pub fn max_coverage(pixels: &[Px], colors: u32) -> Vec<Px> {
    let mut palette = vec![Px::default(); colors as usize];
    let mut mean = [0u32; 3];
    let mut table = Table::new(Compare::Unshifted);
    for &pixel in pixels {
        table.insert(pixel, 0xffff_ffff);
        mean[0] += u32::from(pixel.r);
        mean[1] += u32::from(pixel.g);
        mean[2] += u32::from(pixel.b);
    }
    let total = pixels.len() as u32;
    let mut new =
        Px { r: mean_channel(mean[0], total), g: mean_channel(mean[1], total), b: mean_channel(mean[2], total) };
    for (i, entry) in palette.iter_mut().enumerate() {
        let mut furthest_distance = 0u32;
        let mut furthest = pixels[0];
        let second = i == 1;
        for chain in &mut table.chains {
            for node in chain.iter_mut() {
                let mut old = node.1;
                let distance = dist_sqr(new, node.0);
                if second || distance < old {
                    node.1 = distance;
                    old = distance;
                }
                if old > furthest_distance {
                    furthest_distance = old;
                    furthest = node.0;
                }
            }
        }
        *entry = furthest;
        new = furthest;
    }

    let tables = build_distances(&palette);
    let mut cache: HashMap<Px, u32> = HashMap::new();
    pixels
        .iter()
        .map(|&pixel| {
            let entry = *cache.entry(pixel).or_insert_with(|| nearest(&tables, &palette, pixel, 0));
            palette[entry as usize]
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Fast octree
// ---------------------------------------------------------------------------

/// A bucket of the octree: its count and channel totals.
#[derive(Clone, Copy, Default)]
struct Bucket {
    count: u32,
    r: u64,
    g: u64,
    b: u64,
    a: u64,
}

struct Cube {
    bits: [u32; 3],
    offset: [u32; 3],
    width: [u32; 3],
    buckets: Vec<Bucket>,
}

impl Cube {
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn new(r: u32, g: u32, b: u32) -> Self {
        let size = 1usize << (r + g + b);
        Self {
            bits: [r, g, b],
            offset: [g + b, b, 0],
            width: [1 << r, 1 << g, 1 << b],
            buckets: vec![Bucket::default(); size],
        }
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn position(&self, r: u32, g: u32, b: u32) -> usize {
        (r << self.offset[0] | g << self.offset[1] | b << self.offset[2]) as usize
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn offset_of(&self, p: Px) -> usize {
        self.position(
            u32::from(p.r) >> (8 - self.bits[0]),
            u32::from(p.g) >> (8 - self.bits[1]),
            u32::from(p.b) >> (8 - self.bits[2]),
        )
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn used(&self) -> usize {
        self.buckets.iter().filter(|bucket| bucket.count > 0).count()
    }

    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "indices within the page's sixels and a palette of at most 32 colours"
    )]
    fn copied(&self, r: u32, g: u32, b: u32) -> Cube {
        let mut result = Cube::new(r, g, b);
        let mut source_reduce = [0u32; 3];
        let mut target_reduce = [0u32; 3];
        let mut width = [0u32; 3];
        for axis in 0..3 {
            if self.bits[axis] > result.bits[axis] {
                target_reduce[axis] = self.bits[axis] - result.bits[axis];
                width[axis] = self.width[axis];
            } else {
                source_reduce[axis] = result.bits[axis] - self.bits[axis];
                width[axis] = result.width[axis];
            }
        }
        for x in 0..width[0] {
            for y in 0..width[1] {
                for z in 0..width[2] {
                    let from = self.position(x >> source_reduce[0], y >> source_reduce[1], z >> source_reduce[2]);
                    let to = result.position(x >> target_reduce[0], y >> target_reduce[1], z >> target_reduce[2]);
                    let source = self.buckets[from];
                    let target = &mut result.buckets[to];
                    target.count = target.count.wrapping_add(source.count);
                    target.r = target.r.wrapping_add(source.r);
                    target.g = target.g.wrapping_add(source.g);
                    target.b = target.b.wrapping_add(source.b);
                    target.a = target.a.wrapping_add(source.a);
                }
            }
        }
        result
    }
}

/// A bucket's mean colour, in single precision and truncated, as the original.
#[allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::if_not_else,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn average(bucket: &Bucket) -> Px {
    let count = bucket.count as f32;
    if count != 0.0 {
        let channel = |total: u64| ((total as f32 / count) as i32).clamp(0, 255) as u8;
        Px { r: channel(bucket.r), g: channel(bucket.g), b: channel(bucket.b) }
    } else {
        Px::default()
    }
}

#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn subtract(cube: &mut Cube, buckets: &[Bucket]) {
    for subtrahend in buckets {
        if subtrahend.count == 0 {
            continue;
        }
        let at = cube.offset_of(average(subtrahend));
        let minuend = &mut cube.buckets[at];
        minuend.count = minuend.count.wrapping_sub(subtrahend.count);
        minuend.r = minuend.r.wrapping_sub(subtrahend.r);
        minuend.g = minuend.g.wrapping_sub(subtrahend.g);
        minuend.b = minuend.b.wrapping_sub(subtrahend.b);
        minuend.a = minuend.a.wrapping_sub(subtrahend.a);
    }
}

#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn add_lookup(cube: &mut Cube, palette: &[Bucket], count: usize, offset: usize) {
    for i in (offset..offset + count).rev() {
        let at = cube.offset_of(average(&palette[i]));
        cube.buckets[at].count = i as u32;
    }
}

/// By count, descending, and among equal counts in bucket order: a stable
/// sort gives the same palette on every machine (DEC-2142 item 3).
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
fn sorted(buckets: &[Bucket]) -> Vec<Bucket> {
    let mut out = buckets.to_vec();
    out.sort_by_key(|bucket| std::cmp::Reverse(bucket.count));
    out
}

#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "indices within the page's sixels and a palette of at most 32 colours"
)]
pub fn octree(pixels: &[Px], colors: u32) -> Vec<Px> {
    const FINE: [u32; 3] = [4, 4, 4];
    const COARSE: [u32; 3] = [2, 2, 2];
    let wanted = colors as usize;

    let mut fine_cube = Cube::new(FINE[0], FINE[1], FINE[2]);
    for &pixel in pixels {
        let at = fine_cube.offset_of(pixel);
        let bucket = &mut fine_cube.buckets[at];
        bucket.count += 1;
        bucket.r += u64::from(pixel.r);
        bucket.g += u64::from(pixel.g);
        bucket.b += u64::from(pixel.b);
        bucket.a += 255;
    }

    let mut coarse_cube = fine_cube.copied(COARSE[0], COARSE[1], COARSE[2]);
    let mut coarse_colors = coarse_cube.used().min(wanted);
    let mut fine_colors = wanted - coarse_colors;
    let fine_palette = sorted(&fine_cube.buckets);
    subtract(&mut coarse_cube, &fine_palette[..fine_colors]);
    while coarse_colors > coarse_cube.used() {
        let already = fine_colors;
        coarse_colors = coarse_cube.used();
        fine_colors = wanted - coarse_colors;
        subtract(&mut coarse_cube, &fine_palette[already..fine_colors]);
    }

    let coarse_palette = sorted(&coarse_cube.buckets);
    let mut buckets = coarse_palette[..coarse_colors].to_vec();
    buckets.extend_from_slice(&fine_palette[..fine_colors]);

    let mut coarse_lookup = Cube::new(COARSE[0], COARSE[1], COARSE[2]);
    add_lookup(&mut coarse_lookup, &buckets, coarse_colors, 0);
    let mut lookup = coarse_lookup.copied(FINE[0], FINE[1], FINE[2]);
    add_lookup(&mut lookup, &buckets, fine_colors, coarse_colors);

    let palette: Vec<Px> = buckets.iter().map(average).collect();
    pixels.iter().map(|&pixel| palette[lookup.buckets[lookup.offset_of(pixel)].count as usize]).collect()
}
