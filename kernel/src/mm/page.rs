use core::sync::atomic::{AtomicU16, Ordering};

use alloc::collections::btree_map::{BTreeMap, Entry};
use ksync::RwLock;

use crate::{error::Error, mm::PhysAddr};

static PAGE_METADATA_TABLE: RwLock<PageMetadataTable> = RwLock::new(PageMetadataTable {
    map: BTreeMap::new(),
});

struct PageMetadataTable {
    map: BTreeMap<PhysAddr, PageMetadata>,
}

#[repr(C)]
struct PageMetadata {
    refcount: AtomicU16,
}

pub fn add(addr: PhysAddr) {
    let mut table = PAGE_METADATA_TABLE.write_lock();
    match table.map.entry(addr) {
        Entry::Vacant(v) => {
            v.insert(PageMetadata {
                refcount: AtomicU16::new(1),
            });
        }
        Entry::Occupied(o) => {
            let _ = o.get().refcount.fetch_add(1, Ordering::Acquire);
        }
    }
}

pub fn remove(addr: PhysAddr) -> Result<u16, Error> {
    let mut table = PAGE_METADATA_TABLE.write_lock();
    let Entry::Occupied(entry) = table.map.entry(addr) else {
        return Err(Error::NotFound);
    };

    let new_refcount = entry.get().refcount.fetch_sub(1, Ordering::AcqRel) - 1;
    if new_refcount == 0 {
        entry.remove();
    }

    Ok(new_refcount)
}
