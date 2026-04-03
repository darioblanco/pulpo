use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use pulpo_common::api::NodeCommand;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct CommandQueue {
    queues: Arc<RwLock<HashMap<String, VecDeque<NodeCommand>>>>,
}

impl CommandQueue {
    pub fn new() -> Self {
        Self {
            queues: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Push a command onto the queue for the given node.
    pub async fn enqueue(&self, node_name: &str, command: NodeCommand) {
        let mut queues = self.queues.write().await;
        queues
            .entry(node_name.to_owned())
            .or_default()
            .push_back(command);
    }

    /// Take all pending commands for a node, leaving its queue empty.
    pub async fn drain(&self, node_name: &str) -> Vec<NodeCommand> {
        let mut queues = self.queues.write().await;
        queues
            .get_mut(node_name)
            .map(|q| q.drain(..).collect())
            .unwrap_or_default()
    }

    /// Return the number of pending commands for a node.
    pub async fn pending_count(&self, node_name: &str) -> usize {
        let queues = self.queues.read().await;
        queues.get(node_name).map_or(0, VecDeque::len)
    }
}

impl Default for CommandQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop_cmd(id: &str, session_id: &str) -> NodeCommand {
        NodeCommand::StopSession {
            command_id: id.into(),
            session_id: session_id.into(),
        }
    }

    fn create_cmd(id: &str, name: &str) -> NodeCommand {
        NodeCommand::CreateSession {
            command_id: id.into(),
            name: name.into(),
            workdir: None,
            command: None,
            ink: None,
            description: None,
        }
    }

    #[tokio::test]
    async fn test_enqueue_and_drain() {
        let queue = CommandQueue::new();
        queue.enqueue("node-1", create_cmd("c1", "task-1")).await;
        queue.enqueue("node-1", create_cmd("c2", "task-2")).await;
        queue.enqueue("node-1", stop_cmd("c3", "s1")).await;

        let commands = queue.drain("node-1").await;
        assert_eq!(commands.len(), 3);

        // Verify order (FIFO)
        match &commands[0] {
            NodeCommand::CreateSession { command_id, .. } => assert_eq!(command_id, "c1"),
            NodeCommand::StopSession { .. } => panic!("expected CreateSession"),
        }
        match &commands[1] {
            NodeCommand::CreateSession { command_id, .. } => assert_eq!(command_id, "c2"),
            NodeCommand::StopSession { .. } => panic!("expected CreateSession"),
        }
        match &commands[2] {
            NodeCommand::StopSession { command_id, .. } => assert_eq!(command_id, "c3"),
            NodeCommand::CreateSession { .. } => panic!("expected StopSession"),
        }
    }

    #[tokio::test]
    async fn test_drain_empty() {
        let queue = CommandQueue::new();
        let commands = queue.drain("node-1").await;
        assert!(commands.is_empty());
    }

    #[tokio::test]
    async fn test_drain_clears_queue() {
        let queue = CommandQueue::new();
        queue.enqueue("node-1", create_cmd("c1", "task-1")).await;
        queue.enqueue("node-1", create_cmd("c2", "task-2")).await;

        let first = queue.drain("node-1").await;
        assert_eq!(first.len(), 2);

        let second = queue.drain("node-1").await;
        assert!(second.is_empty());
    }

    #[tokio::test]
    async fn test_separate_node_queues() {
        let queue = CommandQueue::new();
        queue.enqueue("node-1", create_cmd("c1", "task-1")).await;
        queue.enqueue("node-2", create_cmd("c2", "task-2")).await;

        let w1 = queue.drain("node-1").await;
        assert_eq!(w1.len(), 1);
        match &w1[0] {
            NodeCommand::CreateSession { command_id, .. } => assert_eq!(command_id, "c1"),
            NodeCommand::StopSession { .. } => panic!("expected CreateSession"),
        }

        // node-2 queue unaffected
        let w2 = queue.drain("node-2").await;
        assert_eq!(w2.len(), 1);
        match &w2[0] {
            NodeCommand::CreateSession { command_id, .. } => assert_eq!(command_id, "c2"),
            NodeCommand::StopSession { .. } => panic!("expected CreateSession"),
        }
    }

    #[tokio::test]
    async fn test_pending_count() {
        let queue = CommandQueue::new();
        assert_eq!(queue.pending_count("node-1").await, 0);

        queue.enqueue("node-1", create_cmd("c1", "t1")).await;
        queue.enqueue("node-1", create_cmd("c2", "t2")).await;
        queue.enqueue("node-1", stop_cmd("c3", "s1")).await;
        assert_eq!(queue.pending_count("node-1").await, 3);

        queue.drain("node-1").await;
        assert_eq!(queue.pending_count("node-1").await, 0);
    }

    #[tokio::test]
    async fn test_default() {
        let queue = CommandQueue::default();
        assert_eq!(queue.pending_count("any").await, 0);
    }

    #[tokio::test]
    async fn test_clone_shares_state() {
        let queue = CommandQueue::new();
        let cloned = queue.clone();
        queue.enqueue("node-1", create_cmd("c1", "t1")).await;
        assert_eq!(cloned.pending_count("node-1").await, 1);
    }

    #[tokio::test]
    async fn test_debug() {
        let queue = CommandQueue::new();
        let debug = format!("{queue:?}");
        assert!(debug.contains("CommandQueue"));
    }
}
