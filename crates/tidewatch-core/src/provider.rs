//! Provider：按进程注入（环境变量 / 命令行覆盖），不写用户的 ~/.claude、~/.codex。
use crate::agent::AgentKind;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// 官方默认：沿用本机登录态，不注入任何东西。
    Native,
    Anthropic,
    OpenaiResponses,
    OpenaiChat,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Native => "native",
            Protocol::Anthropic => "anthropic",
            Protocol::OpenaiResponses => "openai_responses",
            Protocol::OpenaiChat => "openai_chat",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "native" => Ok(Protocol::Native),
            "anthropic" => Ok(Protocol::Anthropic),
            "openai_responses" => Ok(Protocol::OpenaiResponses),
            "openai_chat" => Ok(Protocol::OpenaiChat),
            other => Err(Error::invalid(format!("未知协议：{other}"))),
        }
    }
    pub fn supports(self, kind: AgentKind) -> bool {
        matches!(
            (self, kind),
            (_, AgentKind::Mock)
                | (Protocol::Native, _)
                | (Protocol::Anthropic, AgentKind::ClaudeCode)
                | (
                    Protocol::OpenaiResponses | Protocol::OpenaiChat,
                    AgentKind::Codex
                )
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub protocol: Protocol,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    pub small_model: Option<String>,
    #[serde(default)]
    pub extra_env: Vec<(String, String)>,
}

/// 注入结果：只作用于本次子进程。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Injection {
    pub env: Vec<(String, String)>,
    pub args: Vec<String>,
    pub model: Option<String>,
}

/// Codex 读取密钥的环境变量名（`model_providers.tidewatch.env_key`）。
pub const CODEX_KEY_ENV: &str = "TIDEWATCH_PROVIDER_KEY";
const CODEX_PROVIDER_ID: &str = "tidewatch";

pub fn inject(
    kind: AgentKind,
    provider: &ProviderConfig,
    secret: Option<&str>,
) -> Result<Injection> {
    if !provider.protocol.supports(kind) {
        return Err(Error::invalid(format!(
            "Provider「{}」的协议 {} 与 {} 不兼容，需要经 LiteLLM 之类的代理桥接。",
            provider.name,
            provider.protocol.as_str(),
            kind.display_name()
        )));
    }
    let mut inj = Injection {
        model: provider.default_model.clone(),
        ..Default::default()
    };
    if provider.protocol == Protocol::Native || kind == AgentKind::Mock {
        inj.env = provider.extra_env.clone();
        return Ok(inj);
    }
    match kind {
        AgentKind::ClaudeCode => {
            match (&provider.base_url, secret) {
                (Some(url), key) => {
                    inj.env.push((
                        "ANTHROPIC_BASE_URL".into(),
                        url.trim_end_matches('/').to_string(),
                    ));
                    if let Some(k) = key {
                        inj.env.push(("ANTHROPIC_AUTH_TOKEN".into(), k.to_string()));
                    }
                }
                (None, Some(k)) => inj.env.push(("ANTHROPIC_API_KEY".into(), k.to_string())),
                (None, None) => {}
            }
            if let Some(m) = &provider.default_model {
                inj.env.push(("ANTHROPIC_MODEL".into(), m.clone()));
            }
            if let Some(m) = &provider.small_model {
                inj.env
                    .push(("ANTHROPIC_SMALL_FAST_MODEL".into(), m.clone()));
            }
        }
        AgentKind::Codex => {
            let base = provider
                .base_url
                .as_deref()
                .unwrap_or("https://api.openai.com/v1")
                .trim_end_matches('/');
            let wire = if provider.protocol == Protocol::OpenaiChat {
                "chat"
            } else {
                "responses"
            };
            let p = format!("model_providers.{CODEX_PROVIDER_ID}");
            // 值不加引号：Codex 对无法按 TOML 解析的值按字面字符串处理，
            // 这样在 Windows 的 .cmd 包装下也不会有引号转义问题。
            for (k, v) in [
                ("name", "Tidewatch"),
                ("base_url", base),
                ("env_key", CODEX_KEY_ENV),
                ("wire_api", wire),
            ] {
                inj.args.extend(["-c".into(), format!("{p}.{k}={v}")]);
            }
            inj.args
                .extend(["-c".into(), format!("model_provider={CODEX_PROVIDER_ID}")]);
            if let Some(k) = secret {
                inj.env.push((CODEX_KEY_ENV.into(), k.to_string()));
            }
        }
        AgentKind::Mock => unreachable!(),
    }
    inj.env.extend(provider.extra_env.iter().cloned());
    Ok(inj)
}

