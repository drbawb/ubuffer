#[macro_use] extern crate failure;
#[macro_use] extern crate log;
#[macro_use] extern crate serde_derive;

extern crate base64;
extern crate bincode;
extern crate byteorder;
extern crate clap;
extern crate env_logger;
extern crate rand;
extern crate ring;
extern crate serde;
extern crate udt;

use crate::proto::{Sender, Receiver};
use clap::{Arg, App, SubCommand};
use std::io;

mod error;
mod proto;

const CLI_TITLE: &str = "UDT buffer"; 

const CLI_SUB_GENKEY: &str = "genkey";
const CLI_SUB_SEND: &str = "sender";
const CLI_SUB_RECV: &str = "receiver";

const CLI_ARG_KEY: &str = "KEY";
const CLI_ARG_KEY_SHORT: &str = "k";
const CLI_ARG_KEY_LONG: &str = "key";

const CLI_TXT_APP: &str = "Transfer files between two nodes using the UDT protocol.";
const CLI_TXT_KEY: &str = "The encryption key used to encrypt data blocks. (Must match on both sender & receiver.)";
const CLI_TXT_GENKEY: &str = "generates a random encryption key on stdout (256-bits, base64 encoded)";
const CLI_TXT_SEND: &str = "starts `ubuffer` in sender mode.";
const CLI_TXT_RECV: &str = "starts `ubuffer` in receiver mode.";

fn main() -> Result<(), failure::Error> {
	env_logger::init();

	let matches = App::new(CLI_TITLE)
		.version(env!("CARGO_PKG_VERSION")) 
		.about(CLI_TXT_APP)
		.subcommand(SubCommand::with_name(CLI_SUB_GENKEY)
					.about(CLI_TXT_GENKEY))
		.subcommand(SubCommand::with_name(CLI_SUB_SEND)
					.about(CLI_TXT_SEND)
					.arg(Arg::with_name("REMOTE_ADDR")
						 .help("The address of the remote peer waiting in `receiver` mode.")
						 .required(true))
					.arg(Arg::with_name(CLI_ARG_KEY)
						 .short(CLI_ARG_KEY_SHORT)
						 .long(CLI_ARG_KEY_LONG)
						 .help(CLI_TXT_KEY)
						 .required(true)))
		.subcommand(SubCommand::with_name(CLI_SUB_RECV)
					.about(CLI_TXT_RECV)
					.arg(Arg::with_name("LISTEN_ADDR")
						 .help("The address and port to listen on for incoming senders.")
						 .required(true))
					.arg(Arg::with_name(CLI_ARG_KEY)
						 .short(CLI_ARG_KEY_SHORT)
						 .long(CLI_ARG_KEY_LONG)
						 .help(CLI_TXT_KEY)
						 .required(true)))
		.get_matches();

	if let Some(_cmd) = matches.subcommand_matches("sender") {
		let key = matches.value_of(CLI_ARG_KEY)
			.expect("fatal: sender requires an encryption key.");

		let addr = matches.value_of("REMOTE_ADDR")
			.expect("fatal: sender requires a remote address.");

		start_sender(addr, key)?;
	} else if let Some(_cmd) = matches.subcommand_matches("receiver") {
		let key = matches.value_of(CLI_ARG_KEY)
			.expect("fatal: sender requires an encryption key.");

		let addr = matches.value_of("LISTEN_ADDR")
			.expect("fatal: sender requires a remote address.");

		start_receiver(addr, key)?;
	} else if let Some(_cmd) = matches.subcommand_matches("genkey") {
		genkey();
	} else {
		panic!("no matching subcommand specified?");
	}

	Ok(())
}

fn start_sender(addr: &str, key: &str) -> Result<(), failure::Error> {
	let key = base64::decode(key)?;
	let mut sender = Sender::new(addr, &key)?;
	sender.run(io::stdin())?;

	Ok(())
}

fn start_receiver(addr: &str, key: &str) -> Result<(), failure::Error> {
	let key = base64::decode(key)?;
	let mut receiver = Receiver::new(addr, &key)?;
	receiver.run(io::stdout())?;

	Ok(())
}

fn genkey() {
	use rand::Rng;

	let mut rng = rand::thread_rng();
	let mut key = [0u8; 32];

	for key_byte in &mut key {
		*key_byte = rng.gen();
	}

	let key_b64 = base64::encode(&key);
	println!("{}", key_b64);
}
