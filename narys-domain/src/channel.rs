//! Transport-neutral event subscriber. No execution authority is serializable.
use serde::Serialize;
use std::sync::Arc;
#[derive(Clone)]
pub enum InvokeResponseBody {
    Json(String),
}
enum Sender<T> {
    Encoded(Arc<dyn Fn(InvokeResponseBody) -> Result<(), std::io::Error> + Send + Sync>),
    Typed(Arc<dyn Fn(T) -> Result<(), std::io::Error> + Send + Sync>),
}
pub struct Channel<T> {
    sender: Sender<T>,
}
impl<T> Clone for Channel<T> {
    fn clone(&self) -> Self {
        Self {
            sender: match &self.sender {
                Sender::Encoded(s) => Sender::Encoded(s.clone()),
                Sender::Typed(s) => Sender::Typed(s.clone()),
            },
        }
    }
}
impl<T: Serialize> Channel<T> {
    pub fn new(
        sender: impl Fn(InvokeResponseBody) -> Result<(), std::io::Error> + Send + Sync + 'static,
    ) -> Self {
        Self {
            sender: Sender::Encoded(Arc::new(sender)),
        }
    }
    pub fn from_sender(
        sender: impl Fn(T) -> Result<(), std::io::Error> + Send + Sync + 'static,
    ) -> Self {
        Self {
            sender: Sender::Typed(Arc::new(sender)),
        }
    }
    pub fn send(&self, value: T) -> Result<(), std::io::Error> {
        match &self.sender {
            Sender::Typed(send) => send(value),
            Sender::Encoded(send) => send(InvokeResponseBody::Json(
                serde_json::to_string(&value).map_err(std::io::Error::other)?,
            )),
        }
    }
}
