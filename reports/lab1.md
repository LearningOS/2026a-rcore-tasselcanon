# Lab 1 实验报告：多道程序与分时多任务

## 一、实现功能总结

- 实现了函数 `sys_trace`
- 当`trace_request`为**0**时，表示读取当前任务 id 地址处一个字节的无符号整数值。返回值为 id 地址处的值。
- 当`trace_request`为**1**时，则 id 被视作 *mut u8 ，表示写入 data 到该用户程序 id 地址处。返回值为0。
- 当`trace_request`为**2**时，对当前正在运行的每一个Task进行某种系统调用次数的统计，返回值为这个调用次数

## 二、简答作业

### 第 1 题：U 态的限制

运行三个 `ch2b_bad_*.rs` 测例，描述各程序的出错行为，并说明你使用的 SBI 实现及其版本。可使用你之前 `make -C os run BASE=2` 的实际日志填写。

本次运行命令：`make -C os run BASE=2`。

SBI 实现及版本：**RustSBI 0.4.1**。运行日志中显示 `RustSBI version 0.4.1`；仓库的 `bootloader/rustsbi-qemu.toml` 将该固件记录为 RustSBI Prototyper。

| 测例 | 出错行为及观察到的输出 |
| --- | --- |
| `ch2b_bad_address.rs` | 源码尝试向地址 `0x0` 写入一个字节；日志输出 `PageFault in application`，错误地址为 `0x0`，出错指令地址为 `0x804003a4`，内核终止该应用。 |
| `ch2b_bad_instructions.rs` | 源码尝试执行 `sret`；本次日志中出现 `IllegalInstruction in application, kernel killed it.`。 |
| `ch2b_bad_register.rs` | 源码尝试读取 `sstatus`；本次日志中出现 `IllegalInstruction in application, kernel killed it.`。 |

日志中的两条 `IllegalInstruction` 信息相同，没有显示应用名称；表中对应的程序行为依据各测例源码确定。地址错误的日志统一使用 `PageFault` 文案；当前内核对 `StoreFault` 和 `StorePageFault` 使用同一文案，仅凭这条日志不能确定具体的硬件异常类型。

### 第 2 题：Trap 的保存与恢复

请对照当前分支的 `os/src/trap/trap.S` 和[官网题目](https://learningos.cn/rCore-Tutorial-Guide/chapter3/5exercise.html)作答。题目中的行号用于定位；如文件发生变化，请以对应指令为准。

#### 2.1 `__restore` 的入口和使用情景

刚进入 `__restore` 时，`sp` 指向什么？这个恢复过程有哪两种使用情景？

`sp`指向内核栈上`TrapContext`起始地址
1. 首次启动用户程序
2. 用户程序发生 Trap，内核处理完毕，需要恢复用户程序。

#### 2.2 特殊寄存器的恢复

查看 `__restore` 中第 43～48 行对寄存器的特殊处理：代码涉及哪些寄存器？分别说明这些值对进入用户态的意义。

代码设计的寄存器: 
- `sstatus`: SPP的值决定返回特权级相关状态。
- `sepc`: 依照其确定`sret`返回位置。
- `sscratch`: 后续与`sp`交换，恢复用户栈

#### 2.3 跳过的寄存器

恢复通用寄存器时，为什么没有在这组加载指令中直接恢复 `x2` 和 `x4`？

x2是栈指针sp，当前SP还需要指向内核栈上的TrapContext，所以不能提前恢复用户栈指针。
x4是线程指针tp，目前暂未使用。

#### 2.4 `__restore` 末尾的栈指针交换

`__restore` 中执行 `csrrw sp, sscratch, sp` 后，`sp` 和 `sscratch` 各表示什么？

sp是保存或恢复的用户栈指针，sscratch是内核栈栈顶。

#### 2.5 从内核返回用户态

`__restore` 中通过哪条指令完成特权级转换？为什么执行后会进入 U 态？

`sret`
因为 `sret` 会根据 `sstatus.SPP` 决定返回的特权级，而在本实验中，内核会将准备返回用户态的TrapContext的SPP字段设为User，也就是0。

#### 2.6 `__alltraps` 入口的栈指针交换

`__alltraps` 入口执行 `csrrw sp, sscratch, sp` 后，`sp` 和 `sscratch` 各表示什么？

sp是内核栈栈顶，sscratch是保存或恢复的用户栈指针。

#### 2.7 从用户态进入内核

官网最后一问涉及从 U 态进入 S 态的触发指令。请结合**系统调用场景**，说明用户程序执行了什么指令来触发这次转换。

在进行系统调用的时候，会使用封装好的`syscall`函数，内嵌了汇编`"ecall"`，触发环境调用异常后，CPU通过硬件Trap机制进入S模式，跳转到stvec指定的入口。

## 三、荣誉准则

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 以下各位 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容： `无`

2. 此外，我也参考了 以下资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：
- rCore-Tutorial-Guide：学习 CH3 内容，查看实验和报告要求。
- 本人作业仓库中的课程框架和测试程序：阅读任务管理、Trap 和系统调用实现，运行测试。
- OpenAI Codex / ChatGPT：辅助理解任务切换、Trap 和系统调用，讨论 sys_trace 的设计，参考过局部代码片段和接口骨架建议，并用于分析编译问题、整理运行日志和核对报告。本人在本地操作代码修改和测试。

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
