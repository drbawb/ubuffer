#[macro_use] extern crate log;

extern crate clap;
extern crate env_logger;
extern crate failure;
extern crate utp;

use clap::{Arg, App};
use std::io::{self, BufReader, BufRead, Read, Write};
use utp::{UtpSocket, UtpStream};

fn main() -> Result<(), failure::Error> {
	env_logger::init();

	let matches = App::new("uTP buffer")
		.version("0.1.0") // TODO: from cargo ENV
		.about("Transfers files between two nodes over uTP.")
		.arg(Arg::with_name("recv_addr")
			 .short("r")
			 .long("recv")
			 .value_name("RECV_ADDR")
			 .takes_value(true)
			 .help("Starts listening on specified inet address for incoming data."))
		.arg(Arg::with_name("send_addr")
			 .short("s")
			 .long("send")
			 .value_name("SEND_ADDR")
			 .takes_value(true)
			 .help("Sends stdin to the specified receiver."))
		.get_matches();

	if matches.is_present("send_addr") {
		let addr = matches.value_of("send_addr")
			.expect("Entered sending mode w/o supplying an address?");

		start_sender(addr)?;
	} else if matches.is_present("recv_addr") {
		let addr = matches.value_of("recv_addr")
			.expect("Entered receiver mode w/o supplying an address?");

		start_receiver(addr)?;
	}

	Ok(())
}

fn start_sender(addr: &str) -> Result<(), failure::Error> {
	info!("acquiring stdin lock ...");
	let stdin = io::stdin();
	let stdin_lock = stdin.lock();

	info!("connecting to utp receiver ...");
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
		stream.flush()?;
		reader.consume(bytes_read);
		info!("consumed {} bytes of input", bytes_read);
	}

	info!("closing utp stream");
	stream.close()?;

	Ok(())
}

fn start_receiver(addr: &str) -> Result<(), failure::Error> {
	info!("creating receive buffer");
	let mut buf = vec![0u8; 128 * 1024];
	let mut stdout = io::stdout();
	let mut total_bytes = 0;

	info!("starting utp receiver ...");
	let listen = UtpSocket::bind(addr)?;
	let mut stream: UtpStream = listen.into();

	'copy: loop {
		let bytes_read = stream.read(&mut buf)?;
		if bytes_read == 0 {
			info!("stream reached EOF");
			break 'copy;
		}

		stdout.write(&buf[0..bytes_read])?;
		stdout.flush()?;
		total_bytes += bytes_read;

		info!("read {} bytes", total_bytes);
	}

	info!("read {} bytes", total_bytes);

	Ok(())
}
