use amiquip::AmqpProperties;
use amiquip::Connection;
use amiquip::Exchange;
use amiquip::QueueDeclareOptions;
use amiquip::{Channel, Publish};
use amiquip::{ConsumerMessage, ConsumerOptions, Queue};
use async_std::sync::Arc;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use log::{info, trace};
use serde::Deserialize;
use std::marker::PhantomData;

use anyhow::{anyhow, Result};
use serde::Serialize;
use tokio::sync::RwLock;

pub enum Message<T>
where
    T: Serialize,
{
    AsBytes(String, T),
    AsJson(String, T),
}

pub enum MessageResult {
    Sent,
    Error,
}

pub struct MessageConsumer<T>
where
    for<'de> T: Serialize + Deserialize<'de> + Clone,
{
    shutdown_channel: Sender<()>,
    message_channel: Receiver<T>,
    _pd: PhantomData<T>,
}
pub struct MessageQueue<T>
where
    for<'de> T: Serialize + Deserialize<'de> + Clone,
{
    channel: Arc<RwLock<Option<Channel>>>,
    connection: Arc<RwLock<Option<Connection>>>,
    _pd: PhantomData<T>,
}

impl<T> MessageConsumer<T>
where
    for<'de> T: Serialize + Deserialize<'de> + Clone,
{
    /// Blocking method that will receive message when one is available in the queue
    pub async fn recv(&self) -> Result<T> {
        self.message_channel
            .recv()
            .map_err(|err| anyhow!("Could not receive from message channel: {}", err))
    }

    /// Shutdown this message consumer.
    /// Will drop the join handle and close all channels produced by this consumer.

    pub fn shutdown(&self) -> Result<()> {
        self.shutdown_channel
            .send(())
            .map_err(|err| anyhow!("Could not send shutdown signal to consumer: {}", err))
    }
}

impl<'a, T> MessageQueue<T>
where
    for<'de> T: Serialize + Deserialize<'de> + Clone + Send + 'static,
{
    fn new(url: &str) -> Result<MessageQueue<T>> {
        let mut connection = Connection::insecure_open(url)
            .map_err(|err| anyhow!("Could not open amqp connection at {}: {}", url, err))?;

        // Open a channel - None says let the library choose the channel ID.
        let channel = connection
            .open_channel(None)
            .map_err(|err| anyhow!("Could not open channel: {}", err))?;

        // Get a handle to the direct exchange on our channel.

        Ok(MessageQueue {
            channel: Arc::new(RwLock::new(Some(channel))),
            connection: Arc::new(RwLock::new(Some(connection))),
            _pd: Default::default(),
        })
    }
    async fn publish(&self, msg: Message<T>) -> Result<MessageResult> {
        let publish_result = self
            .channel
            .read()
            .await
            .as_ref()
            .map(|channel| {
                let exchange = Exchange::direct(channel);
                match msg {
                    Message::AsBytes(routing_key, msg) => {
                        let properties = AmqpProperties::default()
                            .with_content_type("application/octet-stream".into());
                        exchange.publish(Publish::with_properties(
                            &bincode::serialize(&msg)?,
                            routing_key,
                            properties,
                        ))
                    }
                    Message::AsJson(routing_key, obj) => exchange.publish(Publish::new(
                        serde_json::to_string(&obj)
                            .map_err(|err| anyhow!("Could not map object to json: {}", err))?
                            .as_bytes(),
                        routing_key,
                    )),
                }
                .map_err(|err| anyhow!("Error publishing message: {}", err))
            })
            .unwrap();

        match publish_result {
            Ok(_) => Ok(MessageResult::Sent),
            Err(_) => Err(anyhow!("Error sending message")),
        }
    }

    async fn consumer<'b: 'a, FN>(
        &'a self,
        queue_name: &'static str,
        processor: FN,
    ) -> MessageConsumer<T>
    where
        FN: Fn(T) + Send + 'static,
    {
        // Open a channel - None says let the library choose the channel ID.

        // Declare the "hello" queue.
        let conn = self.connection.clone();
        let mut c_w = conn.write().await;

        let chan = c_w
            .as_mut()
            .unwrap()
            .open_channel(None)
            .map_err(|err| anyhow!("Could not open channel: {}", err))
            .unwrap();

        drop(c_w);

        let (tx, rx) = crossbeam_channel::bounded::<()>(2);
        let (msg_tx, msg_rx) = crossbeam_channel::unbounded::<T>();

        let t_rx = rx.clone();
        let msg_tx2 = msg_tx.clone();

        let jh = tokio::spawn(async move {
            let queue: Queue = chan
                .queue_declare(queue_name, QueueDeclareOptions::default())
                .map_err(|err| anyhow!("Could not create or declare queue: {}", err))
                .unwrap();

            // Start a consumer.
            let consumer = queue
                .consume(ConsumerOptions::default())
                .map_err(|err| anyhow!("Could not consume amqp message: {}", err))
                .unwrap();

            for (_i, message) in consumer.receiver().iter().enumerate() {
                match message {
                    ConsumerMessage::Delivery(delivery) => {
                        let dev = delivery.body.clone();

                        let props: &AmqpProperties = &delivery.properties;

                        let result: T = if let Some(content_type) = props.content_type() {
                            match content_type.as_str() {
                                "application/octet-stream" => {
                                    bincode::deserialize_from(&dev[..]).unwrap()
                                }
                                _ => {
                                    let body = String::from_utf8_lossy(&dev);
                                    serde_json::from_str(body.as_ref()).unwrap()
                                }
                            }
                        } else {
                            let body = String::from_utf8_lossy(&dev);
                            serde_json::from_str(body.as_ref()).unwrap()
                        };

                        consumer
                            .ack(delivery)
                            .unwrap_or_else(|_o| trace!("Could not acknowledge message."));

                        let clone: T = result.clone();

                        processor(result);
                        msg_tx2
                            .clone()
                            .send(clone)
                            .unwrap_or_else(|_o| info!("Could not send message to channel."));
                    }
                    other => {
                        println!("Consumer ended: {:?}", other);
                        break;
                    }
                }
            }
        });

        tokio::spawn(async move {
            t_rx.recv().unwrap();
            drop(jh);
        });

        MessageConsumer {
            message_channel: msg_rx.clone(),
            shutdown_channel: tx.clone(),
            _pd: Default::default(),
        }
    }

    async fn close(&self) -> Result<()> {
        let arc = self.connection.clone();
        let mut opt = arc.write().await;

        if let Some(conn) = opt.take() {
            conn.close()
                .map_err(|err| anyhow!("Could not close connection: {}", err))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod test {

    use super::{Deserialize, Message, MessageQueue, Result, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone)]
    struct TestMessage {
        msg: String,
    }
    #[tokio::test(threaded_scheduler)]
    async fn test_amqp() -> Result<()> {
        let mp = MessageQueue::new("amqp://guest:guest@localhost:5672")?;

        let recv = mp
            .consumer("test_queue", |msg| {
                println!("Got message : {:?}", msg);
            })
            .await;

        let _result = mp
            .publish(Message::AsBytes(
                "test_queue".into(),
                TestMessage {
                    msg: "Hello!".into(),
                },
            ))
            .await?;

        let msg = recv.recv().await?;

        println!("Got message from channel: {:?}", msg);

        recv.shutdown()?;

        Ok(())
    }
}
