//! 图片附件与 PRD 里的图片引用。
//!
//! 文件放在 `<工作区>/attachments/<工单号>/`，只增不改；库里只有元数据，路径相对工作区。
//! 效力（权威 / 仅参考）写在 Markdown 里：`![说明](attachment:a_xxx "authoritative")`，
//! `prd_image` 只是保存版本时解析出来的索引。
use super::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
/// 还没有工单的附件（新建工单页导入、粘贴的图）先放这里，建单时认领。
const INBOX: &str = "_inbox";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Attachment {
    pub id: String,
    pub ticket_id: Option<String>,
    pub file_name: String,
    /// 相对工作区，正斜杠。
    pub path: String,
    pub mime: String,
    pub description: Option<String>,
    /// prd（随文档导入）| user（需求方贴的）
    pub origin: String,
    pub sha256: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub created_at: String,
}

const COLS: &str =
    "id, ticket_id, file_name, path, mime, description, origin, sha256, width, height, created_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: r.get(0)?,
        ticket_id: r.get(1)?,
        file_name: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
        path: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
        mime: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        description: r.get(5)?,
        origin: r
            .get::<_, Option<String>>(6)?
            .unwrap_or_else(|| "other".into()),
        sha256: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
        width: r.get(8)?,
        height: r.get(9)?,
        created_at: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
    })
}

/// 按文件头认图片格式，返回 (mime, 扩展名)。只收浏览器和两种 Agent 都能看的格式。
pub fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

fn new_attachment_id() -> String {
    // ULID 的后 10 位是随机部分
    let id = crate::util::new_id().to_lowercase();
    format!("a_{}", &id[id.len() - 10..])
}

/// 存一张图。同一张工单里内容相同（sha256）的图不存第二份，直接返回已有的。
/// `ticket`：(id, 工单号)；还没建单时为 `None`。
pub fn add(
    conn: &Connection,
    workspace: &Path,
    ticket: Option<(&str, &str)>,
    file_name: &str,
    bytes: &[u8],
    origin: &str,
) -> Result<Attachment> {
    if !matches!(origin, "prd" | "user") {
        return Err(Error::invalid(format!("未知的图片来源：{origin}")));
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(Error::invalid(format!(
            "图片「{file_name}」超过 {} MB，没有保存。",
            MAX_IMAGE_BYTES / 1024 / 1024
        )));
    }
    let Some((mime, ext)) = sniff(bytes) else {
        return Err(Error::invalid(format!(
            "「{file_name}」不是支持的图片格式（PNG、JPEG、GIF、WebP）。"
        )));
    };
    let sha = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let ticket_id = ticket.map(|t| t.0);
    let existing = conn
        .query_row(
            &format!(
                "SELECT {COLS} FROM attachment WHERE sha256 = ?1 AND ticket_id IS ?2 ORDER BY created_at LIMIT 1"
            ),
            rusqlite::params![sha, ticket_id],
            row,
        )
        .ok();
    if let Some(a) = existing {
        if workspace.join(&a.path).is_file() {
            return Ok(a);
        }
    }
    let id = new_attachment_id();
    let rel = format!(
        "attachments/{}/{id}.{ext}",
        ticket.map(|t| t.1).unwrap_or(INBOX)
    );
    let abs = workspace.join(&rel);
    if let Some(dir) = abs.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&abs, bytes)?;
    let (width, height) = match imagesize::blob_size(bytes) {
        Ok(s) => (Some(s.width as i64), Some(s.height as i64)),
        Err(_) => (None, None),
    };
    let a = Attachment {
        id,
        ticket_id: ticket_id.map(String::from),
        file_name: file_name.trim().to_string(),
        path: rel,
        mime: mime.into(),
        description: None,
        origin: origin.into(),
        sha256: sha,
        width,
        height,
        created_at: crate::util::now(),
    };
    conn.execute(
        &format!(
            "INSERT INTO attachment ({COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
        ),
        rusqlite::params![
            a.id,
            a.ticket_id,
            a.file_name,
            a.path,
            a.mime,
            a.description,
            a.origin,
            a.sha256,
            a.width,
            a.height,
            a.created_at
        ],
    )?;
    Ok(a)
}

