use std::{fs::File, io::Write, os::unix::io::FromRawFd};

pub fn s6_ready(fd: Option<i32>) {
	if let Some(fd) = fd {
		// SAFETY: main is the only owner of the fd. We suppose that from_raw_fd do nothing if the fd is invalid
		let mut f = unsafe { File::from_raw_fd(fd) };
		let _ = writeln!(&mut f);
	}
}
