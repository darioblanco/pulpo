use pulpo_common::event::{PulpoEvent, ScheduleEvent, SessionEvent};
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
/// - running  → green  (`0x2ecc71`)
/// - completed → blue  (`0x3498db`)
/// - dead     → red    (`0xe74c3c`)
/// - stale    → orange (`0xe67e22`)
/// - other    → gray   (`0x95a5a6`)
pub const fn status_color(status: &str) -> u32 {
    // const fn can't use match on &str, so use byte comparison
    match status.as_bytes() {
        b"running" => 0x2e_cc71,
        b"completed" => 0x34_98db,
        b"dead" => 0xe7_4c3c,
        b"stale" => 0xe6_7e22,
        _ => 0x95_a5a6,
    }
}

/// Returns the Discord embed color for a schedule event type.
///
/// - fired    → green  (`0x2ecc71`)
/// - failed   → red    (`0xe74c3c`)
/// - exhausted → blue  (`0x3498db`)
/// - other    → gray   (`0x95a5a6`)
pub const fn schedule_event_color(event_type: &str) -> u32 {
    match event_type.as_bytes() {
        b"fired" => 0x2e_cc71,
        b"failed" => 0xe7_4c3c,
        b"exhausted" => 0x34_98db,
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

/// Builds the Discord webhook JSON payload for a schedule event.
pub fn build_schedule_discord_payload(event: &ScheduleEvent) -> serde_json::Value {
    let color = schedule_event_color(&event.event_type);
    let mut fields = vec![
        DiscordField {
            name: "Event".into(),
            value: event.event_type.clone(),
            inline: true,
        },
        DiscordField {
            name: "Node".into(),
            value: event.node_name.clone(),
            inline: true,
        },
    ];

    if let Some(session_id) = &event.session_id {
        fields.push(DiscordField {
            name: "Session".into(),
            value: session_id.clone(),
            inline: true,
        });
    }

    if let Some(err) = &event.error {
        fields.push(DiscordField {
            name: "Error".into(),
            value: format!("```\n{err}\n```"),
            inline: false,
        });
    }

    let payload = DiscordPayload {
        embeds: vec![DiscordEmbed {
            title: format!("Schedule: {}", event.schedule_name),
            description: format!(
                "Schedule `{}` — **{}**",
                event.schedule_id, event.event_type
            ),
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

/// Returns whether the notifier should send a notification for a schedule event.
pub fn should_notify_schedule(config: &DiscordWebhookConfig, event_type: &str) -> bool {
    // Schedule events are always sent if event filter is empty,
    // or if "schedule_fired", "schedule_failed" etc. are in the filter
    config.events.is_empty() || config.events.iter().any(|e| e == event_type)
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

    /// Send a schedule event as a Discord webhook notification.
    pub async fn send_schedule(&self, event: &ScheduleEvent) -> Result<(), reqwest::Error> {
        let payload = build_schedule_discord_payload(event);
        info!(
            schedule = %event.schedule_name,
            event_type = %event.event_type,
            "Sending Discord schedule notification"
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
                        PulpoEvent::Schedule(ref se) => {
                            if should_notify_schedule(&notifier.config, &se.event_type)
                                && let Err(e) = notifier.send_schedule(se).await
                            {
                                error!(error = %e, "Discord schedule notification failed");
                            }
                        }
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

    fn test_event(status: &str) -> SessionEvent {
        SessionEvent {
            session_id: "abc-123".into(),
            session_name: "my-session".into(),
            status: status.into(),
            previous_status: None,
            node_name: "node-1".into(),
            output_snippet: None,
            waiting_for_input: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn test_schedule_event(event_type: &str) -> ScheduleEvent {
        ScheduleEvent {
            schedule_id: "sch-1".into(),
            schedule_name: "nightly-review".into(),
            event_type: event_type.into(),
            session_id: None,
            error: None,
            node_name: "node-1".into(),
            timestamp: "2026-01-01T02:00:00Z".into(),
        }
    }

    // --- status_color tests ---

    #[test]
    fn test_status_color_running() {
        assert_eq!(status_color("running"), 0x2e_cc71);
    }

    #[test]
    fn test_status_color_completed() {
        assert_eq!(status_color("completed"), 0x34_98db);
    }

    #[test]
    fn test_status_color_dead() {
        assert_eq!(status_color("dead"), 0xe7_4c3c);
    }

    #[test]
    fn test_status_color_stale() {
        assert_eq!(status_color("stale"), 0xe6_7e22);
    }

    #[test]
    fn test_status_color_unknown() {
        assert_eq!(status_color("creating"), 0x95_a5a6);
        assert_eq!(status_color(""), 0x95_a5a6);
    }

    // --- schedule_event_color tests ---

    #[test]
    fn test_schedule_event_color_fired() {
        assert_eq!(schedule_event_color("fired"), 0x2e_cc71);
    }

    #[test]
    fn test_schedule_event_color_failed() {
        assert_eq!(schedule_event_color("failed"), 0xe7_4c3c);
    }

    #[test]
    fn test_schedule_event_color_exhausted() {
        assert_eq!(schedule_event_color("exhausted"), 0x34_98db);
    }

    #[test]
    fn test_schedule_event_color_unknown() {
        assert_eq!(schedule_event_color("paused"), 0x95_a5a6);
        assert_eq!(schedule_event_color(""), 0x95_a5a6);
    }

    // --- should_notify tests ---

    #[test]
    fn test_should_notify_empty_filter_allows_all() {
        let config = DiscordWebhookConfig {
            webhook_url: "https://example.com".into(),
            events: vec![],
        };
        assert!(should_notify(&config, "running"));
        assert!(should_notify(&config, "dead"));
        assert!(should_notify(&config, "completed"));
    }

    #[test]
    fn test_should_notify_with_filter() {
        let config = DiscordWebhookConfig {
            webhook_url: "https://example.com".into(),
            events: vec!["completed".into(), "dead".into()],
        };
        assert!(!should_notify(&config, "running"));
        assert!(should_notify(&config, "dead"));
        assert!(should_notify(&config, "completed"));
        assert!(!should_notify(&config, "stale"));
    }

    // --- should_notify_schedule tests ---

    #[test]
    fn test_should_notify_schedule_empty_filter() {
        let config = DiscordWebhookConfig {
            webhook_url: "https://example.com".into(),
            events: vec![],
        };
        assert!(should_notify_schedule(&config, "fired"));
        assert!(should_notify_schedule(&config, "failed"));
    }

    #[test]
    fn test_should_notify_schedule_with_filter() {
        let config = DiscordWebhookConfig {
            webhook_url: "https://example.com".into(),
            events: vec!["fired".into()],
        };
        assert!(should_notify_schedule(&config, "fired"));
        assert!(!should_notify_schedule(&config, "failed"));
    }

    // --- build_discord_payload tests ---

    #[test]
    fn test_build_payload_basic() {
        let event = test_event("running");
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
        assert_eq!(fields[0]["value"], "running");
        assert_eq!(fields[1]["name"], "Node");
        assert_eq!(fields[1]["value"], "node-1");
    }

    #[test]
    fn test_build_payload_with_previous_status() {
        let event = SessionEvent {
            previous_status: Some("creating".into()),
            ..test_event("running")
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
            ..test_event("completed")
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
            previous_status: Some("stale".into()),
            output_snippet: Some("output".into()),
            ..test_event("running")
        };
        let payload = build_discord_payload(&event);

        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 4); // Status + Node + Previous + Output
    }

    #[test]
    fn test_build_payload_dead_color() {
        let event = test_event("dead");
        let payload = build_discord_payload(&event);
        assert_eq!(payload["embeds"][0]["color"], 0xe7_4c3c);
    }

    // --- build_schedule_discord_payload tests ---

    #[test]
    fn test_build_schedule_payload_basic() {
        let event = test_schedule_event("fired");
        let payload = build_schedule_discord_payload(&event);

        let embeds = payload["embeds"].as_array().unwrap();
        assert_eq!(embeds.len(), 1);

        let embed = &embeds[0];
        assert_eq!(embed["title"], "Schedule: nightly-review");
        assert!(embed["description"].as_str().unwrap().contains("sch-1"));
        assert_eq!(embed["color"], 0x2e_cc71);

        let fields = embed["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2); // Event + Node
        assert_eq!(fields[0]["name"], "Event");
        assert_eq!(fields[0]["value"], "fired");
    }

    #[test]
    fn test_build_schedule_payload_with_session() {
        let event = ScheduleEvent {
            session_id: Some("sess-42".into()),
            ..test_schedule_event("fired")
        };
        let payload = build_schedule_discord_payload(&event);

        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[2]["name"], "Session");
        assert_eq!(fields[2]["value"], "sess-42");
    }

    #[test]
    fn test_build_schedule_payload_with_error() {
        let event = ScheduleEvent {
            error: Some("spawn failed".into()),
            ..test_schedule_event("failed")
        };
        let payload = build_schedule_discord_payload(&event);

        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[2]["name"], "Error");
        assert!(
            fields[2]["value"]
                .as_str()
                .unwrap()
                .contains("spawn failed")
        );
    }

    #[test]
    fn test_build_schedule_payload_failed_color() {
        let event = test_schedule_event("failed");
        let payload = build_schedule_discord_payload(&event);
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
        let result = notifier.send(&test_event("running")).await;
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
        let result = notifier.send(&test_event("running")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_schedule_success() {
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
        let result = notifier.send_schedule(&test_schedule_event("fired")).await;
        assert!(result.is_ok());
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
            events: vec!["dead".into()], // Only dead events
        };
        let notifier = DiscordNotifier::new(config);
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Send a "running" event — should be filtered out (won't attempt HTTP)
        event_tx
            .send(PulpoEvent::Session(test_event("running")))
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
            .send(PulpoEvent::Session(test_event("running")))
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
            events: vec!["dead".into()],
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
                status: "running".into(),
                previous_status: None,
                node_name: "n".into(),
                output_snippet: None,
                waiting_for_input: None,
                timestamp: "t".into(),
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

    #[tokio::test]
    async fn test_notification_loop_schedule_send_error() {
        // Use a URL that will fail immediately — tests the schedule send error path
        let config = DiscordWebhookConfig {
            webhook_url: "http://127.0.0.1:1/webhook".into(),
            events: vec![],
        };
        let notifier = DiscordNotifier::new(config);
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        event_tx
            .send(PulpoEvent::Schedule(test_schedule_event("fired")))
            .unwrap();

        let handle = tokio::spawn(run_notification_loop(notifier, rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("should finish")
            .expect("should not panic");
    }

    #[tokio::test]
    async fn test_notification_loop_schedule_event() {
        // Use a filter that won't match schedule events, so no HTTP
        let config = DiscordWebhookConfig {
            webhook_url: "https://example.com/webhook".into(),
            events: vec!["dead".into()],
        };
        let notifier = DiscordNotifier::new(config);
        let (event_tx, rx) = tokio::sync::broadcast::channel::<PulpoEvent>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Send a schedule event
        event_tx
            .send(PulpoEvent::Schedule(test_schedule_event("fired")))
            .unwrap();

        let handle = tokio::spawn(run_notification_loop(notifier, rx, shutdown_rx));

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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