pub fn get(conn: &Connection, id: &str) -> Result<Attachment> {
    conn.query_row(
        &format!("SELECT {COLS} FROM attachment WHERE id = ?1"),
        [id],
        row,
    )
    .map_err(|_| not_found("图片", id))
}

pub fn list_for_ticket(conn: &Connection, ticket_id: &str) -> Result<Vec<Attachment>> {
    let mut st = conn.prepare(&format!(
        "SELECT {COLS} FROM attachment WHERE ticket_id = ?1 ORDER BY created_at, rowid"
    ))?;
    let rows = st
        .query_map([ticket_id], row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 图片的文字描述：给看不了图的模型用，需求方可以改。
pub fn set_description(conn: &Connection, id: &str, description: &str) -> Result<Attachment> {
    let n = conn.execute(
        "UPDATE attachment SET description = ?2 WHERE id = ?1",
        rusqlite::params![id, clean(Some(description.to_string()))],
    )?;
    if n == 0 {
        return Err(not_found("图片", id));
    }
    get(conn, id)
}

/// 读出图片内容。
pub fn read(workspace: &Path, a: &Attachment) -> Result<Vec<u8>> {
    Ok(std::fs::read(workspace.join(&a.path))?)
}

/// 界面显示用：`data:image/png;base64,...`。
pub fn data_url(workspace: &Path, a: &Attachment) -> Result<String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(read(workspace, a)?);
    Ok(format!("data:{};base64,{b64}", a.mime))
}

/// 界面传来的图片内容（base64，可带 `data:...;base64,` 前缀）。
pub fn decode_base64(data: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let raw = data.rsplit_once("base64,").map(|x| x.1).unwrap_or(data);
    base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|_| Error::invalid("图片内容不是有效的 base64。"))
}

/// 建单后把收件箱里被认领的图挪到工单自己的目录。挪不动就留在原处（路径照旧有效）。
pub fn relocate(conn: &Connection, workspace: &Path, ticket_id: &str, key: &str) -> Result<()> {
    for a in list_for_ticket(conn, ticket_id)? {
        let Some(name) = a
            .path
            .strip_prefix(&format!("attachments/{INBOX}/"))
            .map(String::from)
        else {
            continue;
        };
        let rel = format!("attachments/{key}/{name}");
        let to = workspace.join(&rel);
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if std::fs::rename(workspace.join(&a.path), &to).is_ok() {
            conn.execute(
                "UPDATE attachment SET path = ?2 WHERE id = ?1",
                rusqlite::params![a.id, rel],
            )?;
        }
    }
    Ok(())
}

// ---- Markdown 里的图片引用 ----

pub const AUTHORITATIVE: &str = "authoritative";
pub const REFERENCE: &str = "reference";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImageRef {
    pub attachment_id: String,
    pub alt: String,
    pub authority: String,
    pub superseded_by: Option<String>,
    /// 在正文里的字节范围（整个 `![..](..)`）。
    #[serde(skip)]
    pub span: (usize, usize),
}

impl ImageRef {
    pub fn to_markdown(&self) -> String {
        image_markdown(
            &self.alt,
            &self.attachment_id,
            &self.authority,
            self.superseded_by.as_deref(),
        )
    }
}

pub fn image_markdown(alt: &str, id: &str, authority: &str, superseded_by: Option<&str>) -> String {
    let alt = alt.replace(['[', ']', '\n'], " ");
    match superseded_by {
        Some(s) => format!("![{alt}](attachment:{id} \"{authority}; superseded-by={s}\")"),
        None => format!("![{alt}](attachment:{id} \"{authority}\")"),
    }
}

