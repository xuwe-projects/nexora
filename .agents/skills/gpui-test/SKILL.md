---
name: gpui-test
description: >-
  用于在 Xuwe workspace 中编写、调试或复现确定性 GPUI 测试，包括 gpui::test 参数、
  TestAppContext、VisualTestContext、调度器种子、ITERATIONS/SEED、禁止停驻失败和待处理任务追踪。
---

# GPUI 测试调试

当用户询问 `#[gpui::test]`、GPUI 测试种子或迭代次数、确定性调度器故障、停驻或待处理任务故障，或者如何复现不稳定的 GPUI 测试时，使用此技能。

本 Skill 负责基于模拟平台的确定性行为测试：无窗口状态使用 `TestAppContext`，模拟窗口交互使用 `VisualTestContext`。真实平台像素输出由专用视觉测试 runner 和 `VisualTestAppContext` 负责；不要把两类上下文混用，也不要用截图测试替代可确定复现的状态与交互测试。

## `#[gpui::test]` 的作用

`#[gpui::test]` 会展开为普通的 Rust `#[test]`，因此可以通过 `cargo test` 和 `cargo nextest` 等标准 Rust 测试运行器执行。

它会用 GPUI 的确定性测试分发器和调度器包装测试体，并可使用不同种子多次运行同一测试。种子控制调度器中的任务交错顺序，以及注入测试的所有 `StdRng` 参数。

该宏同时支持同步和异步测试。

### 支持的函数参数

该宏按类型名称识别参数：

| 测试类型 | 支持的参数 |
| --- | --- |
| 同步和异步 | `&TestAppContext`、`&mut TestAppContext`、`StdRng` |
| 仅异步 | `BackgroundExecutor` |
| 仅同步 | `&App`、`&mut App` |

`StdRng` 使用当前 GPUI 测试种子初始化，`BackgroundExecutor` 由同一个确定性测试分发器驱动。

### 属性参数

在 `#[gpui::test(arguments)]` 中使用以下形式：

- 不传参数：使用种子 `0` 运行一次，除非设置了 `SEED`。
- `seed = N`：添加一个显式种子。
- `seeds(...)`：添加多个显式种子。
- `iterations = N`：默认从 `0` 开始按连续种子运行。
- `retries = N`：失败后最多重试 `N` 次，全部重试仍失败才报告故障。
- `on_failure = "path::to::function"`：最终失败后、恢复 panic 前调用指定函数。
- `iterations` 可以与显式 `seed` 或 `seeds` 组合；显式种子会追加到 `0..iterations` 范围之后。
- 如果设置了 `SEED` 环境变量，它会优先于显式种子。
- 同时使用 `SEED=N` 与 `ITERATIONS=M` 或 `iterations = M` 时，测试工具会运行种子 `N..N+M`。

## 环境变量

### GPUI 测试宏和调度器执行

- `SEED=<u64>`：选择调度器种子。使用它复现输出为 `failing seed: N` 的故障。它也会用于初始化注入的 `StdRng` 参数。对于 `#[gpui::property_test]`，它控制调度器种子，GPUI 还会将其应用到 proptest 配置，从而确定性地生成测试用例。
- `ITERATIONS=<usize>`：在运行时覆盖 `iterations = ...` 的值。使用它扫描大量种子，无需修改测试代码。
- `PENDING_TRACES=1` 或 `PENDING_TRACES=true`：当测试调度器因 `Parking forbidden` 而 panic 时，捕获并打印待处理任务追踪。当 `run_until_parked()` 或清理阶段报告仍有待处理工作时使用。
- `GPUI_RUN_UNTIL_PARKED_LOG=1`：在启用 `allow_parking()` 时记录日志。使用它查找明确允许停驻或保留待处理工作的测试。
- `DEBUG_SCHEDULER=1`：打印 `scheduler::TestScheduler` 的调度器时钟和计时器调试信息。

### 底层调度器测试

- `SCHEDULER_NONINTERACTIVE=1`：禁止 `scheduler::TestScheduler::many` 输出交互式种子进度。该变量不影响 `#[gpui::test]` 测试工具路径。

### 调试 GPUI 测试时常用的通用 Rust 环境变量

- `RUST_BACKTRACE=1` 或 `RUST_BACKTRACE=full`：显示 panic 回溯。
- `RUST_LOG=<filter>`：当测试已初始化日志系统时启用日志。
- `ZED_HEADLESS=1`：GPUI 上游保留的环境变量名，用于让平台推断倾向无头模式；名称来自上游，但在 Xuwe 测试中仍按该原名设置。

