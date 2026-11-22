//! 备份与导出：数据库每日备份（保留 7 份），工单导出成 Markdown 归档（带附件）。
//! 密钥不在数据库里（只存凭据管理器的引用），所以备份和导出都不含密钥。
use crate::store::{attachments, tasks, tickets};
use crate::{workflow, Error, Result};
use chrono::Local;
use rusqlite::Connection;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// 每日备份保留的份数。
pub const KEEP: usize = 7;
const PREFIX: &str = "tidewatch-";

#[derive(Debug, Clone, Serialize)]
pub struct BackupFile {
    pub name: String,
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Exported {
    pub dir: String,
    pub tickets: usize,
    pub attachments: usize,
    /// 数据库里有记录、但文件已经不在的附件。
    pub missing: Vec<String>,
}

pub fn backup_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

/// 把数据库完整复制到 `dest`（`VACUUM INTO`：一致的快照，不受 WAL 影响）。
pub fn snapshot(conn: &Connection, dest: &Path) -> Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if dest.exists() {
        std::fs::remove_file(dest)?;
    }
    let path = dest
        .to_str()
        .ok_or_else(|| Error::invalid("备份路径不是合法的 UTF-8。"))?;
    conn.execute("VACUUM INTO ?1", [path])?;
    Ok(())
}

/// 今天还没备份过就备份一次，然后只留最近 [`KEEP`] 份。已经备份过返回 `None`。
pub fn daily(conn: &Connection, data_dir: &Path) -> Result<Option<PathBuf>> {
    // 今天手动备份过也算：每日那份的文件名没有时间，排序时当作当天最早的一份
    let today = format!("{PREFIX}{}", Local::now().format("%Y-%m-%d"));
    if list(data_dir).iter().any(|b| b.name.starts_with(&today)) {
        return Ok(None);
    }
    let dest = backup_dir(data_dir).join(format!("{today}.db"));
    snapshot(conn, &dest)?;
    prune(data_dir)?;
    Ok(Some(dest))
}

/// 立刻备份一份（不受"每天一份"限制），同样只留最近 [`KEEP`] 份。
pub fn now(conn: &Connection, data_dir: &Path) -> Result<PathBuf> {
    let dest = backup_dir(data_dir).join(format!(
        "{PREFIX}{}.db",
        Local::now().format("%Y-%m-%d-%H%M%S")
    ));
    snapshot(conn, &dest)?;
    prune(data_dir)?;
    Ok(dest)
}

