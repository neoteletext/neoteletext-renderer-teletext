//! Where the fields of the packets with fixed layouts lie, as indices into a
//! packet's 42 bytes ([`super::PACKET_BYTES`]): index n is packet byte n + 4
//! of clause 7.1, after the clock run-in and framing code that a slicer
//! removes. Each value is read from the standard's figure or table named.

/// The page units digit of a page header, packet byte 6 (clause 9.3.1,
/// Figure 9).
pub const HEADER_UNITS: usize = 2;
/// The page tens digit, packet byte 7.
pub const HEADER_TENS: usize = 3;
/// Sub-code S1, packet byte 8.
pub const HEADER_S1: usize = 4;
/// Sub-code S2 and control bit C4, packet byte 9.
pub const HEADER_S2_C4: usize = 5;
/// Sub-code S3, packet byte 10.
pub const HEADER_S3: usize = 6;
/// Sub-code S4 and control bits C5 and C6, packet byte 11.
pub const HEADER_S4_C5_C6: usize = 7;
/// Control bits C7 to C10, packet byte 12.
pub const HEADER_C7_C10: usize = 8;
/// Control bits C11 to C14, packet byte 13.
pub const HEADER_C11_C14: usize = 9;

/// Bytes of a page link: page units and tens, then four bytes of sub-code
/// and magazine bits (clause 9.6.1, Figure 12); the same layout gives the
/// initial page of packet 8/30 (clause 9.8.1, Table 18).
pub const PAGE_LINK_BYTES: usize = 6;
/// The first link of packet X/27/0, packet byte 7 (clause 9.6.1, Figure 12).
pub const EDITORIAL_LINK_START: usize = 3;
/// Links of an editorial link packet X/27/0 to X/27/3 (Figure 12).
pub const EDITORIAL_LINKS: usize = 6;
/// The link of X/27/0 for the Index key: links 0 to 3 are the coloured
/// keys, link 4 is not a viewer link, link 5 is Index (Annex H).
pub const EDITORIAL_INDEX_LINK: usize = 5;
/// The link control byte of X/27/0, packet byte 43 (clause 9.6.1).
pub const EDITORIAL_LINK_CONTROL: usize = 39;
/// Data bit 4 of the link control byte: display row 24 (Table 14).
pub const EDITORIAL_LINK_CONTROL_ROW_24: u8 = 0x8;
/// The first checksum byte of X/27/0, packet byte 44 (clause 9.6.1).
pub const EDITORIAL_LINK_CHECKSUM: usize = 40;
/// The highest designation code of an editorial link packet (clause 9.6.1).
pub const EDITORIAL_LAST_DESIGNATION: u8 = 3;

/// The initial page of packet 8/30, packet bytes 7 to 12 (clause 9.8.1,
/// Table 18).
pub const SERVICE_INITIAL_PAGE: usize = 3;
/// The network identification of format 1, packet bytes 13 and 14.
pub const SERVICE_NETWORK_ID: usize = 9;
/// The time offset of format 1, packet byte 15.
pub const SERVICE_TIME_OFFSET: usize = 11;
/// The date and time of format 1, packet bytes 16 to 21.
pub const SERVICE_DATE_TIME: usize = 12;
/// The Programme Delivery Control data of format 2, packet bytes 13 to 25.
pub const SERVICE_PDC: usize = 9;
/// The status display, packet bytes 26 to 45.
pub const SERVICE_STATUS: usize = 22;
/// Bit 7 of the time offset byte, numbering from 1: the offset is negative
/// (Table 18).
pub const SERVICE_TIME_OFFSET_NEGATIVE: u8 = 0x40;
/// Bits 2 to 6 of the time offset byte, after shifting out bit 1: the
/// offset in half-hours (Table 18).
pub const SERVICE_TIME_OFFSET_HALF_HOURS_MASK: u8 = 0x1F;

/// The packet byte number of a packet's first data byte, byte 6 (clause
/// 7.1): the tables of the Magazine Organization Table and others give
/// positions as packet byte numbers.
pub const DATA_FIRST_BYTE_NUMBER: usize = super::PACKET_FIRST_BYTE_NUMBER + super::DATA_START;
