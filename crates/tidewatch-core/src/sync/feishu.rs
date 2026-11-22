//! 飞书文档：用自建应用的 App ID / App Secret 按链接读新版文档（docx），转成 Markdown。
//!
//! 按开放平台文档写成，**没有对着真实的飞书验证过**（没有可用的自建应用）；
//! 测试用的是照文档手写的响应。
use std::collections::HashMap;

use serde_json::{json, Value};

use super::http;
use crate::import::{Parsed, ParsedImage};
use crate::{Error, Result};

pub const DEFAULT_API: &str = "https://open.feishu.cn";
const MAX_BLOCK_PAGES: usize = 40;
const MAX_IMAGES: usize = 60;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct Doc {
    pub document_id: String,
    pub title: String,
    /// 文档版本号，变了就是"源已变更"。
    pub revision: String,
    pub parsed: Parsed,
}

/// 链接里的文档：`/docx/<id>` 或知识库 `/wiki/<token>`。
#[derive(Debug, Clone, PartialEq)]
pub enum DocRef {
    Docx(String),
    Wiki(String),
}

pub fn parse_url(url: &str) -> Result<DocRef> {
    let path = url
        .trim()
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let mut parts = path.rsplit('/');
    let token = parts.next().unwrap_or("");
    let kind = parts.next().unwrap_or("");
    let ok = !token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    match (kind, ok) {
        ("docx", true) => Ok(DocRef::Docx(token.into())),
        ("wiki", true) => Ok(DocRef::Wiki(token.into())),
        ("docs" | "doc", _) => Err(Error::invalid(
            "这是旧版飞书文档（/docs/），只支持新版文档（/docx/）和知识库（/wiki/）里的新版文档。",
        )),
        _ => Err(Error::invalid(
            "认不出这个飞书链接。支持 https://<企业>.feishu.cn/docx/<文档 id> 和 /wiki/<节点>。",
        )),
    }
}

pub struct Client {
    api: String,
    token: String,
}

impl Client {
    /// 用 App ID / App Secret 换 tenant_access_token。
    pub fn login(api: &str, app_id: &str, app_secret: &str) -> Result<Client> {
        let api = api.trim_end_matches('/').to_string();
        let resp = http::send(
            "POST",
            &format!("{api}/open-apis/auth/v3/tenant_access_token/internal"),
            &[],
            &json!({ "app_id": app_id, "app_secret": app_secret }),
        )?;
        let v = resp.json();
        match v["tenant_access_token"].as_str() {
            Some(t) if v["code"].as_i64() == Some(0) => Ok(Client {
                api,
                token: t.to_string(),
            }),
            _ => Err(Error::invalid(format!(
                "飞书应用登录失败（{} {}）：检查 App ID 和 App Secret。",
                v["code"],
                v["msg"].as_str().unwrap_or("")
            ))),
        }
    }

    fn get(&self, path: &str, what: &str) -> Result<Value> {
        let resp = http::get(
            &format!("{}{path}", self.api),
            &[("Authorization", format!("Bearer {}", self.token))],
        )?;
        let v = resp.json();
        if resp.ok() && v["code"].as_i64() == Some(0) {
            return Ok(v["data"].clone());
        }
        Err(Error::invalid(format!(
            "{what}失败（HTTP {}，飞书错误码 {}：{}）。要给应用开通「查看新版文档」权限，并把文档分享给这个应用。",
            resp.status,
            v["code"],
            v["msg"].as_str().unwrap_or("")
        )))
    }

    fn resolve(&self, doc: &DocRef) -> Result<String> {
        match doc {
            DocRef::Docx(id) => Ok(id.clone()),
            DocRef::Wiki(token) => {
                let data = self.get(
                    &format!(
                        "/open-apis/wiki/v2/spaces/get_node?token={}",
                        http::enc(token)
                    ),
                    "读取知识库节点",
                )?;
                let node = &data["node"];
                match (node["obj_type"].as_str(), node["obj_token"].as_str()) {
                    (Some("docx"), Some(id)) => Ok(id.to_string()),
                    (kind, _) => Err(Error::invalid(format!(
                        "这个知识库节点是「{}」，不是新版文档，不能导入。",
                        kind.unwrap_or("未知类型")
                    ))),
                }
            }
        }
    }

