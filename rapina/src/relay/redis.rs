//! Redis-backed relay backend. Requires the `relay-redis` feature flag.
//!
//! This backend routes relay messages across nodes through Redis pub/sub.
//! Presence remains per-node: [`PresenceMap`](crate::relay::channel::PresenceMap)
//! is not shared across nodes by this implementation.

use std::sync::Arc;

use futures_util::StreamExt;

use super::backend::{RelayBackend, RelayFuture, TopicReceiver};
use crate::error::Error;

/// Redis relay backend using a multiplexed connection for publishing.
pub struct RedisRelayBackend {
    client: redis::Client,
    conn: redis::aio::MultiplexedConnection,
}

impl RedisRelayBackend {
    /// Connects to Redis at the given URL.
    pub async fn connect(url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(url)?;
        let conn = client.get_multiplexed_async_connection().await?;
        Ok(Self { client, conn })
    }
}

impl RelayBackend for RedisRelayBackend {
    fn push(&self, topic: &str, json: Arc<String>) -> RelayFuture<'_, Result<(), Error>> {
        let topic = topic.to_owned();
        let mut conn = self.conn.clone();

        Box::pin(async move {
            redis::cmd("PUBLISH")
                .arg(topic)
                .arg(&*json)
                .query_async::<i64>(&mut conn)
                .await
                .map(|_| ())
                .map_err(|e| Error::internal(format!("relay redis publish error: {e}")))
        })
    }

    fn subscribe(&self, topic: &str) -> RelayFuture<'_, Box<dyn TopicReceiver>> {
        let client = self.client.clone();
        let topic = topic.to_owned();

        Box::pin(async move {
            // The `RelayBackend::subscribe` contract is infallible (it returns a
            // receiver, not a `Result`), so a Pub/Sub connection or `SUBSCRIBE`
            // failure cannot be propagated to the caller here. Log it at `error`
            // level so the failure is observable rather than silent: the returned
            // receiver yields `None` on first `recv`, which ends the forwarding
            // task for this topic.
            let mut pubsub = match client.get_async_pubsub().await {
                Ok(pubsub) => pubsub,
                Err(e) => {
                    tracing::error!(error = %e, topic = %topic, "relay redis pubsub connection failed");
                    return Box::new(RedisTopicReceiver {
                        pubsub: None,
                        topic,
                    }) as Box<dyn TopicReceiver>;
                }
            };

            if let Err(e) = pubsub.subscribe(&topic).await {
                tracing::error!(error = %e, topic = %topic, "relay redis subscribe failed");
                return Box::new(RedisTopicReceiver {
                    pubsub: None,
                    topic,
                }) as Box<dyn TopicReceiver>;
            }

            Box::new(RedisTopicReceiver {
                pubsub: Some(pubsub),
                topic,
            }) as Box<dyn TopicReceiver>
        })
    }
}

/// Receives messages from a dedicated Redis pub/sub connection.
///
/// Dropping the receiver drops that connection, which Redis treats as an
/// implicit unsubscribe/disconnect. Explicit `UNSUBSCRIBE` is async and cannot
/// be awaited from `Drop`.
struct RedisTopicReceiver {
    pubsub: Option<redis::aio::PubSub>,
    topic: String,
}

impl TopicReceiver for RedisTopicReceiver {
    fn recv(&mut self) -> RelayFuture<'_, Option<Arc<String>>> {
        Box::pin(async move {
            let pubsub = self.pubsub.as_mut()?;
            let mut messages = pubsub.on_message();
            let msg = messages.next().await?;

            if msg.get_channel_name() != self.topic {
                return None;
            }

            msg.get_payload::<String>().ok().map(Arc::new)
        })
    }
}

impl Drop for RedisTopicReceiver {
    fn drop(&mut self) {
        self.pubsub.take();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;

    use super::*;

    // Run with: cargo test --features relay-redis -- --ignored
    #[ignore]
    #[tokio::test]
    async fn test_redis_relay_cross_instance_delivery() {
        let subscriber = RedisRelayBackend::connect("redis://127.0.0.1:6379")
            .await
            .expect("Redis connection failed for subscriber");
        let publisher = RedisRelayBackend::connect("redis://127.0.0.1:6379")
            .await
            .expect("Redis connection failed for publisher");

        let topic = "rapina:test:relay";
        let mut receiver = subscriber.subscribe(topic).await;
        publisher
            .push(topic, Arc::new(r#"{"ok":true}"#.to_owned()))
            .await
            .expect("Redis publish failed");

        let msg = timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("timed out waiting for Redis relay message")
            .expect("Redis relay subscription closed");

        assert_eq!(&*msg, r#"{"ok":true}"#);
    }
}
