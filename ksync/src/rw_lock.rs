use core::{
    cell::UnsafeCell,
    hint::spin_loop,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicUsize, Ordering},
};

const NO_LOCK_HELD: usize = 0;
const WRITE_LOCK_HELD: usize = usize::MAX;

/// Spin-based read-write lock.
///
/// Note that this is not a write-aware RWLock, meaning on a read-heavy system,
/// the writers might starve forever.
pub struct RwLock<T: ?Sized> {
    readers: AtomicUsize,
    data: UnsafeCell<T>,
}

impl<T> RwLock<T> {
    pub const fn new(data: T) -> Self {
        RwLock {
            readers: AtomicUsize::new(0),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized> RwLock<T> {
    pub fn read_lock<'a>(&'a self) -> ReadLockGuard<'a, T> {
        loop {
            let readers = self.readers.load(Ordering::Acquire);
            if readers == usize::MAX {
                spin_loop();
                continue;
            }
            assert!(readers < WRITE_LOCK_HELD - 1);

            if self
                .readers
                .compare_exchange_weak(readers, readers + 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return ReadLockGuard {
                    lock: self,
                    _marker: PhantomData,
                };
            }

            spin_loop();
        }
    }

    pub fn write_lock<'a>(&'a self) -> WriteLockGuard<'a, T> {
        while !self
            .readers
            .compare_exchange(
                NO_LOCK_HELD,
                WRITE_LOCK_HELD,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
        {}

        WriteLockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }
}

pub struct ReadLockGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
    // Causes this struct to impl !Send which is required since we don't want
    // this to be send between threads.
    _marker: PhantomData<*mut ()>,
}

impl<'a, T: ?Sized> Drop for ReadLockGuard<'a, T> {
    fn drop(&mut self) {
        // TODO(aeryz): should this be relaxed?
        self.lock.readers.fetch_sub(1, Ordering::Release);
    }
}

impl<'a, T: ?Sized> Deref for ReadLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY:
        // - If used with safe Rust, `data` can never be null
        unsafe { &*self.lock.data.get() }
    }
}

pub struct WriteLockGuard<'a, T: ?Sized> {
    lock: &'a RwLock<T>,
    // Causes this struct to impl !Send which is required since we don't want
    // this to be send between threads.
    _marker: PhantomData<*mut ()>,
}

impl<'a, T: ?Sized> Drop for WriteLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.readers.store(NO_LOCK_HELD, Ordering::Release);
    }
}

impl<'a, T: ?Sized> Deref for WriteLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY:
        // - If used with safe Rust, `data` can never be null
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for WriteLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY:
        // - If used with safe Rust, `data` can never be null
        // - Rust enforces `deref_mut` to follow regular ownership rules since
        // it's prototyped with `&mut self`.
        unsafe { &mut *self.lock.data.get() }
    }
}

unsafe impl<T: ?Sized + Send + Sync> Sync for RwLock<T> {}
unsafe impl<T: ?Sized + Send> Send for RwLock<T> {}

unsafe impl<'a, T: ?Sized + Send> Sync for ReadLockGuard<'a, T> {}
unsafe impl<'a, T: ?Sized + Send> Sync for WriteLockGuard<'a, T> {}
