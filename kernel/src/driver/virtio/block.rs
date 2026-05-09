#![allow(unused)]

use core::{
    alloc::Layout,
    sync::atomic::{self, Ordering},
};

use alloc::{boxed::Box, vec::Vec};
use bitflags::bitflags;
use ksync::{SpinLock, SpinLockGuard};

use crate::{
    driver::virtio::{
        VIRTIO_F_VERSION_1,
        mmio::{self, RegisterOffset},
        virtqueue::{AvailableRing, Descriptor, DescriptorFlag, UsedRing},
    },
    mm,
};

use vfs::{BlockDevice, VfsError, VfsResult};

use super::Status;

const QUEUE_SIZE: usize = 16;

static DRIVER: SpinLock<VirtioBlkDriver> = SpinLock::new(VirtioBlkDriver {
    virtqueue: core::ptr::null_mut(),
    desc_ptr: core::ptr::null_mut(),
    avail_ptr: core::ptr::null_mut(),
    used_ptr: core::ptr::null_mut(),
    device_base: 0,
    last_used_idx: 0,
});

unsafe impl Sync for VirtioBlkDriver {}
unsafe impl Send for VirtioBlkDriver {}

pub struct VirtioBlkDevice;

impl BlockDevice for VirtioBlkDriver {
    fn read_sector(sector: usize, buf: &mut [u8; vfs::SECTOR_SIZE]) -> VfsResult<()> {
        let status = unsafe { read(buf, sector as u64) };

        if status == VirtioBlkStatus::Ok as u8 {
            Ok(())
        } else {
            Err(VfsError::DeviceIO)
        }
    }

    fn write_sector(sector: usize, buf: &[u8; vfs::SECTOR_SIZE]) -> VfsResult<()> {
        let status = unsafe { write(buf, sector as u64) };

        if status == VirtioBlkStatus::Ok as u8 {
            Ok(())
        } else {
            Err(VfsError::DeviceIO)
        }
    }
}

#[repr(C)]
pub struct VirtioBlkDriver {
    virtqueue: *mut u8,
    desc_ptr: *mut Descriptor,
    avail_ptr: *mut AvailableRing<QUEUE_SIZE>,
    used_ptr: *mut UsedRing<QUEUE_SIZE>,
    device_base: usize,
    last_used_idx: u16,
}

const fn align_up(x: usize, align: usize) -> usize {
    (x + align - 1) & !(align - 1)
}

