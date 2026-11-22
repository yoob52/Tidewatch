//! 文件导入：`parse(file) -> { markdown, images[] }`。支持 Markdown / 文本、docx、pdf。
//! 解析只做"尽量保留结构的纯文本化"；图片抽出来交给附件，导入的图一律是仅参考图。
use crate::store::attachments::{self, Attachment};
use crate::{Error, Result};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

pub const MAX_FILE_BYTES: usize = 50 * 1024 * 1024;
/// docx 里单个 XML 解压后的上限：正常文档远小于它，超过的多半是压缩炸弹。
const MAX_XML_BYTES: u64 = 64 * 1024 * 1024;
/// 一次导入里所有图片解压后的总量上限。
const MAX_IMAGES_TOTAL_BYTES: usize = 200 * 1024 * 1024;

/// 最多读 `limit` 字节；超过就算读不了，不把整个内容装进内存。
fn read_capped(r: impl Read, limit: u64) -> Option<Vec<u8>> {
    let mut buf = vec![];
    r.take(limit + 1).read_to_end(&mut buf).ok()?;
    (buf.len() as u64 <= limit).then_some(buf)
}

#[derive(Debug, Clone, Default)]
pub struct ParsedImage {
    pub file_name: String,
    pub bytes: Vec<u8>,
}

/// 解析结果。正文里的图片写成 `![说明](import:<下标>)`，下标指向 `images`。
#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub markdown: String,
    pub images: Vec<ParsedImage>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Imported {
    pub file_name: String,
    pub markdown: String,
    pub attachments: Vec<Attachment>,
    /// 没能原样导入的内容（不支持的图片格式、扫描件没有文字等），要让用户看到。
    pub warnings: Vec<String>,
}

fn ext(file_name: &str) -> String {
    Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// `base_dir`：文件所在目录，用来找 Markdown 里相对路径引用的图片。
pub fn parse(file_name: &str, bytes: &[u8], base_dir: Option<&Path>) -> Result<Parsed> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Error::invalid(format!(
            "文件超过 {} MB，没有导入。",
            MAX_FILE_BYTES / 1024 / 1024
        )));
    }
    match ext(file_name).as_str() {
        "md" | "markdown" => Ok(markdown(&decode_text(bytes), base_dir)),
        "txt" | "" => Ok(Parsed {
            markdown: decode_text(bytes),
            ..Default::default()
        }),
        "docx" => docx(bytes),
        "pdf" => pdf(bytes),
        "doc" => Err(Error::invalid(
            "不支持旧版 .doc，请在 Word 里另存为 .docx 后再导入。",
        )),
        other => Err(Error::invalid(format!(
            "不支持导入 .{other} 文件（支持 md、txt、docx、pdf）。"
        ))),
    }
}

/// 解析并把图片存成附件（还没有工单，建单时认领）。
pub fn import(
    conn: &rusqlite::Connection,
    workspace: &Path,
    file_name: &str,
    bytes: &[u8],
    base_dir: Option<&Path>,
) -> Result<Imported> {
    let mut p = parse(file_name, bytes, base_dir)?;
    let mut atts: Vec<Attachment> = vec![];
    let mut md = p.markdown.clone();
    for (i, img) in p.images.iter().enumerate() {
        let token = format!("(import:{i})");
        match attachments::add(conn, workspace, None, &img.file_name, &img.bytes, "prd") {
            Ok(a) => {
                md = md.replace(&token, &format!("(attachment:{} \"reference\")", a.id));
                if !atts.iter().any(|x| x.id == a.id) {
                    atts.push(a);
                }
            }
            Err(e) => {
                p.warnings.push(e.to_string());
                md = replace_image(&md, &token, &format!("（图片 {} 没有导入）", img.file_name));
            }
        }
    }
    Ok(Imported {
        file_name: file_name.to_string(),
        markdown: md,
        attachments: atts,
        warnings: p.warnings,
    })
}

/// 把以 `token` 结尾的整个 `![..](token)` 换成一句话。
fn replace_image(md: &str, token: &str, with: &str) -> String {
    let Some(end) = md.find(token) else {
        return md.to_string();
    };
    let start = md[..end].rfind("![").unwrap_or(end);
    format!("{}{with}{}", &md[..start], &md[end + token.len()..])
}

/// UTF-8（可带 BOM）、UTF-16（带 BOM），否则按 GB18030：中文 Windows 上的记事本文件常是它。
fn decode_text(bytes: &[u8]) -> String {
    let (text, _, _) = match std::str::from_utf8(bytes) {
        Ok(_) => encoding_rs::UTF_8.decode(bytes),
        Err(_) if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) => {
            encoding_rs::UTF_16LE.decode(bytes)
        }
        Err(_) => encoding_rs::GB18030.decode(bytes),
    };
    text.replace("\r\n", "\n")
}

