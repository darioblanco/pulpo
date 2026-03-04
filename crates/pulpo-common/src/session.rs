use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use std::collections::HashMap;

use crate::guard::GuardConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Claude,
    Codex,
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claude => write!(f, "claude"),
            Self::Codex => write!(f, "codex"),
        }
    }
}

impl FromStr for Provider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            other => Err(format!("unknown provider: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Creating,
    Running,
    Completed,
    Dead,
    Stale,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Creating => write!(f, "creating"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Dead => write!(f, "dead"),
            Self::Stale => write!(f, "stale"),
        }
    }
}

impl FromStr for SessionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "creating" => Ok(Self::Creating),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "dead" => Ok(Self::Dead),
            "stale" => Ok(Self::Stale),
            other => Err(format!("unknown session status: {other}")),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    #[default]
    Interactive,
    Autonomous,
}

impl fmt::Display for SessionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interactive => write!(f, "interactive"),
            Self::Autonomous => write!(f, "autonomous"),
        }
    }
}

impl FromStr for SessionMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "interactive" => Ok(Self::Interactive),
            "autonomous" => Ok(Self::Autonomous),
            other => Err(format!("unknown session mode: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub workdir: String,
    pub provider: Provider,
    pub prompt: String,
    pub status: SessionStatus,
    pub mode: SessionMode,
    pub conversation_id: Option<String>,
    pub exit_code: Option<i32>,
    pub tmux_session: Option<String>,
    pub output_snapshot: Option<String>,
    pub guard_config: Option<GuardConfig>,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub system_prompt: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
    pub persona: Option<String>,
    pub max_turns: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub output_format: Option<String>,
    pub intervention_reason: Option<String>,
    pub intervention_at: Option<DateTime<Utc>>,
    pub last_output_at: Option<DateTime<Utc>>,
    pub idle_since: Option<DateTime<Utc>>,
    pub waiting_for_input: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_session() -> Session {
        Session {
            id: Uuid::new_v4(),
            name: "test-session".into(),
            workdir: "/tmp/repo".into(),
            provider: Provider::Claude,
            prompt: "Fix the bug".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: Some("conv-123".into()),
            exit_code: None,
            tmux_session: Some("pulpo-test".into()),

            output_snapshot: Some("some output".into()),
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_provider_serialize() {
        assert_eq!(
            serde_json::to_string(&Provider::Claude).unwrap(),
            "\"claude\""
        );
        assert_eq!(
            serde_json::to_string(&Provider::Codex).unwrap(),
            "\"codex\""
        );
    }

    #[test]
    fn test_provider_deserialize() {
        assert_eq!(
            serde_json::from_str::<Provider>("\"claude\"").unwrap(),
            Provider::Claude
        );
        assert_eq!(
            serde_json::from_str::<Provider>("\"codex\"").unwrap(),
            Provider::Codex
        );
    }

    #[test]
    fn test_provider_invalid_deserialize() {
        assert!(serde_json::from_str::<Provider>("\"invalid\"").is_err());
    }

    #[test]
    fn test_provider_display() {
        assert_eq!(Provider::Claude.to_string(), "claude");
        assert_eq!(Provider::Codex.to_string(), "codex");
    }

    #[test]
    fn test_provider_from_str() {
        assert_eq!("claude".parse::<Provider>().unwrap(), Provider::Claude);
        assert_eq!("codex".parse::<Provider>().unwrap(), Provider::Codex);
    }

    #[test]
    fn test_provider_from_str_invalid() {
        let err = "invalid".parse::<Provider>().unwrap_err();
        assert!(err.contains("unknown provider"));
    }

    #[test]
    fn test_session_status_serialize() {
        assert_eq!(
            serde_json::to_string(&SessionStatus::Creating).unwrap(),
            "\"creating\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Running).unwrap(),
            "\"running\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Completed).unwrap(),
            "\"completed\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Dead).unwrap(),
            "\"dead\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Stale).unwrap(),
            "\"stale\""
        );
    }

    #[test]
    fn test_session_status_deserialize() {
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"creating\"").unwrap(),
            SessionStatus::Creating
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"running\"").unwrap(),
            SessionStatus::Running
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"completed\"").unwrap(),
            SessionStatus::Completed
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"dead\"").unwrap(),
            SessionStatus::Dead
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"stale\"").unwrap(),
            SessionStatus::Stale
        );
    }

    #[test]
    fn test_session_status_invalid_deserialize() {
        assert!(serde_json::from_str::<SessionStatus>("\"invalid\"").is_err());
    }

    #[test]
    fn test_session_status_display() {
        assert_eq!(SessionStatus::Creating.to_string(), "creating");
        assert_eq!(SessionStatus::Running.to_string(), "running");
        assert_eq!(SessionStatus::Completed.to_string(), "completed");
        assert_eq!(SessionStatus::Dead.to_string(), "dead");
        assert_eq!(SessionStatus::Stale.to_string(), "stale");
    }

    #[test]
    fn test_session_status_from_str() {
        assert_eq!(
            "creating".parse::<SessionStatus>().unwrap(),
            SessionStatus::Creating
        );
        assert_eq!(
            "running".parse::<SessionStatus>().unwrap(),
            SessionStatus::Running
        );
        assert_eq!(
            "completed".parse::<SessionStatus>().unwrap(),
            SessionStatus::Completed
        );
        assert_eq!(
            "dead".parse::<SessionStatus>().unwrap(),
            SessionStatus::Dead
        );
        assert_eq!(
            "stale".parse::<SessionStatus>().unwrap(),
            SessionStatus::Stale
        );
    }

    #[test]
    fn test_session_status_from_str_invalid() {
        let err = "invalid".parse::<SessionStatus>().unwrap_err();
        assert!(err.contains("unknown session status"));
    }

    #[test]
    fn test_session_mode_serialize() {
        assert_eq!(
            serde_json::to_string(&SessionMode::Interactive).unwrap(),
            "\"interactive\""
        );
        assert_eq!(
            serde_json::to_string(&SessionMode::Autonomous).unwrap(),
            "\"autonomous\""
        );
    }

    #[test]
    fn test_session_mode_deserialize() {
        assert_eq!(
            serde_json::from_str::<SessionMode>("\"interactive\"").unwrap(),
            SessionMode::Interactive
        );
        assert_eq!(
            serde_json::from_str::<SessionMode>("\"autonomous\"").unwrap(),
            SessionMode::Autonomous
        );
    }

    #[test]
    fn test_session_mode_invalid_deserialize() {
        assert!(serde_json::from_str::<SessionMode>("\"invalid\"").is_err());
    }

    #[test]
    fn test_session_mode_display() {
        assert_eq!(SessionMode::Interactive.to_string(), "interactive");
        assert_eq!(SessionMode::Autonomous.to_string(), "autonomous");
    }

    #[test]
    fn test_session_mode_from_str() {
        assert_eq!(
            "interactive".parse::<SessionMode>().unwrap(),
            SessionMode::Interactive
        );
        assert_eq!(
            "autonomous".parse::<SessionMode>().unwrap(),
            SessionMode::Autonomous
        );
    }

    #[test]
    fn test_session_mode_from_str_invalid() {
        let err = "invalid".parse::<SessionMode>().unwrap_err();
        assert!(err.contains("unknown session mode"));
    }

    #[test]
    fn test_session_mode_default() {
        assert_eq!(SessionMode::default(), SessionMode::Interactive);
    }

    #[test]
    fn test_session_mode_clone_and_copy() {
        let m = SessionMode::Autonomous;
        let m2 = m;
        #[allow(clippy::clone_on_copy)]
        let m3 = m.clone();
        assert_eq!(m, m2);
        assert_eq!(m, m3);
    }

    #[test]
    fn test_session_mode_debug() {
        assert_eq!(format!("{:?}", SessionMode::Interactive), "Interactive");
        assert_eq!(format!("{:?}", SessionMode::Autonomous), "Autonomous");
    }

    #[test]
    fn test_session_serialize_roundtrip() {
        let session = make_session();
        let json = serde_json::to_string(&session).unwrap();
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, session.id);
        assert_eq!(deserialized.name, session.name);
        assert_eq!(deserialized.provider, session.provider);
        assert_eq!(deserialized.status, session.status);
        assert_eq!(deserialized.mode, session.mode);
        assert_eq!(deserialized.conversation_id, session.conversation_id);
    }

    #[test]
    fn test_session_with_all_none_optionals() {
        let session = Session {
            id: Uuid::new_v4(),
            name: "minimal".into(),
            workdir: "/tmp".into(),
            provider: Provider::Codex,
            prompt: "test".into(),
            status: SessionStatus::Creating,
            mode: SessionMode::Autonomous,
            conversation_id: None,
            exit_code: None,
            tmux_session: None,

            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"conversation_id\":null"));
        assert!(json.contains("\"autonomous\""));
    }

    #[test]
    fn test_provider_clone_and_copy() {
        let p = Provider::Claude;
        let p2 = p;
        #[allow(clippy::clone_on_copy)]
        let p3 = p.clone();
        assert_eq!(p, p2);
        assert_eq!(p, p3);
    }

    #[test]
    fn test_session_status_clone_and_copy() {
        let s = SessionStatus::Running;
        let s2 = s;
        #[allow(clippy::clone_on_copy)]
        let s3 = s.clone();
        assert_eq!(s, s2);
        assert_eq!(s, s3);
    }

    #[test]
    fn test_provider_debug() {
        assert_eq!(format!("{:?}", Provider::Claude), "Claude");
    }

    #[test]
    fn test_session_status_debug() {
        assert_eq!(format!("{:?}", SessionStatus::Running), "Running");
    }

    #[test]
    fn test_session_debug() {
        let session = Session {
            id: Uuid::nil(),
            name: "test".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            tmux_session: None,

            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let debug = format!("{session:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_session_clone() {
        let session = Session {
            id: Uuid::new_v4(),
            name: "clone-test".into(),
            workdir: "/tmp".into(),
            provider: Provider::Codex,
            prompt: "test".into(),
            status: SessionStatus::Completed,
            mode: SessionMode::Autonomous,
            conversation_id: None,
            exit_code: Some(0),
            tmux_session: None,

            output_snapshot: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let cloned = session.clone();
        assert_eq!(cloned.id, session.id);
        assert_eq!(cloned.exit_code, Some(0));
        assert_eq!(cloned.mode, SessionMode::Autonomous);
    }

    #[test]
    fn test_session_with_new_fields() {
        let mut meta = HashMap::new();
        meta.insert("discord_channel".into(), "123".into());
        let session = Session {
            id: Uuid::new_v4(),
            name: "full".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Autonomous,
            conversation_id: None,
            exit_code: None,
            tmux_session: None,
            output_snapshot: None,
            guard_config: None,
            model: Some("opus".into()),
            allowed_tools: Some(vec!["Read".into(), "Grep".into()]),
            system_prompt: Some("Be concise".into()),
            metadata: Some(meta),
            persona: Some("coder".into()),
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&session).unwrap();
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model, Some("opus".into()));
        assert_eq!(
            deserialized.allowed_tools,
            Some(vec!["Read".into(), "Grep".into()])
        );
        assert_eq!(deserialized.system_prompt, Some("Be concise".into()));
        assert_eq!(
            deserialized
                .metadata
                .as_ref()
                .unwrap()
                .get("discord_channel"),
            Some(&"123".into())
        );
        assert_eq!(deserialized.persona, Some("coder".into()));
    }
}
