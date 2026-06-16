#![allow(unused)]

use core::sync::atomic::{AtomicU8, Ordering};

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TaskState {
    /// Task is sleeping and cannot be run
    Sleeping,
    /// Task is running
    Running,
    /// Task is ready to be run (possibly in the runqueue)
    Ready,
    /// Task is blocked by either a syscall or an I/O op
    Blocked,
    /// Task is dead/killed but its resources aren't claimed yet
    Zombie,
    /// Task is fully exited and its resources are claimed
    Exited,
}

/// Atomic wrapper over [`TaskState`]
pub struct AtomicTaskState(AtomicU8);

impl AtomicTaskState {
    pub fn set(&self, state: TaskState) {
        self.0.store(state as u8, Ordering::Release);
    }

    pub fn raw(&self) -> TaskState {
        // SAFETY: self is guaranteed to be a valid `TaskState`
        unsafe { core::mem::transmute(self.0.load(Ordering::Relaxed)) }
    }
}

impl PartialEq<TaskState> for AtomicTaskState {
    fn eq(&self, other: &TaskState) -> bool {
        self.0.load(Ordering::Relaxed) == *other as u8
    }
}

impl From<TaskState> for AtomicTaskState {
    fn from(value: TaskState) -> Self {
        Self(AtomicU8::new(value as u8))
    }
}