/// 找出正文里所有 `![说明](attachment:<id> "效力")`。标题位缺省或写了别的，按仅参考处理。
pub fn parse_images(md: &str) -> Vec<ImageRef> {
    let mut out = vec![];
    let mut from = 0;
    while let Some(i) = md[from..].find("![") {
        let start = from + i;
        from = start + 2;
        let Some(alt_len) = md[from..].find(']') else {
            break;
        };
        let alt = &md[from..from + alt_len];
        let rest = &md[from + alt_len + 1..];
        if alt.contains('\n') || !rest.starts_with("(attachment:") {
            continue;
        }
        let Some(close) = rest.find(')') else {
            continue;
        };
        let inner = &rest["(attachment:".len()..close];
        if inner.contains('\n') {
            continue;
        }
        let (id, title) = match inner.find(char::is_whitespace) {
            Some(n) => (&inner[..n], inner[n..].trim().trim_matches('"')),
            None => (inner, ""),
        };
        if id.is_empty() {
            continue;
        }
        let mut authority = REFERENCE;
        let mut superseded_by = None;
        for part in title.split(';').map(str::trim) {
            if part.eq_ignore_ascii_case(AUTHORITATIVE) {
                authority = AUTHORITATIVE;
            } else if let Some(s) = part.strip_prefix("superseded-by=") {
                superseded_by = Some(s.trim().to_string()).filter(|s| !s.is_empty());
            }
        }
        let end = from + alt_len + 1 + close + 1;
        out.push(ImageRef {
            attachment_id: id.to_string(),
            alt: alt.trim().to_string(),
            authority: authority.into(),
            superseded_by,
            span: (start, end),
        });
        from = end;
    }
    out
}