pub fn init(device_base: usize) -> Result<(), ()> {
    mmio::init_device(device_base, 0, || {
        // - Drivers SHOULD NOT negotiate VIRTIO_BLK_F_FLUSH if they are incapable of
        //   sending VIRTIO_BLK_T_FLUSH commands.
        //
        // - If neither VIRTIO_BLK_F_CONFIG_WCE nor VIRTIO_BLK_F_FLUSH are negotiated,
        //   the driver MAY deduce the presence of a writethrough cache. If
        //   VIRTIO_BLK_F_CONFIG_WCE was not negotiated but VIRTIO_BLK_F_FLUSH was, the
        //   driver SHOULD assume presence of a writeback cache.
        //
        // - The driver MUST NOT read writeback before setting the FEATURES_OK device
        //   status bit.
        //
        // - Drivers MUST NOT negotiate the VIRTIO_BLK_F_ZONED feature if they are
        //   incapable of supporting devices with the VIRTIO_BLK_Z_HM, VIRTIO_BLK_Z_HA
        //   or VIRTIO_BLK_Z_NONE zoned model.
        //
        // - If the VIRTIO_BLK_F_ZONED feature is offered by the device with the
        // VIRTIO_BLK_Z_HM zone model, then the VIRTIO_BLK_F_DISCARD feature
        // MUST NOT be offered by the driver.
        //
        // - If the VIRTIO_BLK_F_ZONED feature and VIRTIO_BLK_F_DISCARD feature
        // are both offered by the device with the VIRTIO_BLK_Z_HA or
        // VIRTIO_BLK_Z_NONE zone model, then the driver MAY negotiate these two
        // bits independently.

        // - If the VIRTIO_BLK_F_ZONED feature is negotiated, then
        //
        //     a. if the driver that can not support host-managed zoned devices
        // reads VIRTIO_BLK_Z_HM from the model field of zoned, the driver MUST
        // NOT set FEATURES_OK flag and instead set the FAILED bit.
        //     b. if the driver that can not support zoned devices reads
        // VIRTIO_BLK_Z_HA from the model field of zoned, the driver MAY handle
        // the device as a non-zoned device. In this case, the driver SHOULD
        // ignore all other fields in zoned.

        mmio::write32(device_base, mmio::RegisterOffset::DeviceFeaturesSel, 1u32);
        mmio::write32(
            device_base,
            mmio::RegisterOffset::DriverFeatures,
            VIRTIO_F_VERSION_1,
        );

        // low 32 bits: accept no block features for now
        mmio::write32(device_base, mmio::RegisterOffset::DriverFeaturesSel, 0u32);
        mmio::write32(device_base, mmio::RegisterOffset::DriverFeatures, 0u32);

        Ok(())
    })?;

    // Virtqueue configuration
    // 1. Select the queue by writing its index to QueueSel. Select the queue by
    // writing its index to QueueSel.
    mmio::write32(device_base, mmio::RegisterOffset::QueueSel, 0u32);
    // 2. Check if the queue is not already in use: read QueueReady, and expect a
    // returned value of zero (0x0).
    if mmio::read32(device_base, mmio::RegisterOffset::QueueReady) != 0 {
        return Err(());
    }
    // 3. Read maximum queue size (number of elements) from QueueSizeMax. If the
    // returned value is zero (0x0) the queue is not available.
    match mmio::read32(device_base, mmio::RegisterOffset::QueueSizeMax) {
        0 => return Err(()),
        n => {
            log::debug!("virtqueue size max: {n}");
        }
    }
    // 4. Allocate and zero the queue memory, making sure the memory is physically
    // contiguous.
    // 5. Notify the device about the queue size by writing the size to QueueSize.
    mmio::write32(
        device_base,
        mmio::RegisterOffset::QueueSize,
        QUEUE_SIZE as u32,
    );

    let mut driver = DRIVER.lock();

    // 6. Write physical addresses of the queue’s Descriptor Area, Driver Area and
    // Device Area to (respectively) the QueueDescLow/QueueDescHigh,
    // QueueDriverLow/QueueDriverHigh and QueueDeviceLow/QueueDeviceHigh register
    // pairs.
    let layout = Layout::from_size_align(4096, 16).unwrap();
    let ptr = unsafe { alloc::alloc::alloc(layout) };
    driver.virtqueue = ptr;
    let base = driver.virtqueue as usize;

    let (desc_ptr, mut desc_start) = save_to_virtqueue::<_, 16>(
        device_base,
        base,
        0,
        RegisterOffset::QueueDescLow,
        RegisterOffset::QueueDescHigh,
    );
    let offset = desc_start + size_of::<Descriptor>() * QUEUE_SIZE;

    let (avail_ptr, avail_start) = save_to_virtqueue::<_, 2>(
        device_base,
        base,
        offset,
        RegisterOffset::QueueDriverLow,
        RegisterOffset::QueueDriverHigh,
    );
    let offset = avail_start + size_of::<AvailableRing<QUEUE_SIZE>>();

    let (used_ptr, _) = save_to_virtqueue::<_, 4>(
        device_base,
        base,
        offset,
        RegisterOffset::QueueDeviceLow,
        RegisterOffset::QueueDeviceHigh,
    );

    driver.desc_ptr = desc_ptr;
    driver.avail_ptr = avail_ptr;
    driver.used_ptr = used_ptr;
    driver.device_base = device_base;

    // 7. Write 0x1 to QueueReady.
    mmio::write32(device_base, mmio::RegisterOffset::QueueReady, 1u32);

    Ok(())
}

fn save_to_virtqueue<T, const ALIGN: usize>(
    device_base: usize,
    virtqueue_base: usize,
    alignment_base: usize,
    low_reg: RegisterOffset,
    high_reg: RegisterOffset,
) -> (*mut T, usize) {
    let offset = align_up(alignment_base, ALIGN);
    let ptr = (virtqueue_base + offset) as *mut T;
    let ptr_pa = mm::virt_to_phys(ptr as usize);
    assert_eq!(ptr_pa % ALIGN, 0);

    mmio::write32(device_base, low_reg, ptr_pa as u32);
    mmio::write32(device_base, high_reg, ((ptr_pa as u64) >> 32) as u32);

    (ptr, offset)
}

