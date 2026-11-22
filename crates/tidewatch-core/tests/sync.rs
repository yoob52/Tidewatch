//! 外部同步，对着本机的假 GitHub / 假飞书接口跑。
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tidewatch_core::db::{self, lock};
use tidewatch_core::secrets::MemoryStore;
use tidewatch_core::store::{attachments, tickets};
use tidewatch_core::sync::{self, SourceConfig, SourceInput};

type Reply = (u16, &'static str, Vec<u8>);

/// 假接口：`route(方法, 路径和查询, 请求体)` 给出响应；收到的请求都记下来。
struct Fake {
    url: String,
    log: Arc<Mutex<Vec<(String, String, String, Option<String>)>>>,
    server: Arc<tiny_http::Server>,
}

impl Fake {
    fn start(route: impl Fn(&str, &str, &str) -> Reply + Send + 'static) -> Fake {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let url = format!("http://{}", server.server_addr());
        let log = Arc::new(Mutex::new(vec![]));
        let (s, l) = (server.clone(), log.clone());
        std::thread::spawn(move || {
            for mut req in s.incoming_requests() {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let auth = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Authorization"))
                    .map(|h| h.value.to_string());
                let (method, path) = (req.method().to_string(), req.url().to_string());
                let (status, content_type, bytes) = route(&method, &path, &body);
                l.lock().unwrap().push((method, path, body, auth));
                let header = tiny_http::Header::from_bytes("Content-Type", content_type).unwrap();
                let _ = req.respond(
                    tiny_http::Response::from_data(bytes)
                        .with_status_code(status)
                        .with_header(header),
                );
            }
        });
        Fake { url, log, server }
    }

    fn requests(&self, method: &str) -> Vec<(String, String)> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.0 == method)
            .map(|r| (r.1.clone(), r.2.clone()))
            .collect()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

fn reply(v: Value) -> Reply {
    (200, "application/json", v.to_string().into_bytes())
}

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0,
    1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89,
];

