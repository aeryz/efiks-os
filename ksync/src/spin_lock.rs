use core::{
    cell::UnsafeCell,
    hint::spin_loop,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct SpinLock<T: ?Sized> {
    flag: AtomicBool,
    data: UnsafeCell<T>,
}

pub struct SpinLockGuard<'a, T: ?Sized> {
    lock: &'a SpinLock<T>,
    // Causes this struct to impl !Send which is required since we don't want
    // this to be send between threads.
    _marker: PhantomData<*mut ()>,
}

impl<T> SpinLock<T> {
    pub const fn new(data: T) -> Self {
        SpinLock {
            flag: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized> SpinLock<T> {
    pub fn lock<'a>(&'a self) -> SpinLockGuard<'a, T> {
        while !self
            .flag
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            spin_loop();
        }

        SpinLockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }
}

impl<'a, T: ?Sized> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.flag.store(false, Ordering::Release);
    }
}

impl<'a, T: ?Sized> Deref for SpinLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY:
        // - If used with safe Rust, `data` can never be null
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY:
        // - If used with safe Rust, `data` can never be null
        // - Rust enforces `deref_mut` to follow regular ownership rules since
        // it's prototyped with `&mut self`.
        unsafe { &mut *self.lock.data.get() }
    }
}

unsafe impl<T: ?Sized + Send> Sync for SpinLock<T> {}
unsafe impl<T: ?Sized + Send> Send for SpinLock<T> {}

unsafe impl<'a, T: ?Sized + Send> Sync for SpinLockGuard<'a, T> {}