pub unsafe fn write(data: &[u8; 512], sector: u64) -> u8 {
    let mut driver = DRIVER.lock();

    // NOTE: We are allocating instead of using the stack because the stack can have
    // any VA at this moment. Say this `write` is called during a user trap.
    // Then the kernel stack will be somewhere at 0x4fff_xxxx etc. Then
    // `virt_to_phys` will definitely fail. But the allocation here guarantees
    // that the `req` will live in the `KERNEL_DIRECT_MAPPING_BASE` space.
    let req = Box::new(VirtioBlkReqHeader {
        ty: VirtioBlkReqType::Out,
        _reserved: 0,
        sector: sector,
    });

    let mut status = Box::new(0xffu8);
    // Write the header as the first param
    unsafe {
        *driver.desc_ptr = Descriptor {
            addr: mm::virt_to_phys((req.as_ref() as *const VirtioBlkReqHeader) as usize) as u64,
            len: size_of::<VirtioBlkReqHeader>() as u32,
            flags: DescriptorFlag::NEXT,
            next: 1,
        };
    }

    // The buffer with size 512 that will be written to the disc goes next
    unsafe {
        *driver.desc_ptr.offset(1) = Descriptor {
            addr: mm::virt_to_phys(data.as_ptr() as usize) as u64,
            len: 512,
            flags: DescriptorFlag::NEXT,
            next: 2,
        };
    }

    // Finally we write the status and label it with `WRITE` since the device will
    // write to this
    unsafe {
        *driver.desc_ptr.offset(2) = Descriptor {
            addr: mm::virt_to_phys((status.as_mut() as *mut u8) as usize) as u64,
            len: 1,
            flags: DescriptorFlag::WRITE,
            next: 0,
        };
    }

    driver.operate(status)
}

pub unsafe fn read(data: &mut [u8; 512], sector: u64) -> u8 {
    let mut driver = DRIVER.lock();

    // NOTE: We are allocating instead of using the stack because the stack can have
    // any VA at this moment. Say this `write` is called during a user trap.
    // Then the kernel stack will be somewhere at 0x4fff_xxxx etc. Then
    // `virt_to_phys` will definitely fail. But the allocation here guarantees
    // that the `req` will live in the `KERNEL_DIRECT_MAPPING_BASE` space.
    let req = Box::new(VirtioBlkReqHeader {
        ty: VirtioBlkReqType::In,
        _reserved: 0,
        sector: sector,
    });

    let mut status = Box::new(0xffu8);

    // Write the header as the first param
    unsafe {
        *driver.desc_ptr = Descriptor {
            addr: mm::virt_to_phys((req.as_ref() as *const VirtioBlkReqHeader) as usize) as u64,
            len: size_of::<VirtioBlkReqHeader>() as u32,
            flags: DescriptorFlag::NEXT,
            next: 1,
        };
    }

    // The buffer with size 512 that will be written to the disc goes next
    unsafe {
        *driver.desc_ptr.offset(1) = Descriptor {
            addr: mm::virt_to_phys(data.as_ptr() as usize) as u64,
            len: 512,
            flags: DescriptorFlag::NEXT | DescriptorFlag::WRITE,
            next: 2,
        };
    }

    // Finally we write the status and label it with `WRITE` since the device will
    // write to this
    unsafe {
        *driver.desc_ptr.offset(2) = Descriptor {
            addr: mm::virt_to_phys((status.as_mut() as *mut u8) as usize) as u64,
            len: 1,
            flags: DescriptorFlag::WRITE,
            next: 0,
        };
    }

    driver.operate(status)
}

impl VirtioBlkDriver {
    fn operate(&mut self, status: Box<u8>) -> u8 {
        let avail = unsafe { &mut *self.avail_ptr };

        let slot = avail.idx as usize % QUEUE_SIZE;

        // submit descriptor chain starting at desc[0]
        avail.ring[slot] = 0;

        // make desc[0..2] and avail.ring visible before idx update
        atomic::fence(Ordering::Release);

        avail.idx = avail.idx.wrapping_add(1);

        atomic::fence(Ordering::Release);

        // notify queue 0
        mmio::write32(self.device_base, mmio::RegisterOffset::QueueNotify, 0u32);

        let used = unsafe { &*self.used_ptr };

        let used_slot = self.last_used_idx as usize % QUEUE_SIZE;

        while unsafe { core::ptr::read_volatile(&used.idx) } == self.last_used_idx {
            core::hint::spin_loop();
        }

        atomic::fence(Ordering::Acquire);

        let new_used_idx = unsafe { core::ptr::read_volatile(&used.idx) };
        self.last_used_idx = new_used_idx;

        *status
    }
}

