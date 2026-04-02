use pulpo_common::event::{PulpoEvent, SessionEvent};
use serde::Serialize;
use tracing::{error, info};

use crate::config::DiscordWebhookConfig;

/// Builds and sends Discord webhook notifications for session events.
pub struct DiscordNotifier {
    config: DiscordWebhookConfig,
    client: reqwest::Client,
}

/// Discord embed structure.
#[derive(Debug, Serialize)]
struct DiscordEmbed {
    title: String,
    description: String,
    color: u32,
    fields: Vec<DiscordField>,
}

/// A field within a Discord embed.
#[derive(Debug, Serialize)]
struct DiscordField {
    name: String,
    value: String,
    inline: bool,
}

/// Discord webhook payload.
#[derive(Debug, Serialize)]
struct DiscordPayload {
    embeds: Vec<DiscordEmbed>,
}

/// Returns the Discord embed color for a given session status.
///
/// - active   → green  (`0x2ecc71`)
/// - ready    → blue   (`0x3498db`)
/// - stopped  → red    (`0xe74c3c`)
/// - lost     → orange (`0xe67e22`)
/// - other    → gray   (`0x95a5a6`)
pub const fn status_color(status: &str) -> u32 {
    // const fn can't use match on &str, so use byte comparison
    match status.as_bytes() {
        b"active" => 0x2e_cc71,
        b"ready" => 0x34_98db,
        b"stopped" => 0xe7_4c3c,
        b"lost" => 0xe6_7e22,
        _ => 0x95_a5a6,
    }
}

/// Builds the Discord webhook JSON payload for a session event.
pub fn build_discord_payload(event: &SessionEvent) -> serde_json::Value {
    let color = status_color(&event.status);
    let mut fields = vec![
        DiscordField {
            name: "Status".into(),
            value: event.status.clone(),
            inline: true,
        },
        DiscordField {
            name: "Node".into(),
            value: event.node_name.clone(),
            inline: true,
        },
    ];

    if let Some(prev) = &event.previous_status {
        fields.push(DiscordField {
            name: "Previous".into(),
            value: prev.clone(),
            inline: true,
        });
    }

    if let Some(url) = &event.pr_url {
        fields.push(DiscordField {
            name: "PR".into(),
            value: format!("[View PR]({url})"),
            inline: true,
        });
    }

    if let Some(branch) = &event.git_branch {
        let value = event
            .git_commit
            .as_ref()
            .map_or_else(|| branch.clone(), |commit| format!("{branch}@{commit}"));
        fields.push(DiscordField {
            name: "Branch".into(),
            value,
            inline: true,
        });
    }

    let ins = event.git_insertions.unwrap_or(0);
    let del = event.git_deletions.unwrap_or(0);
    if ins > 0 || del > 0 {
        let files = event.git_files_changed.unwrap_or(0);
        fields.push(DiscordField {
            name: "Changes".into(),
            value: format!("+{ins}/-{del} ({files} files)"),
            inline: true,
        });
    }

    if let Some(err) = &event.error_status {
        fields.push(DiscordField {
            name: "Error".into(),
            value: err.clone(),
            inline: false,
        });
    }

    if let Some(snippet) = &event.output_snippet {
        fields.push(DiscordField {
            name: "Output".into(),
            value: format!("```\n{snippet}\n```"),
            inline: false,
        });
    }

    let payload = DiscordPayload {
        embeds: vec![DiscordEmbed {
            title: format!("Session: {}", event.session_name),
            description: format!("Session `{}` is now **{}**", event.session_id, event.status),
            color,
            fields,
        }],
    };

    serde_json::to_value(&payload).unwrap_or_default()
}

/// Returns whether the notifier should send a notification for the given status.
pub fn should_notify(config: &DiscordWebhookConfig, status: &str) -> bool {
    config.events.is_empty() || config.events.iter().any(|e| e == status)
}

