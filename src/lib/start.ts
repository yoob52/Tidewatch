import { api, type Run } from './api';
import { useData } from './data';
import { useUi } from './ui';

/** 开始 / 恢复一个子任务，所有入口共用。full_auto 每次都要二次确认。 */
export function useStartTask(reload?: () => Promise<unknown>) {
  const ui = useUi();
  const data = useData();
  return async (task: { id: string }, instruction?: string): Promise<Run | undefined> => {
    const plan = await ui.attempt(api.taskPlan(task.id, { instruction }));
    if (!plan) return undefined;
    if (
      plan.permission === 'full_auto' &&
      !(await ui.confirm('以 full_auto 运行', 'Agent 将不经确认执行任意命令。建议只在可丢弃的 worktree 中使用。', '我了解风险，开始', true))
    )
      return undefined;
    const run = await ui.attempt(api.taskStart(task.id, { instruction, command_id: crypto.randomUUID() }));
    if (run) {
      ui.toast(plan.resume ? `已入队，将续接会话 ${plan.resume[1].slice(0, 8)}` : '已入队，有空闲并发时自动开始');
      await Promise.all([reload?.(), data.reload()]);
    }
    return run;
  };
}

/** 默认权限是 full_auto 时，AI 预审 / 拆分 / 影响分析和跟随默认的子任务都以 full_auto 运行：发起前确认一次。 */
export function useConfirmTicketRun() {
  const ui = useUi();
  const data = useData();
  return async (): Promise<boolean> =>
    data.settings.default_permission !== 'full_auto' ||
    ui.confirm('以 full_auto 运行', '默认权限是 full_auto：这次 Agent 运行会不经确认执行任意命令。', '我了解风险，开始', true);
}