#[test]
fn github_issues_become_tickets_and_changes_wait_for_the_user() {
    let dir = tempfile::tempdir().unwrap();
    let issues = Arc::new(Mutex::new(json!([
        { "number": 7, "title": "订单列表为空时崩溃", "body": "打开订单页直接白屏。", "state": "open",
          "html_url": "https://github.com/acme/shop/issues/7", "labels": [{ "name": "tidewatch" }, { "name": "Bug" }], "comments": 1 },
        { "number": 8, "title": "加优惠券页", "body": "", "state": "open",
          "html_url": "https://github.com/acme/shop/issues/8", "labels": [{ "name": "tidewatch" }], "comments": 0 },
        { "number": 9, "title": "一个 PR", "body": "x", "state": "open", "pull_request": {},
          "html_url": "https://github.com/acme/shop/pull/9", "labels": [], "comments": 0 },
    ])));
    let shared = issues.clone();
    let fake = Fake::start(move |method, path, _| match (method, path) {
        ("GET", p) if p.starts_with("/repos/acme/shop/issues?") => {
            reply(shared.lock().unwrap().clone())
        }
        ("GET", p) if p.starts_with("/repos/acme/shop/issues/7/comments") => reply(json!([
            { "id": 501, "user": { "login": "liu" }, "body": "安卓上也能复现", "html_url": "https://github.com/acme/shop/issues/7#issuecomment-501" }
        ])),
        ("POST", "/repos/acme/shop/issues/7/comments") => (201, "application/json", b"{}".to_vec()),
        ("PATCH", "/repos/acme/shop/issues/7") => reply(json!({ "state": "closed" })),
        _ => (
            404,
            "application/json",
            br#"{"message":"Not Found"}"#.to_vec(),
        ),
    });

    let db = db::shared(db::open(&dir.path().join("t.db")).unwrap());
    let secrets = MemoryStore::default();
    let source = sync::create(
        &lock(&db),
        SourceInput {
            kind: "github".into(),
            name: "商城".into(),
            enabled: true,
            config: SourceConfig {
                repo: "https://github.com/acme/shop".into(),
                labels: vec!["tidewatch".into()],
                write_back: true,
                api_base: Some(fake.url.clone()),
                ..Default::default()
            },
        },
    )
    .unwrap();
    assert_eq!(source.config.repo, "acme/shop");
    let source = sync::store_secret(&lock(&db), &secrets, &source.id, Some("ghp_test")).unwrap();
    assert!(source.has_secret);

    // 第一次：两个 Issue 建成草稿工单，PR 跳过，评论进时间线
    let r = sync::run(&db, &secrets, dir.path(), &source.id).unwrap();
    assert_eq!(
        (r.created.len(), r.changed.len(), r.comments),
        (2, 0, 1),
        "{r:?}"
    );
    let list = tickets::list(&lock(&db), None).unwrap();
    let bug = list
        .iter()
        .find(|t| t.title.contains("崩溃"))
        .unwrap()
        .clone();
    let feature = list
        .iter()
        .find(|t| t.title.contains("优惠券"))
        .unwrap()
        .clone();
    assert_eq!(
        (bug.kind.as_str(), bug.source_kind.as_str()),
        ("bug", "github")
    );
    assert_eq!(feature.kind, "feature_mixed");
    assert!(tickets::current_prd(&lock(&db), &feature.id)
        .unwrap()
        .unwrap()
        .content_md
        .contains("没有正文"));
    let timeline = tickets::timeline(&lock(&db), &bug.id).unwrap();
    let comment = timeline
        .iter()
        .find(|e| e.kind == "source_comment")
        .unwrap();
    assert_eq!(comment.payload["author"], "liu");
    let first = fake.requests("GET")[0].0.clone();
    assert!(
        first.contains("state=open")
            && first.contains("labels=tidewatch")
            && !first.contains("since="),
        "{first}"
    );
    assert_eq!(
        fake.log.lock().unwrap()[0].3.as_deref(),
        Some("Bearer ghp_test")
    );

    // 第二次什么都没变：不重复建单，评论不重复
    let r = sync::run(&db, &secrets, dir.path(), &source.id).unwrap();
    assert_eq!(r, Default::default());
    assert_eq!(tickets::list(&lock(&db), None).unwrap().len(), 2);
    assert!(fake
        .requests("GET")
        .iter()
        .any(|r| r.0.contains("state=all") && r.0.contains("since=")));

    // Issue 正文改了：只记成待定，PRD 不动
    issues.lock().unwrap()[0]["body"] = json!("打开订单页直接白屏。\n\n补充：只在没有订单时出现。");
    let r = sync::run(&db, &secrets, dir.path(), &source.id).unwrap();
    assert_eq!(r.changed, [bug.key.clone()]);
    let ts = sync::ticket_source(&lock(&db), &bug.id).unwrap().unwrap();
    assert!(ts.pending_md.as_deref().unwrap().contains("补充"));
    assert_eq!(tickets::prd_versions(&lock(&db), &bug.id).unwrap().len(), 1);
    // 同一版不重复提示
    assert!(sync::run(&db, &secrets, dir.path(), &source.id)
        .unwrap()
        .changed
        .is_empty());

    // 导入为新版本
    assert!(!sync::needs_change_request(&bug).unwrap());
    sync::apply_pending(&mut lock(&db), &bug.id).unwrap();
    let versions = tickets::prd_versions(&lock(&db), &bug.id).unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[1].origin.as_deref(), Some("import"));
    assert!(sync::ticket_source(&lock(&db), &bug.id)
        .unwrap()
        .unwrap()
        .pending_rev
        .is_none());
    assert!(sync::apply_pending(&mut lock(&db), &bug.id).is_err());

    // 标题改了：忽略这一版，之后不再提示
    issues.lock().unwrap()[1]["title"] = json!("加优惠券页（二期）");
    assert_eq!(
        sync::run(&db, &secrets, dir.path(), &source.id)
            .unwrap()
            .changed,
        [feature.key.clone()]
    );
    sync::dismiss_pending(&lock(&db), &feature.id).unwrap();
    assert!(sync::run(&db, &secrets, dir.path(), &source.id)
        .unwrap()
        .changed
        .is_empty());
    assert_eq!(
        tickets::get(&lock(&db), &feature.id).unwrap().title,
        "加优惠券页"
    );

    // 已定稿的工单要先走变更流程
    let mut finalized = feature.clone();
    finalized.status = "finalized".into();
    assert!(sync::needs_change_request(&finalized).unwrap());
    finalized.status = "done".into();
    assert!(sync::needs_change_request(&finalized).is_err());

    // 工单完成后回写：评论一条并关闭，只做一次
    lock(&db)
        .execute("UPDATE ticket SET status = 'done' WHERE id = ?1", [&bug.id])
        .unwrap();
    let r = sync::run(&db, &secrets, dir.path(), &source.id).unwrap();
    assert_eq!(r.written_back, [bug.key.clone()]);
    let posts = fake.requests("POST");
    assert_eq!(posts.len(), 1);
    assert!(posts[0].1.contains(&bug.key), "{}", posts[0].1);
    assert!(fake.requests("PATCH")[0].1.contains("closed"));
    assert!(sync::run(&db, &secrets, dir.path(), &source.id)
        .unwrap()
        .written_back
        .is_empty());
    assert_eq!(fake.requests("POST").len(), 1);

    // 删掉来源：工单留着，令牌清掉
    sync::delete(&lock(&db), &secrets, &source.id).unwrap();
    assert!(sync::ticket_source(&lock(&db), &bug.id).unwrap().is_none());
    assert_eq!(tickets::list(&lock(&db), None).unwrap().len(), 2);
    use tidewatch_core::secrets::SecretStore;
    assert_eq!(secrets.get(&sync::secret_name(&source.id)).unwrap(), None);
}

