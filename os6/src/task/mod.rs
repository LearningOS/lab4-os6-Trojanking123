//! Implementation of process management mechanism
//!
//! Here is the entry for process scheduling required by other modules
//! (such as syscall or clock interrupt).
//! By suspending or exiting the current process, you can
//! modify the process state, manage the process queue through TASK_MANAGER,
//! and switch the control flow through PROCESSOR.
//!
//! Be careful when you see [`__switch`]. Control flow around this function
//! might not be what you expect.

mod context;
mod manager;
mod pid;
mod processor;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use alloc::sync::Arc;
use lazy_static::*;
use manager::fetch_task;
use switch::__switch;
use crate::mm::VirtAddr;
use crate::mm::MapPermission;
use crate::config::PAGE_SIZE;
use crate::timer::get_time_us;
pub use crate::syscall::process::TaskInfo;
use crate::fs::{open_file, OpenFlags};
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;
pub use manager::add_task;
pub use pid::{pid_alloc, KernelStack, PidHandle};
pub use processor::{
    current_task, current_trap_cx, current_user_token, run_tasks, schedule, take_current_task,
        add_one_to_current_task, get_current_task_costed_time, get_current_task_status, get_current_task_syscall_times,
        mmap, munmap
};

use crate::timer::get_time_ms;

/// Make current task suspended and switch to the next task
pub fn suspend_current_and_run_next() {
    // There must be an application running.
    let task = take_current_task().unwrap();

    // ---- access current TCB exclusively
    let mut task_inner = task.inner_exclusive_access();
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    // Change status to Ready
    task_inner.task_status = TaskStatus::Ready;
    drop(task_inner);
    // ---- release current PCB

    // push back to ready queue.
    add_task(task);
    // jump to scheduling cycle
    schedule(task_cx_ptr);
}

/// Exit current task, recycle process resources and switch to the next task
pub fn exit_current_and_run_next(exit_code: i32) {
    // take from Processor
    let task = take_current_task().unwrap();
    // **** access current TCB exclusively
    let mut inner = task.inner_exclusive_access();
    // Change status to Zombie
    inner.task_status = TaskStatus::Zombie;
    // Record exit code
    inner.exit_code = exit_code;
    // do not move to its parent but under initproc

    // ++++++ access initproc TCB exclusively
    {
        let mut initproc_inner = INITPROC.inner_exclusive_access();
        for child in inner.children.iter() {
            child.inner_exclusive_access().parent = Some(Arc::downgrade(&INITPROC));
            initproc_inner.children.push(child.clone());
        }
    }
    // ++++++ release parent PCB

    inner.children.clear();
    // deallocate user space
    inner.memory_set.recycle_data_pages();
    drop(inner);
    // **** release current PCB
    // drop task manually to maintain rc correctly
    drop(task);
    // we do not have to save task context
    let mut _unused = TaskContext::zero_init();
    schedule(&mut _unused as *mut _);
}

lazy_static! {
    /// Creation of initial process
    ///
    /// the name "initproc" may be changed to any other app name like "usertests",
    /// but we have user_shell, so we don't need to change it.
    pub static ref INITPROC: Arc<TaskControlBlock> = Arc::new({
        let inode = open_file("ch6b_initproc", OpenFlags::RDONLY).unwrap();
        let v = inode.read_all();
        TaskControlBlock::new(v.as_slice())
    });
}

pub fn add_initproc() {
    add_task(INITPROC.clone());
}


pub fn add_one_while_syscall(id: usize) {
    add_one_to_current_task(id);
}


pub fn get_task_info_inner(t: *mut TaskInfo) {
    let a = get_current_task_status();
    let b = get_current_task_syscall_times();
    let c = get_current_task_costed_time();
    // info!("get info okkkkkkkkkk");
    // info!("a : {:?}", a);
    // info!("b : {:?}", b.clone());
    // info!("c : {:?}", c);
    unsafe {
        *t = TaskInfo {

            status : a,
            syscall_times: b,
            time: c,
        }
    }

} 

pub fn sys_mmap_inner(start: usize, len: usize, port: usize) -> isize {
    let va = VirtAddr(start);
    if ! va.aligned() || port & !0x7 != 0  || port & 0x7 == 0 {
        return -1;
    }
    mmap(start, len, port)
}

pub fn sys_munmap_inner(start: usize, len: usize ) -> isize {
    let va = VirtAddr(start);
    if ! va.aligned()  {
        return -1;
    }
    munmap(start, len)
}

pub fn set_priority_inner(prio: isize) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    info!("set task: {:?} prio from {:?} to {:?}", task.pid.0, inner.prio, prio);
    inner.prio = prio;

    prio
}