#[macro_use] extern crate failure;
#[macro_use] extern crate log;
#[macro_use] extern crate serde_derive;

extern crate bincode;
extern crate byteorder;
extern crate clap;
extern crate env_logger;
extern crate rand;
extern crate ring;
extern crate serde;
extern crate udt;

use crate::proto::{Sender, Receiver};

use clap::{Arg, App};
use std::io;

mod error;
mod proto;

fn main() -> Result<(), failure::Error> {
	env_logger::init();

	let matches = App::new("UDT buffer")
		.version(env!("CARGO_PKG_VERSION")) 
		.about("Transfers files between two nodes using the UDT protocol.")
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
	let mut sender = Sender::new(addr, b"dcb23a241cd82c4152b8e5637a4ae121")?;
	sender.run(io::stdin())?;

	Ok(())
}

fn start_receiver(addr: &str) -> Result<(), failure::Error> {
	let mut receiver = Receiver::new(addr, b"dcb23a241cd82c4152b8e5637a4ae121")?;
	receiver.run(io::stdout())?;

	Ok(())
}