fn markdown(text: &str, base_dir: Option<&Path>) -> Parsed {
    let mut out = Parsed::default();
    let mut md = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("![") {
        md.push_str(&rest[..i]);
        let tail = &rest[i..];
        let parsed = (|| {
            let close = tail.find("](")?;
            let end = close + 2 + tail[close + 2..].find(')')?;
            let alt = &tail[2..close];
            let target = tail[close + 2..end].split_whitespace().next()?;
            if alt.contains('\n') {
                return None;
            }
            Some((alt, target, end + 1))
        })();
        let Some((alt, target, len)) = parsed else {
            md.push_str("![");
            rest = &tail[2..];
            continue;
        };
        let remote = ["http://", "https://", "data:", "attachment:"]
            .iter()
            .any(|p| target.starts_with(p));
        // 只自动带上文档所在目录里面的图：别人给的 Markdown 不能借绝对路径或 `..`
        // 把本机其它位置的图片塞进附件（附件之后会发给 Agent）
        let local = base_dir.filter(|_| !remote).map(|d| {
            let path = d.join(target.trim_matches(|c| c == '<' || c == '>'));
            let (Ok(root), Ok(real)) = (d.canonicalize(), path.canonicalize()) else {
                return Err(false);
            };
            if !real.starts_with(&root) {
                return Err(true);
            }
            let used: usize = out.images.iter().map(|i| i.bytes.len()).sum();
            let room = MAX_IMAGES_TOTAL_BYTES.saturating_sub(used);
            std::fs::File::open(&real)
                .ok()
                .and_then(|f| read_capped(f, attachments::MAX_IMAGE_BYTES.min(room) as u64))
                .ok_or(false)
        });
        match local {
            Some(Err(true)) => {
                out.warnings.push(format!(
                    "图片 {target} 不在文档所在的目录里，没有自动导入；需要的话请手动上传。"
                ));
                md.push_str(&tail[..len]);
            }
            Some(Ok(bytes)) => {
                md.push_str(&format!("![{alt}](import:{})", out.images.len()));
                out.images.push(ParsedImage {
                    file_name: Path::new(target)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(target)
                        .to_string(),
                    bytes,
                });
            }
            Some(Err(false)) => {
                out.warnings.push(format!(
                    "正文引用的图片 {target} 找不到或太大，保留了原来的写法。"
                ));
                md.push_str(&tail[..len]);
            }
            None => md.push_str(&tail[..len]),
        }
        rest = &tail[len..];
    }
    md.push_str(rest);
    out.markdown = md;
    out
}

// ---- docx ----

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn zip_text<Rd: Read + std::io::Seek>(z: &mut zip::ZipArchive<Rd>, name: &str) -> Option<String> {
    let f = z.by_name(name).ok()?;
    String::from_utf8(read_capped(f, MAX_XML_BYTES)?).ok()
}

