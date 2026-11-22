//! 统计：完成工单、周期、各 Agent 成功率、Token 与费用。
use super::*;
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AgentStat {
    pub agent: String,
    pub runs: i64,
    pub succeeded: i64,
    pub failed: i64,
    pub stopped: i64,
    /// 不含缓存读取的输入。
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// 从缓存读取的输入（各 Agent 的解析器把它从 input_tokens 里单独拆出来了）。
    pub cache_tokens: i64,
    pub cost_usd: f64,
    /// Agent 运行耗时合计（秒），不含收尾。
    pub agent_seconds: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DayCost {
    pub day: String,
    pub runs: i64,
    pub cost_usd: f64,
    /// 输入 + 输出 + 缓存读取。
    pub tokens: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct WeekDone {
    /// 这一周的周一（UTC），`YYYY-MM-DD`。
    pub week: String,
    pub done: i64,
}

/// 统计里展示最近几周的完成数。
pub const WEEKS: i64 = 8;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Stats {
    pub tickets_total: i64,
    pub tickets_done: i64,
    /// 创建到完成的平均小时数。
    pub avg_cycle_hours: Option<f64>,
    pub runs_total: i64,
    pub cost_usd: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_tokens: i64,
    pub agents: Vec<AgentStat>,
    pub days: Vec<DayCost>,
    /// provider_id（空 = 官方默认）→ 费用与运行数
    pub providers: Vec<(Option<String>, i64, f64)>,
    /// 最近 [`WEEKS`] 周每周完成的工单数，旧的在前；没有完成的周也在里面。
    pub weeks: Vec<WeekDone>,
    /// 打回返工的运行次数。
    pub rework_runs: i64,
    /// 检查没过后自动修复的运行次数，以及其中修好了（检查通过）的次数。
    pub auto_fix_runs: i64,
    pub auto_fix_succeeded: i64,
    /// AI 交叉审查的次数与报出的问题数。
    pub code_reviews: i64,
    pub code_review_findings: i64,
}

pub fn compute(conn: &Connection) -> Result<Stats> {
    compute_at(conn, chrono::Utc::now().date_naive())
}

/// `today`：以哪一天为"本周"（测试里固定日期）。
pub fn compute_at(conn: &Connection, today: chrono::NaiveDate) -> Result<Stats> {
    use chrono::Datelike;
    let mut s = Stats::default();
    let monday = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    let first = monday - chrono::Duration::weeks(WEEKS - 1);
    // 'weekday 0' 取当天或之后最近的周日，再减 6 天就是所在周的周一
    let mut st = conn.prepare(
        "SELECT date(updated_at, 'weekday 0', '-6 days'), COUNT(*) FROM ticket
         WHERE status = 'done' AND date(updated_at) >= ?1 GROUP BY 1",
    )?;
    let done: std::collections::HashMap<String, i64> = st
        .query_map([first.to_string()], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                r.get(1)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    s.weeks = (0..WEEKS)
        .map(|i| {
            let week = (first + chrono::Duration::weeks(i)).to_string();
            WeekDone {
                done: done.get(&week).copied().unwrap_or(0),
                week,
            }
        })
        .collect();
    (s.rework_runs, s.auto_fix_runs, s.auto_fix_succeeded) = conn.query_row(
        "SELECT COALESCE(SUM(purpose = 'rework'), 0), COALESCE(SUM(purpose = 'test_fix'), 0),
                COALESCE(SUM(purpose = 'test_fix' AND status = 'succeeded' AND NOT EXISTS (
                  SELECT 1 FROM test_report t
                  WHERE t.run_id = run.id AND (t.exit_code IS NULL OR t.exit_code != 0))), 0)
         FROM run WHERE status != 'queued'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    (s.code_reviews, s.code_review_findings) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(json_array_length(findings_json)), 0) FROM code_review WHERE status = 'done'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    (s.tickets_total, s.tickets_done) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(status = 'done'), 0) FROM ticket",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    s.avg_cycle_hours = conn.query_row(
        "SELECT AVG((julianday(updated_at) - julianday(created_at)) * 24.0) FROM ticket WHERE status = 'done'",
        [],
        |r| r.get(0),
    )?;
    let mut st = conn.prepare(
        "SELECT agent, COUNT(*), COALESCE(SUM(status = 'succeeded'), 0), COALESCE(SUM(status IN ('failed', 'orphaned')), 0),
                COALESCE(SUM(status = 'stopped'), 0), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cost_usd), 0.0),
                COALESCE(SUM((julianday(COALESCE(agent_finished_at, finished_at)) - julianday(started_at)) * 86400.0), 0.0),
                COALESCE(SUM(cache_tokens), 0)
         FROM run WHERE status != 'queued' GROUP BY agent ORDER BY agent",
    )?;
    s.agents = st
        .query_map([], |r| {
            Ok(AgentStat {
                agent: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                runs: r.get(1)?,
                succeeded: r.get(2)?,
                failed: r.get(3)?,
                stopped: r.get(4)?,
                input_tokens: r.get(5)?,
                output_tokens: r.get(6)?,
                cost_usd: r.get(7)?,
                agent_seconds: r.get(8)?,
                cache_tokens: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for a in &s.agents {
        s.runs_total += a.runs;
        s.cost_usd += a.cost_usd;
        s.input_tokens += a.input_tokens;
        s.output_tokens += a.output_tokens;
        s.cache_tokens += a.cache_tokens;
    }
    let mut st = conn.prepare(
        "SELECT substr(started_at, 1, 10), COUNT(*), COALESCE(SUM(cost_usd), 0.0),
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0) + COALESCE(cache_tokens, 0)), 0)
         FROM run WHERE started_at IS NOT NULL GROUP BY 1 ORDER BY 1 DESC LIMIT 30",
    )?;
    s.days = st
        .query_map([], |r| {
            Ok(DayCost {
                day: r.get(0)?,
                runs: r.get(1)?,
                cost_usd: r.get(2)?,
                tokens: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    s.days.reverse();
    let mut st = conn.prepare(
        "SELECT provider_id, COUNT(*), COALESCE(SUM(cost_usd), 0.0) FROM run WHERE status != 'queued' GROUP BY 1 ORDER BY 2 DESC",
    )?;
    s.providers = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_totals_include_cached_input() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO run (id, agent, status, started_at, input_tokens, output_tokens, cache_tokens)
             VALUES ('r1', 'codex', 'succeeded', '2026-01-02T03:04:05Z', 5000, 800, 95000),
                    ('r2', 'codex', 'failed', '2026-01-02T05:00:00Z', 100, 10, NULL);",
        )
        .unwrap();
        let s = compute(&conn).unwrap();
        assert_eq!(
            (s.input_tokens, s.output_tokens, s.cache_tokens),
            (5100, 810, 95000)
        );
        assert_eq!(s.agents[0].cache_tokens, 95000);
        assert_eq!(s.days[0].tokens, 5100 + 810 + 95000);
    }

    #[test]
    fn weekly_done_counts_cover_the_last_eight_weeks_including_empty_ones() {
        let conn = crate::db::open_in_memory().unwrap();
        // 2026-01-14 是周三；这一周的周一是 01-12
        conn.execute_batch(
            "INSERT INTO ticket (id, key, title, type, status, created_at, updated_at) VALUES
               ('a', 'TW-1', 'a', 'bug', 'done', '2026-01-01T00:00:00Z', '2026-01-12T00:00:00Z'),
               ('b', 'TW-2', 'b', 'bug', 'done', '2026-01-01T00:00:00Z', '2026-01-11T23:59:59Z'),
               ('c', 'TW-3', 'c', 'bug', 'done', '2026-01-01T00:00:00Z', '2026-01-05T08:00:00Z'),
               ('d', 'TW-4', 'd', 'bug', 'developing', '2026-01-01T00:00:00Z', '2026-01-13T00:00:00Z'),
               ('e', 'TW-5', 'e', 'bug', 'done', '2025-01-01T00:00:00Z', '2025-06-01T00:00:00Z');
             INSERT INTO run (id, agent, status, purpose) VALUES
               ('r1', 'codex', 'succeeded', 'test_fix'), ('r2', 'codex', 'failed', 'test_fix'),
               ('r3', 'codex', 'succeeded', 'rework'), ('r4', 'codex', 'queued', 'rework'),
               ('r5', 'codex', 'succeeded', 'test_fix'), ('r6', 'codex', 'succeeded', 'test_fix');
             INSERT INTO test_report (id, run_id, command, exit_code) VALUES
               ('p1', 'r1', 'cargo test', 0), ('p2', 'r5', 'cargo test', 0),
               ('p3', 'r5', 'cargo clippy', 1), ('p4', 'r6', 'cargo test', NULL);
             INSERT INTO code_review (id, task_id, ticket_id, run_id, agent, status, findings_json, created_at, updated_at) VALUES
               ('c1', 't', 'a', 'r1', 'codex', 'done', '[{},{}]', '', ''),
               ('c2', 't', 'a', 'r2', 'codex', 'failed', '[]', '', '');",
        )
        .unwrap();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 1, 14).unwrap();
        let s = compute_at(&conn, today).unwrap();
        assert_eq!(s.weeks.len(), WEEKS as usize);
        assert_eq!(s.weeks[0].week, "2025-11-24");
        let tail: Vec<(&str, i64)> = s.weeks[5..]
            .iter()
            .map(|w| (w.week.as_str(), w.done))
            .collect();
        assert_eq!(
            tail,
            [("2025-12-29", 0), ("2026-01-05", 2), ("2026-01-12", 1)]
        );
        assert_eq!(
            (s.rework_runs, s.auto_fix_runs, s.auto_fix_succeeded),
            (1, 4, 1),
            "Agent 正常结束但检查还没过（r5）或检查没跑完（r6）的不算修好"
        );
        assert_eq!((s.code_reviews, s.code_review_findings), (1, 2));
    }
}
