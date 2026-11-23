import { tr } from './i18n';
// 合法流转（docs/02-workflow/workflow.md）。只用来给界面做提示，是否允许以后端为准。
import type { TicketStatus, TicketSummary } from './api';
import { STAGE_NAME } from './ui';

export const FLOW: Partial<Record<TicketStatus, Partial<Record<TicketStatus, string>>>> = {
  draft: { ai_review: tr('提交预审'), developing: tr('Bug 快速通道') },
  ai_review: { human_review: tr('停止预审') },
  human_review: { ai_review: tr('重新预审'), finalized: tr('定稿') },
  finalized: { developing: tr('开始开发'), human_review: tr('发起变更') },
  developing: { acceptance: tr('全部子任务完成'), human_review: tr('发起变更') },
  acceptance: { developing: tr('退回开发'), done: tr('完成工单') },
  done: {},
};

const WHY: Partial<Record<TicketStatus, string>> = {
  draft: tr('草稿需要先提交预审。'),
  ai_review: tr('请等待预审完成，或停止预审直接进入人工审核。'),
  human_review: tr('需要先定稿。'),
  finalized: tr('定稿后只能开始开发，或发起变更回到审核。'),
  developing: tr('需要全部子任务完成后进入验收。'),
  acceptance: tr('验收只能通过或打回。'),
  done: tr('已完成的工单不能再流转。'),
};

export function canMove(t: TicketSummary, to: TicketStatus): { ok: boolean; label?: string; why?: string } {
  const from = t.status;
  const label = FLOW[from]?.[to];
  if (!label) return { ok: false, why: tr('「{0}」不能直接到「{1}」。{2}', STAGE_NAME[from], STAGE_NAME[to], WHY[from] ?? '') };
  if (from === 'draft' && to === 'developing' && t.type !== 'bug')
    return { ok: false, why: tr('只有 Bug 可以走快速通道跳过 PRD 审核，需求工单请先提交预审。') };
  return { ok: true, label };
}