fn docx(bytes: &[u8]) -> Result<Parsed> {
    let bad = |e: &dyn std::fmt::Display| Error::invalid(format!("这个 docx 文件读不了：{e}"));
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| bad(&e))?;
    let doc_xml =
        zip_text(&mut z, "word/document.xml").ok_or_else(|| bad(&"里面没有 word/document.xml"))?;
    let doc = roxmltree::Document::parse(&doc_xml).map_err(|e| bad(&e))?;

    // 关系：rId -> word/media/xxx
    let mut rels: HashMap<String, String> = HashMap::new();
    if let Some(x) = zip_text(&mut z, "word/_rels/document.xml.rels") {
        if let Ok(d) = roxmltree::Document::parse(&x) {
            for n in d.descendants().filter(|n| n.has_tag_name("Relationship")) {
                if let (Some(id), Some(t)) = (n.attribute("Id"), n.attribute("Target")) {
                    rels.insert(id.into(), t.trim_start_matches('/').into());
                }
            }
        }
    }
    // 样式：styleId -> 标题级别
    let mut headings: HashMap<String, usize> = HashMap::new();
    if let Some(x) = zip_text(&mut z, "word/styles.xml") {
        if let Ok(d) = roxmltree::Document::parse(&x) {
            for s in d.descendants().filter(|n| n.has_tag_name((W, "style"))) {
                let Some(id) = s.attribute((W, "styleId")) else {
                    continue;
                };
                let name = s
                    .children()
                    .find(|n| n.has_tag_name((W, "name")))
                    .and_then(|n| n.attribute((W, "val")))
                    .unwrap_or("")
                    .to_lowercase();
                let outline = s
                    .descendants()
                    .find(|n| n.has_tag_name((W, "outlineLvl")))
                    .and_then(|n| n.attribute((W, "val")))
                    .and_then(|v| v.parse::<usize>().ok());
                let level = if name == "title" {
                    Some(1)
                } else if let Some(n) = name
                    .strip_prefix("heading ")
                    .or_else(|| name.strip_prefix("标题 "))
                    .and_then(|n| n.trim().parse::<usize>().ok())
                {
                    Some(n)
                } else {
                    outline.map(|o| o + 1)
                };
                if let Some(l) = level.filter(|l| (1..=6).contains(l)) {
                    headings.insert(id.to_string(), l);
                }
            }
        }
    }
    // 编号：numId -> 各级是不是有序
    let mut ordered: HashMap<(String, String), bool> = HashMap::new();
    if let Some(x) = zip_text(&mut z, "word/numbering.xml") {
        if let Ok(d) = roxmltree::Document::parse(&x) {
            let mut abs: HashMap<String, Vec<(String, bool)>> = HashMap::new();
            for a in d
                .descendants()
                .filter(|n| n.has_tag_name((W, "abstractNum")))
            {
                let id = a.attribute((W, "abstractNumId")).unwrap_or("").to_string();
                let lvls = a
                    .children()
                    .filter(|n| n.has_tag_name((W, "lvl")))
                    .map(|l| {
                        let fmt = l
                            .children()
                            .find(|n| n.has_tag_name((W, "numFmt")))
                            .and_then(|n| n.attribute((W, "val")))
                            .unwrap_or("bullet");
                        (
                            l.attribute((W, "ilvl")).unwrap_or("0").to_string(),
                            fmt != "bullet" && fmt != "none",
                        )
                    })
                    .collect();
                abs.insert(id, lvls);
            }
            for n in d.descendants().filter(|n| n.has_tag_name((W, "num"))) {
                let num = n.attribute((W, "numId")).unwrap_or("").to_string();
                let a = n
                    .children()
                    .find(|c| c.has_tag_name((W, "abstractNumId")))
                    .and_then(|c| c.attribute((W, "val")))
                    .unwrap_or("");
                for (lvl, ord) in abs.get(a).cloned().unwrap_or_default() {
                    ordered.insert((num.clone(), lvl), ord);
                }
            }
        }
    }

    let mut out = Parsed::default();
    let mut media: HashMap<String, usize> = HashMap::new();
    let mut blocks: Vec<String> = vec![];
    let Some(body) = doc.descendants().find(|n| n.has_tag_name((W, "body"))) else {
        return Err(bad(&"里面没有正文"));
    };

    // 一个段落里的文字和图片
    let mut para_text = |p: roxmltree::Node, out: &mut Parsed| -> String {
        let mut s = String::new();
        for n in p.descendants() {
            if n.has_tag_name((W, "t")) {
                let bold = n
                    .parent()
                    .and_then(|r| r.children().find(|c| c.has_tag_name((W, "rPr"))))
                    .and_then(|pr| pr.children().find(|c| c.has_tag_name((W, "b"))))
                    .is_some_and(|b| !matches!(b.attribute((W, "val")), Some("0" | "false")));
                let t = n.text().unwrap_or("");
                if bold && !t.trim().is_empty() {
                    // 相邻的加粗片段并成一段
                    if s.ends_with("**") && !s.ends_with(" **") && s.len() > 2 {
                        s.truncate(s.len() - 2);
                        s.push_str(t);
                    } else {
                        s.push_str("**");
                        s.push_str(t);
                    }
                    s.push_str("**");
                } else {
                    s.push_str(t);
                }
            } else if n.has_tag_name((W, "tab")) {
                s.push(' ');
            } else if n.has_tag_name((W, "br")) || n.has_tag_name((W, "cr")) {
                s.push('\n');
            } else if n.tag_name().name() == "blip" || n.tag_name().name() == "imagedata" {
                let rid = n
                    .attribute((R, "embed"))
                    .or_else(|| n.attribute((R, "id")))
                    .unwrap_or("");
                let Some(target) = rels.get(rid) else {
                    continue;
                };
                let path = if target.starts_with("word/") {
                    target.clone()
                } else {
                    format!("word/{target}")
                };
                let idx = match media.get(&path) {
                    Some(i) => Some(*i),
                    None => {
                        let used: usize = out.images.iter().map(|i| i.bytes.len()).sum();
                        let room = MAX_IMAGES_TOTAL_BYTES.saturating_sub(used);
                        let read = z.by_name(&path).ok().and_then(|f| {
                            read_capped(f, attachments::MAX_IMAGE_BYTES.min(room) as u64)
                        });
                        read.map(|buf| {
                            out.images.push(ParsedImage {
                                file_name: path.rsplit('/').next().unwrap_or(&path).to_string(),
                                bytes: buf,
                            });
                            media.insert(path.clone(), out.images.len() - 1);
                            out.images.len() - 1
                        })
                    }
                };
                // 图片说明：docPr 的 descr / title（Word 里的"替换文字"）
                let alt = n
                    .ancestors()
                    .find(|a| a.tag_name().name() == "inline" || a.tag_name().name() == "anchor")
                    .and_then(|a| a.children().find(|c| c.tag_name().name() == "docPr"))
                    .and_then(|d| d.attribute("descr").or_else(|| d.attribute("title")))
                    .filter(|d| !d.trim().is_empty())
                    .unwrap_or("原 PRD 图")
                    .replace(['[', ']', '\n'], " ");
                match idx {
                    Some(i) => s.push_str(&format!("\n\n![{alt}](import:{i})\n\n")),
                    None => out
                        .warnings
                        .push(format!("文档里的图片 {path} 读不出来或太大，已跳过。")),
                }
            }
        }
        s.trim().to_string()
    };

    for node in body.children().filter(|n| n.is_element()) {
        if node.has_tag_name((W, "p")) {
            let text = para_text(node, &mut out);
            if text.is_empty() {
                continue;
            }
            let ppr = node.children().find(|n| n.has_tag_name((W, "pPr")));
            let style = ppr
                .and_then(|p| p.children().find(|n| n.has_tag_name((W, "pStyle"))))
                .and_then(|n| n.attribute((W, "val")));
            let level = style.and_then(|s| headings.get(s).copied()).or_else(|| {
                ppr.and_then(|p| p.children().find(|n| n.has_tag_name((W, "outlineLvl"))))
                    .and_then(|n| n.attribute((W, "val")))
                    .and_then(|v| v.parse::<usize>().ok())
                    .filter(|l| *l < 6)
                    .map(|l| l + 1)
            });
            let num = ppr.and_then(|p| p.children().find(|n| n.has_tag_name((W, "numPr"))));
            if let Some(l) = level {
                blocks.push(format!(
                    "{} {}",
                    "#".repeat(l),
                    text.replace("**", "").replace('\n', " ")
                ));
            } else if let Some(num) = num {
                let val = |tag: &str| {
                    num.children()
                        .find(|n| n.has_tag_name((W, tag)))
                        .and_then(|n| n.attribute((W, "val")))
                        .unwrap_or("0")
                        .to_string()
                };
                let (ilvl, id) = (val("ilvl"), val("numId"));
                let depth = ilvl.parse::<usize>().unwrap_or(0);
                let mark = if ordered.get(&(id, ilvl)).copied().unwrap_or(false) {
                    "1."
                } else {
                    "-"
                };
                let item = format!("{}{mark} {}", "  ".repeat(depth), text.replace('\n', " "));
                // 连续的列表项之间不空行
                match blocks.last_mut() {
                    Some(last) if is_list_block(last) => {
                        last.push('\n');
                        last.push_str(&item);
                    }
                    _ => blocks.push(item),
                }
            } else {
                blocks.push(text);
            }
        } else if node.has_tag_name((W, "tbl")) {
            let mut rows: Vec<Vec<String>> = vec![];
            for tr in node.children().filter(|n| n.has_tag_name((W, "tr"))) {
                let cells: Vec<String> = tr
                    .children()
                    .filter(|n| n.has_tag_name((W, "tc")))
                    .map(|tc| {
                        tc.descendants()
                            .filter(|n| n.has_tag_name((W, "p")))
                            .map(|p| para_text(p, &mut out))
                            .filter(|t| !t.is_empty())
                            .collect::<Vec<_>>()
                            .join("<br>")
                            .replace('\n', "<br>")
                            .replace('|', "\\|")
                    })
                    .collect();
                rows.push(cells);
            }
            let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
            if cols == 0 {
                continue;
            }
            let line = |cells: &[String]| {
                let mut c = cells.to_vec();
                c.resize(cols, String::new());
                format!("| {} |", c.join(" | "))
            };
            let mut t = vec![line(&rows[0]), format!("|{}", " --- |".repeat(cols))];
            t.extend(rows[1..].iter().map(|r| line(r)));
            blocks.push(t.join("\n"));
        }
    }
    out.markdown = blocks.join("\n\n") + "\n";
    if out.markdown.trim().is_empty() {
        out.warnings.push("文档里没有读到文字。".into());
    }
    Ok(out)
}

