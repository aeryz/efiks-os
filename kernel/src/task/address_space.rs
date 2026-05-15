use alloc::vec::Vec;

use crate::{
    Arch,
    arch::{PhysicalAddressOf, VirtualAddressOf, mmu::PhysicalAddress},
};

pub const ADDRESS_SPACE_EMPTY: AddressSpace = AddressSpace {
    root_pt: PhysicalAddress::ZERO,
    regions: Vec::new(),
};

#[allow(unused)]
#[derive(Clone)]
pub struct VmRegion {
    /// Start address of this region
    pub start: VirtualAddressOf<Arch>,
    /// End address of this region
    pub end: VirtualAddressOf<Arch>,
}

#[derive(Clone)]
pub struct AddressSpace {
    pub root_pt: PhysicalAddressOf<Arch>,
    pub regions: Vec<VmRegion>,
}
