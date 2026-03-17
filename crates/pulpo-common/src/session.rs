use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use std::collections::HashMap;

/// Machine-readable intervention reason codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InterventionCode {
    /// Killed due to system memory pressure exceeding threshold.
    MemoryPressure,
    /// Killed due to session idle timeout.
    IdleTimeout,
    /// Manually killed by user via API/CLI.
    UserKill,
}

impl fmt::Display for InterventionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MemoryPressure => write!(f, "memory_pressure"),
            Self::IdleTimeout => write!(f, "idle_timeout"),
            Self::UserKill => write!(f, "user_kill"),
        }
    }
}

impl FromStr for InterventionCode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "memory_pressure" => Ok(Self::MemoryPressure),
            "idle_timeout" => Ok(Self::IdleTimeout),
            "user_kill" => Ok(Self::UserKill),
            other => Err(format!("unknown intervention code: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Creating,
    Active,
    Idle,
    Ready,
    Killed,
    Lost,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Creating => write!(f, "creating"),
            Self::Active => write!(f, "active"),
            Self::Idle => write!(f, "idle"),
            Self::Ready => write!(f, "ready"),
            Self::Killed => write!(f, "killed"),
            Self::Lost => write!(f, "lost"),
        }
    }
}

impl FromStr for SessionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "creating" => Ok(Self::Creating),
            "active" => Ok(Self::Active),
            "idle" => Ok(Self::Idle),
            "ready" => Ok(Self::Ready),
            "killed" => Ok(Self::Killed),
            "lost" => Ok(Self::Lost),
            other => Err(format!("unknown session status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub workdir: String,
    pub command: String,
    pub description: Option<String>,
    pub status: SessionStatus,
    pub exit_code: Option<i32>,
    pub backend_session_id: Option<String>,
    pub output_snapshot: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
    pub ink: Option<String>,
    pub intervention_code: Option<InterventionCode>,
    pub intervention_reason: Option<String>,
    pub intervention_at: Option<DateTime<Utc>>,
    pub last_output_at: Option<DateTime<Utc>>,
    pub idle_since: Option<DateTime<Utc>>,
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
            command: "claude -p 'Fix the bug'".into(),
            description: Some("Fix the bug".into()),
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: Some("test-session".into()),
            output_snapshot: Some("some output".into()),
            metadata: None,
            ink: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_session_status_serialize() {
        assert_eq!(
            serde_json::to_string(&SessionStatus::Creating).unwrap(),
            "\"creating\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Idle).unwrap(),
            "\"idle\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Ready).unwrap(),
            "\"ready\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Killed).unwrap(),
            "\"killed\""
        );
        assert_eq!(
            serde_json::to_string(&SessionStatus::Lost).unwrap(),
            "\"lost\""
        );
    }

    #[test]
    fn test_session_status_deserialize() {
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"creating\"").unwrap(),
            SessionStatus::Creating
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"active\"").unwrap(),
            SessionStatus::Active
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"idle\"").unwrap(),
            SessionStatus::Idle
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"ready\"").unwrap(),
            SessionStatus::Ready
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"killed\"").unwrap(),
            SessionStatus::Killed
        );
        assert_eq!(
            serde_json::from_str::<SessionStatus>("\"lost\"").unwrap(),
            SessionStatus::Lost
        );
    }

    #[test]
    fn test_session_status_invalid_deserialize() {
        assert!(serde_json::from_str::<SessionStatus>("\"invalid\"").is_err());
    }

    #[test]
    fn test_session_status_display() {
        assert_eq!(SessionStatus::Creating.to_string(), "creating");
        assert_eq!(SessionStatus::Active.to_string(), "active");
        assert_eq!(SessionStatus::Idle.to_string(), "idle");
        assert_eq!(SessionStatus::Ready.to_string(), "ready");
        assert_eq!(SessionStatus::Killed.to_string(), "killed");
        assert_eq!(SessionStatus::Lost.to_string(), "lost");
    }

    #[test]
    fn test_session_status_from_str() {
        assert_eq!(
            "creating".parse::<SessionStatus>().unwrap(),
            SessionStatus::Creating
        );
        assert_eq!(
            "active".parse::<SessionStatus>().unwrap(),
            SessionStatus::Active
        );
        assert_eq!(
            "idle".parse::<SessionStatus>().unwrap(),
            SessionStatus::Idle
        );
        assert_eq!(
            "ready".parse::<SessionStatus>().unwrap(),
            SessionStatus::Ready
        );
        assert_eq!(
            "killed".parse::<SessionStatus>().unwrap(),
            SessionStatus::Killed
        );
        assert_eq!(
            "lost".parse::<SessionStatus>().unwrap(),
            SessionStatus::Lost
        );
    }

    #[test]
    fn test_session_status_from_str_invalid() {
        let err = "invalid".parse::<SessionStatus>().unwrap_err();
        assert!(err.contains("unknown session status"));
    }

    #[test]
    fn test_session_serialize_roundtrip() {
        let session = make_session();
        let json = serde_json::to_string(&session).unwrap();
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, session.id);
        assert_eq!(deserialized.name, session.name);
        assert_eq!(deserialized.command, session.command);
        assert_eq!(deserialized.status, session.status);
        assert_eq!(deserialized.description, session.description);
    }

    #[test]
    fn test_session_with_all_none_optionals() {
        let session = Session {
            id: Uuid::new_v4(),
            name: "minimal".into(),
            workdir: "/tmp".into(),
            command: "echo hello".into(),
            description: None,
            status: SessionStatus::Creating,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            metadata: None,
            ink: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"description\":null"));
        assert!(json.contains("\"echo hello\""));
    }

    #[test]
    fn test_session_status_clone_and_copy() {
        let s = SessionStatus::Active;
        let s2 = s;
        #[allow(clippy::clone_on_copy)]
        let s3 = s.clone();
        assert_eq!(s, s2);
        assert_eq!(s, s3);
    }

    #[test]
    fn test_session_status_debug() {
        assert_eq!(format!("{:?}", SessionStatus::Active), "Active");
    }

    #[test]
    fn test_session_debug() {
        let session = Session {
            id: Uuid::nil(),
            name: "test".into(),
            workdir: "/tmp".into(),
            command: "echo test".into(),
            description: None,
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            metadata: None,
            ink: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
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
            command: "echo test".into(),
            description: None,
            status: SessionStatus::Ready,
            exit_code: Some(0),
            backend_session_id: None,
            output_snapshot: None,
            metadata: None,
            ink: None,
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let cloned = session.clone();
        assert_eq!(cloned.id, session.id);
        assert_eq!(cloned.exit_code, Some(0));
    }

    #[test]
    fn test_session_with_metadata() {
        let mut meta = HashMap::new();
        meta.insert("discord_channel".into(), "123".into());
        let session = Session {
            id: Uuid::new_v4(),
            name: "full".into(),
            workdir: "/tmp".into(),
            command: "claude -p 'test'".into(),
            description: Some("Testing".into()),
            status: SessionStatus::Active,
            exit_code: None,
            backend_session_id: None,
            output_snapshot: None,
            metadata: Some(meta),
            ink: Some("coder".into()),
            intervention_code: None,
            intervention_reason: None,
            intervention_at: None,
            last_output_at: None,
            idle_since: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&session).unwrap();
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized
                .metadata
                .as_ref()
                .unwrap()
                .get("discord_channel"),
            Some(&"123".into())
        );
        assert_eq!(deserialized.ink, Some("coder".into()));
    }

    #[test]
    fn test_intervention_code_serialize() {
        assert_eq!(
            serde_json::to_string(&InterventionCode::MemoryPressure).unwrap(),
            "\"memory_pressure\""
        );
        assert_eq!(
            serde_json::to_string(&InterventionCode::IdleTimeout).unwrap(),
            "\"idle_timeout\""
        );
        assert_eq!(
            serde_json::to_string(&InterventionCode::UserKill).unwrap(),
            "\"user_kill\""
        );
    }

    #[test]
    fn test_intervention_code_deserialize() {
        assert_eq!(
            serde_json::from_str::<InterventionCode>("\"memory_pressure\"").unwrap(),
            InterventionCode::MemoryPressure
        );
        assert_eq!(
            serde_json::from_str::<InterventionCode>("\"idle_timeout\"").unwrap(),
            InterventionCode::IdleTimeout
        );
        assert_eq!(
            serde_json::from_str::<InterventionCode>("\"user_kill\"").unwrap(),
            InterventionCode::UserKill
        );
    }

    #[test]
    fn test_intervention_code_invalid_deserialize() {
        assert!(serde_json::from_str::<InterventionCode>("\"invalid\"").is_err());
    }

    #[test]
    fn test_intervention_code_display() {
        assert_eq!(
            InterventionCode::MemoryPressure.to_string(),
            "memory_pressure"
        );
        assert_eq!(InterventionCode::IdleTimeout.to_string(), "idle_timeout");
        assert_eq!(InterventionCode::UserKill.to_string(), "user_kill");
    }

    #[test]
    fn test_intervention_code_from_str() {
        assert_eq!(
            "memory_pressure".parse::<InterventionCode>().unwrap(),
            InterventionCode::MemoryPressure
        );
        assert_eq!(
            "idle_timeout".parse::<InterventionCode>().unwrap(),
            InterventionCode::IdleTimeout
        );
        assert_eq!(
            "user_kill".parse::<InterventionCode>().unwrap(),
            InterventionCode::UserKill
        );
    }

    #[test]
    fn test_intervention_code_from_str_invalid() {
        let err = "invalid".parse::<InterventionCode>().unwrap_err();
        assert!(err.contains("unknown intervention code"));
    }

    #[test]
    fn test_intervention_code_clone_and_copy() {
        let c = InterventionCode::MemoryPressure;
        let c2 = c;
        #[allow(clippy::clone_on_copy)]
        let c3 = c.clone();
        assert_eq!(c, c2);
        assert_eq!(c, c3);
    }

    #[test]
    fn test_intervention_code_debug() {
        assert_eq!(
            format!("{:?}", InterventionCode::MemoryPressure),
            "MemoryPressure"
        );
        assert_eq!(
            format!("{:?}", InterventionCode::IdleTimeout),
            "IdleTimeout"
        );
        assert_eq!(format!("{:?}", InterventionCode::UserKill), "UserKill");
    }

    #[test]
    fn test_session_with_intervention_code() {
        let mut session = make_session();
        session.intervention_code = Some(InterventionCode::MemoryPressure);
        session.intervention_reason = Some("Memory 95%".into());
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"memory_pressure\""));
        let deserialized: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.intervention_code,
            Some(InterventionCode::MemoryPressure)
        );
    }
}
