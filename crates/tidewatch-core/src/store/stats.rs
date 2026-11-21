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
}

pub fn compute(conn: &Connection) -> Result<Stats> {
    let mut s = Stats::default();
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
}
