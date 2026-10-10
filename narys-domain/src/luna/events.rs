//! Native Conversation observation. Execution owns a sink; the main WebView is
//! a replaceable subscriber. Other task entry points remain explicitly UI-bound.
use super::task::{TaskEvent, TaskEventKind, TaskId, TaskState};
use serde::Serialize;
use std::{collections::VecDeque, sync::{Arc, Mutex}};
use crate::channel::Channel;

pub const MAX_EVENTS: usize = 128;
pub const MAX_REPLAY_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskAttachmentPolicy { UiBound, HeadlessSafe }

pub enum TaskEventSink {
    UiBound(Channel<TaskEvent>),
    HeadlessSafe(Arc<Mutex<Observation>>),
}
impl TaskEventSink {
    pub fn send(&self, event: TaskEvent) -> Result<(), String> {
        match self {
            Self::UiBound(channel) => channel.send(event).map_err(|_| "channel_closed".into()),
            Self::HeadlessSafe(observation) => {
                let mut observation = observation.lock().map_err(|_| "event_broker_failed")?;
                if event.task_id != observation.task_id || event.sequence <= observation.sequence {
                    return Err("event_sequence_invalid".into());
                }
                observation.sequence = event.sequence;
                observation.state = event.state;
                if matches!(event.kind, TaskEventKind::TaskCompleted | TaskEventKind::TaskCancelled | TaskEventKind::TaskFailed { .. }) {
                    observation.terminal = Some(event.clone());
                }
                let bytes = serde_json::to_vec(&event).map_err(|_| "event_encoding_failed")?.len();
                if bytes <= MAX_REPLAY_BYTES {
                    observation.events.push_back((event.clone(), bytes));
                    observation.bytes += bytes;
                }
                while observation.events.len() > MAX_EVENTS || observation.bytes > MAX_REPLAY_BYTES {
                    if let Some((_, bytes)) = observation.events.pop_front() { observation.bytes -= bytes; }
                }
                // A failed visual subscriber has no authority over execution.
                if observation.subscriber.as_ref().is_some_and(|channel| channel.send(event).is_err()) {
                    observation.subscriber = None;
                }
                Ok(())
            }
        }
    }
    pub fn policy(&self) -> TaskAttachmentPolicy {
        match self { Self::UiBound(_) => TaskAttachmentPolicy::UiBound, Self::HeadlessSafe(_) => TaskAttachmentPolicy::HeadlessSafe }
    }
}
impl From<Channel<TaskEvent>> for TaskEventSink {
    fn from(channel: Channel<TaskEvent>) -> Self { Self::UiBound(channel) }
}