impl DiscordNotifier {
    /// Create a new `DiscordNotifier` from config.
    pub fn new(config: DiscordWebhookConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Send a JSON payload as a Discord webhook notification.
    pub async fn send_payload(&self, payload: &serde_json::Value) -> Result<(), reqwest::Error> {
        self.client
            .post(&self.config.webhook_url)
            .json(payload)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Send a session event as a Discord webhook notification.
    pub async fn send(&self, event: &SessionEvent) -> Result<(), reqwest::Error> {
        let payload = build_discord_payload(event);
        info!(
            session = %event.session_name,
            status = %event.status,
            "Sending Discord notification"
        );
        self.send_payload(&payload).await
    }
}

/// Run the notification loop — subscribes to the event bus and sends Discord notifications.
pub async fn run_notification_loop(
    notifier: DiscordNotifier,
    mut rx: tokio::sync::broadcast::Receiver<PulpoEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => match event {
                        PulpoEvent::Session(ref se) => {
                            if should_notify(&notifier.config, &se.status)
                                && let Err(e) = notifier.send(se).await
                            {
                                error!(error = %e, "Discord notification failed");
                            }
                        }
                        PulpoEvent::SessionDeleted(_) => {}
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(missed = n, "Discord notifier lagged, skipping events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Event bus closed, stopping Discord notifier");
                        break;
                    }
                }
            }
            _ = shutdown.changed() => {
                info!("Discord notifier shutting down");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::test_event;

    // --- status_color tests ---

    #[test]
    fn test_status_color_running() {
        assert_eq!(status_color("active"), 0x2e_cc71);
    }

    #[test]
    fn test_status_color_completed() {
        assert_eq!(status_color("ready"), 0x34_98db);
    }

    #[test]
    fn test_status_color_stopped() {
        assert_eq!(status_color("stopped"), 0xe7_4c3c);
    }

    #[test]
    fn test_status_color_stale() {
        assert_eq!(status_color("lost"), 0xe6_7e22);
    }

    #[test]
    fn test_status_color_unknown() {
        assert_eq!(status_color("creating"), 0x95_a5a6);
        assert_eq!(status_color(""), 0x95_a5a6);
    }

    // --- should_notify tests ---

    #[test]
    fn test_should_notify_empty_filter_allows_all() {
        let config = DiscordWebhookConfig {
            webhook_url: "https://example.com".into(),
            events: vec![],
        };
        assert!(should_notify(&config, "active"));
        assert!(should_notify(&config, "stopped"));
        assert!(should_notify(&config, "ready"));
    }

    #[test]
    fn test_should_notify_with_filter() {
        let config = DiscordWebhookConfig {
            webhook_url: "https://example.com".into(),
            events: vec!["ready".into(), "stopped".into()],
        };
        assert!(!should_notify(&config, "active"));
        assert!(should_notify(&config, "stopped"));
        assert!(should_notify(&config, "ready"));
        assert!(!should_notify(&config, "lost"));
    }

    // --- build_discord_payload tests ---

    #[test]
    fn test_build_payload_basic() {
        let event = test_event("active");
        let payload = build_discord_payload(&event);

        let embeds = payload["embeds"].as_array().unwrap();
        assert_eq!(embeds.len(), 1);

        let embed = &embeds[0];
        assert_eq!(embed["title"], "Session: my-session");
        assert!(embed["description"].as_str().unwrap().contains("abc-123"));
        assert_eq!(embed["color"], 0x2e_cc71);

        let fields = embed["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2); // Status + Node
        assert_eq!(fields[0]["name"], "Status");
        assert_eq!(fields[0]["value"], "active");
        assert_eq!(fields[1]["name"], "Node");
        assert_eq!(fields[1]["value"], "node-1");
    }

    #[test]
    fn test_build_payload_with_previous_status() {
        let event = SessionEvent {
            previous_status: Some("creating".into()),
            ..test_event("active")
        };
        let payload = build_discord_payload(&event);

        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[2]["name"], "Previous");
        assert_eq!(fields[2]["value"], "creating");
    }

    #[test]
    fn test_build_payload_with_output_snippet() {
        let event = SessionEvent {
            output_snippet: Some("hello world".into()),
            ..test_event("ready")
        };
        let payload = build_discord_payload(&event);

        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[2]["name"], "Output");
        assert!(fields[2]["value"].as_str().unwrap().contains("hello world"));
        assert!(!fields[2]["inline"].as_bool().unwrap());
    }

