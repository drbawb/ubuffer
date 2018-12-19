use crate::error::ProtoError;
use crate::proto::{MessageTy, Message, Mode, State, Stream};
use crate::proto::{MAGIC_BYTES, MESSAGE_SIZE};

use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};
use rand::Rng;
use ring::aead::{self, OpeningKey, SealingKey};
use std::io::{Cursor, Read, Write};
use std::mem;
use std::net::ToSocketAddrs;

pub struct Receiver {
	dec_key: OpeningKey,
	enc_key: SealingKey,

	stream: Stream,
	state: State,

	counter: u64,
	nonce:   u32,
}

impl Receiver {
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

	pub fn run<W: Write>(&mut self, mut input: W) -> Result<(), ProtoError> {
		loop {
			match self.state {
				State::WaitHello => self.wait_hello()?,

				_ => panic!("receiver state not implemented ..."),
			}
		}
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

		let msg_nonce = self.get_next_nonce()?; // nonce 0 for client hello
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
		let msg_nonce = self.get_next_nonce()?;
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

	fn get_next_nonce(&mut self) -> Result<Box<[u8]>, ProtoError> {
		let buf = vec![0u8; 12];
		let mut cursor = Cursor::new(buf);

		let nonce = self.nonce;
		let counter = self.counter;
		self.counter += 1;
		
		cursor.write_u32::<NetworkEndian>(self.nonce)?;
		cursor.write_u64::<NetworkEndian>(self.counter)?;

		Ok(cursor.into_inner().into_boxed_slice())
	}
}