/// 已有的备份，新的在前。
pub fn list(data_dir: &Path) -> Vec<BackupFile> {
    let mut out: Vec<BackupFile> = std::fs::read_dir(backup_dir(data_dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            (name.starts_with(PREFIX) && name.ends_with(".db")).then(|| BackupFile {
                path: e.path().display().to_string(),
                bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
                name,
            })
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(order_key(&b.name)));
    out
}

/// 排序用：文件名里的日期和时间。每日备份（`tidewatch-2026-01-14.db`）没有时间，算当天 0 点，
/// 否则直接比文件名的话 `.` 排在 `-` 后面，当天稍后手动备份的那份反而会被当成更旧的删掉。
fn order_key(name: &str) -> String {
    let stem = name.trim_end_matches(".db");
    if stem.len() == PREFIX.len() + "2026-01-14".len() {
        format!("{stem}-000000")
    } else {
        stem.to_string()
    }
}

fn prune(data_dir: &Path) -> Result<()> {
    for old in list(data_dir).into_iter().skip(KEEP) {
        std::fs::remove_file(&old.path)?;
    }
    Ok(())
}

/// 把全部工单导出到 `dest` 下的一个新目录：每个工单一个文件夹（`ticket.md`、各版本 PRD、附件），
/// 外加一份数据库快照和索引。
pub fn export(conn: &Connection, workspace: &Path, dest: &Path) -> Result<Exported> {
    if !dest.is_dir() {
        return Err(Error::invalid(format!(
            "导出目录不存在：{}",
            dest.display()
        )));
    }
    let root = dest.join(format!(
        "tidewatch-export-{}",
        Local::now().format("%Y%m%d-%H%M%S")
    ));
    std::fs::create_dir_all(root.join("tickets"))?;
    snapshot(conn, &root.join("tidewatch.db"))?;

    let all = tickets::list(conn, None)?;
    let mut index = String::from("# 观澜工单归档\n\n| 工单 | 标题 | 类型 | 状态 | 更新时间 |\n| --- | --- | --- | --- | --- |\n");
    let mut copied = 0;
    let mut missing = vec![];
    for t in &all {
        let dir = root.join("tickets").join(&t.key);
        std::fs::create_dir_all(&dir)?;
        let atts = attachments::list_for_ticket(conn, &t.id)?;
        for a in &atts {
            let from = workspace.join(&a.path);
            if !from.is_file() {
                missing.push(format!("{} {}", t.key, a.file_name));
                continue;
            }
            std::fs::create_dir_all(dir.join("attachments"))?;
            std::fs::copy(&from, dir.join("attachments").join(disk_name(a)))?;
            copied += 1;
        }
        let versions = tickets::prd_versions(conn, &t.id)?;
        for v in &versions {
            std::fs::write(
                dir.join(format!("prd-v{}.md", v.version)),
                portable(&v.content_md, &atts),
            )?;
        }
        std::fs::write(
            dir.join("ticket.md"),
            ticket_markdown(conn, t, &versions, &atts)?,
        )?;
        index.push_str(&format!(
            "| [{key}](tickets/{key}/ticket.md) | {} | {} | {} | {} |\n",
            cell(&t.title),
            t.kind,
            workflow::ticket_status_label(&t.status),
            t.updated_at,
            key = t.key,
        ));
    }
    index.push_str(
        "\n`tidewatch.db` 是导出时的数据库快照。密钥不在数据库里，这份归档不含任何密钥。\n",
    );
    std::fs::write(root.join("README.md"), index)?;
    Ok(Exported {
        dir: root.display().to_string(),
        tickets: all.len(),
        attachments: copied,
        missing,
    })
}

fn disk_name(a: &attachments::Attachment) -> String {
    a.path.rsplit('/').next().unwrap_or(&a.id).to_string()
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// 把 `attachment:<id>` 换成归档里的相对路径，效力标记（标题位）原样保留，任何 Markdown 工具都能读。
fn portable(md: &str, atts: &[attachments::Attachment]) -> String {
    let mut out = String::with_capacity(md.len());
    let mut at = 0;
    for img in attachments::parse_images(md) {
        let Some(a) = atts.iter().find(|a| a.id == img.attachment_id) else {
            continue;
        };
        let (start, end) = img.span;
        let whole = &md[start..end];
        let needle = format!("(attachment:{}", img.attachment_id);
        out.push_str(&md[at..start]);
        out.push_str(&whole.replacen(&needle, &format!("(attachments/{}", disk_name(a)), 1));
        at = end;
    }
    out.push_str(&md[at..]);
    out
}

fn ticket_markdown(
    conn: &Connection,
    t: &tickets::Ticket,
    versions: &[tickets::PrdVersion],
    atts: &[attachments::Attachment],
) -> Result<String> {
    let mut s = format!("# {} {}\n\n", t.key, t.title);
    s.push_str(&format!(
        "- 类型：{}\n- 状态：{}\n- 优先级：{}\n",
        t.kind,
        workflow::ticket_status_label(&t.status),
        t.priority
    ));
    if !t.labels.is_empty() {
        s.push_str(&format!("- 标签：{}\n", t.labels.join("、")));
    }
    if let Some(d) = &t.due_date {
        s.push_str(&format!("- 截止：{d}\n"));
    }
    s.push_str(&format!(
        "- 创建：{}\n- 更新：{}\n\n",
        t.created_at, t.updated_at
    ));

    let shown = versions
        .iter()
        .find(|v| Some(&v.id) == t.finalized_version_id.as_ref())
        .or_else(|| {
            versions
                .iter()
                .find(|v| Some(&v.id) == t.current_version_id.as_ref())
        })
        .or(versions.last());
    if let Some(v) = shown {
        let which = if Some(&v.id) == t.finalized_version_id.as_ref() {
            "定稿"
        } else {
            "当前"
        };
        s.push_str(&format!(
            "## 需求（v{}，{which}）\n\n{}\n\n",
            v.version,
            portable(&v.content_md, atts).trim()
        ));
    }
    if !t.acceptance_criteria.is_empty() {
        s.push_str("## 验收标准\n\n");
        for c in &t.acceptance_criteria {
            s.push_str(&format!("- {c}\n"));
        }
        s.push('\n');
    }
    if versions.len() > 1 {
        s.push_str("## PRD 版本\n\n");
        for v in versions {
            s.push_str(&format!(
                "- [v{n}](prd-v{n}.md)　{}　{}{}\n",
                v.created_at,
                v.author,
                v.note
                    .as_deref()
                    .filter(|n| !n.is_empty())
                    .map(|n| format!("　{}", cell(n)))
                    .unwrap_or_default(),
                n = v.version,
            ));
        }
        s.push('\n');
    }

    let list = tasks::list_for_ticket(conn, &t.id)?;
    if !list.is_empty() {
        s.push_str("## 子任务\n\n");
        for k in &list {
            s.push_str(&format!(
                "### {}（{}，{}）\n\n",
                k.title,
                k.kind,
                workflow::task_status_label(&k.status)
            ));
            if !k.spec_md.trim().is_empty() {
                s.push_str(&format!("{}\n\n", k.spec_md.trim()));
            }
            for (label, v) in [
                ("分支", &k.branch),
                ("合并提交", &k.merged_commit),
                ("PR", &k.pr_url),
            ] {
                if let Some(v) = v {
                    s.push_str(&format!("- {label}：{v}\n"));
                }
            }
            s.push('\n');
        }
    }

    let events = tickets::timeline(conn, &t.id)?;
    if !events.is_empty() {
        s.push_str("## 时间线\n\n");
        for e in &events {
            s.push_str(&format!("- {}　`{}`　{}\n", e.ts, e.kind, e.payload));
        }
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, b'I', b'H', b'D', b'R', 0,
        0, 0, 2, 0, 0, 0, 3, 8, 2, 0, 0, 0, 0, 0, 0, 0,
    ];

    #[test]
    fn daily_backup_runs_once_a_day_and_keeps_the_last_seven() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::open(&dir.path().join("tidewatch.db")).unwrap();
        let bdir = backup_dir(dir.path());
        std::fs::create_dir_all(&bdir).unwrap();
        for d in 1..=9 {
            std::fs::write(bdir.join(format!("tidewatch-2020-01-0{d}.db")), b"old").unwrap();
        }
        std::fs::write(bdir.join("notes.txt"), b"keep me").unwrap();

        let made = daily(&conn, dir.path())
            .unwrap()
            .expect("first run backs up");
        assert!(daily(&conn, dir.path()).unwrap().is_none());
        let names: Vec<String> = list(dir.path()).into_iter().map(|b| b.name).collect();
        assert_eq!(names.len(), KEEP);
        assert_eq!(
            names[0],
            made.file_name().unwrap().to_string_lossy().to_string()
        );
        assert_eq!(names.last().unwrap(), "tidewatch-2020-01-04.db");
        assert!(bdir.join("notes.txt").is_file());

        // 当天稍后手动备份的那份比每日那份新：排在最前，清理时留下的也是它
        let manual = now(&conn, dir.path()).unwrap();
        let names: Vec<String> = list(dir.path()).into_iter().map(|b| b.name).collect();
        assert_eq!(names.len(), KEEP);
        assert_eq!(
            names[0],
            manual.file_name().unwrap().to_string_lossy().to_string()
        );
        assert_eq!(
            names[1],
            made.file_name().unwrap().to_string_lossy().to_string()
        );
        assert_eq!(names.last().unwrap(), "tidewatch-2020-01-05.db");
        // 今天只手动备份过，也不再补一份每日备份
        std::fs::remove_file(&made).unwrap();
        assert!(daily(&conn, dir.path()).unwrap().is_none());
        let made = manual;

        // 备份是一份能打开的完整数据库
        let copy = Connection::open(&made).unwrap();
        let n: i64 = copy
            .query_row("SELECT count(*) FROM ticket", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn export_writes_portable_markdown_with_attachments() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("workspace");
        let mut conn = crate::db::open(&dir.path().join("tidewatch.db")).unwrap();
        let t = tickets::create(
            &mut conn,
            tickets::TicketInput {
                title: "结算页 | 优惠明细".into(),
                kind: "feature_frontend".into(),
                content_md: "第一版".into(),
                acceptance_criteria: vec!["明细可展开".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let a =
            attachments::add(&conn, &ws, Some((&t.id, &t.key)), "shot.png", PNG, "user").unwrap();
        let md = format!(
            "第二版\n\n{}\n",
            attachments::image_markdown("结算页", &a.id, "authoritative", None)
        );
        tickets::save_prd(&mut conn, &t.id, &md, None).unwrap();

        let out = dir.path().join("out");
        assert!(export(&conn, &ws, &out).is_err(), "目录不存在时不自己创建");
        std::fs::create_dir_all(&out).unwrap();
        let done = export(&conn, &ws, &out).unwrap();
        assert_eq!((done.tickets, done.attachments), (1, 1));
        assert!(done.missing.is_empty());

        let root = PathBuf::from(&done.dir);
        let tdir = root.join("tickets").join(&t.key);
        let file = a.path.rsplit('/').next().unwrap();
        assert_eq!(
            std::fs::read(tdir.join("attachments").join(file)).unwrap(),
            PNG
        );
        let v2 = std::fs::read_to_string(tdir.join("prd-v2.md")).unwrap();
        assert!(
            v2.contains(&format!("![结算页](attachments/{file} \"authoritative\")")),
            "{v2}"
        );
        assert_eq!(
            std::fs::read_to_string(tdir.join("prd-v1.md")).unwrap(),
            "第一版"
        );
        let ticket = std::fs::read_to_string(tdir.join("ticket.md")).unwrap();
        assert!(ticket.contains("## 需求（v2，当前）"), "{ticket}");
        assert!(ticket.contains("- 明细可展开"));
        assert!(!ticket.contains("attachment:"));
        let index = std::fs::read_to_string(root.join("README.md")).unwrap();
        assert!(index.contains("结算页 \\| 优惠明细"), "{index}");
        assert!(root.join("tidewatch.db").is_file());

        // 附件文件丢了：照常导出，并说明少了哪张
        std::fs::remove_file(ws.join(&a.path)).unwrap();
        let again = export(&conn, &ws, &out);
        let again = match again {
            Ok(x) => x,
            Err(e) => panic!("{e}"),
        };
        assert_eq!(again.attachments, 0);
        assert_eq!(again.missing, vec![format!("{} shot.png", t.key)]);
    }
}
