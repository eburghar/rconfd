use std::{fs::File, os::unix::io::FromRawFd, io::Write};

pub fn s6_ready(fd: Option<i32>) {
	if let Some(fd) = fd {
		// SAFETY: main is the only owner of the fd. We suppose that from_raw_fd do nothing if the fd is invalid
		let mut f = unsafe { File::from_raw_fd(fd) };
		let _ = write!(&mut f, "\n");
	}
}
