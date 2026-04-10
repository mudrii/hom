use tokio::sync::mpsc;
use hom_core::types::McpRequest;

pub struct McpServer {
    tx: mpsc::Sender<McpRequest>,
}

impl McpServer {
    pub fn new(tx: mpsc::Sender<McpRequest>) -> Self {
        McpServer { tx }
    }

    pub async fn run(self) {
        // TODO: implemented in Task 5
        let _ = self.tx;
    }
}