fn is_list_block(b: &str) -> bool {
    let l = b.lines().last().unwrap_or("").trim_start();
    l.starts_with("- ") || l.starts_with("1. ")
}

// ---- pdf ----

fn pdf(bytes: &[u8]) -> Result<Parsed> {
    let mut out = Parsed::default();
    // pdf-extract 遇到不规范的文件可能 panic；导入失败不该带垮整个应用
    let owned = bytes.to_vec();
    let text = std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem(&owned))
        .map_err(|_| Error::invalid("这个 PDF 解析时出错，没有导入。可以先把内容复制出来再粘贴。"))?
        .map_err(|e| Error::invalid(format!("这个 PDF 读不了：{e}")))?;
    // 每行去掉行尾空白，连续空行并成一行
    let mut md = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let l = line.trim_end();
        if l.trim().is_empty() {
            blank += 1;
            continue;
        }
        if blank > 0 && !md.is_empty() {
            md.push('\n');
        }
        blank = 0;
        md.push_str(l);
        md.push('\n');
    }
    if md.trim().is_empty() {
        out.warnings
            .push("PDF 里没有可提取的文字（可能是扫描件），只导入了图片。".into());
    }
    // 内嵌图片：JPEG 原样取出；无损位图（截图多半是这种）转成 PNG；其它编码的不导入
    let owned = bytes.to_vec();
    let (images, skipped) = std::panic::catch_unwind(move || {
        let mut images: Vec<(&'static str, Vec<u8>)> = vec![];
        let mut skipped = 0usize;
        let mut used = 0usize;
        if let Ok(doc) = pdf_extract::Document::load_mem(&owned) {
            let is_image = |s: &pdf_extract::Stream| {
                s.dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    .is_some_and(|n| n == b"Image")
            };
            // 透明度蒙版也是图片对象，但它只是另一张图的一部分
            let masks: std::collections::HashSet<pdf_extract::ObjectId> = doc
                .objects
                .values()
                .filter_map(|o| o.as_stream().ok())
                .filter(|s| is_image(s))
                .flat_map(|s| [s.dict.get(b"SMask"), s.dict.get(b"Mask")])
                .filter_map(|m| m.ok().and_then(|m| m.as_reference().ok()))
                .collect();
            for (id, obj) in &doc.objects {
                let pdf_extract::Object::Stream(s) = obj else {
                    continue;
                };
                if !is_image(s) || masks.contains(id) {
                    continue;
                }
                let room = MAX_IMAGES_TOTAL_BYTES.saturating_sub(used);
                let filter = pdf_filter(s);
                let image = if filter == Some(b"DCTDecode") && s.content.starts_with(b"\xff\xd8") {
                    Some(("jpg", s.content.clone()))
                } else {
                    pdf_bitmap_png(&doc, s).map(|b| ("png", b))
                };
                match image.filter(|(_, b)| b.len() <= attachments::MAX_IMAGE_BYTES.min(room)) {
                    Some(img) => {
                        used += img.1.len();
                        images.push(img);
                    }
                    None => skipped += 1,
                }
            }
        }
        (images, skipped)
    })
    .unwrap_or_default();
    if !images.is_empty() {
        md.push_str("\n## 文档里的图片\n\n");
        for (i, (ext, bytes)) in images.into_iter().enumerate() {
            md.push_str(&format!("![原 PDF 图 {}](import:{i})\n\n", i + 1));
            out.images.push(ParsedImage {
                file_name: format!("pdf-image-{}.{ext}", i + 1),
                bytes,
            });
        }
        out.warnings.push(
            "PDF 里的图片放在了正文末尾（PDF 不保留图片在文字里的位置），请挪到对应的功能点下面。"
                .into(),
        );
    }
    if skipped > 0 {
        out.warnings.push(format!(
            "PDF 里有 {skipped} 张图片的编码不支持（能导入的是 JPEG 和常见的无损位图），没有导入；需要的话请截图后贴进来。"
        ));
    }
    out.markdown = md;
    Ok(out)
}

