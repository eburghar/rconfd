use libc::{gid_t, uid_t};
use std::ffi::CString;
use std::path::Path;

/// Encapsulate libc uid and gid
pub struct User {
	uid: uid_t,
	gid: gid_t,
}

impl User {
	/// Try to create a User from a name
	 pub fn new(name: &str) -> Option<Self> {
		let c_name = CString::new(name).unwrap();
		// SAFETY: this is standard call to libc
		unsafe {
			let pwd = libc::getpwnam(c_name.as_ptr());
			if !pwd.is_null() {
				return Some(User {
					uid: (*pwd).pw_uid,
					gid: (*pwd).pw_gid
				})
			} else {
				log::error!("Can't find user {}", name);
			}
		}
		None
	 }

	pub fn chown<T>(&self, path: T)
	where
		T: AsRef<Path>,
	{
		let path = path.as_ref().to_string_lossy();
		let c_path = CString::new(path.as_bytes()).unwrap();
		// SAFETY: this is standard call to libc
		let res = unsafe {
			libc::chown(c_path.as_ptr(), self.uid, self.gid)
		};
		if res != 0 {
			log::error!("Can't change ownership of \"{}\"", path);
		}
	}
}
