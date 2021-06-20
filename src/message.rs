use async_std::channel::Sender;
use anyhow::Result;

/// Message sent by tasks to main_loop
#[derive(Debug)]
pub enum Message {
	/// log in and re log in (role)
	Login(String),
	// get/refresh a secret (path)
	GetSecret(String),
	// generate template (config name)
	GenerateTemplate(String),
}

/// convert the error in the return signature of sender.send to anyhow::Error
pub async fn send_message(sender: Sender<Message>, msg: Message) -> Result<()> {
	sender.send(msg).await.map_err(|e| anyhow::Error::from(e))
}
