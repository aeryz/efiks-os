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

pub fn add(addr: PhysAddr) -> Result<(), Error> {
    let mut table = PAGE_METADATA_TABLE.write_lock();
    match table.map.entry(addr) {
        Entry::Vacant(v) => {
            v.insert(PageMetadata {
                refcount: AtomicU16::new(1),
            });
            Ok(())
        }
        Entry::Occupied(_) => Err(Error::AlreadyExists),
    }
}

pub fn remove(addr: PhysAddr) -> Result<u16, Error> {
    let rlock = PAGE_METADATA_TABLE.read_lock();

    let entry = rlock.map.get(&addr).ok_or(Error::NotFound)?;

    let new_val = entry.refcount.fetch_sub(1, Ordering::Acquire) - 1;
    if new_val == 0 {
        drop(rlock);
        let entry = PAGE_METADATA_TABLE.write_lock().map.remove(&addr);
        debug_assert!(entry.is_some());
    }

    Ok(new_val)
}
