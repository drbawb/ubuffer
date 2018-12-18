#[macro_use] extern crate log;

extern crate clap;
extern crate env_logger;
extern crate failure;
extern crate utp;

use std::env;
use std::io::{self, BufReader, BufRead, Read, Write};
use utp::{UtpSocket, UtpStream};

fn main() -> Result<(), failure::Error> {
	env_logger::init();

	for arg in env::args().skip(1) {
		match &arg[..] {
			"-s" | "--send" => start_sender()?,
			"-r" | "--recv" => start_receiver()?,
			_ => unreachable!(),
		}
	}

	Ok(())
}

fn start_sender() -> Result<(), failure::Error> {
	info!("acquiring stdin lock ...");
	let stdin = io::stdin();
	let stdin_lock = stdin.lock();

	info!("connecting to utp receiver ...");
	let addr = "127.0.0.1:5999";
	let mut stream = UtpStream::connect(addr)?;

	info!("setting up 128KiB buffer");
	let mut reader = BufReader::with_capacity(128 * 1024, stdin_lock);

	'copy: loop {
		let buf = reader.fill_buf()?;
		let bytes_read = buf.len();
		if bytes_read == 0 {
			info!("buffer reached EOF");
			break 'copy;
		}

		stream.write(buf)?;
		reader.consume(bytes_read);
	}

	info!("closing utp stream");
	stream.close()?;

	Ok(())
}

fn start_receiver() -> Result<(), failure::Error> {
	info!("creating receive buffer");
	let mut buf = vec![0u8; 128 * 1024];
	let mut stdout = io::stdout();
	let mut total_bytes = 0;

	info!("starting utp receiver ...");
	let listen = UtpSocket::bind("0.0.0.0:5999")?;
	let mut stream: UtpStream = listen.into();

	'copy: loop {
		let bytes_read = stream.read(&mut buf)?;
		if bytes_read == 0 {
			info!("stream reached EOF");
			break 'copy;
		}

		stdout.write(&buf[0..bytes_read])?;
		total_bytes += bytes_read;
	}

	info!("read {} bytes", total_bytes);

	Ok(())
}
