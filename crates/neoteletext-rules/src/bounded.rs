//! A list in a fixed array (RULE-56: no allocation): every list of a
//! decoded table is one, sized by its limit of RULE-57.

/// At most `N` items of `T`, in order. It reads as a slice of the items it
/// holds. `N` is at most 65,535.
#[derive(Clone, Copy)]
pub struct Bounded<T: Copy, const N: usize> {
    items: [T; N],
    /// How many are held: a 16-bit count keeps a short list small.
    count: u16,
}

impl<T: Copy, const N: usize> Bounded<T, N> {
    // The count fits its field.
    const FITS: () = assert!(N <= u16::MAX as usize);

    /// An empty list; `fill` is what its unused places hold at first.
    #[must_use]
    pub const fn new(fill: T) -> Self {
        let () = Self::FITS;
        Self { items: [fill; N], count: 0 }
    }

    /// Adds an item at the end, and gives its place.
    ///
    /// # Errors
    /// The item back, when the list already holds `N`.
    pub fn push(&mut self, item: T) -> Result<usize, T> {
        let place = usize::from(self.count);
        let Some(slot) = self.items.get_mut(place) else { return Err(item) };
        *slot = item;
        self.count = self.count.saturating_add(1);
        Ok(place)
    }

    /// Takes the last item away, and gives it.
    pub fn pop(&mut self) -> Option<T> {
        self.count = self.count.checked_sub(1)?;
        self.items.get(usize::from(self.count)).copied()
    }

    /// Forgets every item.
    pub fn clear(&mut self) {
        self.count = 0;
    }

    /// The items held, in order.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        self.items.get(..usize::from(self.count)).unwrap_or(&[])
    }

    /// The items held, in order, to change in place.
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.items.get_mut(..usize::from(self.count)).unwrap_or(&mut [])
    }
}

impl<T: Copy, const N: usize> core::ops::Deref for Bounded<T, N> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: Copy + PartialEq, const N: usize> PartialEq for Bounded<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Copy + Eq, const N: usize> Eq for Bounded<T, N> {}

impl<T: Copy + core::fmt::Debug, const N: usize> core::fmt::Debug for Bounded<T, N> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_list().entries(self.as_slice()).finish()
    }
}