/// 流只用了一种编码时返回它的名字；没有编码返回空串；多种编码叠在一起的不处理。
fn pdf_filter(s: &pdf_extract::Stream) -> Option<&[u8]> {
    match s.dict.get(b"Filter") {
        Err(_) => Some(b""),
        Ok(pdf_extract::Object::Name(n)) => Some(n),
        Ok(pdf_extract::Object::Array(a)) if a.len() == 1 => a[0].as_name().ok(),
        Ok(_) => None,
    }
}

/// 解压后的像素数据上限。
const MAX_PDF_BITMAP_BYTES: u64 = 128 * 1024 * 1024;

/// 流的内容：没压缩的原样返回，Flate 的解压（有上限），其它编码不处理。
fn pdf_stream_bytes(s: &pdf_extract::Stream, limit: u64) -> Option<Vec<u8>> {
    match pdf_filter(s)? {
        b"" => (s.content.len() as u64 <= limit).then(|| s.content.clone()),
        b"FlateDecode" => read_capped(flate2::read::ZlibDecoder::new(&s.content[..]), limit),
        _ => None,
    }
}

/// 颜色空间的分量数；调色板的话再给出 RGB 调色板。认不出的返回 `None`。
fn pdf_color_space(
    doc: &pdf_extract::Document,
    cs: &pdf_extract::Object,
) -> Option<(usize, Option<Vec<u8>>)> {
    use pdf_extract::Object;
    let (_, cs) = doc.dereference(cs).ok()?;
    let by_name = |n: &[u8]| match n {
        b"DeviceGray" | b"CalGray" | b"G" => Some(1),
        b"DeviceRGB" | b"CalRGB" | b"RGB" => Some(3),
        _ => None,
    };
    match cs {
        Object::Name(n) => Some((by_name(n)?, None)),
        Object::Array(a) => match a.first()?.as_name().ok()? {
            b"ICCBased" => {
                let (_, icc) = doc.dereference(a.get(1)?).ok()?;
                let n = icc.as_stream().ok()?.dict.get(b"N").ok()?.as_i64().ok()?;
                matches!(n, 1 | 3).then_some((n as usize, None))
            }
            b"Indexed" | b"I" => {
                let (base, None) = pdf_color_space(doc, a.get(1)?)? else {
                    return None;
                };
                let (_, lookup) = doc.dereference(a.get(3)?).ok()?;
                let table = match lookup {
                    Object::String(b, _) => b.clone(),
                    Object::Stream(s) => pdf_stream_bytes(s, 256 * 4)?,
                    _ => return None,
                };
                let rgb: Vec<u8> = match base {
                    3 => table,
                    _ => table.iter().flat_map(|g| [*g, *g, *g]).collect(),
                };
                let entries = (rgb.len() / 3).min(256);
                (entries > 0).then(|| (1, Some(rgb[..entries * 3].to_vec())))
            }
            n => Some((by_name(n)?, None)),
        },
        _ => None,
    }
}