/// 预置模板（docs/03-agent/providers.md §7）。
pub fn templates() -> Vec<ProviderConfig> {
    let p =
        |name: &str, protocol, base_url: Option<&str>, model: Option<&str>, small: Option<&str>| {
            ProviderConfig {
                name: name.into(),
                protocol,
                base_url: base_url.map(String::from),
                default_model: model.map(String::from),
                small_model: small.map(String::from),
                extra_env: vec![],
            }
        };
    let mut deepseek = p(
        "DeepSeek（Anthropic 兼容）",
        Protocol::Anthropic,
        Some("https://api.deepseek.com/anthropic"),
        Some("deepseek-chat"),
        Some("deepseek-chat"),
    );
    deepseek.extra_env = vec![
        ("API_TIMEOUT_MS".into(), "600000".into()),
        (
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".into(),
            "1".into(),
        ),
    ];
    vec![
        p("官方默认（本机登录态）", Protocol::Native, None, None, None),
        p("Anthropic API", Protocol::Anthropic, None, None, None),
        p(
            "OpenAI API",
            Protocol::OpenaiResponses,
            Some("https://api.openai.com/v1"),
            None,
            None,
        ),
        deepseek,
        p(
            "DeepSeek（OpenAI 兼容）",
            Protocol::OpenaiChat,
            Some("https://api.deepseek.com/v1"),
            Some("deepseek-chat"),
            None,
        ),
        p(
            "自定义 Anthropic 兼容",
            Protocol::Anthropic,
            Some("https://"),
            None,
            None,
        ),
        p(
            "自定义 OpenAI 兼容",
            Protocol::OpenaiResponses,
            Some("https://"),
            None,
            None,
        ),
    ]
}

pub fn deepseek() -> ProviderConfig {
    templates()
        .into_iter()
        .find(|t| t.name.starts_with("DeepSeek（Anthropic"))
        .expect("deepseek template")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(inj: &'a Injection, k: &str) -> Option<&'a str> {
        inj.env
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn deepseek_injects_claude_env_only() {
        let inj = inject(AgentKind::ClaudeCode, &deepseek(), Some("sk-test")).unwrap();
        assert_eq!(
            env(&inj, "ANTHROPIC_BASE_URL"),
            Some("https://api.deepseek.com/anthropic")
        );
        assert_eq!(env(&inj, "ANTHROPIC_AUTH_TOKEN"), Some("sk-test"));
        assert_eq!(env(&inj, "ANTHROPIC_MODEL"), Some("deepseek-chat"));
        assert_eq!(
            env(&inj, "ANTHROPIC_SMALL_FAST_MODEL"),
            Some("deepseek-chat")
        );
        assert!(inj.args.is_empty());
    }

    #[test]
    fn native_injects_nothing() {
        let native = &templates()[0];
        for kind in [AgentKind::ClaudeCode, AgentKind::Codex] {
            let inj = inject(kind, native, None).unwrap();
            assert!(inj.env.is_empty() && inj.args.is_empty());
        }
    }

    #[test]
    fn codex_gets_config_overrides_and_key_env() {
        let p = ProviderConfig {
            name: "网关".into(),
            protocol: Protocol::OpenaiResponses,
            base_url: Some("https://gw.example.com/v1/".into()),
            default_model: Some("gpt-x".into()),
            small_model: None,
            extra_env: vec![],
        };
        let inj = inject(AgentKind::Codex, &p, Some("k")).unwrap();
        assert!(inj
            .args
            .contains(&"model_providers.tidewatch.base_url=https://gw.example.com/v1".to_string()));
        assert!(inj
            .args
            .contains(&"model_providers.tidewatch.wire_api=responses".to_string()));
        assert!(inj.args.contains(&"model_provider=tidewatch".to_string()));
        assert_eq!(env(&inj, CODEX_KEY_ENV), Some("k"));
        assert!(
            !inj.args.iter().any(|a| a.contains('k') && a == "k"),
            "密钥不进命令行"
        );
    }

    #[test]
    fn protocol_mismatch_is_rejected() {
        let err = inject(AgentKind::Codex, &deepseek(), Some("k")).unwrap_err();
        assert!(err.to_string().contains("不兼容"));
    }
}