/// 保存一个 PRD 版本时：校验图片引用、认领还没有工单的图、写 `prd_image` 索引。
/// `previous`：上一版正文；作者是 AI 的版本不得改动任何图片的效力标记。
pub fn index_version(
    conn: &Connection,
    ticket_id: &str,
    version_id: &str,
    content: &str,
    author: &str,
    previous: Option<&str>,
) -> Result<()> {
    let refs = parse_images(content);
    if author == "ai" {
        let before = previous.map(parse_images).unwrap_or_default();
        for r in &refs {
            let same = before.iter().any(|b| {
                b.attachment_id == r.attachment_id
                    && b.authority == r.authority
                    && b.superseded_by == r.superseded_by
            });
            if !same {
                return Err(Error::invalid(format!(
                    "AI 生成的版本不能改动图片的效力标记（{}）。哪张图说了算只能由需求方决定。",
                    r.attachment_id
                )));
            }
        }
    }
    for r in &refs {
        let a = get(conn, &r.attachment_id)
            .map_err(|_| Error::invalid(format!("正文引用的图片 {} 不存在。", r.attachment_id)))?;
        match a.ticket_id.as_deref() {
            Some(t) if t == ticket_id => {}
            Some(_) => {
                return Err(Error::invalid(format!(
                    "图片 {} 属于另一张工单，不能在这里引用。",
                    r.attachment_id
                )))
            }
            None => {
                conn.execute(
                    "UPDATE attachment SET ticket_id = ?2 WHERE id = ?1",
                    rusqlite::params![a.id, ticket_id],
                )?;
            }
        }
        if let Some(s) = &r.superseded_by {
            let ok = refs
                .iter()
                .any(|o| &o.attachment_id == s && o.authority == AUTHORITATIVE);
            if !ok {
                return Err(Error::invalid(format!(
                    "图片 {} 标着被 {s} 取代，但这一版里没有这张权威图。",
                    r.attachment_id
                )));
            }
        }
    }
    for (pos, r) in refs.iter().enumerate() {
        conn.execute(
            "INSERT OR IGNORE INTO prd_image (prd_version_id, attachment_id, authority, superseded_by, position)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![version_id, r.attachment_id, r.authority, r.superseded_by, pos as i64],
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct PrdImage {
    pub attachment: Attachment,
    pub authority: String,
    pub superseded_by: Option<String>,
    pub position: i64,
}

/// 某个 PRD 版本里的图片，按正文顺序。
pub fn images_of_version(conn: &Connection, version_id: &str) -> Result<Vec<PrdImage>> {
    let mut st = conn.prepare(
        "SELECT attachment_id, authority, superseded_by, position FROM prd_image
         WHERE prd_version_id = ?1 ORDER BY position",
    )?;
    let rows = st
        .query_map([version_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(id, authority, superseded_by, position)| {
            Ok(PrdImage {
                attachment: get(conn, &id)?,
                authority,
                superseded_by,
                position,
            })
        })
        .collect()
}

// ---- 交给 Agent ----

#[derive(Debug, Clone, Default)]
pub struct Prepared {
    pub prompt: String,
    /// 实际发给 Agent 的图片文件（绝对路径）。
    pub files: Vec<PathBuf>,
    /// 图片所在目录；没有图片要发时为 `None`。
    pub dir: Option<PathBuf>,
    /// 引用了但读不到的图片 id。
    pub missing: Vec<String>,
}

pub struct PrepareOptions<'a> {
    /// 图片复制到这里（`<工作区>/tmp/<run-id>/images`）。
    pub dir: &'a Path,
    /// 模型看不了图：改发文字描述。
    pub text_only: bool,
    /// 不发已被取代的参考图（开发阶段默认如此；预审要看全部）。
    pub skip_superseded: bool,
    /// 预审：附上图片相关的检查点。
    pub review: bool,
}

const RULES: &str = "规则：
1. 权威图是需求方确认过的，界面、字段、文案以它为准。
2. 仅参考图未经需求方确认（多数来自原始 PRD，经常有错），只用于理解背景，不得据此决定界面或字段。
3. 权威图与正文文字冲突时，按权威图处理，并在输出中明确指出冲突点。
";

const REVIEW_CHECKS: &str = "预审时另外检查图片：
- 权威图与正文文字矛盾（字段、文案、流程不一致）：major。
- 权威图之间互相矛盾：major。
- 某个界面类功能点只有参考图、没有权威图，且文字不足以确定界面：minor，问题写为「待确认：此图是否可信，或请补一张截图」。
- 正文引用了图但图缺失 / 无法读取：blocker。
- 参考图与正文不一致本身不算问题；只有当它提示正文可能有误时，作为 suggestion 提出。
";

/// 把 Prompt 里的 `attachment:<id>` 换成 Agent 读得到的文件路径（或文字描述），并在最前面加图片清单。
/// Prompt 里没有图片引用时原样返回。
pub fn prepare_prompt(
    conn: &Connection,
    workspace: &Path,
    prompt: &str,
    opts: &PrepareOptions,
) -> Result<Prepared> {
    let refs = parse_images(prompt);
    if refs.is_empty() {
        return Ok(Prepared {
            prompt: prompt.to_string(),
            ..Default::default()
        });
    }
    let mut out = String::with_capacity(prompt.len() + 1024);
    let mut rows: Vec<String> = vec![];
    let mut files: Vec<PathBuf> = vec![];
    let mut missing = vec![];
    let mut cursor = 0;
    for r in &refs {
        out.push_str(&prompt[cursor..r.span.0]);
        cursor = r.span.1;
        let section = prompt[..r.span.0]
            .lines()
            .rev()
            .find(|l| l.starts_with('#'))
            .map(|l| l.trim_start_matches('#').trim().replace('|', "/"))
            .unwrap_or_default();
        let alt = if r.alt.is_empty() {
            "（无说明）"
        } else {
            &r.alt
        };
        let effect = match (&r.authority[..], &r.superseded_by) {
            (AUTHORITATIVE, _) => "权威".to_string(),
            (_, Some(s)) => format!("仅参考，已被 {s} 取代"),
            _ => "仅参考".to_string(),
        };
        if opts.skip_superseded && r.superseded_by.is_some() {
            out.push_str(&format!(
                "（此处原有一张参考图「{alt}」，已被需求方用 {} 取代，未发送。）",
                r.superseded_by.as_deref().unwrap_or_default()
            ));
            continue;
        }
        let found = get(conn, &r.attachment_id)
            .ok()
            .filter(|a| workspace.join(&a.path).is_file());
        let Some(a) = found else {
            missing.push(r.attachment_id.clone());
            out.push_str(&format!("（图片缺失：{alt}，{}）", r.attachment_id));
            rows.push(format!(
                "| （缺失：{}） | {effect} | {} | {section} |",
                r.attachment_id,
                alt.replace('|', "/")
            ));
            continue;
        };
        if opts.text_only {
            let desc = a
                .description
                .as_deref()
                .filter(|d| !d.trim().is_empty())
                .unwrap_or("（没有文字描述，只有上面的说明）");
            out.push_str(&format!("【图片（{effect}）：{alt}。图片内容：{desc}】"));
            rows.push(format!(
                "| {}（当前模型看不了图，只有文字描述） | {effect} | {} | {section} |",
                a.id,
                alt.replace('|', "/")
            ));
            continue;
        }
        let ext = a.path.rsplit('.').next().unwrap_or("png");
        let file = opts.dir.join(format!("{}-{}.{ext}", r.authority, a.id));
        if !files.contains(&file) {
            std::fs::create_dir_all(opts.dir)?;
            std::fs::copy(workspace.join(&a.path), &file)?;
            files.push(file.clone());
        }
        let shown = file.display().to_string();
        out.push_str(&format!("![{alt}]({shown} \"{}\")", r.authority));
        rows.push(format!(
            "| {shown} | {effect} | {} | {section} |",
            alt.replace('|', "/")
        ));
    }
    out.push_str(&prompt[cursor..]);
    let mut head = String::from("## 图片清单\n");
    if rows.is_empty() {
        head.push_str("（正文里的图片都已被取代，没有发送。）\n\n");
    } else {
        head.push_str("| 图片 | 效力 | 说明 | 位置 |\n| --- | --- | --- | --- |\n");
        head.push_str(&rows.join("\n"));
        head.push_str("\n\n");
        if !opts.text_only && !files.is_empty() {
            head.push_str("图片是本机文件，请按路径直接读取查看。\n\n");
        }
        head.push_str(RULES);
        if opts.review {
            head.push('\n');
            head.push_str(REVIEW_CHECKS);
        }
        head.push('\n');
    }
    Ok(Prepared {
        prompt: head + &out,
        dir: (!files.is_empty()).then(|| opts.dir.to_path_buf()),
        files,
        missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, b'I', b'H', b'D', b'R', 0,
        0, 0, 2, 0, 0, 0, 3, 8, 2, 0, 0, 0, 0, 0, 0, 0,
    ];

    fn setup() -> (tempfile::TempDir, Connection, String) {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = crate::db::open(&dir.path().join("t.db")).unwrap();
        let t = tickets::create(
            &mut conn,
            tickets::TicketInput {
                title: "带图的需求".into(),
                kind: "feature_frontend".into(),
                ..Default::default()
            },
        )
        .unwrap();
        (dir, conn, t.id)
    }

    #[test]
    fn parses_image_references() {
        let md = "前文\n![权威图](attachment:a_1 \"authoritative\")\n![旧图](attachment:a_2 \"reference; superseded-by=a_1\")\n![缺省](attachment:a_3)\n![外链](https://x/y.png)\n";
        let refs = parse_images(md);
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].authority, AUTHORITATIVE);
        assert_eq!(refs[1].superseded_by.as_deref(), Some("a_1"));
        assert_eq!(refs[2].authority, REFERENCE);
        assert_eq!(&md[refs[1].span.0..refs[1].span.1], refs[1].to_markdown());
    }

    #[test]
    fn stores_once_per_ticket_and_reads_size() {
        let (dir, conn, ticket) = setup();
        let a = add(
            &conn,
            dir.path(),
            Some((&ticket, "TW-1")),
            "shot.png",
            PNG,
            "user",
        )
        .unwrap();
        assert_eq!((a.width, a.height), (Some(2), Some(3)));
        assert_eq!(a.path, format!("attachments/TW-1/{}.png", a.id));
        assert!(dir.path().join(&a.path).is_file());
        let b = add(
            &conn,
            dir.path(),
            Some((&ticket, "TW-1")),
            "again.png",
            PNG,
            "user",
        )
        .unwrap();
        assert_eq!(a.id, b.id, "同一张图不存第二份");
        assert!(add(&conn, dir.path(), None, "x.txt", b"hello", "user").is_err());
    }

    #[test]
    fn saving_a_version_validates_and_indexes_images() {
        let (dir, mut conn, ticket) = setup();
        let t = (ticket.as_str(), "TW-1");
        let old = add(&conn, dir.path(), None, "old.png", PNG, "prd").unwrap();
        let mut other = PNG.to_vec();
        other.push(1);
        let new = add(&conn, dir.path(), Some(t), "new.png", &other, "user").unwrap();

        let err =
            tickets::save_prd(&mut conn, &ticket, "![x](attachment:a_none)", None).unwrap_err();
        assert!(err.to_string().contains("不存在"), "{err}");
        let bad = format!(
            "![旧]({} \"reference; superseded-by={}\")",
            format_args!("attachment:{}", old.id),
            new.id
        );
        let err = tickets::save_prd(&mut conn, &ticket, &bad, None).unwrap_err();
        assert!(err.to_string().contains("没有这张权威图"), "{err}");

        let good = format!(
            "## F1\n{}\n{}\n",
            image_markdown("新图", &new.id, AUTHORITATIVE, None),
            image_markdown("旧图", &old.id, REFERENCE, Some(&new.id))
        );
        let v = tickets::save_prd(&mut conn, &ticket, &good, None).unwrap();
        let imgs = images_of_version(&conn, &v.id).unwrap();
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[1].superseded_by.as_deref(), Some(new.id.as_str()));
        assert_eq!(
            get(&conn, &old.id).unwrap().ticket_id.as_deref(),
            Some(ticket.as_str()),
            "收件箱里的图被这张工单认领"
        );
        relocate(&conn, dir.path(), &ticket, "TW-1").unwrap();
        let moved = get(&conn, &old.id).unwrap();
        assert_eq!(moved.path, format!("attachments/TW-1/{}.png", old.id));
        assert!(dir.path().join(&moved.path).is_file());

        // AI 版本不能把参考图改成权威图
        let flipped = good.replace("reference; superseded-by", "authoritative; superseded-by");
        let err =
            tickets::save_prd_as(&mut conn, &ticket, &flipped, "ai_suggestion", None).unwrap_err();
        assert!(err.to_string().contains("效力"), "{err}");
        // 只改文字可以
        tickets::save_prd_as(
            &mut conn,
            &ticket,
            &format!("{good}\n补一句。"),
            "ai_suggestion",
            None,
        )
        .unwrap();
    }

    #[test]
    fn prompt_gets_manifest_paths_and_descriptions() {
        let (dir, mut conn, ticket) = setup();
        let t = (ticket.as_str(), "TW-1");
        let mut other = PNG.to_vec();
        other.push(1);
        let new = add(&conn, dir.path(), Some(t), "new.png", &other, "user").unwrap();
        let old = add(&conn, dir.path(), Some(t), "old.png", PNG, "prd").unwrap();
        set_description(&conn, &new.id, "结算页，含优惠明细三行").unwrap();
        let md = format!(
            "### F2 结算页\n{}\n{}\n",
            image_markdown("结算页截图", &new.id, AUTHORITATIVE, None),
            image_markdown("原示意", &old.id, REFERENCE, Some(&new.id))
        );
        tickets::save_prd(&mut conn, &ticket, &md, None).unwrap();
        let images = dir.path().join("tmp/R1/images");

        let all = prepare_prompt(
            &conn,
            dir.path(),
            &md,
            &PrepareOptions {
                dir: &images,
                text_only: false,
                skip_superseded: false,
                review: true,
            },
        )
        .unwrap();
        assert_eq!(all.files.len(), 2);
        assert!(all.files[0].ends_with(format!("authoritative-{}.png", new.id)));
        assert!(all.files.iter().all(|f| f.is_file()));
        assert!(all.prompt.starts_with("## 图片清单"));
        assert!(all.prompt.contains("| 权威 | 结算页截图 | F2 结算页 |"));
        assert!(all
            .prompt
            .contains(&format!("仅参考，已被 {} 取代", new.id)));
        assert!(all.prompt.contains("预审时另外检查图片"));
        assert!(!all.prompt.contains("attachment:"));

        let dev = prepare_prompt(
            &conn,
            dir.path(),
            &md,
            &PrepareOptions {
                dir: &images,
                text_only: false,
                skip_superseded: true,
                review: false,
            },
        )
        .unwrap();
        assert_eq!(dev.files.len(), 1, "开发阶段不发已被取代的参考图");
        assert!(dev.prompt.contains("未发送"));

        let text = prepare_prompt(
            &conn,
            dir.path(),
            &md,
            &PrepareOptions {
                dir: &images,
                text_only: true,
                skip_superseded: true,
                review: false,
            },
        )
        .unwrap();
        assert!(text.files.is_empty() && text.dir.is_none());
        assert!(text.prompt.contains("图片内容：结算页，含优惠明细三行"));

        std::fs::remove_file(dir.path().join(&new.path)).unwrap();
        let gone = prepare_prompt(
            &conn,
            dir.path(),
            &md,
            &PrepareOptions {
                dir: &images,
                text_only: false,
                skip_superseded: true,
                review: true,
            },
        )
        .unwrap();
        assert_eq!(gone.missing, vec![new.id.clone()]);
        assert!(gone.prompt.contains("图片缺失"));
    }
}