    /// 只读标题和版本号（检查有没有更新用）。
    pub fn meta(&self, doc: &DocRef) -> Result<Doc> {
        let id = self.resolve(doc)?;
        let data = self.get(
            &format!("/open-apis/docx/v1/documents/{}", http::enc(&id)),
            "读取飞书文档",
        )?;
        let d = &data["document"];
        Ok(Doc {
            document_id: id,
            title: d["title"].as_str().unwrap_or("").trim().to_string(),
            revision: match &d["revision_id"] {
                Value::Number(n) => n.to_string(),
                other => other.as_str().unwrap_or("").to_string(),
            },
            parsed: Parsed::default(),
        })
    }

    /// 读整篇文档，图片一并下载。
    pub fn fetch(&self, doc: &DocRef) -> Result<Doc> {
        let mut out = self.meta(doc)?;
        let mut blocks: Vec<Value> = vec![];
        let mut page_token = String::new();
        for _ in 0..MAX_BLOCK_PAGES {
            let mut path = format!(
                "/open-apis/docx/v1/documents/{}/blocks?page_size=500&document_revision_id=-1",
                http::enc(&out.document_id)
            );
            if !page_token.is_empty() {
                path.push_str(&format!("&page_token={}", http::enc(&page_token)));
            }
            let data = self.get(&path, "读取飞书文档内容")?;
            blocks.extend(data["items"].as_array().cloned().unwrap_or_default());
            page_token = data["page_token"].as_str().unwrap_or("").to_string();
            if data["has_more"].as_bool() != Some(true) || page_token.is_empty() {
                break;
            }
        }
        let (markdown, tokens) = to_markdown(&blocks);
        let mut parsed = Parsed {
            markdown,
            ..Default::default()
        };
        for (i, token) in tokens.iter().enumerate() {
            let placeholder = format!("(feishu-image:{token})");
            let fetched = if i < MAX_IMAGES {
                self.image(token)
            } else {
                Err(Error::invalid(format!("图片超过 {MAX_IMAGES} 张")))
            };
            match fetched {
                Ok(img) => {
                    parsed.markdown = parsed
                        .markdown
                        .replace(&placeholder, &format!("(import:{})", parsed.images.len()));
                    parsed.images.push(img);
                }
                Err(e) => {
                    parsed.warnings.push(format!("有一张图片没有导入：{e}"));
                    parsed.markdown = parsed.markdown.replace(
                        &format!("![图片]{placeholder}"),
                        "（这里有一张图片没有导入）",
                    );
                }
            }
        }
        out.parsed = parsed;
        Ok(out)
    }

    fn image(&self, token: &str) -> Result<ParsedImage> {
        let resp = http::request(
            "GET",
            &format!(
                "{}/open-apis/drive/v1/medias/{}/download",
                self.api,
                http::enc(token)
            ),
            &[("Authorization", format!("Bearer {}", self.token))],
            None,
            MAX_IMAGE_BYTES,
        )?;
        if !resp.ok() {
            return Err(Error::invalid(format!("下载返回 {}", resp.status)));
        }
        let ext = match resp.content_type.split(';').next().unwrap_or("").trim() {
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => "png",
        };
        Ok(ParsedImage {
            file_name: format!("feishu-{token}.{ext}"),
            bytes: resp.body,
        })
    }
}

/// 文本类块里装内容的字段名。
fn text_key(block_type: i64) -> Option<&'static str> {
    Some(match block_type {
        2 => "text",
        3 => "heading1",
        4 => "heading2",
        5 => "heading3",
        6 => "heading4",
        7 => "heading5",
        8 => "heading6",
        9 => "heading7",
        10 => "heading8",
        11 => "heading9",
        12 => "bullet",
        13 => "ordered",
        14 => "code",
        15 => "quote",
        17 => "todo",
        _ => return None,
    })
}