/// 把 PDF 里一张无损位图转成 PNG。只处理常见的几种（灰度 / RGB / 调色板，没压缩或 Flate），
/// 其它的（CMYK、JPEG 2000、传真编码、模板蒙版……）返回 `None`。
fn pdf_bitmap_png(doc: &pdf_extract::Document, s: &pdf_extract::Stream) -> Option<Vec<u8>> {
    let int = |key: &[u8]| -> Option<i64> { s.dict.get_deref(key, doc).ok()?.as_i64().ok() };
    let (width, height) = (int(b"Width")?, int(b"Height")?);
    if !(1..=30_000).contains(&width) || !(1..=30_000).contains(&height) {
        return None;
    }
    let (width, height) = (width as usize, height as usize);
    let bits = int(b"BitsPerComponent")? as usize;
    let (comps, palette) = pdf_color_space(doc, s.dict.get(b"ColorSpace").ok()?)?;
    let depth = match (comps, bits) {
        (1, 1) => png::BitDepth::One,
        (1, 2) => png::BitDepth::Two,
        (1, 4) => png::BitDepth::Four,
        (_, 8) => png::BitDepth::Eight,
        _ => return None,
    };
    let row = (width * comps * bits).div_ceil(8);
    let size = (row * height) as u64;
    if size > MAX_PDF_BITMAP_BYTES {
        return None;
    }
    // Flate 可以带 PNG 预测器：每行前面多一个字节，说明这一行是怎么编码的
    let parms = s.dict.get_deref(b"DecodeParms", doc).ok();
    let parms = match parms {
        Some(pdf_extract::Object::Array(a)) if a.len() == 1 => {
            doc.dereference(&a[0]).ok().map(|p| p.1)
        }
        other => other,
    }
    .and_then(|p| p.as_dict().ok());
    let predictor = parms
        .and_then(|p| p.get(b"Predictor").ok()?.as_i64().ok())
        .unwrap_or(1);
    let mut data = match predictor {
        1 => pdf_stream_bytes(s, size)?,
        10..=15 => {
            let raw = pdf_stream_bytes(s, size + height as u64)?;
            png_unfilter(&raw, row, (comps * bits).div_ceil(8))?
        }
        _ => return None,
    };
    if data.len() < row * height {
        return None;
    }
    data.truncate(row * height);
    let mut out = vec![];
    let mut enc = png::Encoder::new(&mut out, width as u32, height as u32);
    enc.set_depth(depth);
    enc.set_color(match (comps, &palette) {
        (_, Some(_)) => png::ColorType::Indexed,
        (1, None) => png::ColorType::Grayscale,
        _ => png::ColorType::Rgb,
    });
    if let Some(p) = palette {
        enc.set_palette(p);
    }
    let mut w = enc.write_header().ok()?;
    w.write_image_data(&data).ok()?;
    w.finish().ok()?;
    Some(out)
}

