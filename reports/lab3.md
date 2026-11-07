# Lab 3：进程及进程管理

## 一、实现功能总结

适配进程地址空间下的获取时间、内存映射和解除映射。实现 spawn，按应用名创建子进程并建立父子关系；实现优先级设置和 stride 调度，根据进程优先级调整调度机会。

## 二、测试记录

- 执行 `make -C os run BASE=2`，在用户 shell 中运行 `ch5_usertest`，输出 `ch5 Usertests passed!`。
- `ch5_spawn0`、`ch5_spawn1`、`ch5_setprio` 和 `ch5_stride` 均以退出码 0 结束。
- 该次 stride 测试中，最大 `ratio` 为 3,786,360，最小为 3,650,250，最大值除以最小值约为 1.04，小于题目要求的 1.5。
- `ch4_mmap1`、`ch4_mmap2` 是预期触发非法访问的测例，截图中退出码为 -2。

## 三、问答作业：stride 算法

### 1. stride 溢出后的调度

两个进程的 `pass` 都为 10，stride 使用 8 位无符号整数保存。初始时 `p1.stride = 255`、`p2.stride = 250`，p2 执行一个时间片后：使用普通无符号数比较，下一次会选择 p1 吗？为什么？

**回答：**

P2 执行一个时间片后，250 加上 pass 的 10 等于 260。而 `u8` 表示的范围是 0～255，假设加法发生溢出回绕，最终的值为 `260 % 256 == 4`。由于 P2 的值等于 4，小于 P1 的值，所以会再次让 P2 继续运行。

### 2. 最大 stride 与最小 stride 的差

在不考虑溢出、所有进程优先级不小于 2，且严格按照 stride 算法调度的前提下，为什么 `STRIDE_MAX - STRIDE_MIN <= BigStride / 2`？

**回答：**

- 假设参与比较的是一组固定的可运行进程，初始 stride 相同，且期间严格按这个规则调度。
- 由于 `pass = BigStride / priority`，且 `priority >= 2`，所以必然有 `pass <= BigStride / 2`。
- 因为每次调度时，总是选择 `stride` 最小的进程运行，而每次增加的 `pass` 不会超过 `BigStride / 2`。所以，即使最小的 `stride` 增加后变成了最大的，它也不会超过原来最小值加上 `BigStride / 2`。因此，只要初始时所有进程的 `stride` 相同，之后最大值与最小值的差距就始终不会超过 `BigStride / 2`。

### 3. 考虑回绕的比较器

结合上一问的结论，为 `Stride(u64)` 实现考虑回绕的比较，并说明如何用它从 `BinaryHeap` 中取出逻辑上 stride 最小的进程。假设参与比较的两个 stride 不相等。

8 位例子：`BigStride = 255`，期望 `(125 < 255) == false`，`(129 < 255) == true`。

**思路：**

- 可以使用 `wrapping_sub()` 计算两个 stride 的环绕差值。根据第二小题的结论，两个进程的实际 stride 差距不会超过 `BigStride / 2`，因此可以通过判断差值是否超过整数范围的一半，确定哪个进程的 stride 更小。
- Rust 的 `BinaryHeap` 默认弹出比较结果最大的元素；若希望弹出逻辑上最小的 stride，需要使用 `Reverse` 等方式反转排序。实际用于堆时还需实现一致的 `Ord` 和 `Eq`。
- 比较器要求两个 stride 的逻辑差距严格小于整数范围的一半；恰好相差半圈时无法判断先后。

**代码：**

```rust
use core::cmp::Ordering;

struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let diff = self.0.wrapping_sub(other.0);
        let half = 1u64 << 63;

        if diff > half {
            Some(Ordering::Less)
        } else {
            Some(Ordering::Greater)
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}
```

上面的 `PartialEq` 沿用题目框架中两个 stride 不相等的假设。实际使用时，应正确处理相等值，并使各比较接口保持一致。

## 四、荣誉准则

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与以下各位就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

无

2. 此外，我也参考了以下资料，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

- rCore Tutorial Book 与 rCore-Tutorial-Guide：学习进程管理、系统调用与调度，并查看 CH5 实验和报告要求。
- 本人作业仓库中的课程框架、CH4 实现和测试程序：参考 `fork`、`exec`、任务管理及先前系统调用实现，并运行相关测试。
- OpenAI Codex / ChatGPT：协助梳理 CH5 数据结构，讨论已有系统调用的迁移、`spawn` 的实现和 stride 调度，提供过局部代码示例，协助检查代码，并起草报告功能总结和测试记录。本人在本地修改代码并运行测试。

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
