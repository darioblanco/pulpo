use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CultureKind {
    Summary,
    Failure,
}

impl fmt::Display for CultureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Summary => write!(f, "summary"),
            Self::Failure => write!(f, "failure"),
        }
    }
}

impl FromStr for CultureKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "summary" => Ok(Self::Summary),
            "failure" => Ok(Self::Failure),
            other => Err(format!("unknown culture kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Culture {
    pub id: Uuid,
    pub session_id: Uuid,
    pub kind: CultureKind,
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
    /// When this entry was last used in context injection. `None` if never referenced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_referenced_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_culture() -> Culture {
        Culture {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            kind: CultureKind::Summary,
            scope_repo: Some("/tmp/repo".into()),
            scope_ink: Some("coder".into()),
            title: "Claude session completed successfully".into(),
            body: "Fixed the bug in auth module.".into(),
            tags: vec!["claude".into(), "completed".into()],
            relevance: 0.5,
            created_at: Utc::now(),
            last_referenced_at: None,
        }
    }

    #[test]
    fn test_culture_kind_serialize() {
        assert_eq!(
            serde_json::to_string(&CultureKind::Summary).unwrap(),
            "\"summary\""
        );
        assert_eq!(
            serde_json::to_string(&CultureKind::Failure).unwrap(),
            "\"failure\""
        );
    }

    #[test]
    fn test_culture_kind_deserialize() {
        assert_eq!(
            serde_json::from_str::<CultureKind>("\"summary\"").unwrap(),
            CultureKind::Summary
        );
        assert_eq!(
            serde_json::from_str::<CultureKind>("\"failure\"").unwrap(),
            CultureKind::Failure
        );
    }

    #[test]
    fn test_culture_kind_invalid_deserialize() {
        assert!(serde_json::from_str::<CultureKind>("\"invalid\"").is_err());
    }

    #[test]
    fn test_culture_kind_display() {
        assert_eq!(CultureKind::Summary.to_string(), "summary");
        assert_eq!(CultureKind::Failure.to_string(), "failure");
    }

    #[test]
    fn test_culture_kind_from_str() {
        assert_eq!(
            "summary".parse::<CultureKind>().unwrap(),
            CultureKind::Summary
        );
        assert_eq!(
            "failure".parse::<CultureKind>().unwrap(),
            CultureKind::Failure
        );
    }

    #[test]
    fn test_culture_kind_from_str_invalid() {
        let err = "invalid".parse::<CultureKind>().unwrap_err();
        assert!(err.contains("unknown culture kind"));
    }

    #[test]
    fn test_culture_kind_clone_and_copy() {
        let k = CultureKind::Summary;
        let k2 = k;
        #[allow(clippy::clone_on_copy)]
        let k3 = k.clone();
        assert_eq!(k, k2);
        assert_eq!(k, k3);
    }

    #[test]
    fn test_culture_kind_debug() {
        assert_eq!(format!("{:?}", CultureKind::Summary), "Summary");
        assert_eq!(format!("{:?}", CultureKind::Failure), "Failure");
    }

    #[test]
    fn test_culture_serialize_roundtrip() {
        let k = make_culture();
        let json = serde_json::to_string(&k).unwrap();
        let deserialized: Culture = serde_json::from_str(&json).unwrap();
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
    fn test_culture_with_global_scope() {
        let k = Culture {
            scope_repo: None,
            scope_ink: None,
            ..make_culture()
        };
        let json = serde_json::to_string(&k).unwrap();
        assert!(json.contains("\"scope_repo\":null"));
        assert!(json.contains("\"scope_ink\":null"));
    }

    #[test]
    fn test_culture_clone() {
        let k = make_culture();
        #[allow(clippy::redundant_clone)]
        let cloned = k.clone();
        assert_eq!(cloned.id, k.id);
        assert_eq!(cloned.title, k.title);
    }

    #[test]
    fn test_culture_debug() {
        let k = make_culture();
        let debug = format!("{k:?}");
        assert!(debug.contains("Culture"));
        assert!(debug.contains("Summary"));
    }

    #[test]
    fn test_culture_failure_kind() {
        let k = Culture {
            kind: CultureKind::Failure,
            relevance: 0.8,
            ..make_culture()
        };
        assert_eq!(k.kind, CultureKind::Failure);
        assert!((k.relevance - 0.8).abs() < f64::EPSILON);
    }
}
