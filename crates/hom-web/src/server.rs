use axum::{
    Router,
    extract::{State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    response::{Html, IntoResponse},
    routing::get,
};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};

use crate::{frame::{WebFrame, WebInput}, viewer::VIEWER_HTML};

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
    pub fn new(port: u16, tx: broadcast::Sender<WebFrame>, input_tx: mpsc::Sender<WebInput>) -> Self {
        WebServer { port, tx, input_tx }
    }

    pub async fn run(self) {
        let state = AppState { tx: self.tx, input_tx: self.input_tx };
        let app = Router::new()
            .route("/", get(serve_viewer))
            .route("/ws", get(ws_handler))
            .with_state(state);

        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
        info!("HOM web view at http://{addr}");

        let listener = tokio::net::TcpListener::bind(addr).await.expect("bind web server");
        axum::serve(listener, app).await.expect("web server error");
    }
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
