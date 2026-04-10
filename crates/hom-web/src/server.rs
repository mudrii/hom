use tokio::sync::{broadcast, mpsc};

use crate::frame::{WebFrame, WebInput};

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
        let _ = (self.port, self.tx, self.input_tx);
    }
}