fn inline(elements: &Value, plain: bool) -> String {
    let mut out = String::new();
    for el in elements.as_array().map(Vec::as_slice).unwrap_or_default() {
        if let Some(run) = el.get("text_run") {
            let text = run["content"].as_str().unwrap_or("");
            let style = &run["text_element_style"];
            if plain || text.trim().is_empty() {
                out.push_str(text);
                continue;
            }
            let mut s = text.to_string();
            if style["inline_code"].as_bool() == Some(true) {
                s = format!("`{s}`");
            }
            if style["bold"].as_bool() == Some(true) {
                s = format!("**{s}**");
            }
            if style["italic"].as_bool() == Some(true) {
                s = format!("*{s}*");
            }
            if style["strikethrough"].as_bool() == Some(true) {
                s = format!("~~{s}~~");
            }
            if let Some(url) = style["link"]["url"].as_str() {
                s = format!("[{s}]({})", percent_decode(url));
            }
            out.push_str(&s);
        } else if let Some(m) = el.get("mention_doc") {
            let title = m["title"].as_str().unwrap_or("文档");
            match m["url"].as_str() {
                Some(url) if !plain => out.push_str(&format!("[{title}]({})", percent_decode(url))),
                _ => out.push_str(title),
            }
        } else if let Some(eq) = el.get("equation") {
            out.push_str(&format!(
                "${}$",
                eq["content"].as_str().unwrap_or("").trim()
            ));
        } else if el.get("mention_user").is_some() {
            out.push_str("@某人");
        }
    }
    out
}

/// 飞书把链接地址整个做了百分号编码。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((a * 16 + b) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Render<'a> {
    by_id: HashMap<&'a str, &'a Value>,
    out: String,
    images: Vec<String>,
    /// 最外层上一个列表项的种类（12 无序 / 13 有序 / 17 待办）：种类变了就另起一个列表。
    top_list: i64,
}

