use crate::error::ProtoError;
use crate::proto::util;
use crate::proto::{MessageTy, Message, Mode, State, Stream};
use crate::proto::{BLOCK_SIZE, MAGIC_BYTES, MESSAGE_SIZE};

use byteorder::{NetworkEndian, WriteBytesExt};
use rand::Rng;
use ring::aead::{self, OpeningKey, SealingKey};
use std::io::{Cursor, Read, Write};
use std::mem;
use std::net::ToSocketAddrs;

/// The `Receiver` represents the listening half of a `ubuffer`.
/// 
/// It maintains a state machine along with an underlying UDT socket.
/// When the Receiver is running it will block the current thread until
/// it has run its state machine to completion (or the corresponding sender
/// has hung-up the connection.)
///
/// A `Receiver` goes through the following states during it's lifecycle:
///
/// 1. `State::WaitHello`: the receiver waits for a sender to connect, it
///    accepts the connection and the handshake is performed.
///
/// 2. `State:Transmit`: the receiver waits for incoming blocks. The only
///    messages which are legal during this point are either `MessageTy::Block`
///    which specifies the length of an encrypted payload, or `MessageTy::Goodbye`
///    which indicates that the sender wants to hang up.
///
/// 3. `State::WaitHangup` the receiver enters this state after receiving a goodbye.
///    In this state the receiver performs its end of the closing handshake, and then
///    terminates the `run()` loop.
///
pub struct Receiver {
	dec_key: OpeningKey,
	enc_key: SealingKey,

	stream: Stream,
	state: State,

	counter: u64,
	nonce:   u32,
}

impl Receiver {
	/// Creates a `Receiver` which listens on the specified network address (`addr`)
	/// and will use the `key` to decrypt incoming packets. Note that a Receiver will
	/// only `accept()` a single incoming connection, all other clients will be ignored.
	/// If a client connects and fails to create the proper handshake the receiver will
	/// eventually timeout and exit.
	pub fn new<S: ToSocketAddrs>(addr: S, key: &[u8]) -> Result<Self, ProtoError> {
		info!("starting receiver ...");
		let stream = Stream::new(Mode::Receiver, addr)?;
		let dec_key = OpeningKey::new(&aead::AES_256_GCM, key)?;
		let enc_key = SealingKey::new(&aead::AES_256_GCM, key)?;
		info!("accepted connection ...");

		Ok(Self {
			dec_key: dec_key,
			enc_key: enc_key,

			stream: stream,
			state: State::WaitHello,

			counter: 0,
			nonce:   0,
		})
	}

	/// Starts the `Receiver` using the current thread.
	///
	/// The receiver will write all output to `out` as it is received. If the
	/// result is `Ok(_)` then the sender successfully completed the transfer
	/// and hung-up the connection gracefully. Any other response indicates the
	/// message is either corrupt or incopmlete.
	///
	/// Note that if the receiver & sender successfully handshake (that is: they
	/// exchange `MessageTy::Hello` with one another) and only later encounter
	/// a crypto error it likely indicates a packet was corrupted or the sender
	/// was interrupted.
	///
	pub fn run<W: Write>(&mut self, mut out: W) -> Result<(), ProtoError> {
		let mut block_buf = vec![0u8; BLOCK_SIZE + self.enc_key.algorithm().tag_len()];

		loop {
			match self.state {
				State::WaitHello => self.wait_hello()?,
				State::Transmit => self.wait_chunk(&mut block_buf, &mut out)?,

				State::WaitHangup => {
					self.wait_goodbye()?;
					self.stream.as_socket().close()?;
					return Ok(());
				},
			}
		}
	}

	fn wait_chunk<W: Write>(&mut self, block_buf: &mut [u8], mut out: W) -> Result<(), ProtoError> {
		debug!("waiting for block from client ...");
		let mut buf = vec![0u8; MESSAGE_SIZE];
		self.stream.read_exact(&mut buf)?;

		// read the block header
		let message: Message = bincode::deserialize(&buf)?;
		match message.ty {
			MessageTy::Goodbye => {
				self.state = State::WaitHangup;
				return Ok(());
			},

			_ => {},
		}

		assert_eq!(message.ty, MessageTy::Block);
		
		let block_sz = message.len;
		let msg_nonce = util::get_next_nonce(&mut self.nonce, &mut self.counter)?;
		assert!(block_sz <= block_buf.len());

		// decrypt the message
		let mut pos = 0;
		'copy: loop {
			let bytes_read = self.stream.read(&mut block_buf[pos..message.len])?;

			if bytes_read == 0 {
				debug!("stream reached EOF");
				break 'copy;
			}

			trace!("recv {} bytes", bytes_read);
			pos += bytes_read;
			if pos >= block_sz {
				trace!("done copying encrypted block...");
				break 'copy;
			}
		}

