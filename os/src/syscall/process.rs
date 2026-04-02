//! Process management syscalls
use core::mem::size_of;

use crate::{
    mm::{translated_byte_buffer, translated_byte, translated_byte_ref},
    task::{
        change_program_brk, current_user_token, exit_current_and_run_next, get_syscall_count,
        suspend_current_and_run_next,
    },
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
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

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace request={}, id={}, data={}", trace_request, id, data);
    match trace_request {
        0 => {
            let token = current_user_token();
            let ptr = id as *const u8;
            if let Some(data) = translated_byte(token, ptr) {
                data as isize
            } else {
                -1
            }
        }
        1 => {
            let token = current_user_token();
            let ptr = id as *mut u8;
            if let Some(data_ref) = translated_byte_ref(token, ptr) {
                let data = (data & 0xFF) as u8;
                *data_ref = data;
                0
            } else {
                -1
            }
        }
        2 => get_syscall_count(id) as isize,
        _ => -1,
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    -1
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    -1
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
