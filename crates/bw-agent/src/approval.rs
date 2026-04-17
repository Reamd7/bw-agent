use crate::process::ProcessInfo;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, oneshot};

/// Request for user approval of an SSH signing operation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub key_name: String,
    pub key_fingerprint: String,
    pub client_exe: String,
    pub client_pid: u32,
    pub process_chain: Vec<ProcessInfo>,
    pub timestamp: u64,
}

/// Queue of pending SSH sign approval requests.
///
/// Each request gets a oneshot channel. The SSH agent awaits the receiver;
/// the UI (Tauri frontend) calls `respond()` to send the decision.
pub struct ApprovalQueue {
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    requests: Arc<Mutex<Vec<ApprovalRequest>>>,
}

impl ApprovalQueue {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a new approval request. Returns the request metadata and a
    /// receiver that resolves to `true` (approved) or `false` (denied).
    pub async fn create_request(
        &self,
        key_name: &str,
        fingerprint: &str,
        client_exe: &str,
        pid: u32,
        process_chain: Vec<ProcessInfo>,
    ) -> (ApprovalRequest, oneshot::Receiver<bool>) {
        let (tx, rx) = oneshot::channel();
        let request = ApprovalRequest {
            id: uuid::Uuid::new_v4().to_string(),
            key_name: key_name.to_string(),
            key_fingerprint: fingerprint.to_string(),
            client_exe: client_exe.to_string(),
            client_pid: pid,
            process_chain,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        self.pending.lock().await.insert(request.id.clone(), tx);
        self.requests.lock().await.push(request.clone());
        (request, rx)
    }

    /// Respond to a pending approval request.
    pub async fn respond(&self, request_id: &str, approved: bool) {
        if let Some(tx) = self.pending.lock().await.remove(request_id) {
            let _ = tx.send(approved);
        }
        self.requests.lock().await.retain(|r| r.id != request_id);
    }

    /// Get all pending (unanswered) approval requests.
    pub async fn pending(&self) -> Vec<ApprovalRequest> {
        self.requests.lock().await.clone()
    }
}

impl Default for ApprovalQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_approval_request_approve() {
        let queue = ApprovalQueue::new();
        let (request, rx) = queue
            .create_request("ssh-ed25519", "SHA256:abc", "ssh.exe", 1234, vec![])
            .await;
        assert_eq!(request.key_fingerprint, "SHA256:abc");
        queue.respond(&request.id, true).await;
        assert!(rx.await.unwrap());
    }

    #[tokio::test]
    async fn test_approval_request_deny() {
        let queue = ApprovalQueue::new();
        let (request, rx) = queue
            .create_request("ssh-ed25519", "SHA256:abc", "ssh.exe", 1234, vec![])
            .await;
        queue.respond(&request.id, false).await;
        assert!(!rx.await.unwrap());
    }

    #[tokio::test]
    async fn test_pending_approvals() {
        let queue = ApprovalQueue::new();
        let (req, _rx) = queue
            .create_request("key1", "fp1", "ssh.exe", 100, vec![])
            .await;
        assert_eq!(queue.pending().await.len(), 1);
        queue.respond(&req.id, true).await;
        assert_eq!(queue.pending().await.len(), 0);
    }
}
