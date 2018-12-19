#[macro_use] extern crate failure;
#[macro_use] extern crate log;

extern crate clap;
extern crate crypto;
extern crate env_logger;
extern crate udt;

use crate::error::ApplicationError;
use crate::proto::BLOCK_SIZE;

use clap::{Arg, App};
use std::io::{self, BufReader, BufRead, Write};
use std::net::ToSocketAddrs;
use udt::{SocketFamily, SocketType, UdtSocket};

mod error;
mod proto;

fn main() -> Result<(), failure::Error> {
	env_logger::init();

	let matches = App::new("UDT buffer")
		.version(env!("CARGO_PKG_VERSION")) 
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
	let addr = addr.to_socket_addrs()?
		.take(1).next()
		.expect("no valid sender address?");

	let sock = UdtSocket::new(SocketFamily::AFInet, SocketType::Stream)
		.map_err(|err| ApplicationError::SocketErr { inner: err })?;

	sock.connect(addr)
		.map_err(|err| ApplicationError::SocketErr { inner: err })?;

	

	info!("setting up buffer");
	let mut reader = BufReader::with_capacity(BLOCK_SIZE, stdin_lock);

	'copy: loop {
		let buf = reader.fill_buf()?;
		let bytes_read = buf.len();
		if bytes_read == 0 {
			info!("buffer reached EOF");
			break 'copy;
		}

		let mut pos = 0;
		'write: loop {
			let bytes_sent = sock.send(&buf[pos..bytes_read])
				.map_err(|err| ApplicationError::TxErr { inner: err })?;

			pos += bytes_sent as usize; // NOTE: why the heck is this an i32?
			info!("pos: {}, sent: {}, len: {}", pos, bytes_sent, bytes_read);
			if pos >= bytes_read { break 'write; }
		}

		reader.consume(bytes_read);
		info!("consumed {} bytes of input", bytes_read);
	}

	info!("closing utp stream");
	sock.close()
		.map_err(|err| ApplicationError::SocketErr { inner: err })?;

	Ok(())
}

fn start_receiver(addr: &str) -> Result<(), failure::Error> {
	info!("starting utp receiver ...");
	let addr = addr.to_socket_addrs()?
		.take(1).next()
		.expect("no valid sender address?");

	let sock = UdtSocket::new(SocketFamily::AFInet, SocketType::Stream)
		.map_err(|err| ApplicationError::SocketErr { inner: err })?;

	sock.bind(addr)
		.map_err(|err| ApplicationError::SocketErr { inner: err })?;

	sock.listen(1)
		.map_err(|err| ApplicationError::SocketErr { inner: err })?;

	info!("accepting connection ...");
	let (conn, peer) = sock.accept()
		.map_err(|err| ApplicationError::SocketErr { inner: err })?;

	info!("connected to peer {:?}", peer);

	info!("creating receive buffer");
	let mut buf = vec![0u8; BLOCK_SIZE];
	let mut stdout = io::stdout();
	let mut total_bytes = 0;

	'copy: loop {
		let buf_len = buf.len();
		let bytes_read = conn.recv(&mut buf, buf_len)
			.map_err(|err| ApplicationError::RxErr { inner: err })?;
		if bytes_read == 0 {
			info!("stream reached EOF");
			break 'copy;
		}
		
		info!("recv {} bytes", bytes_read);

		stdout.write(&buf[0..(bytes_read as usize)])?;
		stdout.flush()?;
		total_bytes += bytes_read;

		info!("read {} bytes", total_bytes);
	}

	info!("read {} bytes", total_bytes);

	Ok(())
}
