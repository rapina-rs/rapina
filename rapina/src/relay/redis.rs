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
                        stream: None,
                        topic,
                    }) as Box<dyn TopicReceiver>;
                }
            };

            if let Err(e) = pubsub.subscribe(&topic).await {
                tracing::error!(error = %e, topic = %topic, "relay redis subscribe failed");
                return Box::new(RedisTopicReceiver {
                    stream: None,
                    topic,
                }) as Box<dyn TopicReceiver>;
            }

            let (_sink, stream) = pubsub.split();
            Box::new(RedisTopicReceiver {
                stream: Some(stream),
                topic,
            }) as Box<dyn TopicReceiver>
        })
    }
}

/// Receives messages from a dedicated Redis pub/sub connection.
struct RedisTopicReceiver {
    stream: Option<redis::aio::PubSubStream>,
    topic: String,
}

impl TopicReceiver for RedisTopicReceiver {
    fn recv(&mut self) -> RelayFuture<'_, Option<Arc<String>>> {
        Box::pin(async move {
            let stream = self.stream.as_mut()?;

            loop {
                let Some(msg) = stream.next().await else {
                    tracing::warn!(topic = %self.topic, "relay redis subscription stream ended");
                    return None;
                };

                // Redundant today: each receiver subscribes to exactly one channel, so every
                // message here already belongs to `self.topic`. Kept defensively, becomes
                // necessary if a shared connection ever multiplexes multiple channels and we
                // need to demux by channel name.
                if msg.get_channel_name() != self.topic {
                    tracing::warn!(
                        topic = %self.topic,
                        channel = %msg.get_channel_name(),
                        "relay redis skipped message for unexpected channel"
                    );
                    continue;
                }

                match msg.get_payload::<String>() {
                    Ok(payload) => return Some(Arc::new(payload)),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            topic = %self.topic,
                            "relay redis skipped undecodable message"
                        );
                    }
                }
            }
        })
    }
}

impl Drop for RedisTopicReceiver {
    fn drop(&mut self) {
        // One dedicated connection per topic: dropping the stream closes the
        // connection, and Redis treats that as an implicit unsubscribe. No explicit
        // UNSUBSCRIBE is needed here (and it couldn't run anyway because Drop is sync,
        // UNSUBSCRIBE is async). Explicit UNSUBSCRIBE only becomes necessary with a
        // shared connection (planned as follow-up), and even then it lives in the
        // backend's refcounted subscription management, not in this Drop.
        self.stream.take();
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

    #[ignore]
    #[tokio::test]
    async fn test_redis_relay_skips_mismatched_channel() {
        let subscriber = RedisRelayBackend::connect("redis://127.0.0.1:6379")
            .await
            .expect("Redis connection failed for subscriber");
        let publisher = RedisRelayBackend::connect("redis://127.0.0.1:6379")
            .await
            .expect("Redis connection failed for publisher");

        let topic = "rapina:test:relay:expected";
        let foreign_topic = "rapina:test:relay:foreign";
        let mut pubsub = subscriber
            .client
            .get_async_pubsub()
            .await
            .expect("Redis Pub/Sub connection failed");
        pubsub
            .subscribe(foreign_topic)
            .await
            .expect("Redis foreign-topic subscribe failed");
        pubsub
            .subscribe(topic)
            .await
            .expect("Redis expected-topic subscribe failed");
        let (_sink, stream) = pubsub.split();
        let mut receiver = RedisTopicReceiver {
            stream: Some(stream),
            topic: topic.to_owned(),
        };

        publisher
            .push(foreign_topic, Arc::new(r#"{"foreign":true}"#.to_owned()))
            .await
            .expect("Redis foreign-topic publish failed");
        publisher
            .push(topic, Arc::new(r#"{"ok":true}"#.to_owned()))
            .await
            .expect("Redis expected-topic publish failed");

        let msg = timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("timed out waiting for Redis relay message")
            .expect("Redis relay subscription closed");

        assert_eq!(&*msg, r#"{"ok":true}"#);
    }
}
