use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
use alloc::vec::Vec;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() as isize - 1
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    let task = current_task().unwrap();
    let tid = task.inner_exclusive_access().res.as_ref().unwrap().tid;
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    if let Ok(()) = mutex.lock() {
        0
    } else {
        -0xdead
    }
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        // extend available_sem for deadlock detection
        process_inner.available_sem[id] = res_count as isize;
        for thread_alloc in process_inner.allocated_sem.iter_mut() {
            thread_alloc[id] = 0;
        }
        for thread_need in process_inner.need_sem.iter_mut() {
            thread_need[id] = 0;
        }
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        let id = process_inner.semaphore_list.len() - 1;
        // extend available_sem for deadlock detection
        process_inner.available_sem.push(res_count as isize);
        for thread_alloc in process_inner.allocated_sem.iter_mut() {
            thread_alloc.push(0);
        }
        for thread_need in process_inner.need_sem.iter_mut() {
            thread_need.push(0);
        }
        id
    };
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.up();
    // borrow process_inner again to update banker state after up operation
    let mut process_inner = process.inner_exclusive_access();
    if process_inner.deadlock_detect_enabled {
        // release entrance

        // allocated - 1
        process_inner.allocated_sem[tid][sem_id] -= 1;

        // available + 1
        process_inner.available_sem[sem_id] += 1;

        // trace

        trace!(
            "kernel:pid[{}] tid[{}] sys_semaphore_up after banker: available_sem={:?}",
            current_task().unwrap().process.upgrade().unwrap().getpid(),
            tid,
            process_inner.available_sem
        );
        trace!(
            "kernel:pid[{}] tid[{}] sys_semaphore_up after banker: allocated_sem={:?}",
            current_task().unwrap().process.upgrade().unwrap().getpid(),
            tid,
            process_inner.allocated_sem
        );
    }
    drop(process_inner);
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    if process_inner.deadlock_detect_enabled {
        let threads = process_inner.allocated_sem.len();
        let sems = process_inner.semaphore_list.len();

        trace!(
            "kernel:pid[{}] tid[{}] sys_semaphore_down banker: threads={}, sems={}",
            current_task().unwrap().process.upgrade().unwrap().getpid(),
            tid,
            threads,
            sems
        );

        trace!(
            "kernel:pid[{}] tid[{}] sys_semaphore_down banker: available_sem={:?}",
            current_task().unwrap().process.upgrade().unwrap().getpid(),
            tid,
            process_inner.available_sem
        );

        trace!(
            "kernel:pid[{}] tid[{}] sys_semaphore_down banker: allocated_sem={:?}",
            current_task().unwrap().process.upgrade().unwrap().getpid(),
            tid,
            process_inner.allocated_sem
        );

        // need + 1
        while process_inner.need_sem.len() <= tid {
            process_inner.need_sem.push(Vec::new());
        }
        while process_inner.need_sem[tid].len() <= sem_id {
            process_inner.need_sem[tid].push(0);
        }
        process_inner.need_sem[tid][sem_id] += 1;

        trace!(
            "kernel:pid[{}] tid[{}] sys_semaphore_down banker: need_sem added={:?}",
            current_task().unwrap().process.upgrade().unwrap().getpid(),
            tid,
            process_inner.need_sem
        );

        // run detection
        let mut work = process_inner.available_sem.clone();
        let mut finish = Vec::new();
        for _ in 0..threads {
            finish.push(false);
        }

        for _ in 0..threads {
            for t in 0..threads {
                trace!(
                    "kernel:pid[{}] tid[{}] sys_semaphore_down banker: checking thread {}",
                    current_task().unwrap().process.upgrade().unwrap().getpid(),
                    tid,
                    t
                );
                if !finish[t] {
                trace!(
                    "kernel:pid[{}] tid[{}] sys_semaphore_down banker: thread not finish, need={:?}, work={:?}",
                    current_task().unwrap().process.upgrade().unwrap().getpid(),
                    tid,
                    process_inner.need_sem[t],
                    work
                );
                    if process_inner.need_sem[t]
                        .iter()
                        .enumerate()
                        .all(|(sem_id, need)| *need <= work[sem_id])
                    {
                        for sem_id in 0..sems {
                            work[sem_id] += process_inner.allocated_sem[t][sem_id];
                        }
                        finish[t] = true;
                        trace!(
                            "kernel:pid[{}] tid[{}] sys_semaphore_down banker: thread {} can finish",
                            current_task().unwrap().process.upgrade().unwrap().getpid(),
                            tid,
                            t
                        );
                        trace!(
                            "kernel:pid[{}] tid[{}] sys_semaphore_down banker: work after thread {} finish: {:?}",
                            current_task().unwrap().process.upgrade().unwrap().getpid(),
                            tid,
                            t,
                            work
                        );
                        break;
                    }
                }
            }
        }

        // if all finish, then no deadlock, otherwise deadlock happens
        if finish.iter().any(|f| !f) {
            // rollback need + 1
            process_inner.need_sem[tid][sem_id] -= 1;
            drop(process_inner);
            trace!(
                "kernel:pid[{}] tid[{}] sys_semaphore_down deadlock detected!",
                current_task().unwrap().process.upgrade().unwrap().getpid(),
                tid
            );
            return -0xdead;
        }
    }

    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.down();

    // borrow process_inner again to update banker state after down operation
    let mut process_inner = process.inner_exclusive_access();
    if process_inner.deadlock_detect_enabled {
        // down entrance

        // available - 1
        process_inner.available_sem[sem_id] -= 1;

        // allocated + 1
        while process_inner.allocated_sem.len() <= tid {
            process_inner.allocated_sem.push(Vec::new());
        }
        while process_inner.allocated_sem[tid].len() <= sem_id {
            process_inner.allocated_sem[tid].push(0);
        }
        process_inner.allocated_sem[tid][sem_id] += 1;

        // need - 1
        process_inner.need_sem[tid][sem_id] -= 1;

        // trace
        trace!(
            "kernel:pid[{}] tid[{}] sys_semaphore_down after banker: available_sem={:?}",
            current_task().unwrap().process.upgrade().unwrap().getpid(),
            tid,
            process_inner.available_sem
        );

        trace!(
            "kernel:pid[{}] tid[{}] sys_semaphore_down after banker: allocated_sem={:?}",
            current_task().unwrap().process.upgrade().unwrap().getpid(),
            tid,
            process_inner.allocated_sem
        );
    }
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect enabled={}", enabled);
    let proc = current_process();
    let mut proc_inner = proc.inner_exclusive_access();
    proc_inner.deadlock_detect_enabled = enabled != 0;
    0
}
