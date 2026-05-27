use crate::arch::mmu::PhysicalAddress;
use bitflags::bitflags;

bitflags! {
    #[derive(Copy, Clone)]
    pub struct PteFlags: usize {
        const V = 1 << 0;
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
        const G = 1 << 5;
        const A = 1 << 6;
        const D = 1 << 7;
        /// Readable + Writable
        const RW = (1 << 2) | (1 << 1);
        /// Readable + Executable
        const RX = (1 << 3) | (1 << 1);
        /// Readable + Executable
        const RWX = (1 << 1) | (1 << 2) | (1 << 3);
    }
}

/// Sv39 Page Table Entry (PTE) Layout
///
/// A Sv39 page table entry is 64 bits wide and structured as follows:
///
///  63  62  61 60     54 53      28 27      19 18      10 9   8  7   6   5   4   3   2   1   0
/// +---+------+---------+----------+----------+----------+-----+---+---+---+---+---+---+---+---+
/// | N | PBMT | Reserved|  PPN[2]  |  PPN[1]  |  PPN[0]  | RSW | D | A | G | U | X | W | R | V |
/// +---+------+---------+----------+----------+----------+-----+---+---+---+---+---+---+---+---+
///   1    2        7         26         9          9        2    1   1   1   1   1   1   1   1
///
/// https://docs.riscv.org/reference/isa/priv/supervisor.html#addressing-and-memory-protection
#[rustfmt::skip]
#[derive(Copy, Clone)]
pub struct PageTableEntry(usize);

impl PageTableEntry {
    // const MASK_PPN_0: usize = 0x1ff << Self::OFFSET_PPN_0;
    // const MASK_PPN_1: usize = 0x1ff << Self::OFFSET_PPN_1;
    // const MASK_PPN_2: usize = 0x3ffffff << Self::OFFSET_PPN_2;

    const MASK_PPN: usize = ((1usize << 44) - 1) << Self::OFFSET_PPN_0;

    const OFFSET_PPN_0: usize = 10;
    // const OFFSET_PPN_1: usize = 19;
    // const OFFSET_PPN_2: usize = 28;

    /// Returns a non-leaf valid PTE.
    #[must_use]
    pub fn new_valid() -> Self {
        Self::empty().set_flags(PteFlags::V)
    }

    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn physical_address(&self) -> PhysicalAddress {
        unsafe { PhysicalAddress::from_raw_unchecked((self.0 & Self::MASK_PPN) << 2) }
    }

    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.0 & PteFlags::V.bits() == 1
    }

    #[must_use]
    pub fn set_flags(mut self, flag: PteFlags) -> Self {
        self.0 |= flag.bits();
        self
    }

    /// Sets the PPN\[2\], PPN\[1\] and PPN\[0\] to the bits[55:12] of the given
    /// `addr`
    #[must_use]
    pub fn set_physical_address(mut self, addr: PhysicalAddress) -> Self {
        debug_assert!(
            addr.is_4k_page_aligned(),
            "Physical address must be 4K-aligned"
        );
        let offset_stripped = addr.raw() >> 12;
        self.0 =
            (!Self::MASK_PPN & self.0) | ((offset_stripped << Self::OFFSET_PPN_0) & Self::MASK_PPN);
        self
    }
}
