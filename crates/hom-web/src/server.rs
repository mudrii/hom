use axum::{
    Router,
    extract::ws::{Message, WebSocket},
    extract::{State, WebSocketUpgrade},
    response::{Html, IntoResponse},
    routing::get,
};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};

use crate::{
    frame::{WebFrame, WebInput},
    viewer::VIEWER_HTML,
};

#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<WebFrame>,
    input_tx: mpsc::Sender<WebInput>,
}

pub struct WebServer {
    port: u16,
    tx: broadcast::Sender<WebFrame>,
    input_tx: mpsc::Sender<WebInput>,
}

impl WebServer {
    pub fn new(
        port: u16,
        tx: broadcast::Sender<WebFrame>,
        input_tx: mpsc::Sender<WebInput>,
    ) -> Self {
        WebServer { port, tx, input_tx }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let state = AppState {
            tx: self.tx,
            input_tx: self.input_tx,
        };

        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
        info!("HOM web view at http://{addr}");

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| anyhow::anyhow!("web server bind on port {}: {e}", self.port))?;
        serve_listener(listener, state).await
    }
}

fn app_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_viewer))
        .route("/ws", get(ws_handler))
        .with_state(state)
}

async fn serve_listener(listener: tokio::net::TcpListener, state: AppState) -> anyhow::Result<()> {
    axum::serve(listener, app_router(state))
        .await
        .map_err(|e| anyhow::anyhow!("web server error: {e}"))?;
    Ok(())
}

async fn serve_viewer() -> impl IntoResponse {
    Html(VIEWER_HTML)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: AppState) {
    let mut rx = state.tx.subscribe();
    loop {
        tokio::select! {
            frame = rx.recv() => match frame {
                Ok(f) => {
                    let json = match serde_json::to_string(&f) {
                        Ok(j) => j,
                        Err(e) => { warn!("WS frame serialize failed: {e}"); continue; }
                    };
                    if socket.send(Message::Text(json.into())).await.is_err() {
                        debug!("WS client disconnected");
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => warn!("WS client lagged {n} frames"),
                Err(broadcast::error::RecvError::Closed) => break,
            },
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<WebInput>(&text) {
                        Ok(input) => {
                            if let Err(e) = state.input_tx.try_send(input) {
                                warn!("WS input channel full, dropping keystroke: {e}");
                            }
                        }
                        Err(e) => warn!("WS bad input: {e}"),
                    }
                }
                Some(Ok(Message::Close(_))) | None => { debug!("WS closed"); break; }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::{broadcast, mpsc};
    use tokio_tungstenite::tungstenite::Message;

    use super::*;
    use crate::frame::{WebCell, WebFrame, WebPane};

    async fn spawn_test_server() -> (
        std::net::SocketAddr,
        broadcast::Sender<WebFrame>,
        mpsc::Receiver<WebInput>,
        tokio::task::JoinHandle<()>,
    ) {
        let (tx, _) = broadcast::channel(8);
        let (input_tx, input_rx) = mpsc::channel(8);
        let state = AppState {
            tx: tx.clone(),
            input_tx,
        };

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let _ = serve_listener(listener, state).await;
        });

        wait_until_ready(addr).await;
        (addr, tx, input_rx, handle)
    }

    async fn wait_until_ready(addr: std::net::SocketAddr) {
        for _ in 0..50 {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("web server did not become ready at {addr}");
    }

    fn sample_frame() -> WebFrame {
        WebFrame::new(vec![WebPane {
            pane_id: "1".to_string(),
            title: "Claude".to_string(),
            cols: 2,
            rows: 1,
            cursor_col: 0,
            cursor_row: 0,
            cells: vec![
                WebCell {
                    ch: 'O',
                    ..WebCell::default()
                },
                WebCell {
                    ch: 'K',
                    ..WebCell::default()
                },
            ],
            focused: true,
        }])
    }

    #[tokio::test]
    async fn root_route_serves_viewer_html() {
        let (addr, _tx, _input_rx, handle) = spawn_test_server().await;

        let body = http_get(addr, "/").await;
        assert!(body.contains("HOM"));
        assert!(body.contains("/ws"));

        handle.abort();
    }

    #[tokio::test]
    async fn websocket_broadcasts_frames_to_clients() {
        let (addr, tx, _input_rx, handle) = spawn_test_server().await;
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();

        tx.send(sample_frame()).unwrap();

        let message = tokio::time::timeout(Duration::from_secs(1), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        match message {
            Message::Text(text) => {
                assert!(text.contains("\"pane_id\":\"1\""));
                assert!(text.contains("\"title\":\"Claude\""));
            }
            other => panic!("unexpected websocket message: {other:?}"),
        }

        handle.abort();
    }

    #[tokio::test]
    async fn websocket_forwards_browser_input() {
        let (addr, _tx, mut input_rx, handle) = spawn_test_server().await;
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();

        ws.send(Message::Text(
            serde_json::to_string(&serde_json::json!({
                "pane_id": "7",
                "text": "hello"
            }))
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();

        let input = tokio::time::timeout(Duration::from_secs(1), input_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(input.pane_id, "7");
        assert_eq!(input.text, "hello");

        handle.abort();
    }

    async fn http_get(addr: std::net::SocketAddr, path: &str) -> String {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();

        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await.unwrap();
        let response = String::from_utf8(bytes).unwrap();
        response.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
    }
}