    #[test]
    fn test_build_payload_with_all_optionals() {
        let event = SessionEvent {
            previous_status: Some("lost".into()),
            output_snippet: Some("output".into()),
            ..test_event("active")
        };
        let payload = build_discord_payload(&event);

        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 4); // Status + Node + Previous + Output
    }

    #[test]
    fn test_build_payload_with_pr_url() {
        let mut event = test_event("ready");
        event.pr_url = Some("https://github.com/org/repo/pull/42".into());
        let payload = build_discord_payload(&event);
        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        let pr_field = fields.iter().find(|f| f["name"] == "PR").unwrap();
        assert!(pr_field["value"].as_str().unwrap().contains("[View PR]"));
        assert!(
            pr_field["value"]
                .as_str()
                .unwrap()
                .contains("https://github.com/org/repo/pull/42")
        );
        assert!(pr_field["inline"].as_bool().unwrap());
    }

    #[test]
    fn test_build_payload_with_branch_and_commit() {
        let mut event = test_event("ready");
        event.git_branch = Some("main".into());
        event.git_commit = Some("abc1234".into());
        let payload = build_discord_payload(&event);
        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        let branch_field = fields.iter().find(|f| f["name"] == "Branch").unwrap();
        assert_eq!(branch_field["value"], "main@abc1234");
    }

    #[test]
    fn test_build_payload_with_branch_no_commit() {
        let mut event = test_event("ready");
        event.git_branch = Some("fix-auth".into());
        let payload = build_discord_payload(&event);
        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        let branch_field = fields.iter().find(|f| f["name"] == "Branch").unwrap();
        assert_eq!(branch_field["value"], "fix-auth");
    }

    #[test]
    fn test_build_payload_with_changes() {
        let mut event = test_event("ready");
        event.git_insertions = Some(42);
        event.git_deletions = Some(7);
        event.git_files_changed = Some(3);
        let payload = build_discord_payload(&event);
        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        let changes_field = fields.iter().find(|f| f["name"] == "Changes").unwrap();
        assert_eq!(changes_field["value"], "+42/-7 (3 files)");
    }

    #[test]
    fn test_build_payload_with_zero_changes_omitted() {
        let mut event = test_event("ready");
        event.git_insertions = Some(0);
        event.git_deletions = Some(0);
        let payload = build_discord_payload(&event);
        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        assert!(fields.iter().all(|f| f["name"] != "Changes"));
    }

    #[test]
    fn test_build_payload_with_error_status() {
        let mut event = test_event("stopped");
        event.error_status = Some("Compile error in main.rs".into());
        let payload = build_discord_payload(&event);
        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        let err_field = fields.iter().find(|f| f["name"] == "Error").unwrap();
        assert_eq!(err_field["value"], "Compile error in main.rs");
        assert!(!err_field["inline"].as_bool().unwrap());
    }

    #[test]
    fn test_build_payload_with_all_enrichment_fields() {
        let mut event = test_event("ready");
        event.pr_url = Some("https://github.com/org/repo/pull/1".into());
        event.git_branch = Some("feat-x".into());
        event.git_commit = Some("deadbeef".into());
        event.git_insertions = Some(100);
        event.git_deletions = Some(50);
        event.git_files_changed = Some(10);
        event.error_status = Some("Lint warning".into());
        let payload = build_discord_payload(&event);
        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        // Status + Node + PR + Branch + Changes + Error = 6
        assert_eq!(fields.len(), 6);
    }

    #[test]
    fn test_build_payload_stopped_color() {
        let event = test_event("stopped");
        let payload = build_discord_payload(&event);
        assert_eq!(payload["embeds"][0]["color"], 0xe7_4c3c);
    }

    // --- DiscordNotifier tests ---

    #[test]
    fn test_notifier_new() {
        let config = DiscordWebhookConfig {
            webhook_url: "https://discord.com/api/webhooks/123/abc".into(),
            events: vec![],
        };
        let notifier = DiscordNotifier::new(config);
        assert_eq!(
            notifier.config.webhook_url,
            "https://discord.com/api/webhooks/123/abc"
        );
    }

    // --- send() tests ---

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn test_send_success() {
        // Start a local server that returns 200
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = socket.read(&mut buf).await;
            socket
                .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });

        let config = DiscordWebhookConfig {
            webhook_url: format!("http://{addr}/webhook"),
            events: vec![],
        };
        let notifier = DiscordNotifier::new(config);
        let result = notifier.send(&test_event("active")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_error_for_status() {
        // Start a local server that returns 400
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = socket.read(&mut buf).await;
            socket
                .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });

        let config = DiscordWebhookConfig {
            webhook_url: format!("http://{addr}/webhook"),
            events: vec![],
        };
        let notifier = DiscordNotifier::new(config);
        let result = notifier.send(&test_event("active")).await;
        assert!(result.is_err());
    }

    // --- run_notification_loop tests ---

    #[tokio::test]
    async fn test_notification_loop_shutdown() {
        let config = DiscordWebhookConfig {
            webhook_url: "https://example.com/webhook".into(),
            events: vec![],
        };
        let notifier = DiscordNotifier::new(config);
        let (event_tx, _) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let rx = event_tx.subscribe();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Signal shutdown immediately
        shutdown_tx.send(true).unwrap();

        // Loop should exit promptly
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_notification_loop(notifier, rx, shutdown_rx),
        )
        .await
        .expect("notification loop should exit on shutdown");
    }

    #[tokio::test]
    async fn test_notification_loop_channel_closed() {
        let config = DiscordWebhookConfig {
            webhook_url: "https://example.com/webhook".into(),
            events: vec![],
        };
        let notifier = DiscordNotifier::new(config);
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Drop sender to close the channel
        drop(event_tx);

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_notification_loop(notifier, rx, shutdown_rx),
        )
        .await
        .expect("notification loop should exit when channel closes");
    }

    #[tokio::test]
    async fn test_notification_loop_filtered_event() {
        let config = DiscordWebhookConfig {
            webhook_url: "https://example.com/webhook".into(),
            events: vec!["stopped".into()], // Only stopped events
        };
        let notifier = DiscordNotifier::new(config);
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Send an "active" event — should be filtered out (won't attempt HTTP)
        event_tx
            .send(PulpoEvent::Session(test_event("active")))
            .unwrap();

        // Then shutdown
        let handle = tokio::spawn(run_notification_loop(notifier, rx, shutdown_rx));

        // Give it a moment to process the event
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("should finish")
            .expect("should not panic");
    }

    #[tokio::test]
    async fn test_notification_loop_send_error() {
        // Use a URL that will fail immediately (connection refused)
        let config = DiscordWebhookConfig {
            webhook_url: "http://127.0.0.1:1/webhook".into(),
            events: vec![],
        };
        let notifier = DiscordNotifier::new(config);
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Send an event that passes the filter — will attempt HTTP and fail
        event_tx
            .send(PulpoEvent::Session(test_event("active")))
            .unwrap();

        let handle = tokio::spawn(run_notification_loop(notifier, rx, shutdown_rx));

        // Wait for the HTTP attempt to fail
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("should finish")
            .expect("should not panic");
    }

    #[tokio::test]
    async fn test_notification_loop_lagged() {
        // Filter everything so the loop doesn't attempt HTTP after processing lag
        let config = DiscordWebhookConfig {
            webhook_url: "https://example.com/webhook".into(),
            events: vec!["stopped".into()],
        };
        let notifier = DiscordNotifier::new(config);
        // Tiny buffer to force lag
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(1);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Overflow the buffer before the loop starts
        for i in 0..5 {
            let _ = event_tx.send(PulpoEvent::Session(SessionEvent {
                session_id: format!("id-{i}"),
                session_name: "s".into(),
                status: "active".into(),
                previous_status: None,
                node_name: "n".into(),
                output_snippet: None,
                timestamp: "t".into(),
                ..Default::default()
            }));
        }

        let handle = tokio::spawn(run_notification_loop(notifier, rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("should finish")
            .expect("should not panic");
    }

    #[test]
    fn test_discord_payload_serialize() {
        let payload = DiscordPayload {
            embeds: vec![DiscordEmbed {
                title: "Test".into(),
                description: "Desc".into(),
                color: 0,
                fields: vec![DiscordField {
                    name: "F".into(),
                    value: "V".into(),
                    inline: true,
                }],
            }],
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"title\":\"Test\""));
        assert!(json.contains("\"inline\":true"));
    }

    #[test]
    fn test_discord_embed_debug() {
        let embed = DiscordEmbed {
            title: "T".into(),
            description: "D".into(),
            color: 0,
            fields: vec![],
        };
        let debug = format!("{embed:?}");
        assert!(debug.contains("DiscordEmbed"));
    }

    #[test]
    fn test_discord_field_debug() {
        let field = DiscordField {
            name: "N".into(),
            value: "V".into(),
            inline: false,
        };
        let debug = format!("{field:?}");
        assert!(debug.contains("DiscordField"));
    }

    #[test]
    fn test_discord_payload_debug() {
        let payload = DiscordPayload { embeds: vec![] };
        let debug = format!("{payload:?}");
        assert!(debug.contains("DiscordPayload"));
    }
}
