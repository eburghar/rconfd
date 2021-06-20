use anyhow::{anyhow, Context, Result};
use async_std::{channel::Sender, future, task};
use http::{Request, StatusCode};
use isahc::{
	config::{CaCertificate, Configurable},
	AsyncReadResponseExt, HttpClient,
};
use serde::Deserialize;
use serde_json::Value;
use std::{fs::File, io::Read, time::Duration, collections::HashMap};

use crate::message::{send_message, Message};

pub type VaultClients = HashMap<String, VaultClient>;

/// delay a future by a duration
fn delay_task<F>(fut: F, dur: Duration) -> task::JoinHandle<Result<()>>
where
	F: future::Future<Output = Result<()>> + Send + 'static,
{
	task::spawn(async move {
		let forever = future::pending::<()>();
		// ignore the TimeOut error because forever is staying forever in pending state
		let _ = future::timeout(dur, forever).await;
		fut.await?;
		Ok::<(), anyhow::Error>(())
	})
}

/// structure to keep token from vault login response
#[derive(Debug, Deserialize)]
pub struct Auth {
	client_token: String,
	lease_duration: u64,
	renewable: bool,
}

/// deserialize the vault errors
#[derive(Debug, Deserialize)]
struct VaultErrors {
	errors: Vec<String>,
}

/// vault client that cache it's auth token
pub struct VaultClient {
	url: String,
	jwt: String,
	client: HttpClient,
	pub auth: Option<Auth>
}

impl VaultClient {
	/// Create a new vault client given an url, a role, a token path and a ca certificate path
	pub async fn new(url: &str, token: &str, cacert: &str) -> Result<Self> {
		let mut jwt = String::new();
		File::open(token)
			.with_context(|| format!("unable to open the token file \"{}\"", &token))?
			.read_to_string(&mut jwt)
			.with_context(|| format!("unable to read the token file \"{}\"", &token))?;
		let client = HttpClient::builder()
			.ssl_ca_certificate(CaCertificate::file(cacert))
			.default_header("Content-Type", "application/json")
			.build()?;
		Ok(VaultClient {
			url: url.to_owned(),
			jwt,
			client,
			auth: None,
		})
	}

	/// Log in to the vault client.
	pub async fn login(&mut self, sender: Sender<Message>, role: String) -> Result<()> {
		let url = format!("{}/auth/kubernetes/login", &self.url);
		let body = format!(r#"{{"role": "{}", "jwt": "{}"}}"#, &role, &self.jwt);
		let mut res = self.client.post_async(url, body).await?;
		let status = res.status();
		return if status == StatusCode::OK {
			// parse vault response and cache important information
			let auth: Value = res
				.json()
				.await
				.with_context(|| "can't parse login response")?;
			let lease_duration = auth["auth"]["lease_duration"].as_u64().unwrap_or(0u64);
			let renewable = auth["auth"]["renewable"].as_bool().unwrap_or(false);
			let auth = Auth {
				client_token: auth["auth"]["client_token"]
					.as_str()
					.unwrap_or("")
					.to_owned(),
				lease_duration,
				renewable,
			};

			// schedule a relogin login task at 2/3 of the lease_duration time
			if auth.client_token != "" {
				if auth.renewable {
					let dur = Duration::from_secs(auth.lease_duration * 2 / 3);
					log::debug!("Successfuly logged in. Log in again within {:?}", &dur);
					self.auth = Some(auth);
					delay_task(send_message(sender, Message::Login(role)), dur);
				}
			} else {
				self.auth = None;
			}
			Ok(())
		} else {
			// parse vault error
			let errors: VaultErrors = res.json().await?;
			Err(anyhow!(format!(
				"http error code {}\n{}",
				status,
				errors.errors.join("\n")
			)))
		};
	}

	/// Get a secret from vault server and reschedule a renew with role if necessary
	pub async fn get_secret(
		&mut self,
		sender: Sender<Message>,
		role: String,
		path: String,
	) -> Result<Value> {
		if let Some(ref auth) = self.auth {
			let url = format!("{}/{}", &self.url, &path);
			let request = Request::get(url)
				.header("X-Vault-Token", auth.client_token.as_str())
				.body(())?;
			let mut res = self.client.send_async(request).await?;
			let status = res.status();
			return if status == StatusCode::OK {
				// parse vault response
				let secret_value: Value = res
					.json()
					.await
					.with_context(|| "can't parse returned secret")?;
				let renewable = secret_value["renewable"].as_bool().unwrap_or(false);

				// schedule the newew of the secret if necessary
				if renewable {
					let dur = Duration::from_secs(
						secret_value["lease_duration"].as_u64().unwrap_or(0u64) * 2 / 3,
					);
					log::debug!("Successfuly get secret. Renew within {:?}", &dur);
					delay_task(send_message(sender, Message::GetSecret(role, path)), dur);
				}

				// return the parsed secret
				Ok(secret_value)
			} else {
				// parse vault error
				let errors: VaultErrors = res.json().await?;
				Err(anyhow!(
					"http error code {}\n{}",
					status,
					errors.errors.join("\n")
				))
			};
		} else {
			Err(anyhow!("not logged to vault server"))
		}
	}
}
