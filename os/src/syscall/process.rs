//! Process management syscalls
//!
use core::mem::size_of;

use alloc::sync::Arc;

use crate::{
    config::PAGE_SIZE,
    fs::{open_file, OpenFlags},
    mm::{
        translated_byte_buffer, translated_refmut, translated_str, MapPermission, PageTable,
        VirtAddr, VirtPageNum,
    },
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next, TaskControlBlock,
    },
    timer::get_time_us,
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

/// Get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let sec = us / 1_000_000;
    let usec = us % 1_000_000;

    let sec_bytes = sec.to_ne_bytes();
    let usec_bytes = usec.to_ne_bytes();
    let word_size = size_of::<usize>();
    let mut time_bytes = [0u8; size_of::<TimeVal>()];

    time_bytes[..word_size].copy_from_slice(&sec_bytes);
    time_bytes[word_size..].copy_from_slice(&usec_bytes);

    let buffers =
        translated_byte_buffer(current_user_token(), ts as *const u8, size_of::<TimeVal>());

    let mut offset = 0;
    for buffer in buffers {
        let (start, end) = (offset, offset + buffer.len());
        buffer.copy_from_slice(&time_bytes[start..end]);
        offset += buffer.len();
    }
    0
}

/// To mmap
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    if prot & !0x7 != 0 || prot & 0x7 == 0 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    if start.checked_add(len).is_none() {
        return -1;
    }
    let end_exclusive = start.checked_add(len).unwrap();
    let last = end_exclusive - 1;
    let need = if len % PAGE_SIZE != 0 {
        len / PAGE_SIZE + 1
    } else {
        len / PAGE_SIZE
    };
    let token = current_user_token();
    let page_table = PageTable::from_token(token);
    let start_va = VirtAddr::from(start);
    let restored: usize = start_va.into();
    if restored != start {
        return -1;
    }
    let end_va = VirtAddr::from(last);
    let restored: usize = end_va.into();
    if restored != last {
        return -1;
    }
    let start_vpn = start_va.floor();
    for page_id in 0..need {
        let vpn: VirtPageNum = (start_vpn.0 + page_id).into();
        let current_page_va = VirtAddr::from(start + PAGE_SIZE * page_id);
        let restored: usize = current_page_va.into();
        if restored != start + PAGE_SIZE * page_id {
            return -1;
        }
        match page_table.translate(vpn) {
            Some(pte) => {
                if pte.is_valid() {
                    return -1;
                }
            }
            None => {}
        }
    }
    let mut map_perm = MapPermission::U;
    if prot & 0x1 != 0 {
        map_perm |= MapPermission::R;
    }
    if prot & 0x2 != 0 {
        map_perm |= MapPermission::W;
    }
    if prot & 0x4 != 0 {
        map_perm |= MapPermission::X;
    }
    let current_task = current_task().unwrap();
    let mut inner = current_task.inner_exclusive_access();
    inner
        .memory_set
        .insert_framed_area(start_va, VirtAddr::from(end_exclusive), map_perm);
    0
}

/// Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_munmap", current_task().unwrap().pid.0);
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    if start.checked_add(len).is_none() {
        return -1;
    }
    let end_exclusive = start.checked_add(len).unwrap();
    let last = end_exclusive - 1;
    let need = if len % PAGE_SIZE != 0 {
        len / PAGE_SIZE + 1
    } else {
        len / PAGE_SIZE
    };
    let token = current_user_token();
    let page_table = PageTable::from_token(token);
    let start_va = VirtAddr::from(start);
    let restored: usize = start_va.into();
    if restored != start {
        return -1;
    }
    let end_va = VirtAddr::from(last);
    let restored: usize = end_va.into();
    if restored != last {
        return -1;
    }
    let start_vpn = start_va.floor();
    for page_id in 0..need {
        let vpn: VirtPageNum = (start_vpn.0 + page_id).into();
        let current_page_va = VirtAddr::from(start + PAGE_SIZE * page_id);
        let restored: usize = current_page_va.into();
        if restored != start + PAGE_SIZE * page_id {
            return -1;
        }
        match page_table.translate(vpn) {
            Some(pte) => {
                if !pte.is_valid() {
                    return -1;
                }
            }
            None => {
                return -1;
            }
        }
    }
    let current_task = current_task().unwrap();
    let mut inner = current_task.inner_exclusive_access();
    if inner
        .memory_set
        .unmap_range(start_va, VirtAddr::from(end_exclusive))
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

/// Implement spawn.
pub fn sys_spawn(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_spawn", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let data = app_inode.read_all();
        let parent_task = current_task().unwrap();
        let new_task = Arc::new(TaskControlBlock::new(data.as_slice()));
        {
            let mut parent_inner = parent_task.inner_exclusive_access();
            let mut new_inner = new_task.inner_exclusive_access();
            new_inner.parent = Some(Arc::downgrade(&parent_task));
            parent_inner.children.push(new_task.clone());
        }
        let new_pid = new_task.getpid();
        add_task(new_task.clone());
        new_pid as isize
    } else {
        -1
    }
}

// Set task priority.
pub fn sys_set_priority(prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority",
        current_task().unwrap().pid.0
    );
    if prio <= 1 {
        return -1;
    }
    let current_task = current_task().unwrap();
    let mut inner = current_task.inner_exclusive_access();
    inner.set_priority(prio as usize);
    prio
}
