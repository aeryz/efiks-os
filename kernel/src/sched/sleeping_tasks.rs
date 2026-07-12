use core::cmp::Reverse;

use alloc::{
    collections::binary_heap::BinaryHeap,
    sync::{Arc, Weak},
};

use crate::task::Task;

pub struct SleepingTasks(BinaryHeap<Reverse<SleepNode>>);

impl SleepingTasks {
    pub fn new() -> Self {
        Self(BinaryHeap::new())
    }

    pub fn push(&mut self, task: &Arc<Task>) {
        self.0.push(Reverse(SleepNode {
            sleep_until: task.runtime.lock().wake_up_at,
            task: Arc::downgrade(task),
        }));
    }

    pub fn with_tasks_to_wake_up<F: FnMut(Arc<Task>)>(&mut self, current_time: usize, mut cb: F) {
        while let Some(node) = self.0.peek() {
            if current_time >= node.0.sleep_until {
                let Some(task) = node.0.task.upgrade() else {
                    let _ = self.0.pop();
                    continue;
                };

                cb(task);
                let _ = self.0.pop();
            } else {
                break;
            }
        }
    }
}

struct SleepNode {
    sleep_until: usize,
    task: Weak<Task>,
}

impl PartialOrd for SleepNode {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.sleep_until.partial_cmp(&other.sleep_until)
    }
}

impl Ord for SleepNode {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.sleep_until.cmp(&other.sleep_until)
    }
}

impl PartialEq for SleepNode {
    fn eq(&self, other: &Self) -> bool {
        self.sleep_until == other.sleep_until
    }
}

impl Eq for SleepNode {}
