use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::nonpoison::{Mutex, MutexGuard};
use tokio::sync::broadcast;

use crate::clock;

pub const BUFFER_CAPACITY: usize = 500;
const BROADCAST_CAPACITY: usize = 256;

pub const KIND_CHAT: &str = "chat";
pub const KIND_NOTICE: &str = "notice";
pub const KIND_CLEAR: &str = "clear";
pub const KIND_CLEAR_USER: &str = "clear_user";
pub const KIND_DELETE: &str = "delete";
pub const KIND_EVENT: &str = "event";
pub const LOCAL_ID_PREFIX: &str = "local-";

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Badge {
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct User {
    pub id: String,
    pub login: String,
    pub name: String,
    pub color: Option<String>,
    pub badges: Vec<Badge>,
    pub moderator: bool,
    pub subscriber: bool,
    pub vip: bool,
    pub broadcaster: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Reply {
    pub id: String,
    pub user: String,
    pub text: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Fragment {
    Text {
        text: String,
    },
    Emote {
        text: String,
        id: String,
    },
    Cheermote {
        text: String,
        prefix: String,
        bits: u64,
        tier: u64,
    },
    Mention {
        text: String,
        login: String,
        name: String,
    },
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ChatMessage {
    pub seq: u64,
    pub id: String,
    pub channel: String,
    pub ts: String,
    pub kind: String,
    pub user: User,
    pub text: String,
    pub fragments: Vec<Fragment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<Reply>,
    pub action: bool,
    pub highlighted: bool,
    pub moderatable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<Event>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Event {
    pub kind: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
}

impl ChatMessage {
    pub fn new(
        channel: impl Into<String>,
        kind: impl Into<String>,
        user: User,
        text: impl Into<String>,
    ) -> Self {
        let text = text.into();
        Self {
            seq: 0,
            id: String::new(),
            channel: channel.into(),
            ts: clock::now_rfc3339(),
            kind: kind.into(),
            user,
            fragments: vec![Fragment::Text { text: text.clone() }],
            text,
            reply: None,
            action: false,
            highlighted: false,
            moderatable: false,
            event: None,
        }
    }
}

struct Inner {
    last_seq: u64,
    read: u64,
    items: VecDeque<ChatMessage>,
}

#[derive(Clone)]
pub struct Store {
    inner: Arc<Mutex<Inner>>,
    tx: broadcast::Sender<ChatMessage>,
    capacity: usize,
}

impl Store {
    pub fn new() -> Self {
        Self::with_capacity(BUFFER_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                last_seq: 0,
                read: 0,
                items: VecDeque::with_capacity(capacity.min(1024)),
            })),
            tx,
            capacity,
        }
    }

    pub fn push(&self, mut message: ChatMessage) -> ChatMessage {
        let mut inner = self.lock();
        inner.last_seq += 1;
        message.seq = inner.last_seq;

        match message.kind.as_str() {
            KIND_CLEAR => inner.items.clear(),
            KIND_CLEAR_USER => {
                let login = message.user.login.clone();
                inner.items.retain(|item| item.user.login != login);
            }
            KIND_DELETE => {
                let id = message.id.clone();
                inner.items.retain(|item| item.id != id);
            }
            _ => {
                inner.items.push_back(message.clone());
                while inner.items.len() > self.capacity {
                    inner.items.pop_front();
                }
            }
        }

        drop(inner);
        let _ = self.tx.send(message.clone());
        message
    }

    pub fn last_seq(&self) -> u64 {
        self.lock().last_seq
    }

    pub fn mark_read(&self) -> u64 {
        let mut inner = self.lock();
        inner.read = inner.last_seq;
        inner.read
    }

    pub fn unread(&self) -> u64 {
        let inner = self.lock();
        let read = inner.read;
        inner
            .items
            .iter()
            .filter(|message| message.seq > read && message.kind == KIND_CHAT)
            .count() as u64
    }

    pub fn snapshot(&self, after: u64, limit: usize) -> Vec<ChatMessage> {
        let inner = self.lock();
        let mut items = inner
            .items
            .iter()
            .filter(|m| m.seq > after)
            .cloned()
            .collect::<Vec<_>>();
        if items.len() > limit {
            items.drain(..items.len() - limit);
        }
        items
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ChatMessage> {
        self.tx.subscribe()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock()
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: &str) -> ChatMessage {
        ChatMessage::new("channel", KIND_CHAT, User::default(), text)
    }

    fn from(login: &str, text: &str) -> ChatMessage {
        ChatMessage::new(
            "channel",
            KIND_CHAT,
            User {
                login: login.to_string(),
                ..User::default()
            },
            text,
        )
    }

    fn marker(kind: &str, id: &str) -> ChatMessage {
        let mut message = ChatMessage::new("channel", kind, User::default(), "");
        message.id = id.to_string();
        message
    }

    #[test]
    fn seq_grows_and_snapshot_filters() {
        let store = Store::with_capacity(10);
        store.push(message("one"));
        store.push(message("two"));
        let last = store.push(message("three"));

        assert_eq!(last.seq, 3);
        assert_eq!(store.last_seq(), 3);

        let items = store.snapshot(1, 10);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "two");
        assert_eq!(items[1].text, "three");
    }

    #[test]
    fn buffer_drops_the_oldest() {
        let store = Store::with_capacity(2);
        store.push(message("one"));
        store.push(message("two"));
        store.push(message("three"));

        let items = store.snapshot(0, 10);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "two");
    }

    #[test]
    fn limit_keeps_the_tail() {
        let store = Store::with_capacity(10);
        store.push(message("one"));
        store.push(message("two"));
        store.push(message("three"));

        let items = store.snapshot(0, 2);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "two");
    }

    #[test]
    fn delete_removes_the_message_and_is_not_stored() {
        let store = Store::with_capacity(10);
        let mut first = message("one");
        first.id = "msg-1".to_string();
        store.push(first);
        store.push(message("two"));

        store.push(marker(KIND_DELETE, "msg-1"));

        let items = store.snapshot(0, 10);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "two");
    }

    #[test]
    fn clear_user_removes_only_that_user() {
        let store = Store::with_capacity(10);
        store.push(from("alice", "one"));
        store.push(from("bob", "two"));
        store.push(from("alice", "three"));

        let mut clear = marker(KIND_CLEAR_USER, "");
        clear.user.login = "alice".to_string();
        store.push(clear);

        let items = store.snapshot(0, 10);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].user.login, "bob");
    }

    #[test]
    fn clear_empties_the_buffer() {
        let store = Store::with_capacity(10);
        store.push(message("one"));
        store.push(message("two"));

        store.push(marker(KIND_CLEAR, ""));

        assert!(store.snapshot(0, 10).is_empty());
        assert_eq!(store.last_seq(), 3);
    }

    #[test]
    fn unread_counts_only_chat_messages() {
        let store = Store::with_capacity(10);
        store.push(marker(KIND_NOTICE, ""));
        store.push(message("one"));
        store.push(marker(KIND_DELETE, "missing"));
        store.push(marker(KIND_EVENT, ""));

        assert_eq!(store.unread(), 1);
    }

    #[test]
    fn unread_counts_what_the_buffer_still_holds() {
        let store = Store::with_capacity(2);
        store.push(message("one"));
        store.push(message("two"));
        store.push(message("three"));

        assert_eq!(store.unread(), 2);
    }

    #[test]
    fn mark_read_clears_unread_and_keeps_going() {
        let store = Store::with_capacity(10);
        store.push(message("one"));
        store.push(message("two"));

        assert_eq!(store.mark_read(), 2);
        assert_eq!(store.unread(), 0);

        store.push(message("three"));

        assert_eq!(store.unread(), 1);
        assert_eq!(store.mark_read(), 3);
        assert_eq!(store.unread(), 0);
    }

    #[test]
    fn a_deleted_message_leaves_the_unread_count() {
        let store = Store::with_capacity(10);
        let mut first = message("one");
        first.id = "msg-1".to_string();
        store.push(first);
        store.push(message("two"));

        assert_eq!(store.unread(), 2);

        store.push(marker(KIND_DELETE, "msg-1"));

        assert_eq!(store.unread(), 1);
    }
}
