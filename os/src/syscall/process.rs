//! Process management syscalls
use core::mem::size_of;

use crate::{
    config::PAGE_SIZE,
    mm::{translated_byte_buffer, MapPermission, PTEFlags, PageTable, VirtAddr, VirtPageNum},
    task::{
        change_program_brk, current_user_token, exit_current_and_run_next, get_syscall_count,
        map_current_area, suspend_current_and_run_next, unmap_current_area,
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

pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 => {
            let token = current_user_token();
            let page_table = PageTable::from_token(token);
            let virt_addr = VirtAddr::from(id);
            let restored: usize = virt_addr.into();
            if restored != id {
                return -1;
            }
            let vpn = virt_addr.floor();
            match page_table.translate(vpn) {
                Some(pte) => {
                    let flags = PTEFlags::V | PTEFlags::U | PTEFlags::R;
                    if pte.flags().contains(flags) {
                        let ppn = pte.ppn();
                        let bytes_arr = ppn.get_bytes_array();
                        let offset = virt_addr.page_offset();
                        return bytes_arr[offset] as isize;
                    } else {
                        return -1;
                    }
                }
                None => return -1,
            }
        }
        1 => {
            let token = current_user_token();
            let page_table = PageTable::from_token(token);
            let virt_addr = VirtAddr::from(id);
            let restored: usize = virt_addr.into();
            if restored != id {
                return -1;
            }
            let vpn = virt_addr.floor();
            match page_table.translate(vpn) {
                Some(pte) => {
                    let flags = PTEFlags::V | PTEFlags::U | PTEFlags::W;
                    if pte.flags().contains(flags) {
                        let ppn = pte.ppn();
                        let bytes_arr = ppn.get_bytes_array();
                        let offset = virt_addr.page_offset();
                        bytes_arr[offset] = data as u8;
                        0
                    } else {
                        return -1;
                    }
                }
                None => return -1,
            }
        }
        2 => get_syscall_count(id) as isize,
        _ => -1,
    }
}

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
    map_current_area(start_va, VirtAddr::from(end_exclusive), map_perm);
    0
}

pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
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
    if unmap_current_area(start_va, VirtAddr::from(end_exclusive)) {
        0
    } else {
        -1
    }
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