impl Render<'_> {
    fn children(&mut self, block: &Value, depth: usize) {
        let ids: Vec<String> = block["children"]
            .as_array()
            .map(|c| {
                c.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let mut ordinal = 0;
        for id in ids {
            let Some(child) = self.by_id.get(id.as_str()).copied() else {
                continue;
            };
            if child["block_type"].as_i64() == Some(13) {
                ordinal += 1;
            } else {
                ordinal = 0;
            }
            self.block(child, depth, ordinal);
        }
    }

    fn para(&mut self, text: &str) {
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push_str(if self.out.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            });
        }
        self.out.push_str(text);
        self.out.push_str("\n\n");
    }

    fn item(&mut self, kind: i64, marker: &str, text: &str, depth: usize) {
        // 同一个列表的项之间不空行；换了一种列表就留着空行，另起一个
        let same_list = depth > 0 || self.top_list == kind;
        if same_list && self.out.ends_with("\n\n") && self.last_is_item() {
            self.out.pop();
        }
        if depth == 0 {
            self.top_list = kind;
        }
        self.out
            .push_str(&format!("{}{marker} {text}\n", "  ".repeat(depth)));
    }

    fn last_is_item(&self) -> bool {
        self.out
            .trim_end()
            .rsplit('\n')
            .next()
            .map(|l| {
                let l = l.trim_start();
                l.starts_with("- ")
                    || l.split_once(". ")
                        .is_some_and(|(n, _)| n.parse::<u32>().is_ok())
            })
            .unwrap_or(false)
    }

    fn block(&mut self, b: &Value, depth: usize, ordinal: usize) {
        let kind = b["block_type"].as_i64().unwrap_or(0);
        let text = |plain: bool| {
            text_key(kind)
                .map(|k| inline(&b[k]["elements"], plain))
                .unwrap_or_default()
        };
        match kind {
            1 => self.children(b, depth),
            2 => {
                let t = text(false);
                if !t.trim().is_empty() {
                    if depth > 0 {
                        self.out.push_str(&format!("{}{t}\n", "  ".repeat(depth)));
                    } else {
                        self.para(&t);
                    }
                }
                self.children(b, depth);
            }
            3..=11 => {
                let level = ((kind - 2) as usize).min(6);
                self.para(&format!("{} {}", "#".repeat(level), text(false)));
            }
            12 | 13 | 17 => {
                let marker = match kind {
                    12 => "-".to_string(),
                    13 => format!("{}.", ordinal.max(1)),
                    _ if b["todo"]["style"]["done"].as_bool() == Some(true) => "- [x]".into(),
                    _ => "- [ ]".into(),
                };
                self.item(kind, &marker, &text(false), depth);
                self.children(b, depth + 1);
                if depth == 0 {
                    self.out.push('\n');
                }
            }
            14 => {
                let fence = format!("```\n{}\n```", text(true).trim_end());
                self.para(&fence);
            }
            15 => self.para(&format!("> {}", text(false))),
            22 => self.para("---"),
            27 => {
                if let Some(token) = b["image"]["token"].as_str() {
                    self.images.push(token.to_string());
                    self.para(&format!("![图片](feishu-image:{token})"));
                }
            }
            31 => self.table(b),
            // 高亮块、引用容器、分栏、表格单元格等容器：只取里面的内容
            _ => self.children(b, depth),
        }
    }

    fn table(&mut self, b: &Value) {
        let cols = b["table"]["property"]["column_size"].as_u64().unwrap_or(0) as usize;
        let cells: Vec<String> = b["table"]["cells"]
            .as_array()
            .map(|c| {
                c.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if cols == 0 || cells.is_empty() {
            return;
        }
        let mut rows: Vec<Vec<String>> = vec![];
        for chunk in cells.chunks(cols) {
            let mut row = vec![];
            for id in chunk {
                let mut inner = Render {
                    by_id: self.by_id.clone(),
                    out: String::new(),
                    images: vec![],
                    top_list: 0,
                };
                if let Some(cell) = self.by_id.get(id.as_str()).copied() {
                    inner.children(cell, 0);
                }
                self.images.append(&mut inner.images);
                row.push(inner.out.trim().replace('\n', " ").replace('|', "\\|"));
            }
            row.resize(cols, String::new());
            rows.push(row);
        }
        let mut md = format!("| {} |\n|{}\n", rows[0].join(" | "), " --- |".repeat(cols));
        for row in &rows[1..] {
            md.push_str(&format!("| {} |\n", row.join(" | ")));
        }
        self.para(md.trim_end());
    }
}

/// 块列表转 Markdown。图片写成 `![图片](feishu-image:<token>)`，第二个返回值是这些 token。
pub fn to_markdown(blocks: &[Value]) -> (String, Vec<String>) {
    let by_id: HashMap<&str, &Value> = blocks
        .iter()
        .filter_map(|b| b["block_id"].as_str().map(|id| (id, b)))
        .collect();
    let mut r = Render {
        by_id,
        out: String::new(),
        images: vec![],
        top_list: 0,
    };
    if let Some(root) = blocks.iter().find(|b| b["block_type"].as_i64() == Some(1)) {
        r.block(root, 0, 0);
    }
    let mut md = r.out.trim().to_string();
    while md.contains("\n\n\n") {
        md = md.replace("\n\n\n", "\n\n");
    }
    md.push('\n');
    (md, r.images)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Value {
        json!({ "elements": [{ "text_run": { "content": text, "text_element_style": {} } }] })
    }

    #[test]
    fn urls() {
        assert_eq!(
            parse_url("https://acme.feishu.cn/docx/AbCd123?from=x#h").unwrap(),
            DocRef::Docx("AbCd123".into())
        );
        assert_eq!(
            parse_url("https://acme.larksuite.com/wiki/WikiTok_1/").unwrap(),
            DocRef::Wiki("WikiTok_1".into())
        );
        assert!(parse_url("https://acme.feishu.cn/docs/doccnOld")
            .unwrap_err()
            .to_string()
            .contains("旧版"));
        assert!(parse_url("https://acme.feishu.cn/sheets/x").is_err());
        assert!(parse_url("https://acme.feishu.cn/docx/a/../b").is_err());
    }

    #[test]
    fn blocks_become_markdown() {
        let blocks = vec![
            json!({ "block_id": "root", "block_type": 1, "children": ["h", "p", "b1", "b2", "o1", "o2", "todo", "code", "q", "div", "img", "tbl", "callout"] }),
            json!({ "block_id": "h", "block_type": 3, "heading1": run("优惠券") }),
            json!({ "block_id": "p", "block_type": 2, "text": { "elements": [
                { "text_run": { "content": "结算页显示", "text_element_style": {} } },
                { "text_run": { "content": "优惠明细", "text_element_style": { "bold": true } } },
                { "text_run": { "content": "，见", "text_element_style": {} } },
                { "text_run": { "content": "接口", "text_element_style": { "link": { "url": "https%3A%2F%2Fx.cn%2Fapi%3Fa%3D1" } } } },
                { "text_run": { "content": " 用 ", "text_element_style": {} } },
                { "text_run": { "content": "GET", "text_element_style": { "inline_code": true } } },
            ] } }),
            json!({ "block_id": "b1", "block_type": 12, "bullet": run("每单一张"), "children": ["b1a"] }),
            json!({ "block_id": "b1a", "block_type": 12, "bullet": run("含运费券") }),
            json!({ "block_id": "b2", "block_type": 12, "bullet": run("过期不可用") }),
            json!({ "block_id": "o1", "block_type": 13, "ordered": run("第一步") }),
            json!({ "block_id": "o2", "block_type": 13, "ordered": run("第二步") }),
            json!({ "block_id": "todo", "block_type": 17, "todo": { "elements": run("补埋点")["elements"], "style": { "done": true } } }),
            json!({ "block_id": "code", "block_type": 14, "code": run("GET /api/coupons\n200 **x**") }),
            json!({ "block_id": "q", "block_type": 15, "quote": run("以产品稿为准") }),
            json!({ "block_id": "div", "block_type": 22, "divider": {} }),
            json!({ "block_id": "img", "block_type": 27, "image": { "token": "imgTok1" } }),
            json!({ "block_id": "tbl", "block_type": 31, "table": { "cells": ["c1", "c2", "c3", "c4"], "property": { "row_size": 2, "column_size": 2 } } }),
            json!({ "block_id": "c1", "block_type": 32, "children": ["c1t"] }),
            json!({ "block_id": "c1t", "block_type": 2, "text": run("字段") }),
            json!({ "block_id": "c2", "block_type": 32, "children": ["c2t"] }),
            json!({ "block_id": "c2t", "block_type": 2, "text": run("说明") }),
            json!({ "block_id": "c3", "block_type": 32, "children": ["c3t"] }),
            json!({ "block_id": "c3t", "block_type": 2, "text": run("a|b") }),
            json!({ "block_id": "c4", "block_type": 32, "children": [] }),
            json!({ "block_id": "callout", "block_type": 19, "children": ["ct"] }),
            json!({ "block_id": "ct", "block_type": 2, "text": run("注意时区") }),
        ];
        let (md, images) = to_markdown(&blocks);
        let expected = "# 优惠券\n\n结算页显示**优惠明细**，见[接口](https://x.cn/api?a=1) 用 `GET`\n\n- 每单一张\n  - 含运费券\n- 过期不可用\n\n1. 第一步\n2. 第二步\n\n- [x] 补埋点\n\n```\nGET /api/coupons\n200 **x**\n```\n\n> 以产品稿为准\n\n---\n\n![图片](feishu-image:imgTok1)\n\n| 字段 | 说明 |\n| --- | --- |\n| a\\|b |  |\n\n注意时区\n";
        assert_eq!(md, expected);
        assert_eq!(images, ["imgTok1"]);
    }
}
