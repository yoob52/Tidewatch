//! 文件导入：`parse(file) -> { markdown, images[] }`。支持 Markdown / 文本、docx、pdf。
//! 解析只做"尽量保留结构的纯文本化"；图片抽出来交给附件，导入的图一律是仅参考图。
use crate::store::attachments::{self, Attachment};
use crate::{Error, Result};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

pub const MAX_FILE_BYTES: usize = 50 * 1024 * 1024;

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
        let local = base_dir
            .filter(|_| !remote)
            .map(|d| d.join(target.trim_matches(|c| c == '<' || c == '>')));
        match local.as_ref().map(std::fs::read) {
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
            Some(Err(_)) => {
                out.warnings
                    .push(format!("找不到正文引用的图片 {target}，保留了原来的写法。"));
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
    let mut f = z.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
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
                        let mut buf = vec![];
                        let read = z
                            .by_name(&path)
                            .ok()
                            .and_then(|mut f| f.read_to_end(&mut buf).ok());
                        read.map(|_| {
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
                        .push(format!("文档里的图片 {path} 读不出来，已跳过。")),
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
    // 内嵌图片：只取 JPEG（DCTDecode，可以原样取出）；其它编码的位图要重新编码，这里不做
    let owned = bytes.to_vec();
    let (jpegs, skipped) = std::panic::catch_unwind(move || {
        let mut jpegs: Vec<Vec<u8>> = vec![];
        let mut skipped = 0usize;
        if let Ok(doc) = pdf_extract::Document::load_mem(&owned) {
            for obj in doc.objects.values() {
                let pdf_extract::Object::Stream(s) = obj else {
                    continue;
                };
                let is_image = s
                    .dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    .is_some_and(|n| n == b"Image");
                if !is_image {
                    continue;
                }
                let dct = match s.dict.get(b"Filter") {
                    Ok(pdf_extract::Object::Name(n)) => n == b"DCTDecode",
                    Ok(pdf_extract::Object::Array(a)) => {
                        a.len() == 1 && a[0].as_name().is_ok_and(|n| n == b"DCTDecode")
                    }
                    _ => false,
                };
                if dct && s.content.starts_with(b"\xff\xd8") {
                    jpegs.push(s.content.clone());
                } else {
                    skipped += 1;
                }
            }
        }
        (jpegs, skipped)
    })
    .unwrap_or_default();
    if !jpegs.is_empty() {
        md.push_str("\n## 文档里的图片\n\n");
        for (i, bytes) in jpegs.into_iter().enumerate() {
            md.push_str(&format!("![原 PDF 图 {}](import:{i})\n\n", i + 1));
            out.images.push(ParsedImage {
                file_name: format!("pdf-image-{}.jpg", i + 1),
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
            "PDF 里有 {skipped} 张图片不是 JPEG 编码，没有导入；需要的话请截图后贴进来。"
        ));
    }
    out.markdown = md;
    Ok(out)
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
    fn pdf_text_is_extracted_and_unsupported_images_are_reported() {
        let bytes = include_bytes!("../tests/fixtures/sample.pdf");
        let p = parse("需求.pdf", bytes, None).unwrap();
        assert!(
            p.markdown
                .contains("Coupon requirement: show discount lines"),
            "{}",
            p.markdown
        );
        assert!(p.markdown.contains("Rule 1: one coupon per order"));
        assert!(p.images.is_empty());
        assert!(
            p.warnings.iter().any(|w| w.contains("不是 JPEG 编码")),
            "{:?}",
            p.warnings
        );
        assert!(parse("坏的.pdf", b"not a pdf", None).is_err());
    }
}
