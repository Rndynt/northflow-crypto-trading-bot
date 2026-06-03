use crate::agents::MessageBus;
use crate::agents::messages::{AgentEvent, AgentId};
use chrono::Utc;
use std::time::Duration;

pub fn spawn(bus: MessageBus, from: AgentId) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(20));
        loop {
            tick.tick().await;
            bus.publish(AgentEvent::Heartbeat {
                from,
                ts: Utc::now(),
            });
        }
    });
}
