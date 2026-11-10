use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::syscall::resize_on_demand;
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
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
        process_inner.locked_list[id] = None;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.locked_list.push(None);
        process_inner.mutex_list.len() as isize - 1
    }
}
/// mutex lock syscall
/// 持有、等待记录及环路检查参考 OpenAI Codex 的讨论与局部示例。
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let tid = task_inner.res.as_ref().unwrap().tid;
    drop(task_inner);
    drop(task);
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    if process_inner.enable_deadlock_detect && process_inner.has_mutex_cycle(tid, mutex_id) {
        return -0xDEAD;
    }
    resize_on_demand(&mut process_inner.waiting, tid);
    process_inner.waiting[tid] = Some(mutex_id);
    drop(process_inner);
    drop(process);
    mutex.lock();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.locked_list[mutex_id] = Some(tid);
    process_inner.waiting[tid] = None;
    drop(process_inner);
    0
}
/// mutex unlock syscall
/// 锁交接时的持有者记录参考 OpenAI Codex 的讨论。
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
    let waking_task = mutex.unlock();
    let mut process_inner = process.inner_exclusive_access();
    if let Some(task) = waking_task {
        let task_inner = task.inner_exclusive_access();
        let tid = task_inner.res.as_ref().unwrap().tid;
        drop(task_inner);
        process_inner.locked_list[mutex_id] = Some(tid);
        if let Some(waiting_mutex) = process_inner.waiting.get_mut(tid) {
            if *waiting_mutex == Some(mutex_id) {
                *waiting_mutex = None;
            }
        }
    } else {
        process_inner.locked_list[mutex_id] = None;
    }
    drop(process_inner);
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
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_list.len() - 1
    };
    id as isize
}
/// semaphore up syscall
/// 唤醒线程时的资源交接记录参考 OpenAI Codex 的讨论。
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let tid = task_inner.res.as_ref().unwrap().tid;
    drop(task_inner);
    drop(task);
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    if let Some(row) = process_inner.semaphore_allocation_list.get_mut(tid) {
        if let Some(count) = row.get_mut(sem_id) {
            if *count > 0 {
                *count -= 1;
            }
        }
    }
    drop(process_inner);
    let waiting_task = sem.up();
    if let Some(task) = waiting_task {
        let task_inner = task.inner_exclusive_access();
        let tid = task_inner.res.as_ref().unwrap().tid;
        drop(task_inner);
        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        if let Some(waiting_sem) = process_inner.semaphore_wait_list.get(tid) {
            if *waiting_sem == Some(sem_id) {
                process_inner.semaphore_allocation_list[tid][sem_id] += 1;
                process_inner.semaphore_wait_list[tid] = None;
            }
        }
    }
    0
}
/// semaphore down syscall
/// 等待、分配记录及检测失败回滚参考 OpenAI Codex 的讨论与局部示例。
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
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
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let tid = task_inner.res.as_ref().unwrap().tid;
    drop(task_inner);
    drop(task);
    resize_on_demand(&mut process_inner.semaphore_allocation_list, (tid, sem_id));
    resize_on_demand(&mut process_inner.semaphore_wait_list, tid);
    process_inner.semaphore_wait_list[tid] = Some(sem_id);
    if process_inner.enable_deadlock_detect && process_inner.has_semaphore_deadlock() {
        process_inner.semaphore_wait_list[tid] = None;
        return -0xDEAD;
    }
    drop(process_inner);
    drop(process);
    sem.down();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    if process_inner.semaphore_wait_list[tid] == Some(sem_id) {
        process_inner.semaphore_allocation_list[tid][sem_id] += 1;
        process_inner.semaphore_wait_list[tid] = None;
    }
    drop(process_inner);
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
/// 解锁时复用交接记录、重新加锁后更新持有者的思路参考 OpenAI Codex。
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
    sys_mutex_unlock(mutex_id);
    condvar.wait(mutex);
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let tid = task_inner.res.as_ref().unwrap().tid;
    drop(task_inner);
    let mut process_inner = process.inner_exclusive_access();
    process_inner.locked_list[mutex_id] = Some(tid);
    drop(process_inner);
    0
}
/// enable deadlock detection syscall
/// 进程级开关及参数处理参考 rCore-Tutorial-Guide CH8 与 OpenAI Codex 的讨论。
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect");
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    match enabled {
        0 => {
            process_inner.enable_deadlock_detect = false;
            0
        }
        1 => {
            process_inner.enable_deadlock_detect = true;
            0
        }
        _ => -1,
    }
}
