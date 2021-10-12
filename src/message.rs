use anyhow::Result;
use async_std::channel::Sender;

use crate::conf::TemplateConf;

/// Message sent by tasks to main_loop
#[derive(Debug)]
pub enum Message {
	/// log in and re log in (role)
	Login(String),
	// get/refresh a secret (path) and trigger generate template or not
	GetSecret(String, bool),
	// generate template (config name)
	GenerateTemplate(String),
	// generate all templates
	GenerateAllTemplates,
	// insert template
	InsertTemplate(String, TemplateConf),
}

/// convert the error in the return signature of sender.send to anyhow::Error
pub async fn send_message(sender: Sender<Message>, msg: Message) -> Result<()> {
	sender.send(msg).await.map_err(|e| anyhow::Error::from(e))
}
