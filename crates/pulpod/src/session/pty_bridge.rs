use anyhow::Result;
use axum::extract::ws::Message;
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::{debug, warn};

/// Drive the bridge: read from PTY → send to WebSocket, read from WebSocket → write to PTY.
/// Handles binary data (terminal I/O) and text messages (resize control).
/// Returns when either side disconnects.
pub async fn run_bridge<R, W, S, K, E, F>(
    mut reader: R,
    mut writer: W,
    mut ws_sender: S,
    mut ws_receiver: K,
    resize_fn: F,
) -> Result<()>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
    S: futures::Sink<Message> + Unpin + Send,
    K: futures::Stream<Item = Result<Message, E>> + Unpin + Send,
    F: Fn(u16, u16) -> Result<()>,
{
    use futures::future::{self, Either};

    let mut buf = vec![0u8; 4096];

    loop {
        let read_fut = reader.read(&mut buf);
        let ws_fut = ws_receiver.next();
        let pinned_read = std::pin::pin!(read_fut);
        let pinned_ws = std::pin::pin!(ws_fut);
        let either = future::select(pinned_read, pinned_ws).await;

        match either {
            // PTY → WebSocket
            Either::Left((result, _)) => match result {
                Ok(0) => {
                    debug!("PTY closed");
                    break;
                }
                Ok(n) => {
                    let data = buf[..n].to_vec();
                    // If send fails, WS is disconnected — ws_receiver.next() will
                    // return None on the next iteration, cleanly exiting the loop.
                    let _ = ws_sender.send(Message::Binary(data.into())).await;
                }
                Err(e) => {
                    debug!("PTY read error: {e}");
                    break;
                }
            },
            // WebSocket → PTY
            Either::Right((msg, _)) => match msg {
                Some(Ok(Message::Binary(data))) => {
                    if writer.write_all(&data).await.is_err() {
                        debug!("PTY write failed, closing bridge");
                        break;
                    }
                }
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<pulpo_common::api::WsControl>(&text) {
                        Ok(pulpo_common::api::WsControl::Resize { cols, rows }) => {
                            if let Err(e) = resize_fn(cols, rows) {
                                warn!("Resize failed: {e}");
                            }
                        }
                        Err(_) => {
                            debug!("Ignoring invalid text message: {text}");
                        }
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                _ => {
                    debug!("WebSocket closed or error");
                    break;
                }
            },
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::mpsc;

    type WsItem = Result<Message, axum::Error>;
    type BoxRead = Box<dyn AsyncRead + Unpin + Send>;
    type PtyWrite = tokio::io::WriteHalf<tokio::io::DuplexStream>;

    fn mock_pty() -> (tokio::io::DuplexStream, tokio::io::DuplexStream) {
        tokio::io::duplex(4096)
    }

    fn split_pty(stream: tokio::io::DuplexStream) -> (BoxRead, PtyWrite) {
        let (r, w) = tokio::io::split(stream);
        (Box::new(r), w)
    }

    fn mock_ws() -> (
        mpsc::UnboundedSender<Message>,
        mpsc::UnboundedReceiver<Message>,
        mpsc::UnboundedSender<WsItem>,
        mpsc::UnboundedReceiver<WsItem>,
    ) {
        let (out_tx, out_rx) = mpsc::unbounded::<Message>();
        let (in_tx, in_rx) = mpsc::unbounded::<WsItem>();
        (out_tx, out_rx, in_tx, in_rx)
    }

    type ResizeFn = fn(u16, u16) -> Result<()>;

    #[allow(clippy::unnecessary_wraps)]
    fn noop_resize(_: u16, _: u16) -> Result<()> {
        Ok(())
    }

    fn failing_resize(_: u16, _: u16) -> Result<()> {
        anyhow::bail!("resize failed on purpose")
    }

    struct ErrorReader;

    impl AsyncRead for ErrorReader {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Err(std::io::Error::other("mock read error")))
        }
    }

    #[tokio::test]
    async fn test_pty_to_ws_binary_flow() {
        let (pty_server, mut pty_client) = mock_pty();
        let (pty_read, pty_write) = split_pty(pty_server);
        let (out_tx, mut out_rx, _in_tx, in_rx) = mock_ws();

        let bridge = tokio::spawn(run_bridge(
            pty_read,
            pty_write,
            out_tx,
            in_rx,
            noop_resize as ResizeFn,
        ));

        pty_client.write_all(b"hello from pty").await.unwrap();

        let msg = out_rx.next().await.unwrap();
        assert_eq!(msg, Message::Binary(b"hello from pty".to_vec().into()));

        drop(pty_client);
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), bridge)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ws_to_pty_binary_flow() {
        let (pty_server, mut pty_client) = mock_pty();
        let (pty_read, pty_write) = split_pty(pty_server);
        let (out_tx, _out_rx, in_tx, in_rx) = mock_ws();

        let bridge = tokio::spawn(run_bridge(
            pty_read,
            pty_write,
            out_tx,
            in_rx,
            noop_resize as ResizeFn,
        ));

        in_tx
            .unbounded_send(Ok(Message::Binary(b"hello from ws".to_vec().into())))
            .unwrap();

        let mut buf = vec![0u8; 64];
        let n = tokio::time::timeout(std::time::Duration::from_secs(2), pty_client.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..n], b"hello from ws");

        drop(in_tx);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), bridge).await;
    }

    #[tokio::test]
    async fn test_resize_control_message() {
        let (pty_server, pty_client) = mock_pty();
        let (pty_read, pty_write) = split_pty(pty_server);
        let (out_tx, _out_rx, in_tx, in_rx) = mock_ws();

        let bridge = tokio::spawn(run_bridge(
            pty_read,
            pty_write,
            out_tx,
            in_rx,
            noop_resize as ResizeFn,
        ));

        // Send a valid resize — bridge should process it and continue running
        let resize_msg = r#"{"type":"resize","cols":120,"rows":40}"#;
        in_tx
            .unbounded_send(Ok(Message::Text(resize_msg.into())))
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        drop(in_tx);
        drop(pty_client);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), bridge).await;
    }

    #[tokio::test]
    async fn test_bridge_terminates_on_ws_close() {
        let (pty_server, _pty_client) = mock_pty();
        let (pty_read, pty_write) = split_pty(pty_server);
        let (out_tx, _out_rx, in_tx, in_rx) = mock_ws();

        let bridge = tokio::spawn(run_bridge(
            pty_read,
            pty_write,
            out_tx,
            in_rx,
            noop_resize as ResizeFn,
        ));

        in_tx.unbounded_send(Ok(Message::Close(None))).unwrap();

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), bridge)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_bridge_terminates_on_ws_drop() {
        let (pty_server, _pty_client) = mock_pty();
        let (pty_read, pty_write) = split_pty(pty_server);
        let (out_tx, _out_rx, in_tx, in_rx) = mock_ws();

        let bridge = tokio::spawn(run_bridge(
            pty_read,
            pty_write,
            out_tx,
            in_rx,
            noop_resize as ResizeFn,
        ));

        drop(in_tx);

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), bridge)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_bridge_terminates_on_pty_close() {
        let (pty_server, pty_client) = mock_pty();
        let (pty_read, pty_write) = split_pty(pty_server);
        let (out_tx, _out_rx, _in_tx, in_rx) = mock_ws();

        let bridge = tokio::spawn(run_bridge(
            pty_read,
            pty_write,
            out_tx,
            in_rx,
            noop_resize as ResizeFn,
        ));

        drop(pty_client);

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), bridge)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_text_message_ignored() {
        let (pty_server, pty_client) = mock_pty();
        let (pty_read, pty_write) = split_pty(pty_server);
        let (out_tx, _out_rx, in_tx, in_rx) = mock_ws();

        let bridge = tokio::spawn(run_bridge(
            pty_read,
            pty_write,
            out_tx,
            in_rx,
            noop_resize as ResizeFn,
        ));

        in_tx
            .unbounded_send(Ok(Message::Text("not json".into())))
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        drop(in_tx);
        drop(pty_client);

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), bridge)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_pty_read_error() {
        let (pty_server, _pty_client) = mock_pty();
        let (_, pty_write) = split_pty(pty_server);
        let (out_tx, _out_rx, _in_tx, in_rx) = mock_ws();
        let reader: BoxRead = Box::new(ErrorReader);

        let result: Result<()> =
            run_bridge(reader, pty_write, out_tx, in_rx, noop_resize as ResizeFn).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_pty_write_failure() {
        let (pty_server, _pty_client) = mock_pty();
        let (pty_read, _) = split_pty(pty_server);
        let (out_tx, _out_rx, in_tx, in_rx) = mock_ws();

        // Create a writer whose read-end is dropped → writes fail with BrokenPipe
        let (broken_writer, drop_reader) = tokio::io::duplex(1);
        drop(drop_reader);
        let (_, pty_write) = split_pty(broken_writer);

        let bridge = tokio::spawn(run_bridge(
            pty_read,
            pty_write,
            out_tx,
            in_rx,
            noop_resize as ResizeFn,
        ));

        in_tx
            .unbounded_send(Ok(Message::Binary(b"data".to_vec().into())))
            .unwrap();

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), bridge)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ping_message_ignored() {
        let (pty_server, pty_client) = mock_pty();
        let (pty_read, pty_write) = split_pty(pty_server);
        let (out_tx, _out_rx, in_tx, in_rx) = mock_ws();

        let bridge = tokio::spawn(run_bridge(
            pty_read,
            pty_write,
            out_tx,
            in_rx,
            noop_resize as ResizeFn,
        ));

        in_tx
            .unbounded_send(Ok(Message::Ping(b"ping".to_vec().into())))
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        drop(in_tx);
        drop(pty_client);

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), bridge)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_resize_error_does_not_crash_bridge() {
        let (pty_server, pty_client) = mock_pty();
        let (pty_read, pty_write) = split_pty(pty_server);
        let (out_tx, _out_rx, in_tx, in_rx) = mock_ws();

        let bridge = tokio::spawn(run_bridge(
            pty_read,
            pty_write,
            out_tx,
            in_rx,
            failing_resize as ResizeFn,
        ));

        let resize_msg = r#"{"type":"resize","cols":80,"rows":24}"#;
        in_tx
            .unbounded_send(Ok(Message::Text(resize_msg.into())))
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        drop(in_tx);
        drop(pty_client);

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), bridge)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_ok());
    }
}
