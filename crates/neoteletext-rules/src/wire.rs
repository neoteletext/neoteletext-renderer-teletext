//! Big-endian fields over caller-provided bytes (page-rules.md section 8:
//! "every integer on a wire is big-endian"), read and written without a
//! panic path.

use crate::Fault;

/// Reads fields from bytes; a field cut short is [`Fault::Short`].
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    used: usize,
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, used: 0 }
    }

    /// The bytes read so far.
    pub(crate) const fn used(&self) -> usize {
        self.used
    }

    /// Whether every byte was read.
    pub(crate) const fn finished(&self) -> bool {
        self.used >= self.bytes.len()
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Fault> {
        let end = self.used.checked_add(N).ok_or(Fault::Short)?;
        let taken = self.bytes.get(self.used..end).ok_or(Fault::Short)?;
        self.used = end;
        taken.try_into().map_err(|_| Fault::Short)
    }

    /// The next `length` bytes, as they are.
    pub(crate) fn take(&mut self, length: usize) -> Result<&'a [u8], Fault> {
        let end = self.used.checked_add(length).ok_or(Fault::Short)?;
        let taken = self.bytes.get(self.used..end).ok_or(Fault::Short)?;
        self.used = end;
        Ok(taken)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, Fault> {
        self.array::<1>().map(u8::from_be_bytes)
    }

    pub(crate) fn i8(&mut self) -> Result<i8, Fault> {
        self.array::<1>().map(i8::from_be_bytes)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, Fault> {
        self.array::<2>().map(u16::from_be_bytes)
    }

    pub(crate) fn i16(&mut self) -> Result<i16, Fault> {
        self.array::<2>().map(i16::from_be_bytes)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, Fault> {
        self.array::<4>().map(u32::from_be_bytes)
    }

    pub(crate) fn i32(&mut self) -> Result<i32, Fault> {
        self.array::<4>().map(i32::from_be_bytes)
    }
}

/// Writes fields into a caller's buffer; no room is [`Fault::Full`].
pub(crate) struct Writer<'a> {
    out: &'a mut [u8],
    used: usize,
}

impl<'a> Writer<'a> {
    pub(crate) const fn new(out: &'a mut [u8]) -> Self {
        Self { out, used: 0 }
    }

    /// The bytes written.
    pub(crate) const fn used(&self) -> usize {
        self.used
    }

    pub(crate) fn bytes(&mut self, bytes: &[u8]) -> Result<(), Fault> {
        let end = self.used.checked_add(bytes.len()).ok_or(Fault::Full)?;
        let room = self.out.get_mut(self.used..end).ok_or(Fault::Full)?;
        // Byte by byte: `copy_from_slice` links a panic path for unequal
        // lengths (REQ-GEN-019), though these are equal.
        for (to, &from) in room.iter_mut().zip(bytes) {
            *to = from;
        }
        self.used = end;
        Ok(())
    }

    pub(crate) fn u8(&mut self, value: u8) -> Result<(), Fault> {
        self.bytes(&[value])
    }

    pub(crate) fn i8(&mut self, value: i8) -> Result<(), Fault> {
        self.bytes(&value.to_be_bytes())
    }

    pub(crate) fn u16(&mut self, value: u16) -> Result<(), Fault> {
        self.bytes(&value.to_be_bytes())
    }

    pub(crate) fn i16(&mut self, value: i16) -> Result<(), Fault> {
        self.bytes(&value.to_be_bytes())
    }

    pub(crate) fn u32(&mut self, value: u32) -> Result<(), Fault> {
        self.bytes(&value.to_be_bytes())
    }

    pub(crate) fn i32(&mut self, value: i32) -> Result<(), Fault> {
        self.bytes(&value.to_be_bytes())
    }
}
