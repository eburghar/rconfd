use anyhow::{anyhow, Result, Context};
use std::env;

#[derive(PartialEq, Debug)]
pub enum Token<'a> {
	Var(&'a str),
	Str(&'a str),
	BraceError,
}

pub struct SubstIterator<'a> {
	remainder: &'a str,
}

impl<'a> SubstIterator<'a> {
	pub fn new(string: &'a str) -> Self {
		Self { remainder: string }
	}

	pub fn yield_remainder(&mut self) -> Option<Token<'a>> {
		let chunk = self.remainder;
		self.remainder = "";
		Some(Token::Str(chunk))
	}

	pub fn yield_var(&mut self) -> Option<Token<'a>> {
		return if let Some(end) = self.remainder.find("}") {
			let name = &self.remainder[2..end];
			self.remainder = &self.remainder[end + 1..];
			Some(Token::Var(name))
		} else {
			self.remainder = "";
			Some(Token::BraceError)
		};
	}

	pub fn yield_str(&mut self, end: usize) -> Option<Token<'a>> {
		let chunk = &self.remainder[..end];
		self.remainder = &self.remainder[end..];
		Some(Token::Str(chunk))
	}
}

impl<'a> Iterator for SubstIterator<'a> {
	type Item = Token<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		return if self.remainder.is_empty() {
			None
		} else {
			if self.remainder.starts_with("${") {
				self.yield_var()
			} else {
				match self.remainder.find("${") {
					None => self.yield_remainder(),
					Some(end) => self.yield_str(end),
				}
			}
		};
	}
}

/// return expr with expression ${VAR} subsitued by variable content
/// TODO: use a COW
pub fn subst_envar(s: &str) -> Result<String> {
	let mut res = String::new();
	for token in SubstIterator::new(s) {
		match token {
			Token::Str(chunk) => {
				res += chunk;
			}
			Token::Var(name) => {
				let val = env::var(name)
					.with_context(|| format!("unable to substitute ${{{}}} in {}", name, s))?;
				res += &val;
			}
			Token::BraceError => Err(anyhow!("no matching } found"))?,
		}
	}
	Ok(res)
}

#[test]
fn empty() {
	let tokens: Vec<_> = SubstIterator::new("").collect();
	assert_eq!(tokens, &[]);
}

#[test]
fn var() {
	let tokens: Vec<_> = SubstIterator::new("${TEST}").collect();
	assert_eq!(tokens, &[Token::Var("TEST")]);
}

#[test]
fn mixed() {
	let tokens: Vec<_> = SubstIterator::new("backend:${TEST}:path").collect();
	assert_eq!(
		tokens,
		&[
			Token::Str("backend:"),
			Token::Var("TEST"),
			Token::Str(":path")
		]
	);
}

#[test]
fn error() {
	let tokens: Vec<_> = SubstIterator::new("${TEST").collect();
	assert_eq!(tokens, &[Token::BraceError]);
}