		let payload = aead::open_in_place(&self.dec_key, &msg_nonce, b"", 0, &mut block_buf[..pos])?;
		out.write(&payload)?;
		out.flush()?;

		Ok(())
	}

	fn wait_hello(&mut self) -> Result<(), ProtoError> {
		// TODO: handle timeouts
		self.recv_req_iv()?;
		self.send_rep_iv()?;
		self.recv_client_hello()?;
		self.send_server_hello()?;

		info!("handshake complete!");
		self.state = State::Transmit;

		Ok(())
	}

	fn wait_goodbye(&mut self) -> Result<(), ProtoError> {
		self.send_server_goodbye()
	}

	fn recv_req_iv(&mut self) -> Result<(), ProtoError> {
		// client should send us ReqIV
		info!("waiting for client req iv");
		let mut buf = vec![0u8; MESSAGE_SIZE];
		self.stream.read_exact(&mut buf)?;
		let message: Message = bincode::deserialize(&buf)?;
		
		assert_eq!(message.ty, MessageTy::ReqIV);
		assert_eq!(message.len, 0);

		Ok(())
	}

	fn send_rep_iv(&mut self) -> Result<(), ProtoError> {
		// generate an IV and send it to the client
		info!("sending client IV params ...");
		let mut rng = rand::thread_rng();
		let nonce: u32 = rng.gen();
		self.nonce = nonce;

		// write the nonce into a buffer
		let mut cursor = Cursor::new(vec![0u8; 4]);
		cursor.write_u32::<NetworkEndian>(nonce)?;
		let buf = cursor.into_inner();

		// create the message header
		let rep_iv_msg = Message { 
			ty: MessageTy::RepIV,
			len: buf.len(),
		};

		// send RepIV
		info!("sending rep_iv {:?}", rep_iv_msg);
		let rep_iv_buf = bincode::serialize(&rep_iv_msg)?;

		assert_eq!(MESSAGE_SIZE, rep_iv_buf.len());
		self.stream.write(&rep_iv_buf)?;
		self.stream.write(&buf)?;
		Ok(())
	}

	fn recv_client_hello(&mut self) -> Result<(), ProtoError> {
		// read the hello message header
		info!("waiting for client hello ...");
		let mut hello_buf = vec![0u8; MESSAGE_SIZE];
		self.stream.read_exact(&mut hello_buf)?;

		let hello_msg: Message = bincode::deserialize(&hello_buf)?;
		assert_eq!(hello_msg.ty, MessageTy::Hello);

		// read the encrypted payload
		let mut enc_payload = vec![0u8; hello_msg.len];
		self.stream.read_exact(&mut enc_payload)?;

		let msg_nonce = util::get_next_nonce(&mut self.nonce, &mut self.counter)?;
		let payload = aead::open_in_place(&self.dec_key, &msg_nonce, b"", 0, &mut enc_payload)?;
		info!("got hello from client: {:?}", payload);

		Ok(())
	}

	fn send_server_hello(&mut self) -> Result<(), ProtoError> {
		info!("sending hello ...");

		// write the magic bytes to a buffer
		let tag_len = self.enc_key.algorithm().tag_len();
		let enc_buf = vec![0u8; mem::size_of_val(&MAGIC_BYTES) + tag_len];
		let mut enc_buf = {
			let mut cursor = Cursor::new(enc_buf);
			cursor.write_u32::<NetworkEndian>(MAGIC_BYTES)?;
			cursor.into_inner()
		};

		// encrypt the buffer in-place
		let msg_nonce = util::get_next_nonce(&mut self.nonce, &mut self.counter)?;
		let msg_sz = aead::seal_in_place(&self.enc_key, &msg_nonce, b"", &mut enc_buf, tag_len)?;

		// send `Hello` followed by the encrypted payload
		let hello_msg = Message {
			ty: MessageTy::Hello,
			len: msg_sz,
		};

		let hello_buf = bincode::serialize(&hello_msg)?;
		assert_eq!(hello_buf.len(), MESSAGE_SIZE);

		self.stream.write(&hello_buf)?;
		self.stream.write(&enc_buf[..msg_sz])?;

		Ok(())
	}

	fn send_server_goodbye(&mut self) -> Result<(), ProtoError> {
		info!("sending goodbye ...");

		let goodbye_msg = Message {
			ty: MessageTy::Goodbye,
			len: 0,
		};

		let goodbye_buf = bincode::serialize(&goodbye_msg)?;
		assert_eq!(goodbye_buf.len(), MESSAGE_SIZE);
		self.stream.write(&goodbye_buf)?;

		Ok(())
	}
}
