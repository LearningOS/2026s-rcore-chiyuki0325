//! File and filesystem-related syscalls
use core::mem::size_of;

use crate::fs::{linkat, open_file, unlinkat, OSInode, OpenFlags, Stat};
use crate::mm::{translated_byte_buffer, translated_str, UserBuffer};
use crate::task::{current_task, current_user_token};

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// YOUR JOB: Implement fstat.
pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    let task = current_task().unwrap();

    trace!("kernel:pid[{}] sys_fstat fd={}", task.pid.0, fd);

    let token = task.get_user_token();
    let ptr = st as *mut u8;
    let len = size_of::<Stat>();
    let buffers = translated_byte_buffer(token, ptr, len);

    let inner = task.inner_read_access();
    if let Some(some_file) = inner.fd_table.get(fd) {
        if let Some(file) = some_file {
            let file = file.clone();
            if let Ok(inode) = OSInode::downcast_arc(file) {
                // is a file on disk with deterministic inode
                let stat = inode.stats();
                trace!("kernel:pid[{}] sys_fstat stat={:#?}", task.pid.0, stat);

                unsafe {
                    let result_ptr = &stat as *const Stat as *const u8;
                    let mut result_slice = core::slice::from_raw_parts(result_ptr, len);
                    buffers.into_iter().for_each(|part_dst| {
                        let part_size = part_dst.len();
                        let (part_src, slice_remaining) = result_slice.split_at(part_size);
                        part_dst.copy_from_slice(part_src);
                        result_slice = slice_remaining;
                    });
                }
                return 0;
            }
        }
    }

    -1
}

/// YOUR JOB: Implement linkat.
pub fn sys_linkat(old_name: *const u8, new_name: *const u8) -> isize {
    let token = current_user_token();
    let old_name = translated_str(token, old_name);
    let new_name = translated_str(token, new_name);
    trace!(
        "kernel:pid[{}] sys_linkat old_name={} new_name={}",
        current_task().unwrap().pid.0,
        old_name,
        new_name
    );
    if linkat(&old_name, &new_name) {
        0
    } else {
        -1
    }
}

/// YOUR JOB: Implement unlinkat.
pub fn sys_unlinkat(name: *const u8) -> isize {
    let task = current_task().unwrap();
    let token = task.get_user_token();
    let name = translated_str(token, name);
    trace!(
        "kernel:pid[{}] sys_unlinkat name={}",
        task.pid.0,
        name
    );
    if let Some(links) = unlinkat(&name) {
        trace!(
            "kernel:pid[{}] sys_unlinkat remaining links={}",
            task.pid.0,
            links
        );
        0
    } else {
        trace!(
            "kernel:pid[{}] sys_unlinkat not found",
            task.pid.0,
        );
        -1
    }
}