#[test]
fn github_errors_are_kept_on_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::start(|_, _, _| {
        (
            404,
            "application/json",
            br#"{"message":"Not Found"}"#.to_vec(),
        )
    });
    let db = db::shared(db::open(&dir.path().join("t.db")).unwrap());
    let secrets = MemoryStore::default();
    let input = |repo: &str| SourceInput {
        kind: "github".into(),
        name: "x".into(),
        enabled: true,
        config: SourceConfig {
            repo: repo.into(),
            api_base: Some(fake.url.clone()),
            ..Default::default()
        },
    };
    assert!(sync::create(&lock(&db), input("not a repo")).is_err());
    let source = sync::create(&lock(&db), input("acme/private")).unwrap();
    sync::store_secret(&lock(&db), &secrets, &source.id, Some("t")).unwrap();
    let e = sync::run(&db, &secrets, dir.path(), &source.id)
        .unwrap_err()
        .to_string();
    assert!(e.contains("404") && e.contains("仓库不存在"), "{e}");
    let after = sync::get(&lock(&db), &source.id).unwrap();
    assert!(after.last_error.unwrap().contains("404"));
    assert!(after.last_synced_at.is_none());
}

#[test]
fn feishu_doc_is_imported_by_url_and_new_revisions_wait_for_the_user() {
    let dir = tempfile::tempdir().unwrap();
    let revision = Arc::new(Mutex::new(12));
    let shared = revision.clone();
    let fake = Fake::start(move |method, path, body| {
        let rev = *shared.lock().unwrap();
        match (method, path) {
            ("POST", "/open-apis/auth/v3/tenant_access_token/internal") => {
                let b: Value = serde_json::from_str(body).unwrap();
                if b["app_id"] == "cli_app" && b["app_secret"] == "s3cret" {
                    reply(
                        json!({ "code": 0, "msg": "ok", "tenant_access_token": "t-abc", "expire": 7200 }),
                    )
                } else {
                    reply(json!({ "code": 10014, "msg": "app secret invalid" }))
                }
            }
            ("GET", "/open-apis/wiki/v2/spaces/get_node?token=WikiTok") => {
                reply(json!({ "code": 0,
                "data": { "node": { "obj_type": "docx", "obj_token": "DocA1" } } }))
            }
            ("GET", "/open-apis/docx/v1/documents/DocA1") => reply(json!({ "code": 0,
                "data": { "document": { "document_id": "DocA1", "revision_id": rev, "title": "优惠券需求" } } })),
            ("GET", p) if p.starts_with("/open-apis/docx/v1/documents/DocA1/blocks") => {
                let second = p.contains("page_token=next");
                let text = if rev == 12 {
                    "每单限用一张"
                } else {
                    "每单限用两张"
                };
                if second {
                    reply(json!({ "code": 0, "data": { "has_more": false, "items": [
                        { "block_id": "img", "block_type": 27, "image": { "token": "imgTok" } },
                    ] } }))
                } else {
                    reply(
                        json!({ "code": 0, "data": { "has_more": true, "page_token": "next", "items": [
                        { "block_id": "root", "block_type": 1, "children": ["h", "p", "img"] },
                        { "block_id": "h", "block_type": 4, "heading2": { "elements": [{ "text_run": { "content": "规则" } }] } },
                        { "block_id": "p", "block_type": 2, "text": { "elements": [{ "text_run": { "content": text } }] } },
                    ] } }),
                    )
                }
            }
            ("GET", "/open-apis/drive/v1/medias/imgTok/download") => {
                (200, "image/png", PNG.to_vec())
            }
            _ => (
                404,
                "application/json",
                br#"{"code":404,"msg":"not found"}"#.to_vec(),
            ),
        }
    });
    let db = db::shared(db::open(&dir.path().join("t.db")).unwrap());
    let secrets = MemoryStore::default();
    let source = sync::create(
        &lock(&db),
        SourceInput {
            kind: "feishu".into(),
            name: "产品文档".into(),
            enabled: true,
            config: SourceConfig {
                app_id: "cli_app".into(),
                api_base: Some(fake.url.clone()),
                ticket_type: "feature_backend".into(),
                ..Default::default()
            },
        },
    )
    .unwrap();
    let url = "https://acme.feishu.cn/wiki/WikiTok?from=share";

    // 没填 App Secret、填错，都有能看懂的提示
    let e = sync::import_feishu(&db, &secrets, dir.path(), &source.id, url)
        .unwrap_err()
        .to_string();
    assert!(e.contains("App Secret"), "{e}");
    sync::store_secret(&lock(&db), &secrets, &source.id, Some("wrong")).unwrap();
    let e = sync::import_feishu(&db, &secrets, dir.path(), &source.id, url)
        .unwrap_err()
        .to_string();
    assert!(e.contains("10014"), "{e}");
    sync::store_secret(&lock(&db), &secrets, &source.id, Some("s3cret")).unwrap();

    let (t, warnings) = sync::import_feishu(&db, &secrets, dir.path(), &source.id, url).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        (t.title.as_str(), t.kind.as_str(), t.source_kind.as_str()),
        ("优惠券需求", "feature_backend", "feishu")
    );
    let v1 = tickets::current_prd(&lock(&db), &t.id).unwrap().unwrap();
    assert!(
        v1.content_md
            .starts_with("## 规则\n\n每单限用一张\n\n![图片](attachment:"),
        "{}",
        v1.content_md
    );
    let images = attachments::images_of_version(&lock(&db), &v1.id).unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].authority, "reference");
    let ts = sync::ticket_source(&lock(&db), &t.id).unwrap().unwrap();
    assert_eq!(
        (ts.external_id.as_str(), ts.rev.as_str(), ts.url.as_str()),
        ("DocA1", "12", "https://acme.feishu.cn/wiki/WikiTok")
    );
    assert!(fake
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.0 == "GET")
        .all(|r| r.3.as_deref() == Some("Bearer t-abc")));

    // 同一篇不导第二次
    let e = sync::import_feishu(
        &db,
        &secrets,
        dir.path(),
        &source.id,
        "https://acme.feishu.cn/docx/DocA1",
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains(&t.key), "{e}");

    // 文档没变：不提示；文档出了新版本：记成待定，导入后是第二版
    assert_eq!(
        sync::run(&db, &secrets, dir.path(), &source.id).unwrap(),
        Default::default()
    );
    *revision.lock().unwrap() = 13;
    let r = sync::run(&db, &secrets, dir.path(), &source.id).unwrap();
    assert_eq!(r.changed, [t.key.clone()]);
    assert!(sync::run(&db, &secrets, dir.path(), &source.id)
        .unwrap()
        .changed
        .is_empty());
    sync::apply_pending(&mut lock(&db), &t.id).unwrap();
    let v2 = tickets::current_prd(&lock(&db), &t.id).unwrap().unwrap();
    assert_eq!(v2.version, 2);
    assert!(v2.content_md.contains("每单限用两张"));
    assert_eq!(
        attachments::images_of_version(&lock(&db), &v2.id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        sync::ticket_source(&lock(&db), &t.id).unwrap().unwrap().rev,
        "13"
    );
}