缩小复现范围时，优先使用环境变量，不要修改测试代码。

## 复现指定的 GPUI 测试

1. 确定 crate、package 和测试名称。

2. 先使用最小范围的测试过滤条件；如果已知失败种子，直接跳到第 3 步。

   ```sh
   cargo -q test -p <crate-name> <test_name> -- --nocapture
   ```

3. 如果故障信息包含种子，使用该种子精确重跑。

   ```sh
   SEED=<seed> cargo -q test -p <crate-name> <test_name> -- --nocapture
   ```

4. 如果故障不稳定且种子未知，扫描多个种子。

   ```sh
   ITERATIONS=100 cargo -q test -p <crate-name> <test_name> -- --nocapture
   ```

   测试工具输出 `failing seed: <seed>` 后，后续调试都改用 `SEED=<seed>`。

5. 如果故障为 `Parking forbidden`，启用待处理任务追踪后重跑。

   ```sh
   PENDING_TRACES=1 cargo -q test -p <crate-name> <test_name> -- --nocapture
   ```

   如果已经输出或已知失败种子，同时传入该种子：

   ```sh
   SEED=<seed> PENDING_TRACES=1 cargo -q test -p <crate-name> <test_name> -- --nocapture
   ```

   检查待处理任务追踪，找出已生成但未等待、未分离、未完成，或者未明确允许停驻的任务。

6. 如果问题与时序或计时器推进有关，在测试中优先使用 GPUI 调度器的计时器：

   ```rust
   cx.background_executor().timer(duration).await;
   ```

   依赖 `run_until_parked()` 的 GPUI 测试中避免使用 `smol::Timer::after(...)`，因为 GPUI 调度器可能无法追踪它。

7. 最小化复现用例。

   - 固定失败的 `SEED`。
   - 已知种子后，将 `ITERATIONS` 减少为 `1` 或移除。
   - 只有确认同一种子仍会失败后，才移除无关的初始化代码。
   - 保留对调度敏感的 await 或 yield；移除它们可能掩盖缺陷。
   - 如果测试通过 `StdRng` 控制随机性，固定调度器种子后记录或断言生成的场景。

8. 验证修复。

   - 运行已固定的失败种子。
   - 如果故障对调度顺序敏感，执行适量的种子扫描，例如 `ITERATIONS=20`。
   - 如果修改的代码具有共享行为，运行相关 crate 的测试过滤项或更大范围的测试套件。

## 常见诊断模式

### 与种子相关的断言失败

通常由调度器任务交错顺序或 `StdRng` 驱动的测试数据引起。固定 `SEED` 并复现，然后检查发生变化的任务或生成场景。

### `Parking forbidden`

这通常表示调度器预期测试继续推进或结束时，仍有前台或后台任务处于待处理状态。检查：

- 本应等待但被丢弃的任务。
- 本应分离并记录错误但未处理的任务。
- 永久等待的计时器或接收器。
- 触发异步工作后缺少 `cx.run_until_parked()`。
- 等待防抖任务时缺少 `cx.advance_clock(...)`。
- 使用了测试调度器无法驱动的非 GPUI 计时器或执行器。

修改代码前，先使用 `PENDING_TRACES=1` 重跑。

### 非确定性或线程错误

调度器可能报告来自非预期线程的活动。检查是否有工作逃离 GPUI 的前台或后台执行器、直接生成线程，或者使用了不受测试分发器控制的外部异步运行时。

### 单独运行通过但批量扫描失败

使用扫描输出的失败种子。除非测试运行器明确串行执行，否则不要假设测试顺序。检查全局状态、泄漏的 entity 或任务，以及测试初始化过程中未重置的状态。

## 编写 GPUI 测试

- 需要 `TestAppContext`、确定性执行器、模拟时间或调度器交错覆盖时，优先使用 `#[gpui::test]`。
- 需要模拟窗口的事件、焦点、布局或绘制流程时，从测试窗口创建 `VisualTestContext`；只有真实平台截图/像素测试才使用 `VisualTestAppContext`。
- 测试需要有意检查任务交错时，添加 `iterations = N`。
- 随机测试数据需要与调度器使用同一种子时，将 `StdRng` 用作测试参数。
- 在 GPUI 测试中使用 `cx.background_executor().timer(duration).await` 实现延迟或超时。
- 修复测试时不要添加或增加 `retries`，除非用户明确要求，或者现有测试已说明为何需要概率容错。重试可能掩盖故障，而不是修复故障。
