//! Process management syscalls
//!
use core::mem::size_of;

use alloc::sync::Arc;

use crate::{
    fs::{OpenFlags, open_file},
    mm::{MapPermission, VirtAddr, translated_byte_buffer, translated_refmut, translated_str},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next,
    }, timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    //trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    //trace!("kernel: sys_waitpid");
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel:pid[{}] sys_get_time", current_task().unwrap().pid.0);
    // trace!("kernel: sys_get_time");
    let us = get_time_us();

    // reference: https://rcore-os.cn/rCore-Tutorial-Book-v3/chapter4/6multitasking-based-on-as.html#sys-write

    let token = current_user_token();
    let ptr = ts as *mut u8;
    let len = size_of::<TimeVal>();
    let buffers = translated_byte_buffer(token, ptr, len);

    let result_ts = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };

    unsafe {
        let result_ptr = &result_ts as *const TimeVal as *const u8;
        let mut result_slice = core::slice::from_raw_parts(result_ptr, len);
        buffers.into_iter().for_each(|part_dst| {
            let part_size = part_dst.len();
            let (part_src, slice_remaining) = result_slice.split_at(part_size);
            part_dst.copy_from_slice(part_src);
            result_slice = slice_remaining;
        });
    }
    0
}


/// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    let task = current_task().unwrap();
    trace!(
        "kernel:pid[{}] sys_mmap start={:#x}, len={:#x}, prot={}",
        task.pid.0,
        start,
        len,
        prot
    );

    if len == 0 {
        // No need to map
        return 0;
    }

    let len = (len + 0xfff) & !0xfff;

    if start % 4096 != 0 {
        // Address not aligned
        return -1;
    }

    let start_va: VirtAddr = start.into();
    let end_va: VirtAddr = (start + len).into();

    let start_vpn = start_va.floor();
    let end_vpn = end_va.floor();

    trace!(
        "kernel:pid[{}] sys_mmap start_vpn={:#x}, end_vpn={:#x}",
        task.pid.0,
        start_vpn.0,
        end_vpn.0
    );

    let mmset = &mut task.inner_exclusive_access().memory_set;

    if !mmset.check_vpn_range(start_vpn, end_vpn) {
        // page overlapped
        return -1;
    }

    let mut permission = MapPermission::U;

    if prot & !0b111 != 0 || prot & 0b111 == 0 {
        // invalid "prot" value
        return -1;
    }

    if prot & 0b1 != 0 {
        permission |= MapPermission::R;
    }
    if prot & 0b10 != 0 {
        permission |= MapPermission::W;
    }
    if prot & 0b100 != 0 {
        permission |= MapPermission::X;
    }

    mmset.insert_framed_area(start_va, end_va, permission);
    0
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    let task = current_task().unwrap();
    trace!(
        "kernel:pid[{}] sys_munmap start={:#x}, len={:#x}",
        task.pid.0,
        start,
        len
    );

    if len == 0 {
        // No need to unmap
        return 0;
    }

    if start % 4096 != 0 {
        // Address not aligned
        return -1;
    }
    
    let len = (len + 0xfff) & !0xfff;
    let start_va: VirtAddr = start.into();
    let end_va: VirtAddr = (start+len).into();

    if task
        .inner_exclusive_access()
        .memory_set
        .remove(start_va, end_va)
    {
        0
    } else {
        -1
    }
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    trace!(
        "kernel:pid[{}] sys_spawn path={}",
        current_task().unwrap().pid.0,
        path
    );
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let data = app_inode.read_all();
        let task = current_task().unwrap();
        let new_task = task.spawn(data.as_slice());
        let new_pid = new_task.pid.0;
        // add new task to scheduler
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}


// YOUR JOB: Set task priority.
pub fn sys_set_priority(prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority prio={}",
        current_task().unwrap().pid.0,
        prio
    );
    
    if prio >= 2 {
        let current_task = current_task().unwrap();
        current_task.inner_exclusive_access().priority = prio as usize;
        prio
    } else {
        -1
    }
}