#[repr(u32)]
pub enum VirtioBlkReqType {
    /// Read
    In = 0,
    /// Write
    Out = 1,
    Flush = 4,
    /// Get device ID
    /// Fetches the device ID string from the device into data.
    /// The device ID string is a NUL-padded ASCII string up to 20 bytes long.
    /// If the string is 20 bytes long then there is no NUL terminator.
    GetId = 5,
    /// Get the device lifetime // TODO(aeryz): what's the device lifetime
    /// The data used for VIRTIO_BLK_T_GET_LIFETIME requests is populated by the
    /// device, and is of the form [`VirtioBlkLifetime`]
    GetLifetime = 10,
    Discard = 11,
    /// Fill with zeroes
    WriteZeroes = 13,
    /// Secure erase: TODO(aeryz): what's this
    SecureErase = 14,
}

#[repr(C)]
/// The following
/// The driver enqueues requests to the virtqueues, and they are used by the
/// device (not necessarily in order). Each request except
/// VIRTIO_BLK_T_ZONE_APPEND is of form:
pub struct VirtioBlkReqHeader {
    ty: VirtioBlkReqType,
    _reserved: u32,
    /// Indicates the offset (multiplied by 512) where the read or write is to
    /// occur. This field is unused and set to 0 for commands other than
    /// read, write and some zone operations
    sector: u64,
}

/// The final status byte is written by the device: either VIRTIO_BLK_S_OK for
/// success, VIRTIO_BLK_S_IOERR for device or driver error or
/// VIRTIO_BLK_S_UNSUPP for a request unsupported by device:
#[derive(Debug, Clone)]
#[repr(u8)]
pub enum VirtioBlkStatus {
    Ok = 0,
    IoErr = 1,
    Unsupp = 2,
}

bitflags! {
    #[derive(Debug, Clone)]
    #[repr(transparent)]
    pub struct VirtioBlkFlag: u32 {
        /// Maximum size of any single segment is in size_max.
        const VIRTIO_BLK_F_SIZE_MAX = 1 << 1;
        /// Maximum number of segments in a request is in seg_max.
        const VIRTIO_BLK_F_SEG_MAX = 1 << 2;
        /// Disk-style geometry specified in geometry.
        const VIRTIO_BLK_F_GEOMETRY = 1 << 4;
        /// Device is read-only.
        const VIRTIO_BLK_F_RO = 1 << 5;
        /// Block size of disk is in blk_size.
        const VIRTIO_BLK_F_BLK_SIZE = 1 << 6;
        /// Cache flush command support.
        const VIRTIO_BLK_F_FLUSH = 1 << 9;
        /// Device exports information on optimal I/O alignment.
        const VIRTIO_BLK_F_TOPOLOGY = 1 << 10;
        /// Device can toggle its cache between writeback and writethrough modes.
        const VIRTIO_BLK_F_CONFIG_WCE = 1 << 11;
        /// Device supports multiqueue.
        const VIRTIO_BLK_F_MQ = 1 << 12;
        /// Device can support discard command, maximum discard sectors size in
        /// max_discard_sectors and maximum discard segment number in max_discard_seg.
        const VIRTIO_BLK_F_DISCARD = 1 << 13;
        /// Device can support write zeroes command, maximum write zeroes sectors size
        /// in max_write_zeroes_sectors and maximum write zeroes segment number in
        /// max_write_zeroes_seg.
        const VIRTIO_BLK_F_WRITE_ZEROES = 1 << 14;
        /// Device supports providing storage lifetime information.
        const VIRTIO_BLK_F_LIFETIME = 1 << 15;
        /// Device supports secure erase command, maximum erase sectors count in
        /// max_secure_erase_sectors and maximum erase segment number in
        /// max_secure_erase_seg.
        const VIRTIO_BLK_F_SECURE_ERASE = 1 << 16;
        /// Device is a Zoned Block Device, that is, a device that follows the zoned
        /// storage device behavior that is also supported by industry standards such as
        /// the T10 Zoned Block Command standard (ZBC r05) or the NVMe(TM) NVM Express
        /// Zoned Namespace Command Set Specification 1.1b (ZNS). For brevity, these
        /// standard documents are referred as "ZBD standards" from this point on in the
        /// text.
        const VIRTIO_BLK_F_ZONED = 1 << 17;

        // Note: In the legacy interface, VIRTIO_BLK_F_FLUSH was also called
        // VIRTIO_BLK_F_WCE.
        /// LEGACY: Device supports request barriers.
        const VIRTIO_BLK_F_BARRIE = 1 << 0;
        /// LEGACY: Device supports scsi packet commands.
        const VIRTIO_BLK_F_SCSI = 1 << 7;
    }
}
