//! 演示 Agent 的进程体：按 Claude Code 的 stream-json 格式输出，在当前目录做一个小改动。
//! 不联网、不需要账号；用于走通流程和自动化测试。`twctl mock-agent` 和桌面应用自身都能充当它。
//!
//! 提示词里的标记可以改变行为（测试用）：`[mock:steps=5]`、`[mock:delay=800]`、`[mock:fail]`。
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::time::Duration;

pub struct MockArgs {
    pub model: String,
    pub permission: String,
    pub resume: Option<String>,
    pub steps: u32,
    pub delay_ms: u64,
}

fn emit(v: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn assistant(session: &str, content: Value) {
    emit(
        json!({"type":"assistant","session_id":session,"message":{"role":"assistant","content":[content]}}),
    );
}

/// 提示词中 `[mock:name=值]` 的值。
fn marker(prompt: &str, name: &str) -> Option<u64> {
    let tag = format!("[mock:{name}=");
    let rest = &prompt[prompt.find(&tag)? + tag.len()..];
    rest[..rest.find(']')?].trim().parse().ok()
}

pub fn run(mut args: MockArgs) -> i32 {
    let mut prompt = String::new();
    let _ = std::io::stdin().read_to_string(&mut prompt);
    if let Some(n) = marker(&prompt, "steps") {
        args.steps = n as u32;
    }
    if let Some(n) = marker(&prompt, "delay") {
        args.delay_ms = n;
    }
    let fail = prompt.contains("[mock:fail]");
    let session = args
        .resume
        .clone()
        .unwrap_or_else(|| format!("mock-{}", crate::util::new_id().to_lowercase()));
    let cwd = std::env::current_dir().unwrap_or_default();
    let pause = || std::thread::sleep(Duration::from_millis(args.delay_ms));
    let first_line = prompt
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("（空提示词）")
        .trim()
        .to_string();

    emit(
        json!({"type":"system","subtype":"init","session_id":session,"model":args.model,"cwd":cwd,"permissionMode":args.permission}),
    );
    pause();
    let verb = if args.resume.is_some() {
        "继续上次的会话"
    } else {
        "开始处理"
    };
    assistant(
        &session,
        json!({"type":"text","text":format!("{verb}：{first_line}")}),
    );

    let read_only = args.permission == "read_only";
    let file = cwd.join("TIDEWATCH_DEMO.md");
    let mut body =
        std::fs::read_to_string(&file).unwrap_or_else(|_| "# 观澜演示改动\n".to_string());
    for step in 1..=args.steps {
        pause();
        if read_only {
            assistant(
                &session,
                json!({"type":"tool_use","id":format!("toolu_{step}"),"name":"Read","input":{"file_path":"README.md"}}),
            );
            emit(
                json!({"type":"user","session_id":session,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":format!("toolu_{step}"),"content":"（只读模式：不修改文件）"}]}}),
            );
            continue;
        }
        body.push_str(&format!(
            "\n- 第 {step} 步（会话 {session}）：{first_line}\n"
        ));
        assistant(
            &session,
            json!({"type":"tool_use","id":format!("toolu_{step}"),"name":"Write","input":{"file_path":file,"content":body}}),
        );
        let res = std::fs::write(&file, &body);
        emit(
            json!({"type":"user","session_id":session,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":format!("toolu_{step}"),"content":match &res {Ok(_)=>"已写入".to_string(),Err(e)=>e.to_string()},"is_error":res.is_err()}]}}),
        );
    }
    pause();
    if fail {
        let msg = "演示 Agent 按提示词中的 [mock:fail] 标记失败。";
        emit(
            json!({"type":"result","subtype":"error_during_execution","is_error":true,"session_id":session,
            "num_turns":args.steps+1,"result":msg,"total_cost_usd":0.0,
            "usage":{"input_tokens":120,"output_tokens":20,"cache_read_input_tokens":0}}),
        );
        return 1;
    }
    let summary = if read_only {
        "只读检查完成，没有修改文件。".to_string()
    } else {
        format!("已在 TIDEWATCH_DEMO.md 追加 {} 条记录。", args.steps)
    };
    assistant(&session, json!({"type":"text","text":summary}));
    let chars = prompt.chars().count() as u64;
    emit(
        json!({"type":"result","subtype":"success","is_error":false,"session_id":session,"num_turns":args.steps+1,
        "duration_ms":args.delay_ms*(args.steps as u64+2),"result":summary,"total_cost_usd":0.0,
        "usage":{"input_tokens":chars/2+120,"output_tokens":80+40*args.steps as u64,"cache_read_input_tokens":0}}),
    );
    0
}

#[cfg(test)]
mod tests {
    use super::marker;

    #[test]
    fn reads_prompt_markers() {
        assert_eq!(marker("修一下 [mock:steps=5] 谢谢", "steps"), Some(5));
        assert_eq!(marker("[mock:delay=800]", "delay"), Some(800));
        assert_eq!(marker("[mock:steps=x]", "steps"), None);
        assert_eq!(marker("没有标记", "steps"), None);
    }
}
