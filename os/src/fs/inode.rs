//! `Arc<Inode>` -> `OSInodeInner`: In order to open files concurrently
//! we need to wrap `Inode` into `Arc`,but `Mutex` in `Inode` prevents
//! file systems from being accessed simultaneously
//!
//! `UPSafeCell<OSInodeInner>` -> `OSInode`: for static `ROOT_INODE`,we
//! need to wrap `OSInodeInner` into `UPSafeCell`
use super::File;
use crate::drivers::BLOCK_DEVICE;
use crate::mm::UserBuffer;
use crate::sync::UPSafeCell;
use alloc::vec::Vec;
use alloc::{collections::BTreeMap, sync::Arc};
use bitflags::*;
use easy_fs::{EasyFileSystem, Inode};
use lazy_static::*;

/// inode in memory
/// A wrapper around a filesystem inode
/// to implement File trait atop
pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: UPSafeCell<OSInodeInner>,
}
/// The OS inode inner in 'UPSafeCell'
pub struct OSInodeInner {
    offset: usize,
    inode: Arc<Inode>,
}

impl OSInode {
    /// create a new inode in memory
    pub fn new(readable: bool, writable: bool, inode: Arc<Inode>) -> Self {
        let mut inner = OPEN_INODE_COUNTS.exclusive_access();
        let inode_id = inode.inode_id();
        *inner.entry(inode_id).or_insert(0) += 1;
        drop(inner);
        Self {
            readable,
            writable,
            inner: unsafe { UPSafeCell::new(OSInodeInner { offset: 0, inode }) },
        }
    }
    /// read all data from the inode
    pub fn read_all(&self) -> Vec<u8> {
        let mut inner = self.inner.exclusive_access();
        let mut buffer: Vec<u8> = Vec::with_capacity(512);
        buffer.resize(512, 0);
        let mut v: Vec<u8> = Vec::new();
        loop {
            let len = inner.inode.read_at(inner.offset, &mut buffer);
            if len == 0 {
                break;
            }
            inner.offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
        v
    }
}
impl Drop for OSInode {
    fn drop(&mut self) {
        let inner = self.inner.exclusive_access();
        let inode_id = inner.inode.inode_id();
        drop(inner);
        let mut inner = OPEN_INODE_COUNTS.exclusive_access();
        let count = inner.get_mut(&inode_id).expect("open inode count missing");
        assert!(*count > 0);
        *count -= 1;
        if *count == 0 {
            inner.remove(&inode_id);
        }
        drop(inner);
        try_reclaim_inode(inode_id);
    }
}
lazy_static! {
    pub static ref ROOT_INODE: Arc<Inode> = {
        let efs = EasyFileSystem::open(BLOCK_DEVICE.clone());
        Arc::new(EasyFileSystem::root_inode(&efs))
    };
}

lazy_static! {
    static ref OPEN_INODE_COUNTS: UPSafeCell<BTreeMap<u32, usize>> = {
        let map = BTreeMap::new();
        unsafe { UPSafeCell::new(map) }
    };
}

/// Try to reclaim an unlinked inode and its blocks
fn try_reclaim_inode(inode_id: u32) {
    let count_links = count_links(inode_id);
    let inner = OPEN_INODE_COUNTS.exclusive_access();
    let open_count = match inner.get(&inode_id) {
        Some(count) => *count,
        None => 0,
    };
    drop(inner);
    if open_count == 0 && count_links == 0 {
        ROOT_INODE.reclaim_inode(inode_id)
    }
}
/// List all apps in the root directory
pub fn list_apps() {
    println!("/**** APPS ****");
    for app in ROOT_INODE.ls() {
        println!("{}", app);
    }
    println!("**************/");
}

bitflags! {
    ///  The flags argument to the open() system call is constructed by ORing together zero or more of the following values:
    pub struct OpenFlags: u32 {
        /// readyonly
        const RDONLY = 0;
        /// writeonly
        const WRONLY = 1 << 0;
        /// read and write
        const RDWR = 1 << 1;
        /// create new file
        const CREATE = 1 << 9;
        /// truncate file size to 0
        const TRUNC = 1 << 10;
    }
}

impl OpenFlags {
    /// Do not check validity for simplicity
    /// Return (readable, writable)
    pub fn read_write(&self) -> (bool, bool) {
        if self.is_empty() {
            (true, false)
        } else if self.contains(Self::WRONLY) {
            (false, true)
        } else {
            (true, true)
        }
    }
}
/// Link 2 files
pub fn link_file(old_name: &str, new_name: &str) -> bool {
    ROOT_INODE.link(old_name, new_name)
}
/// Count directory entries referencing an inode.
pub fn count_links(inode_id: u32) -> usize {
    ROOT_INODE.count_links(inode_id)
}
/// Unlink file
pub fn unlink_file(name: &str) -> bool {
    match ROOT_INODE.unlink(name) {
        Some(inode_id) => {
            try_reclaim_inode(inode_id);
            true
        }
        None => false,
    }
}
/// Open a file
pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    let (readable, writable) = flags.read_write();
    if flags.contains(OpenFlags::CREATE) {
        if let Some(inode) = ROOT_INODE.find(name) {
            // clear size
            inode.clear();
            Some(Arc::new(OSInode::new(readable, writable, inode)))
        } else {
            // create file
            ROOT_INODE
                .create(name)
                .map(|inode| Arc::new(OSInode::new(readable, writable, inode)))
        }
    } else {
        ROOT_INODE.find(name).map(|inode| {
            if flags.contains(OpenFlags::TRUNC) {
                inode.clear();
            }
            Arc::new(OSInode::new(readable, writable, inode))
        })
    }
}

impl File for OSInode {
    fn readable(&self) -> bool {
        self.readable
    }
    fn writable(&self) -> bool {
        self.writable
    }
    fn read(&self, mut buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_read_size = 0usize;
        for slice in buf.buffers.iter_mut() {
            let read_size = inner.inode.read_at(inner.offset, *slice);
            if read_size == 0 {
                break;
            }
            inner.offset += read_size;
            total_read_size += read_size;
        }
        total_read_size
    }
    fn write(&self, buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_write_size = 0usize;
        for slice in buf.buffers.iter() {
            let write_size = inner.inode.write_at(inner.offset, *slice);
            assert_eq!(write_size, slice.len());
            inner.offset += write_size;
            total_write_size += write_size;
        }
        total_write_size
    }
    fn inode_id(&self) -> Option<u32> {
        let inner = self.inner.exclusive_access();
        Some(inner.inode.inode_id())
    }
}
