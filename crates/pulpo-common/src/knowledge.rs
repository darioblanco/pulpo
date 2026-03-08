use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeKind {
    Summary,
    Failure,
}

impl fmt::Display for KnowledgeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Summary => write!(f, "summary"),
            Self::Failure => write!(f, "failure"),
        }
    }
}

impl FromStr for KnowledgeKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "summary" => Ok(Self::Summary),
            "failure" => Ok(Self::Failure),
            other => Err(format!("unknown knowledge kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Knowledge {
    pub id: Uuid,
    pub session_id: Uuid,
    pub kind: KnowledgeKind,
    /// Scoped to this working directory / repo path. `None` = global.
    pub scope_repo: Option<String>,
    /// Scoped to this ink name. `None` = any ink.
    pub scope_ink: Option<String>,
    pub title: String,
    pub body: String,
    /// Tags for filtering: provider, status, duration bucket, etc.
    pub tags: Vec<String>,
    /// Relevance score 0.0–1.0 for ranking.
    pub relevance: f64,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_knowledge() -> Knowledge {
        Knowledge {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            kind: KnowledgeKind::Summary,
            scope_repo: Some("/tmp/repo".into()),
            scope_ink: Some("coder".into()),
            title: "Claude session completed successfully".into(),
            body: "Fixed the bug in auth module.".into(),
            tags: vec!["claude".into(), "completed".into()],
            relevance: 0.5,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_knowledge_kind_serialize() {
        assert_eq!(
            serde_json::to_string(&KnowledgeKind::Summary).unwrap(),
            "\"summary\""
        );
        assert_eq!(
            serde_json::to_string(&KnowledgeKind::Failure).unwrap(),
            "\"failure\""
        );
    }

    #[test]
    fn test_knowledge_kind_deserialize() {
        assert_eq!(
            serde_json::from_str::<KnowledgeKind>("\"summary\"").unwrap(),
            KnowledgeKind::Summary
        );
        assert_eq!(
            serde_json::from_str::<KnowledgeKind>("\"failure\"").unwrap(),
            KnowledgeKind::Failure
        );
    }

    #[test]
    fn test_knowledge_kind_invalid_deserialize() {
        assert!(serde_json::from_str::<KnowledgeKind>("\"invalid\"").is_err());
    }

    #[test]
    fn test_knowledge_kind_display() {
        assert_eq!(KnowledgeKind::Summary.to_string(), "summary");
        assert_eq!(KnowledgeKind::Failure.to_string(), "failure");
    }

    #[test]
    fn test_knowledge_kind_from_str() {
        assert_eq!(
            "summary".parse::<KnowledgeKind>().unwrap(),
            KnowledgeKind::Summary
        );
        assert_eq!(
            "failure".parse::<KnowledgeKind>().unwrap(),
            KnowledgeKind::Failure
        );
    }

    #[test]
    fn test_knowledge_kind_from_str_invalid() {
        let err = "invalid".parse::<KnowledgeKind>().unwrap_err();
        assert!(err.contains("unknown knowledge kind"));
    }

    #[test]
    fn test_knowledge_kind_clone_and_copy() {
        let k = KnowledgeKind::Summary;
        let k2 = k;
        #[allow(clippy::clone_on_copy)]
        let k3 = k.clone();
        assert_eq!(k, k2);
        assert_eq!(k, k3);
    }

    #[test]
    fn test_knowledge_kind_debug() {
        assert_eq!(format!("{:?}", KnowledgeKind::Summary), "Summary");
        assert_eq!(format!("{:?}", KnowledgeKind::Failure), "Failure");
    }

    #[test]
    fn test_knowledge_serialize_roundtrip() {
        let k = make_knowledge();
        let json = serde_json::to_string(&k).unwrap();
        let deserialized: Knowledge = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, k.id);
        assert_eq!(deserialized.session_id, k.session_id);
        assert_eq!(deserialized.kind, k.kind);
        assert_eq!(deserialized.scope_repo, k.scope_repo);
        assert_eq!(deserialized.scope_ink, k.scope_ink);
        assert_eq!(deserialized.title, k.title);
        assert_eq!(deserialized.body, k.body);
        assert_eq!(deserialized.tags, k.tags);
        assert!((deserialized.relevance - k.relevance).abs() < f64::EPSILON);
    }

    #[test]
    fn test_knowledge_with_global_scope() {
        let k = Knowledge {
            scope_repo: None,
            scope_ink: None,
            ..make_knowledge()
        };
        let json = serde_json::to_string(&k).unwrap();
        assert!(json.contains("\"scope_repo\":null"));
        assert!(json.contains("\"scope_ink\":null"));
    }

    #[test]
    fn test_knowledge_clone() {
        let k = make_knowledge();
        #[allow(clippy::redundant_clone)]
        let cloned = k.clone();
        assert_eq!(cloned.id, k.id);
        assert_eq!(cloned.title, k.title);
    }

    #[test]
    fn test_knowledge_debug() {
        let k = make_knowledge();
        let debug = format!("{k:?}");
        assert!(debug.contains("Knowledge"));
        assert!(debug.contains("Summary"));
    }

    #[test]
    fn test_knowledge_failure_kind() {
        let k = Knowledge {
            kind: KnowledgeKind::Failure,
            relevance: 0.8,
            ..make_knowledge()
        };
        assert_eq!(k.kind, KnowledgeKind::Failure);
        assert!((k.relevance - 0.8).abs() < f64::EPSILON);
    }
}