pub struct Observation {
    task_id: TaskId,
    session_id: i64,
    state: TaskState,
    sequence: u32,
    terminal: Option<TaskEvent>,
    events: VecDeque<(TaskEvent, usize)>,
    bytes: usize,
    subscriber: Option<Channel<TaskEvent>>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskObservation {
    pub task_id: TaskId,
    pub session_id: i64,
    pub state: TaskState,
    pub sequence: u32,
    pub terminal: Option<TaskEvent>,
    pub replay_complete: bool,
}
impl Observation {
    fn snapshot(&self, after: u32) -> TaskObservation {
        let contiguous = self.events.iter().filter(|(e, _)| e.sequence > after)
            .map(|(e, _)| e.sequence).eq((after + 1)..=self.sequence);
        TaskObservation { task_id: self.task_id, session_id: self.session_id, state: self.state,
            sequence: self.sequence, terminal: self.terminal.clone(), replay_complete: contiguous }
    }
}

/// Exactly one product Conversation and one main subscriber in this process.
/// Retain only the latest task (active or terminal); persisted answers live in SQLite.
#[derive(Default)]
pub struct TaskEventBroker(Mutex<Option<Arc<Mutex<Observation>>>>);
impl TaskEventBroker {
    pub fn start(&self, task_id: TaskId, session_id: i64, channel: Channel<TaskEvent>) -> Result<TaskEventSink, String> {
        let mut latest = self.0.lock().map_err(|_| "event_broker_failed")?;
        if latest.as_ref().is_some_and(|o| matches!(o.lock().unwrap_or_else(|p| p.into_inner()).state, TaskState::Pending | TaskState::Running)) {
            return Err("conversation_busy".into());
        }
        let observation = Arc::new(Mutex::new(Observation { task_id, session_id, state: TaskState::Pending,
            sequence: 0, terminal: None, events: VecDeque::new(), bytes: 0, subscriber: Some(channel) }));
        *latest = Some(observation.clone());
        Ok(TaskEventSink::HeadlessSafe(observation))
    }
    pub fn snapshot(&self, session_id: i64) -> Option<TaskObservation> {
        let latest = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let observation = latest.as_ref()?.lock().unwrap_or_else(|p| p.into_inner());
        (observation.session_id == session_id).then(|| observation.snapshot(0))
    }
    pub fn attach(&self, task_id: TaskId, session_id: i64, after: u32, channel: Channel<TaskEvent>) -> Result<TaskObservation, String> {
        let latest = self.0.lock().map_err(|_| "event_broker_failed")?;
        let observation = latest.as_ref().ok_or("task_not_observable")?;
        let mut observation = observation.lock().map_err(|_| "event_broker_failed")?;
        if observation.task_id != task_id || observation.session_id != session_id || after > observation.sequence {
            return Err("task_not_observable".into());
        }
        // Replay and installing the subscriber share the producer lock: no gaps/races.
        for (event, _) in &observation.events {
            if event.sequence > after { channel.send(event.clone()).map_err(|_| "subscriber_closed")?; }
        }
        let snapshot = observation.snapshot(after);
        observation.subscriber = Some(channel);
        Ok(snapshot)
    }
    pub fn detach_main(&self) {
        let latest = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(observation) = latest.as_ref() { observation.lock().unwrap_or_else(|p| p.into_inner()).subscriber = None; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    fn event(sequence: u32, kind: TaskEventKind, state: TaskState) -> TaskEvent { TaskEvent { task_id: TaskId(17), sequence, state, kind } }
    fn subscriber() -> (Channel<TaskEvent>, mpsc::Receiver<serde_json::Value>) {
        let (tx, rx) = mpsc::channel();
        (Channel::new(move |body| {
            if let crate::channel::InvokeResponseBody::Json(body) = body {
                tx.send(serde_json::from_str(&body).unwrap()).map_err(|_| std::io::Error::other("closed"))?;
            }
            Ok(())
        }), rx)
    }
    #[test]
    fn headless_safe_broken_subscriber_terminal_and_reattach_are_scoped() {
        let broker = TaskEventBroker::default();
        let (channel, rx) = subscriber(); drop(rx);
        let sink = broker.start(TaskId(17), 9, channel).unwrap();
        assert_eq!(sink.policy(), TaskAttachmentPolicy::HeadlessSafe);
        sink.send(event(1, TaskEventKind::TaskStarted, TaskState::Running)).unwrap();
        broker.detach_main();
        sink.send(event(2, TaskEventKind::TaskCompleted, TaskState::Completed)).unwrap();
        let snapshot = broker.snapshot(9).unwrap();
        assert_eq!(snapshot.task_id, TaskId(17)); assert_eq!(snapshot.state, TaskState::Completed);
        assert!(snapshot.terminal.is_some()); assert!(broker.snapshot(10).is_none());
        let (channel, _) = subscriber();
        assert!(broker.attach(TaskId(18), 9, 0, channel).is_err());
        let (channel, rx) = subscriber();
        let recovered = broker.attach(TaskId(17), 9, 0, channel).unwrap();
        assert!(recovered.replay_complete); assert_eq!(rx.recv().unwrap()["sequence"], 1); assert_eq!(rx.recv().unwrap()["sequence"], 2);
        assert!(sink.send(event(2, TaskEventKind::TaskStarted, TaskState::Running)).is_err());
    }
    #[test]
    fn ui_bound_remains_fail_closed() {
        let (channel, rx) = subscriber(); drop(rx);
        let sink = TaskEventSink::from(channel);
        assert_eq!(sink.policy(), TaskAttachmentPolicy::UiBound);
        assert_eq!(sink.send(event(1, TaskEventKind::TaskStarted, TaskState::Running)), Err("channel_closed".into()));
    }
    #[test]
    fn replay_is_bounded_gaps_reported_and_future_events_ordered() {
        let broker = TaskEventBroker::default(); let (channel, _) = subscriber();
        let sink = broker.start(TaskId(17), 9, channel).unwrap();
        for sequence in 1..=200 { sink.send(event(sequence, TaskEventKind::ProviderChunk { provider_id: "test".into(), chunk: "x".repeat(8192) }, TaskState::Running)).unwrap(); }
        let observation = broker.0.lock().unwrap().as_ref().unwrap().clone();
        let o = observation.lock().unwrap(); assert!(o.events.len() <= MAX_EVENTS); assert!(o.bytes <= MAX_REPLAY_BYTES); drop(o);
        let (channel, rx) = subscriber();
        assert!(!broker.attach(TaskId(17), 9, 0, channel).unwrap().replay_complete);
        let replay: Vec<_> = rx.try_iter().collect();
        assert!(replay.windows(2).all(|e| e[0]["sequence"].as_u64().unwrap() + 1 == e[1]["sequence"].as_u64().unwrap()));
        sink.send(event(201, TaskEventKind::TaskCancelled, TaskState::Cancelled)).unwrap();
        assert_eq!(rx.recv().unwrap()["sequence"], 201);
        let (channel, _) = subscriber();
        assert!(broker.attach(TaskId(17), 10, 0, channel).is_err());
    }
}
