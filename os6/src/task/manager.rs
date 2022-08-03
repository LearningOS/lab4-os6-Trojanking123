//! Implementation of [`TaskManager`]
//!
//! It is only used to manage processes and schedule process based on ready queue.
//! Other CPU process monitoring functions are in Processor.


use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::BinaryHeap;
use alloc::sync::Arc;
use lazy_static::*;
use  crate::config::BIG_STRIDE;

pub struct TaskManager {
    ready_queue: BinaryHeap<Arc<TaskControlBlock>>,
}


// YOUR JOB: FIFO->Stride
/// A simple FIFO scheduler.
impl TaskManager {
    pub fn new() -> Self {
        Self {
            ready_queue: BinaryHeap::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push(task);
        // let a = self.ready_queue.clone().into_iter().map(|x| x.clone().pid.0);
        // let b = a.collect::<Vec<usize>>();
        //info!("after add: {:?}", b);
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        let a = self.ready_queue.pop();
        let tcb = a.clone().unwrap();
        let pid = tcb.pid.0;
        let mut inner = tcb.inner_exclusive_access();
        //info!("fetch pid: {:?} and pass is {:?}", pid, inner.pass);
        inner.pass += BIG_STRIDE / inner.prio;
        a
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

pub fn add_task(task: Arc<TaskControlBlock>) {
    TASK_MANAGER.exclusive_access().add(task);
}

pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    TASK_MANAGER.exclusive_access().fetch()
}
