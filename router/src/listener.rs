use async_trait::async_trait;
use route_weaver_common::message::Message;

#[async_trait]
pub trait MessageListener {
    async fn process_message(&mut self, message: Message);
}