/// 还原 PNG 预测器编码的行：`row` 是一行像素的字节数，`bpp` 是一个像素占的字节数。
fn png_unfilter(raw: &[u8], row: usize, bpp: usize) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    for line in raw.chunks(row + 1) {
        let (kind, line) = line.split_first()?;
        if line.len() != row {
            break;
        }
        let start = out.len();
        for (i, &x) in line.iter().enumerate() {
            let a = if i >= bpp { out[start + i - bpp] } else { 0 };
            let b = if start >= row {
                out[start + i - row]
            } else {
                0
            };
            let c = if i >= bpp && start >= row {
                out[start + i - row - bpp]
            } else {
                0
            };
            let pred = match kind {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                4 => {
                    let p = a as i16 + b as i16 - c as i16;
                    let (pa, pb, pc) = (
                        (p - a as i16).abs(),
                        (p - b as i16).abs(),
                        (p - c as i16).abs(),
                    );
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                _ => return None,
            };
            out.push(x.wrapping_add(pred));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, b'I', b'H', b'D', b'R', 0,
        0, 0, 2, 0, 0, 0, 3, 8, 2, 0, 0, 0, 0, 0, 0, 0,
    ];

    fn docx_bytes(with_bad_image: bool) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
        let o = zip::write::SimpleFileOptions::default();
        let mut put = |name: &str, data: &[u8]| {
            z.start_file(name, o).unwrap();
            z.write_all(data).unwrap();
        };
        put("word/styles.xml", format!(r#"<w:styles xmlns:w="{W}"><w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="H2"><w:name w:val="heading 2"/></w:style></w:styles>"#).as_bytes());
        put("word/numbering.xml", format!(r#"<w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#).as_bytes());
        put("word/_rels/document.xml.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId5" Type="x" Target="media/image1.png"/><Relationship Id="rId6" Type="x" Target="media/image2.emf"/></Relationships>"#);
        put("word/media/image1.png", PNG);
        put("word/media/image2.emf", b"not an image we support");
        let bad = if with_bad_image {
            r#"<w:p><w:r><w:drawing><wp:inline><a:blip r:embed="rId6"/></wp:inline></w:drawing></w:r></w:p>"#
        } else {
            ""
        };
        let doc = format!(
            r#"<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body>
<w:p><w:pPr><w:pStyle w:val="1"/></w:pPr><w:r><w:t>优惠券需求</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">结算页要显示</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>优惠</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>明细</w:t></w:r><w:r><w:t>。</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="H2"/></w:pPr><w:r><w:t>规则</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>每单限用一张</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>过期不可用</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>第一步</w:t></w:r></w:p>
<w:p><w:r><w:drawing><wp:inline><wp:docPr id="1" name="图片 1" descr="结算页示意"/><a:graphic><a:blip r:embed="rId5"/></a:graphic></wp:inline></w:drawing></w:r></w:p>
{bad}
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>字段</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>说明</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>amount</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>金额|分</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:body></w:document>"#
        );
        put("word/document.xml", doc.as_bytes());
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn docx_keeps_headings_lists_tables_and_images() {
        let p = parse("需求.docx", &docx_bytes(false), None).unwrap();
        let expected = "# 优惠券需求\n\n结算页要显示**优惠明细**。\n\n## 规则\n\n- 每单限用一张\n- 过期不可用\n1. 第一步\n\n![结算页示意](import:0)\n\n| 字段 | 说明 |\n| --- | --- |\n| amount | 金额\\|分 |\n";
        assert_eq!(p.markdown, expected);
        assert_eq!(p.images.len(), 1);
        assert_eq!(p.images[0].file_name, "image1.png");
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn imported_images_become_reference_attachments() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = crate::db::open(&dir.path().join("t.db")).unwrap();
        let got = import(&conn, dir.path(), "需求.docx", &docx_bytes(true), None).unwrap();
        assert_eq!(got.attachments.len(), 1);
        let a = &got.attachments[0];
        assert_eq!((a.origin.as_str(), a.ticket_id.as_deref()), ("prd", None));
        assert!(got
            .markdown
            .contains(&format!("![结算页示意](attachment:{} \"reference\")", a.id)));
        assert!(got.markdown.contains("（图片 image2.emf 没有导入）"));
        assert!(!got.markdown.contains("import:"));
        assert_eq!(got.warnings.len(), 1, "{:?}", got.warnings);

        // 建单时认领
        let t = crate::store::tickets::create(
            &mut conn,
            crate::store::tickets::TicketInput {
                title: "优惠券".into(),
                kind: "feature_mixed".into(),
                content_md: got.markdown.clone(),
                source_kind: Some("file".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let imgs = attachments::images_of_version(&conn, t.current_version_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].authority, "reference");
        assert_eq!(imgs[0].attachment.ticket_id.as_deref(), Some(t.id.as_str()));
    }

    #[test]
    fn markdown_picks_up_relative_images_and_gbk_text() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("img")).unwrap();
        std::fs::write(dir.path().join("img/a.png"), PNG).unwrap();
        let md = "# 标题\n![示意](img/a.png)\n![外链](https://x/y.png)\n![丢了](img/none.png)\n";
        let p = parse("a.md", md.as_bytes(), Some(dir.path())).unwrap();
        assert!(p.markdown.contains("![示意](import:0)"));
        assert!(p.markdown.contains("![外链](https://x/y.png)"));
        assert!(p.markdown.contains("![丢了](img/none.png)"));
        assert_eq!((p.images.len(), p.warnings.len()), (1, 1));

        // 文档目录之外的图不自动带进来：`..` 和绝对路径都一样
        let outer = tempfile::tempdir().unwrap();
        std::fs::write(outer.path().join("secret.png"), PNG).unwrap();
        let docs = outer.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        let abs = outer.path().join("secret.png");
        let md = format!("![a](../secret.png)\n![b]({})\n", abs.display());
        let p = parse("a.md", md.as_bytes(), Some(&docs)).unwrap();
        assert!(p.images.is_empty(), "{}", p.markdown);
        assert_eq!(p.markdown, md);
        assert_eq!(p.warnings.len(), 2);
        assert!(p.warnings[0].contains("不在文档所在的目录里"));
    }

    #[test]
    fn oversized_entries_in_a_docx_are_not_read_into_memory() {
        assert_eq!(read_capped(&b"abcd"[..], 4).as_deref(), Some(&b"abcd"[..]));
        assert!(read_capped(&b"abcde"[..], 4).is_none());
        assert!(read_capped(std::io::repeat(0), 1024).is_none());

        let (gbk, _, _) = encoding_rs::GB18030.encode("需求：导出订单");
        assert_eq!(
            parse("a.txt", &gbk, None).unwrap().markdown,
            "需求：导出订单"
        );
        assert!(parse("a.doc", b"x", None).is_err());
        assert!(parse("a.xlsx", b"x", None).is_err());
    }
}

#[cfg(test)]
mod pdf_tests {
    use super::*;

    #[test]
    fn pdf_text_and_lossless_images_are_imported() {
        let bytes = include_bytes!("../tests/fixtures/sample.pdf");
        let p = parse("需求.pdf", bytes, None).unwrap();
        assert!(
            p.markdown
                .contains("Coupon requirement: show discount lines"),
            "{}",
            p.markdown
        );
        assert!(p.markdown.contains("Rule 1: one coupon per order"));
        // 里面那张无损位图转成了 PNG 带进来
        assert_eq!(p.images.len(), 1, "{:?}", p.warnings);
        assert_eq!(p.images[0].file_name, "pdf-image-1.png");
        assert_eq!(
            attachments::sniff(&p.images[0].bytes).map(|i| i.0),
            Some("image/png")
        );
        assert!(p.markdown.contains("![原 PDF 图 1](import:0)"));
        assert!(!p.warnings.iter().any(|w| w.contains("编码不支持")));
        assert!(parse("坏的.pdf", b"not a pdf", None).is_err());
    }

    fn image(pairs: Vec<(&str, pdf_extract::Object)>, content: Vec<u8>) -> pdf_extract::Stream {
        let mut dict = pdf_extract::Dictionary::new();
        dict.set("Subtype", pdf_extract::Object::Name(b"Image".to_vec()));
        for (k, v) in pairs {
            dict.set(k, v);
        }
        pdf_extract::Stream::new(dict, content)
    }

    fn name(n: &str) -> pdf_extract::Object {
        pdf_extract::Object::Name(n.as_bytes().to_vec())
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut e = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    /// 解出 PNG 的像素（调色板的展开成 RGB）。
    fn pixels(png_bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut d = png::Decoder::new(std::io::Cursor::new(png_bytes));
        d.set_transformations(png::Transformations::EXPAND);
        let mut r = d.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        (info.width, info.height, buf)
    }

    #[test]
    fn lossless_pdf_bitmaps_become_png() {
        use pdf_extract::Object;
        let doc = pdf_extract::Document::with_version("1.5");
        let rgb = [255, 0, 0, 0, 255, 0, 0, 0, 255, 10, 20, 30];
        let base = |cs: Object| {
            vec![
                ("Width", Object::Integer(2)),
                ("Height", Object::Integer(2)),
                ("BitsPerComponent", Object::Integer(8)),
                ("ColorSpace", cs),
            ]
        };

        // 没压缩的 RGB
        let s = image(base(name("DeviceRGB")), rgb.to_vec());
        assert_eq!(
            pixels(&pdf_bitmap_png(&doc, &s).unwrap()),
            (2, 2, rgb.to_vec())
        );

        // Flate + PNG 预测器（第一行 Sub，第二行 Up）
        let filtered = [
            1, 255, 0, 0, 1, 255, 0, // Sub：第二个像素存的是和左边的差
            2, 1, 0, 255, 10, 21, 30, // Up：存的是和上一行的差
        ];
        let mut pairs = base(name("DeviceRGB"));
        pairs.push(("Filter", name("FlateDecode")));
        let mut parms = pdf_extract::Dictionary::new();
        parms.set("Predictor", Object::Integer(15));
        parms.set("Colors", Object::Integer(3));
        parms.set("Columns", Object::Integer(2));
        pairs.push(("DecodeParms", Object::Dictionary(parms)));
        let s = image(pairs, zlib(&filtered));
        assert_eq!(
            pixels(&pdf_bitmap_png(&doc, &s).unwrap()),
            (2, 2, rgb.to_vec())
        );

        // 调色板
        let indexed = Object::Array(vec![
            name("Indexed"),
            name("DeviceRGB"),
            Object::Integer(1),
            Object::String(
                vec![9, 8, 7, 1, 2, 3],
                pdf_extract::StringFormat::Hexadecimal,
            ),
        ]);
        let mut pairs = base(indexed);
        pairs.push(("Filter", Object::Array(vec![name("FlateDecode")])));
        let s = image(pairs, zlib(&[0, 1, 1, 0]));
        assert_eq!(
            pixels(&pdf_bitmap_png(&doc, &s).unwrap()),
            (2, 2, vec![9, 8, 7, 1, 2, 3, 1, 2, 3, 9, 8, 7])
        );

        // 不认识的不硬转：CMYK、别的编码、数据不够、声明的尺寸大得离谱
        let s = image(base(name("DeviceCMYK")), vec![0; 16]);
        assert!(pdf_bitmap_png(&doc, &s).is_none());
        let mut pairs = base(name("DeviceRGB"));
        pairs.push(("Filter", name("JPXDecode")));
        assert!(pdf_bitmap_png(&doc, &image(pairs, rgb.to_vec())).is_none());
        assert!(pdf_bitmap_png(&doc, &image(base(name("DeviceRGB")), vec![0; 5])).is_none());
        let mut pairs = base(name("DeviceRGB"));
        pairs[0].1 = Object::Integer(30_000);
        pairs[1].1 = Object::Integer(30_000);
        pairs.push(("Filter", name("FlateDecode")));
        assert!(pdf_bitmap_png(&doc, &image(pairs, zlib(&[0; 64]))).is_none());
    }
}
