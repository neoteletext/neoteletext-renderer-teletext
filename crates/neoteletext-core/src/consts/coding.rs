//! Byte coding (clause 8): parity, Hamming 8/4 nibbles and Hamming 24/18
//! triplets.

/// Mask of the seven data bits of an odd-parity byte (clause 8.1).
pub const CODE_MASK_7BIT: u8 = 0x7F;
/// The parity bit of an odd-parity byte.
pub const PARITY_BIT: u8 = 0x80;
/// Mask of a 4-bit Hamming 8/4 value.
pub const NIBBLE_MASK: u8 = 0x0F;
/// Bits in a Hamming 24/18 triplet.
pub const TRIPLET_BITS: u32 = 24;
/// The shift of P6, a triplet's last bit (bit 24), which makes test F odd
/// (clause 8.3).
pub const TRIPLET_P6_SHIFT: u32 = TRIPLET_BITS - 1;
/// Data bits in a Hamming 24/18 triplet.
pub const TRIPLET_DATA_BITS: u32 = 18;
/// Mask of the 18 data bits of a triplet.
pub const TRIPLET_DATA_MASK: u32 = (1 << TRIPLET_DATA_BITS) - 1;
/// Bit shifts of data bits D1 to D18 within a Hamming 24/18 word, where
/// position B1 is shift 0 (clause 8.3): every position that is not a power
/// of two and not B24.
pub const TRIPLET_DATA_SHIFTS: [u32; TRIPLET_DATA_BITS as usize] =
    [2, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 16, 17, 18, 19, 20, 21, 22];
/// The tests A to E of a Hamming 24/18 word (clause 8.3), by the bit of a
/// position's one-based index each covers; protection bits P1 to P5 sit at
/// positions 1, 2, 4, 8 and 16 and make them odd. P6, at B24, makes test F
/// odd.
pub const TRIPLET_TEST_BITS: [u32; 5] = [0, 1, 2, 3, 4];
/// Bytes in a triplet.
pub const TRIPLET_BYTES: usize = 3;
/// Triplets in packets X/26, X/28 and M/29 (clause 9.4.1).
pub const TRIPLETS_PER_PACKET: usize = 13;
/// Designation codes 0 to 15.
pub const DESIGNATION_CODES: usize = 16;
